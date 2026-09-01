//! Derivation: handing a photograph's pixels to an external processor and
//! filing what comes back as a **new asset** (ADR 0107).
//!
//! Three steps, and the middle one is not this crate's:
//!
//! 1. [`plan_derive`] gathers everything from the catalog, so the slow half
//!    runs with no lock held — the split ADR 0023 and ADR 0024 already made
//!    for previews and exports;
//! 2. [`exchange`] decodes and develops the photograph **up to rank 20**
//!    (`stages::DERIVE_RANK`) and hands the buffer to `leyline-derive`,
//!    which runs the processor;
//! 3. [`register`] writes the answer next to the original, imports it, and
//!    gives it its parent's metadata, its parent's development and a
//!    `derived_from` row.
//!
//! What makes the result worth the trouble is where step 2 stops: the
//! derived file replaces the **decode**, not the development, so white
//! balance, exposure, tone, masks and crop are all still live settings on
//! the new asset (ADR 0107 §4).

use std::path::{Path, PathBuf};

use leyline_catalog::{Catalog, NewAsset};
use leyline_core::{AssetId, LeylineError, Result, Settings, SourceEncoding, VersionId};
use leyline_derive::{Exchange, ProcessorSource};

use crate::render;

/// Everything [`exchange`] needs to decode and develop one photograph up to
/// the derivation rank, read from the catalog up front (ADR 0024's split).
pub(crate) struct DerivePlan {
    pub(crate) asset: AssetId,
    /// The revision the derivation starts from — and, minus what it bakes,
    /// the one the derived asset inherits.
    develop: Settings,
    source: PathBuf,
    shot: Option<render::LensShot>,
    sensor: Option<render::SensorShot>,
    library_root: PathBuf,
    /// Where the derived file goes: the original's own folder.
    folder: PathBuf,
    /// The original's file stem, which the derived name is built from.
    stem: String,
}

/// Reads everything needed to derive from a version, without decoding or
/// rendering anything itself.
pub(crate) fn plan_derive(
    catalog: &Catalog,
    library_root: &Path,
    version: VersionId,
) -> Result<DerivePlan> {
    let asset = catalog.version_asset(version)?;
    let head = catalog.version_head(version)?;
    let develop = Settings::parse(&catalog.revision(head)?.settings_json)?;
    let source = crate::roots::locate(catalog, library_root, asset)?;
    let meta = catalog.metadata(asset)?;
    let folder = source
        .parent()
        .ok_or_else(|| {
            LeylineError::InvalidSettings(format!(
                "{} has no parent folder to write a derived file into",
                source.display()
            ))
        })?
        .to_path_buf();
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| {
            LeylineError::InvalidSettings(format!(
                "{} has no usable file name stem",
                source.display()
            ))
        })?
        .to_owned();
    Ok(DerivePlan {
        asset,
        develop,
        source,
        shot: meta.as_ref().and_then(render::lens_shot),
        sensor: meta.as_ref().and_then(render::sensor_shot),
        library_root: library_root.to_path_buf(),
        folder,
        stem,
    })
}

/// Decodes, develops up to `DERIVE_RANK` and runs the processor — the slow
/// half, holding no catalog lock.
pub(crate) fn exchange(
    plan: &DerivePlan,
    source: &ProcessorSource,
    operation: &str,
) -> Result<Exchange> {
    let camera_profile = crate::camera_profile::resolve_from_settings(
        &plan.library_root,
        &plan.develop,
        &plan.source,
    )?;
    let decode_params = crate::stages::decode_params(&plan.develop, false);
    let native_depth = crate::stages::native_bit_depth(&plan.develop);
    let decoded =
        crate::source::decode(&plan.source, &decode_params, native_depth).map_err(|e| {
            LeylineError::DecodeFailed {
                asset: plan.asset,
                reason: e.to_string(),
            }
        })?;
    let lut = crate::lut::resolve_from_settings(&plan.library_root, &plan.develop)?;
    let coverages = crate::mask_coverage::resolve_from_settings(&plan.library_root, &plan.develop)?;
    let (width, height, samples) = crate::stages::develop_until_rank(
        &decoded,
        &plan.develop,
        plan.shot.as_ref(),
        plan.sensor.as_ref(),
        camera_profile.as_ref(),
        lut.as_ref(),
        &coverages,
        crate::source::color(&plan.source),
        crate::stages::DERIVE_RANK,
    )?;
    leyline_derive::process(
        source,
        operation,
        &Exchange {
            width,
            height,
            samples,
        },
    )
    .map_err(|e| LeylineError::InvalidImage(e.to_string()))
}

