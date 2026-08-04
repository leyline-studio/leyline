//! Print orchestration (ADR 0036): "an export with a physical dimension and
//! a destination profile", never a process version, never touching
//! `settings_json`.
//!
//! Each version is rendered at its head revision exactly like an export
//! (decode → `render` → `process1`-`7`), then scaled to fit the printable
//! area in pixels (`PrintSettings::target_pixels`, paper size × DPI, in
//! place of an export's `max_edge`), optionally transformed into a
//! destination ICC profile (`leyline_color::OutputTransform`, ADR 0027),
//! and encoded as a single-page PDF (`leyline_export::encode_print`).
//!
//! There is no `print_history` table and no journal step: unlike an export,
//! a print does not modify a revision or need to be reproduced from the
//! catalog later — it is a one-shot physical artifact, and ADR 0036 is
//! explicit that no state persists beyond the stored preset itself.
//!
//! The hand-off to an actual OS print flow — the risk ADR 0036 names and
//! deliberately defers — is resolved here as: the engine's job ends at a
//! physical-size PDF with the destination profile already baked into the
//! pixels (a portable, print-ready file every OS print dialog accepts);
//! actually invoking that dialog on the rendered file is Studio's job
//! (`docs/adr/0020-menu-bar.md`'s pattern: OS integration lives entirely in
//! `leyline-studio`, no engine surface), left to a follow-up slice.

use std::path::{Path, PathBuf};

use leyline_catalog::Catalog;
use leyline_color::OutputTransform;
use leyline_core::{AssetId, LeylineError, PrintPresetId, Result, Settings, VersionId};
use leyline_export::{ExportError, PrintSettings};
use leyline_preview::Rgb8;

use crate::render;

/// The recipe a [`PrintRequest`] drives a print with: either an ad-hoc set
/// of settings, or a stored preset resolved (and journaled by id) at print
/// time — the same shape [`crate::export::ExportRecipe`] takes (ADR 0025).
#[derive(Debug, Clone, PartialEq)]
pub enum PrintRecipe {
    /// Settings supplied by the caller, not stored anywhere.
    Adhoc(PrintSettings),
    /// A preset stored in the catalog, looked up when the request runs — so
    /// a preset edited after the request was built is picked up at print
    /// time, not when the request was constructed.
    Preset(PrintPresetId),
}

/// One print call: several versions through one recipe, each producing one
/// PDF file in `destination_dir`. `copies` is job data, not part of the
/// recipe/preset (ADR 0036, mirroring `ExportRequest`/`ExportRecipe`).
#[derive(Debug, Clone, PartialEq)]
pub struct PrintRequest {
    /// The versions to print, each at its head revision.
    pub versions: Vec<VersionId>,
    /// The recipe driving every version in this request.
    pub recipe: PrintRecipe,
    /// Directory the rendered PDFs are written into.
    pub destination_dir: PathBuf,
    /// Number of physical copies intended — not enforced by the engine (it
    /// renders one PDF regardless), carried through so a client can pass it
    /// to the OS print dialog's copy count.
    pub copies: u32,
}

/// Everything [`render_print`] needs to decode, develop, scale, transform
/// and encode one version, gathered from the catalog up front so the
/// catalog itself doesn't need to stay locked for the render (ADR 0024's
/// pattern, mirrored from [`crate::export::ExportPlan`]).
pub(crate) struct PrintPlan {
    asset: AssetId,
    develop: Settings,
    source: PathBuf,
    shot: Option<crate::render::LensShot>,
    stem: String,
    /// Library root `develop.camera_profile`'s path (if any) is relative
    /// to — resolved in [`render_print`], mirroring
    /// [`crate::export::ExportPlan`].
    library_root: PathBuf,
}

/// Reads everything needed to render a version, without decoding or
/// rendering anything itself (ADR 0024).
pub(crate) fn plan_print(
    catalog: &Catalog,
    library_root: &Path,
    version: VersionId,
) -> Result<PrintPlan> {
    let asset = catalog.version_asset(version)?;
    let head = catalog.version_head(version)?;
    let develop = Settings::parse(&catalog.revision(head)?.settings_json)?;

    let relative = catalog.asset_relative_path(asset)?;
    let source = library_root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
    let meta = catalog.metadata(asset)?;
    let shot = meta.as_ref().and_then(render::lens_shot);
    let stem = Path::new(&relative)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("print")
        .to_string();

    Ok(PrintPlan {
        asset,
        develop,
        source,
        shot,
        stem,
        library_root: library_root.to_path_buf(),
    })
}

/// Decodes, develops, scales to the printable area, optionally transforms
/// into a destination ICC profile, and encodes a [`PrintPlan`] as a
/// single-page PDF — the slow half of printing a version, deliberately
/// taking no catalog reference (ADR 0024).
///
/// The file is named after the original (`IMG_0001.CR3` → `IMG_0001.pdf`);
/// an existing file is refused, never overwritten.
pub(crate) fn render_print(
    plan: &PrintPlan,
    settings: &PrintSettings,
    destination_dir: &Path,
) -> Result<PathBuf> {
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
    let coverages =
        crate::mask_coverage::resolve_from_settings(&plan.library_root, &plan.develop)?;
    let rendered = render(
        &decoded,
        &plan.develop,
        plan.shot.as_ref(),
        camera_profile.as_ref(),
        lut.as_ref(),
        &coverages,
        crate::source::color(&plan.source),
    )?;

    let image = Rgb8::new(rendered.width, rendered.height, rendered.data)
        .map_err(|e| LeylineError::InvalidImage(e.to_string()))?;
    let (target_w, target_h) = settings.target_pixels().map_err(print_err)?;
    let scaled = image.scaled_to_fit_box(target_w, target_h);

    let mut pixels = scaled.data().to_vec();
    if let Some(profile_path) = &settings.profile {
        let transform = OutputTransform::load(profile_path, settings.intent)
            .map_err(|e| LeylineError::InvalidSettings(e.to_string()))?;
        transform.apply(&mut pixels);
    }

    let filename = format!("{}.pdf", plan.stem);
    let destination = destination_dir.join(&filename);
    std::fs::create_dir_all(destination_dir)?;
    leyline_export::encode_print(
        &destination,
        scaled.width(),
        scaled.height(),
        &pixels,
        settings,
    )
    .map_err(print_err)?;

    Ok(destination)
}

/// One version a batch printed to disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintedVersion {
    /// The printed version.
    pub version: VersionId,
    /// The written PDF.
    pub path: PathBuf,
}

/// One version a batch could not print, with the human-readable reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedPrint {
    /// The version left unprinted.
    pub version: VersionId,
    /// Why the print failed.
    pub reason: String,
}

/// Outcome of one print batch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrintReport {
    /// Versions printed, in request order.
    pub printed: Vec<PrintedVersion>,
    /// Versions left unprinted, with reasons.
    pub failed: Vec<FailedPrint>,
}

/// Maps encoder errors onto the platform error type.
pub(crate) fn print_err(error: ExportError) -> LeylineError {
    match error {
        ExportError::Io(e) => LeylineError::Io(e),
        ExportError::InvalidImage(m) => LeylineError::InvalidImage(m),
        ExportError::InvalidSettings(m) => LeylineError::InvalidSettings(m),
        ExportError::Encode(m) => LeylineError::InvalidImage(m),
    }
}
