//! Export orchestration (`docs/engine-api.md` §12).
//!
//! Each version is rendered at its head revision, with the process version
//! it declares (`docs/pipeline.md` §3.3), scaled to the recipe, encoded by
//! `leyline-export`, and journaled in `export_history` (§28). This is the
//! synchronous core the future `Library` facade wraps in a job.

use std::path::{Path, PathBuf};

use leyline_catalog::Catalog;
use leyline_core::{AssetId, ExportPresetId, LeylineError, Result, Settings, VersionId};
use leyline_export::{ExportError, ExportSettings};
use leyline_preview::Rgb8;

use crate::render;

/// The recipe an [`ExportRequest`] drives an export with: either an ad-hoc
/// set of settings, or a stored preset resolved (and journaled by id)
/// at export time.
///
/// Not `Eq`/`Copy` since ADR 0051: a watermark carries its text.
#[derive(Debug, Clone, PartialEq)]
pub enum ExportRecipe {
    /// Settings supplied by the caller, not stored anywhere.
    Adhoc(ExportSettings),
    /// A preset stored in the catalog (§27), looked up when the request
    /// runs — so a preset edited after the request was built is picked up
    /// at export time, not when the request was constructed.
    Preset(ExportPresetId),
}

/// One export call (§12): several versions through one recipe, into one
/// destination directory. The single request shape [`Library::export`] and
/// [`Library::export_async`] both take, replacing the former split between
/// an ad-hoc batch and a preset batch.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportRequest {
    /// The versions to export, each at its head revision.
    pub versions: Vec<VersionId>,
    /// The recipe driving every version in this request.
    pub recipe: ExportRecipe,
    /// Directory the rendered files are written into.
    pub destination_dir: PathBuf,
    /// How many photos [`crate::Library::export`] keeps in flight, `None`
    /// for the engine's default (ADR 0068 §1).
    ///
    /// A property of *this run*, not of the recipe — which is why it lives
    /// here and never in a preset's `settings_json`. Raising it trades peak
    /// memory (~0.7 GB per 30 Mpx photo in flight) for wall time; 0 is read
    /// as 1.
    pub concurrency: Option<usize>,
}

/// Everything [`render_export`] needs to decode, develop, scale and encode
/// one version, gathered from the catalog up front so the catalog itself
/// doesn't need to stay locked for the render (ADR 0024, mirroring
/// ADR 0023's preview split).
pub(crate) struct ExportPlan {
    pub(crate) asset: AssetId,
    develop: Settings,
    source: PathBuf,
    shot: Option<crate::render::LensShot>,
    /// What the sensor was, and what it was set to — the profiled denoising
    /// stages' input (ADR 0072). Resolved here, from the same metadata as
    /// `shot`, because the render is a pure function of what it is handed.
    sensor: Option<crate::render::SensorShot>,
    /// Output filename stem, derived from the source's relative path.
    stem: String,
    /// Library root `develop.camera_profile`'s path (if any) is relative
    /// to — resolving it is deferred to [`render_export`] since reading a
    /// file from disk has no place in the catalog-bound half of this split
    /// (ADR 0024).
    library_root: PathBuf,
}

/// Reads everything needed to render a version, without decoding or
/// rendering anything itself — the read-only, catalog-bound half of
/// [`export_version`], split out so a caller can drop the catalog lock
/// before the slow half (ADR 0024).
pub(crate) fn plan_export(
    catalog: &Catalog,
    library_root: &Path,
    version: VersionId,
) -> Result<ExportPlan> {
    let asset = catalog.version_asset(version)?;
    let head = catalog.version_head(version)?;
    let develop = Settings::parse(&catalog.revision(head)?.settings_json)?;

    let relative = catalog.asset_relative_path(asset)?;
    let source = library_root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
    let meta = catalog.metadata(asset)?;
    let shot = meta.as_ref().and_then(render::lens_shot);
    let sensor = meta.as_ref().and_then(render::sensor_shot);
    let stem = Path::new(&relative)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("export")
        .to_string();

    Ok(ExportPlan {
        asset,
        develop,
        source,
        shot,
        sensor,
        stem,
        library_root: library_root.to_path_buf(),
    })
}

/// Decodes, develops, scales and encodes an [`ExportPlan`] to disk — the
/// slow half of [`export_version`], deliberately taking no catalog
/// reference so it can run with no catalog lock held (ADR 0024).
///
/// The file is named after the original (`IMG_0001.CR3` → `IMG_0001.jpg`);
/// an existing file is refused, never overwritten.
pub(crate) fn render_export(
    plan: &ExportPlan,
    settings: &ExportSettings,
    destination_dir: &Path,
) -> Result<PathBuf> {
    let destination = destination_dir.join(output_name(plan, settings));
    refuse_existing(&destination)?;
    render_export_to(plan, settings, &destination)?;
    Ok(destination)
}

/// The file name an export writes for `plan` under `settings`: the source's
/// stem with the format's extension (`IMG_0001.CR3` → `IMG_0001.jpg`).
///
/// Split out so a batch can reserve every name up front, in request order,
/// before any rendering starts — the only way two versions of one asset
/// collide deterministically rather than racing for the same path
/// (ADR 0068 §2).
pub(crate) fn output_name(plan: &ExportPlan, settings: &ExportSettings) -> String {
    format!("{}.{}", plan.stem, settings.format.extension())
}

/// Refuses a destination that already exists — exports never overwrite.
pub(crate) fn refuse_existing(destination: &Path) -> Result<()> {
    if destination.exists() {
        return Err(LeylineError::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "{} already exists; exports never overwrite",
                destination.display()
            ),
        )));
    }
    Ok(())
}