/// The develop state a derived asset starts from: its parent's, minus what
/// the exchange file already holds (ADR 0107 §5).
///
/// Three changes and no others, each of them load-bearing:
///
/// * `input` moves to the version that understands a buffer already in the
///   working space, and says so;
/// * `camera_profile` goes, because it is baked into the pixels and
///   applying it twice is not a subtle error;
/// * the profiled noise stages go, because running a wavelet denoiser on
///   top of a neural one is denoising twice — and because they read the
///   sensor's own samples, which the derived file no longer holds.
///
/// Everything else is copied verbatim, which is the entire point: the
/// derived file replaces the decode, not the development.
pub(crate) fn derived_settings(parent: &Settings) -> Settings {
    let mut settings = parent.clone();
    settings.source_encoding = SourceEncoding::LinearWorkspace;
    settings
        .stages
        .insert("input".to_owned(), DERIVED_INPUT_VERSION);
    settings.camera_profile = None;
    settings.noise_reduction = leyline_core::NoiseReduction::default();
    settings
}

/// The `input` version a derived asset is written at — the first one that
/// understands [`SourceEncoding::LinearWorkspace`].
const DERIVED_INPUT_VERSION: u16 = 5;

/// The file name a derivation writes: `IMG_0001-denoise.tif`.
pub(crate) fn derived_name(stem: &str, operation: &str) -> String {
    format!("{stem}-{operation}.tif")
}

/// Writes the processor's answer beside the original and registers it.
///
/// Never overwrites: a name already taken is refused, the rule ADR 0100 set
/// for renaming and the export path has followed since it existed.
///
/// The file is written first and removed again if the catalog refuses it, so
/// the two never disagree in the direction that hurts: a row pointing at
/// nothing. The other direction — a stray file and no row — would be
/// survivable, and is avoided anyway, because it would make the *next*
/// derivation fail on a name nothing in the library knows about.
pub(crate) fn register(
    catalog: &mut Catalog,
    plan: &DerivePlan,
    operation: &str,
    answer: &Exchange,
) -> Result<AssetId> {
    let destination = plan.folder.join(derived_name(&plan.stem, operation));
    if destination.exists() {
        return Err(LeylineError::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "{} already exists; a derivation never overwrites",
                destination.display()
            ),
        )));
    }
    leyline_derive::write_exchange(&destination, answer)
        .map_err(|e| LeylineError::InvalidImage(e.to_string()))?;

    match register_written(catalog, plan, &destination, answer) {
        Ok(asset) => Ok(asset),
        Err(error) => {
            // The catalog refused, so the file it would have described has
            // no reason to stay: leaving it would make the next derivation
            // fail on a name nothing in the library knows about.
            let _ = std::fs::remove_file(&destination);
            Err(error)
        }
    }
}

/// Registers a file already written at `destination`.
fn register_written(
    catalog: &mut Catalog,
    plan: &DerivePlan,
    destination: &Path,
    answer: &Exchange,
) -> Result<AssetId> {
    let (root_id, relative) = catalog.asset_location(plan.asset)?;
    let folder_path = relative.rsplit_once('/').map_or("", |(folder, _)| folder);
    let folder = catalog.ensure_folder_in(root_id, folder_path)?;
    let filename = destination
        .file_name()
        .and_then(|n| n.to_str())
        .expect("the name was built from a UTF-8 stem")
        .to_owned();
    let bytes = std::fs::metadata(destination)?.len();
    let checksum = *blake3::hash(&std::fs::read(destination)?).as_bytes();

    let details = catalog.asset_details(plan.asset)?;
    let registered = catalog.add_asset(
        &NewAsset {
            folder,
            filename,
            extension: "tif".to_owned(),
            media_type: leyline_core::MediaType::Tiff,
            file_size: bytes,
            checksum,
            width: Some(answer.width),
            height: Some(answer.height),
            // The same shot, so the same instant. Re-reading it from a TIFF
            // this program just wrote would answer a question the parent
            // has already answered better.
            capture_date: details.capture_date,
            capture_offset_minutes: catalog.capture_offset_minutes(plan.asset)?,
        },
        &derived_settings(&plan.develop),
    )?;
    if let Some(metadata) = details.metadata {
        catalog.set_metadata(registered.asset, &metadata)?;
    }
    catalog.set_derived_from(registered.asset, plan.asset)?;
    Ok(registered.asset)
}
