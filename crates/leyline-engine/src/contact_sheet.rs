//! Contact-sheet orchestration (ADR 0110): a print whose page holds a grid.
//!
//! Everything below the grid is ADR 0036's print, unchanged. A cell is
//! planned like a print (`plan_print`), rendered like a print
//! (`render_to_box`, the very function [`crate::print::render_print`] calls),
//! and the destination ICC transform is ADR 0027's, applied once per composed
//! page instead of once per photograph. What this module adds is the loop, the
//! pagination, and the decision of what to do when one photograph fails.
//!
//! Two properties are deliberate and tested rather than incidental:
//!
//! * **Pages are rendered and composed one at a time**, so a five hundred
//!   photograph sheet holds one page of pixels in memory, not five hundred
//!   cells.
//! * **A photograph that fails leaves its cell empty** and the sheet is still
//!   written (ADR 0110 §6): grid position is how a person points at a frame,
//!   and shifting the grid to hide a failure silently renumbers every frame
//!   after it.

use std::path::PathBuf;

use leyline_catalog::Catalog;
use leyline_color::OutputTransform;
use leyline_core::{ContactSheetPresetId, LeylineError, Result, VersionId};
use leyline_export::{ContactSheetSettings, SheetCell};
use leyline_preview::Rgb8;

use crate::print::{PrintPlan, plan_print, render_to_box};

/// The recipe a [`ContactSheetRequest`] is driven with — the same shape
/// [`crate::print::PrintRecipe`] takes (ADR 0025/0036).
#[derive(Debug, Clone, PartialEq)]
pub enum ContactSheetRecipe {
    /// Settings supplied by the caller, not stored anywhere.
    Adhoc(ContactSheetSettings),
    /// A preset stored in the catalog, looked up when the request runs.
    Preset(ContactSheetPresetId),
}

/// One contact-sheet call: several versions, one recipe, **one** multi-page
/// PDF — the difference from a print, which writes one file per version
/// (ADR 0110 §2).
#[derive(Debug, Clone, PartialEq)]
pub struct ContactSheetRequest {
    /// The versions to place, in reading order, each at its head revision.
    pub versions: Vec<VersionId>,
    /// The recipe driving the whole sheet.
    pub recipe: ContactSheetRecipe,
    /// The PDF to write. An existing file is refused, never overwritten.
    pub destination: PathBuf,
}

/// Outcome of one contact sheet.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContactSheetReport {
    /// The written PDF.
    pub path: PathBuf,
    /// How many pages it holds.
    pub pages: usize,
    /// How many photographs made it onto the sheet.
    pub placed: usize,
    /// The versions that left an empty cell, with reasons — reusing the
    /// print report's per-version failure (ADR 0110 §6).
    pub failed: Vec<crate::print::FailedPrint>,
}

/// A planned cell: what to render, at what decode size, and what to write
/// under it.
pub(crate) struct CellPlan {
    pub(crate) plan: PrintPlan,
    /// Whether the decoder may hand over half a RAW — true when the cell's
    /// box fits inside half the photograph (ADR 0110 §7).
    pub(crate) half_size: bool,
    /// The caption, already resolved from the recipe's source.
    pub(crate) caption: String,
}

/// Plans one cell: ADR 0036's print plan, plus the two things a cell needs
/// that a print does not — a decode size and a caption.
pub(crate) fn plan_cell(
    catalog: &Catalog,
    library_root: &std::path::Path,
    version: VersionId,
    settings: &ContactSheetSettings,
    box_w: u32,
    box_h: u32,
) -> Result<CellPlan> {
    let plan = plan_print(catalog, library_root, version)?;
    let asset = catalog.version_asset(version)?;
    // The catalog knows the photograph's pixel size; when it does not, the
    // safe answer is the full decode.
    let details = catalog.asset_details(asset)?;
    let half_size = match (details.width, details.height) {
        (Some(width), Some(height)) => box_w * 2 <= width && box_h * 2 <= height,
        _ => false,
    };
    let caption = match settings.caption {
        leyline_export::CaptionSource::None => String::new(),
        leyline_export::CaptionSource::Filename => plan.stem.clone(),
    };
    Ok(CellPlan {
        plan,
        half_size,
        caption,
    })
}

/// Renders one planned cell to the sheet's image box.
pub(crate) fn render_cell(cell: &CellPlan, box_w: u32, box_h: u32) -> Result<Rgb8> {
    render_to_box(&cell.plan, box_w, box_h, cell.half_size)
}

/// Composes one page from its rendered cells and applies the destination
/// profile to it — ADR 0027's transform, once per page rather than once per
/// photograph (ADR 0110 §4).
pub(crate) fn compose_page(
    settings: &ContactSheetSettings,
    cells: &[(Option<Rgb8>, String)],
    transform: Option<&OutputTransform>,
) -> Result<Vec<u8>> {
    let sheet_cells: Vec<Option<SheetCell<'_>>> = cells
        .iter()
        .map(|(image, caption)| {
            image.as_ref().map(|image| SheetCell {
                width: image.width(),
                height: image.height(),
                rgb8: image.data(),
                caption: caption.as_str(),
            })
        })
        .collect();
    let mut page =
        leyline_export::compose_page(settings, &sheet_cells).map_err(crate::print::print_err)?;
    if let Some(transform) = transform {
        transform.apply(&mut page);
    }
    Ok(page)
}

/// Loads the destination profile a sheet asks for, once for the whole run.
pub(crate) fn output_transform(settings: &ContactSheetSettings) -> Result<Option<OutputTransform>> {
    match &settings.page.profile {
        None => Ok(None),
        Some(path) => OutputTransform::load(path, settings.page.intent)
            .map(Some)
            .map_err(|e| LeylineError::InvalidSettings(e.to_string())),
    }
}
