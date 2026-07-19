//! Export orchestration (`docs/engine-api.md` §12).
//!
//! Each version is rendered at its head revision, with the process version
//! it declares (`docs/pipeline.md` §3.3), scaled to the recipe, encoded by
//! `leyline-export`, and journaled in `export_history` (§28). This is the
//! synchronous core the future `Library` facade wraps in a job.

use std::path::{Path, PathBuf};

use leyline_catalog::Catalog;
use leyline_core::{ExportPresetId, LeylineError, Result, Settings, VersionId};
use leyline_export::{ExportError, ExportSettings};
use leyline_preview::Rgb8;
use leyline_raw::DecodeParams;

use crate::render;

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
pub fn export_version(
    catalog: &mut Catalog,
    library_root: &Path,
    version: VersionId,
    settings: &ExportSettings,
    preset: Option<ExportPresetId>,
    destination_dir: &Path,
) -> Result<PathBuf> {
    settings.validate().map_err(export_err)?;
    let asset = catalog.version_asset(version)?;
    let head = catalog.version_head(version)?;
    let develop = Settings::parse(&catalog.revision(head)?.settings_json)?;

    let relative = catalog.asset_relative_path(asset)?;
    let source = library_root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
    let decoded = leyline_raw::decode(&source, &DecodeParams::default()).map_err(|e| {
        LeylineError::DecodeFailed {
            asset,
            reason: e.to_string(),
        }
    })?;
    let rendered = render(&decoded.image, &develop)?;

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

    let stem = Path::new(&relative)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("export");
    let filename = format!("{stem}.{}", settings.format.extension());
    let destination = destination_dir.join(&filename);
    if destination.exists() {
        return Err(LeylineError::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "{} already exists; exports never overwrite",
                destination.display()
            ),
        )));
    }
    std::fs::create_dir_all(destination_dir)?;
    leyline_export::encode(
        &destination,
        output.width(),
        output.height(),
        output.data(),
        settings,
    )
    .map_err(export_err)?;

    catalog.record_export(
        asset,
        preset,
        settings.format.extension(),
        &destination.display().to_string(),
    )?;
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