/// [`render_export`] with the destination already decided and checked — the
/// half a concurrent batch runs, holding no catalog lock (ADR 0024) and no
/// knowledge of the other photos in flight (ADR 0068 §3).
pub(crate) fn render_export_to(
    plan: &ExportPlan,
    settings: &ExportSettings,
    destination: &Path,
) -> Result<()> {
    let camera_profile = crate::camera_profile::resolve_from_settings(
        &plan.library_root,
        &plan.develop,
        &plan.source,
    )?;
    let decode_params = crate::stages::decode_params(&plan.develop, false);
    let decoded = crate::source::decode(&plan.source, &decode_params).map_err(|e| {
        LeylineError::DecodeFailed {
            asset: plan.asset,
            reason: e.to_string(),
        }
    })?;
    let lut = crate::lut::resolve_from_settings(&plan.library_root, &plan.develop)?;
    let coverages = crate::mask_coverage::resolve_from_settings(&plan.library_root, &plan.develop)?;
    let rendered = render(
        &decoded,
        &plan.develop,
        plan.shot.as_ref(),
        plan.sensor.as_ref(),
        camera_profile.as_ref(),
        lut.as_ref(),
        &coverages,
        crate::source::color(&plan.source),
    )?;

    let scaled;
    let image = Rgb8::new(rendered.width, rendered.height, rendered.data)
        .map_err(|e| LeylineError::InvalidImage(e.to_string()))?;
    let output = match settings.max_edge {
        Some(edge) if image.width().max(image.height()) > edge => {
            scaled = image.scaled_to_fit(edge);
            &scaled
        }
        _ => &image,
    };

    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    leyline_export::encode(
        destination,
        output.width(),
        output.height(),
        output.data(),
        settings,
    )
    .map_err(export_err)?;

    Ok(())
}

/// Journals a successful export against its asset — the write half of
/// [`export_version`], and the only phase that needs the catalog again
/// after [`render_export`] (ADR 0024).
pub(crate) fn journal_export(
    catalog: &mut Catalog,
    plan: &ExportPlan,
    preset: Option<ExportPresetId>,
    settings: &ExportSettings,
    destination: &Path,
) -> Result<()> {
    catalog.record_export(
        plan.asset,
        preset,
        settings.format.extension(),
        &destination.display().to_string(),
    )
}

/// One version a batch wrote to disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedVersion {
    /// The exported version.
    pub version: VersionId,
    /// The written file.
    pub path: PathBuf,
}

/// One version a batch could not export, with the human-readable reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedExport {
    /// The version left unexported.
    pub version: VersionId,
    /// Why the export failed.
    pub reason: String,
}

/// Outcome of one export batch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExportReport {
    /// Versions exported, in request order.
    pub exported: Vec<ExportedVersion>,
    /// Versions left unexported, with reasons.
    pub failed: Vec<FailedExport>,
}

/// Exports several versions into `destination_dir` with one recipe.
///
/// One failing version does not stop the batch: it is reported in
/// [`ExportReport::failed`] and the others proceed. Two versions of the
/// same asset collide on the output name, so the second one fails — the
/// exports-never-overwrite rule applies inside a batch too. `progress`
/// receives `(done, total)` after each version, the batching contract of
/// `docs/engine-api.md` §12.
pub fn export_batch(
    catalog: &mut Catalog,
    library_root: &Path,
    versions: &[VersionId],
    settings: &ExportSettings,
    preset: Option<ExportPresetId>,
    destination_dir: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<ExportReport> {
    settings.validate().map_err(export_err)?;
    let total = versions.len() as u64;
    let mut report = ExportReport::default();
    for (done, &version) in versions.iter().enumerate() {
        match export_version(
            catalog,
            library_root,
            version,
            settings,
            preset,
            destination_dir,
        ) {
            Ok(path) => report.exported.push(ExportedVersion { version, path }),
            Err(error) => report.failed.push(FailedExport {
                version,
                reason: error.to_string(),
            }),
        }
        progress(done as u64 + 1, total);
    }
    Ok(report)
}

/// Exports one version into `destination_dir` and returns the written file.
///
/// The file is named after the original (`IMG_0001.CR3` → `IMG_0001.jpg`);
/// an existing file is refused, never overwritten. On success the export is
/// journaled with the preset that produced it, if any.
///
/// This free function drives [`plan_export`], [`render_export`] and
/// [`journal_export`] over one `&mut Catalog` held throughout — the pattern
/// this crate's tests use directly. [`crate::Library::export`] instead
/// sequences the same three phases itself, dropping the catalog lock across
/// [`render_export`] (ADR 0024).
pub fn export_version(
    catalog: &mut Catalog,
    library_root: &Path,
    version: VersionId,
    settings: &ExportSettings,
    preset: Option<ExportPresetId>,
    destination_dir: &Path,
) -> Result<PathBuf> {
    settings.validate().map_err(export_err)?;
    let plan = plan_export(catalog, library_root, version)?;
    let destination = render_export(&plan, settings, destination_dir)?;
    journal_export(catalog, &plan, preset, settings, &destination)?;
    Ok(destination)
}

/// Maps encoder errors onto the platform error type.
pub(crate) fn export_err(error: ExportError) -> LeylineError {
    match error {
        ExportError::Io(e) => LeylineError::Io(e),
        ExportError::InvalidImage(m) => LeylineError::InvalidImage(m),
        ExportError::InvalidSettings(m) => LeylineError::InvalidSettings(m),
        ExportError::Encode(m) => LeylineError::InvalidImage(m),
    }
}
