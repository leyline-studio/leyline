//! `reproduce`: does an export still come out the same? (ADR 0153 §5)
//!
//! The journal row `verify` finds names a revision and the resolved recipe
//! applied to it. Rendering that pair again and comparing is the §5.1
//! promise put to the test on one real file — and when it fails, the verdict
//! names the one condition the journal lets us check (the decoder) and stops
//! there rather than guessing.

use std::path::{Path, PathBuf};

use image::DynamicImage;
use leyline_catalog::{Catalog, ExportRecord};
use leyline_core::{LeylineError, Result};
use leyline_export::{ExportFormat, ExportSettings};

use crate::ExportVerdict;
use crate::export::{ExportPlan, export_err, plan_export_at, render_export};

/// What [`crate::Library::reproduce_export`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReproduceVerdict {
    /// The journal row rendered again.
    pub record: ExportRecord,
    /// How the file asked about matched that row, as `verify` would say it.
    pub matched: ExportMatch,
    /// The file the render wrote, under the destination directory.
    pub reproduced: PathBuf,
    /// What the comparison says.
    pub outcome: Reproduction,
}

/// How the file asked about led to its journal row (ADR 0153 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportMatch {
    /// Its bytes are the ones the export wrote.
    Intact,
    /// Only its `leyline:Revision` matched: it was changed after the export.
    Modified,
}

/// The comparison between the export and its render today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reproduction {
    /// The render is byte for byte the file the export wrote: §5.1 kept.
    BitForBit,
    /// The pixels are the same, the bytes are not: the photograph's metadata
    /// changed since the export (ADR 0157 §4 made it an input). Not a failure
    /// of the promise.
    SamePixels,
    /// The bytes differ and the pixels cannot be compared: AVIF has no
    /// decoder in the tree.
    PixelsNotCompared,
    /// The pixels differ.
    DifferentPixels(DifferenceSuspect),
}

/// The §5.1 condition a pixel difference points at, as far as it can be known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DifferenceSuspect {
    /// The decoder recorded at export time is not the one loaded now.
    Decoder {
        /// `leyline_raw::decoder_version()` when the file was exported.
        recorded: String,
        /// The one loaded now.
        loaded: String,
    },
    /// The file was changed after the export (re-saved, re-compressed), so its
    /// pixels are not the export's own and say nothing about the promise.
    FileModified,
    /// Same decoder, untouched file: the platform or the toolchain is what is
    /// left, and the verdict does not guess further.
    PlatformOrToolchain,
}

/// Everything the render needs, read under the catalog lock.
pub(crate) struct ReproducePlan {
    record: ExportRecord,
    matched: ExportMatch,
    export: ExportPlan,
    settings: ExportSettings,
}

/// The journal row behind `verdict`, turned into a render plan of the
/// revision it recorded. `path` only names the file in a refusal.
pub(crate) fn plan(
    catalog: &Catalog,
    library_root: &Path,
    path: &Path,
    verdict: ExportVerdict,
) -> Result<ReproducePlan> {
    let refuse = |reason: &str| LeylineError::NotReproducible {
        path: path.to_path_buf(),
        reason: reason.to_owned(),
    };
    let (record, matched) = match verdict {
        ExportVerdict::Intact(record) => (record, ExportMatch::Intact),
        ExportVerdict::Modified(record) => (record, ExportMatch::Modified),
        ExportVerdict::Unknown => return Err(refuse("this library never exported it")),
    };
    let (Some(version), Some(revision)) = (record.version, record.revision) else {
        return Err(refuse("the version it was exported from has been deleted"));
    };
    // A row found by checksum or fingerprint was written after migration
    // v14, which always fills the recipe.
    let recipe = record
        .export_settings_json
        .as_deref()
        .ok_or_else(|| refuse("its journal row holds no recipe"))?;
    let settings = ExportSettings::parse(recipe).map_err(export_err)?;
    let export = plan_export_at(catalog, library_root, version, revision)?;
    Ok(ReproducePlan {
        record,
        matched,
        export,
        settings,
    })
}

/// Renders `plan` into `destination_dir` and compares the result with the
/// file asked about. Holds no catalog lock (ADR 0024), and journals nothing:
/// a reproduction checks an export, it is not one.
pub(crate) fn reproduce(
    plan: ReproducePlan,
    asked: &Path,
    destination_dir: &Path,
) -> Result<ReproduceVerdict> {
    let reproduced = render_export(&plan.export, &plan.settings, destination_dir)?;
    let checksum = crate::provenance::file_checksum(&reproduced)?;
    let outcome = if Some(checksum) == plan.record.output_checksum {
        Reproduction::BitForBit
    } else {
        compare_pixels(&plan, asked, &reproduced)?
    };
    Ok(ReproduceVerdict {
        record: plan.record,
        matched: plan.matched,
        reproduced,
        outcome,
    })
}

fn compare_pixels(plan: &ReproducePlan, asked: &Path, reproduced: &Path) -> Result<Reproduction> {
    let format = plan.settings.format;
    let (Some(before), Some(after)) = (pixels(asked, format)?, pixels(reproduced, format)?) else {
        return Ok(Reproduction::PixelsNotCompared);
    };
    if before == after {
        return Ok(Reproduction::SamePixels);
    }
    let loaded = leyline_raw::decoder_version();
    let suspect = match plan.record.decoder.as_deref() {
        Some(recorded) if recorded != loaded => DifferenceSuspect::Decoder {
            recorded: recorded.to_owned(),
            loaded: loaded.to_owned(),
        },
        _ if plan.matched == ExportMatch::Modified => DifferenceSuspect::FileModified,
        _ => DifferenceSuspect::PlatformOrToolchain,
    };
    Ok(Reproduction::DifferentPixels(suspect))
}

/// The decoded pixels of an exported file, or `None` for AVIF, which has no
/// decoder in the tree.
fn pixels(path: &Path, format: ExportFormat) -> Result<Option<DynamicImage>> {
    let undecodable =
        |e: &dyn std::fmt::Display| LeylineError::InvalidImage(format!("{}: {e}", path.display()));
    match format {
        ExportFormat::Avif => Ok(None),
        ExportFormat::Webp => {
            let reader = std::io::BufReader::new(std::fs::File::open(path)?);
            let mut decoder = image_webp::WebPDecoder::new(reader).map_err(|e| undecodable(&e))?;
            let (width, height) = decoder.dimensions();
            let mut samples = vec![0; decoder.output_buffer_size().unwrap_or(0)];
            decoder
                .read_image(&mut samples)
                .map_err(|e| undecodable(&e))?;
            let image = if decoder.has_alpha() {
                image::RgbaImage::from_raw(width, height, samples).map(DynamicImage::ImageRgba8)
            } else {
                image::RgbImage::from_raw(width, height, samples).map(DynamicImage::ImageRgb8)
            };
            image
                .map(Some)
                .ok_or_else(|| undecodable(&"buffer size does not match its dimensions"))
        }
        ExportFormat::Jpeg | ExportFormat::Png | ExportFormat::Tiff => {
            image::ImageReader::open(path)?
                .with_guessed_format()?
                .decode()
                .map(Some)
                .map_err(|e| undecodable(&e))
        }
    }
}
