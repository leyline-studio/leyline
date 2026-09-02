//! Contact sheets (ADR 0110): a print whose page holds a grid.
//!
//! Everything here is geometry and pixel copying. The page is described by a
//! whole [`PrintSettings`] (ADR 0036) — paper, orientation, margins, DPI,
//! destination profile, intent — and this module only adds the grid drawn
//! inside its printable area, composes one raster per page, and encodes the
//! pages as a multi-page PDF through the same path a single print takes.
//!
//! No image is decoded, rendered or scaled here: the engine hands over cells
//! already scaled to [`ContactSheetSettings::image_box_pixels`], and the
//! composer copies them where the grid says. That is what makes a contact
//! sheet add no rendering algorithm to the product.

use std::path::Path;

use ab_glyph::{Font, FontRef, Glyph, ScaleFont};
use serde::{Deserialize, Serialize};

use crate::ExportError;
use crate::print::PrintSettings;
use crate::watermark::DEJAVU_SANS;

/// Millimeters per inch, the one conversion this module does over and over.
const MM_PER_INCH: f32 = 25.4;

/// The band reserved under a cell for its caption, as a multiple of the
/// caption's cap height — the leading a single line needs to sit clear of
/// the image above and the cell below.
const CAPTION_LEADING: f32 = 1.5;

/// What a cell's caption says (ADR 0110 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptionSource {
    /// No caption; the whole cell is image.
    None,
    /// The source file's name, without its extension — the one caption that
    /// is a fact rather than a formatting decision (ADR 0110 §5).
    #[default]
    Filename,
}

/// One contact-sheet recipe — and the `settings_json` of a contact-sheet
/// preset (`docs/catalog.md` §45), exactly as [`PrintSettings`] is a print
/// preset's.
///
/// The page is a whole [`PrintSettings`], not a restatement of it: paper,
/// margins and DPI are described in one place in this codebase (ADR 0110 §1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContactSheetSettings {
    /// The page the grid is drawn on (ADR 0036).
    pub page: PrintSettings,
    /// Cells per row.
    pub columns: u32,
    /// Cells per column.
    pub rows: u32,
    /// Space between two cells, in millimeters.
    pub gutter_mm: f32,
    /// What each cell's caption says.
    pub caption: CaptionSource,
    /// Caption cap height, in millimeters. Zero disables captions as surely
    /// as [`CaptionSource::None`] does.
    pub caption_mm: f32,
}

impl Default for ContactSheetSettings {
    /// A4 portrait, 4 x 5 cells, captioned with the file name — and **200
    /// DPI**, not a print's 300: a sheet is an index read at arm's length,
    /// and the DPI is what its file weighs (ADR 0110 §4).
    fn default() -> Self {
        ContactSheetSettings {
            page: PrintSettings {
                dpi: 200,
                ..PrintSettings::default()
            },
            columns: 4,
            rows: 5,
            gutter_mm: 4.0,
            caption: CaptionSource::Filename,
            caption_mm: 3.0,
        }
    }
}

/// The grid resolved to millimeters, computed once from the settings and
/// never from the photographs (ADR 0110 §3).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Layout {
    /// Top-left of the printable area.
    origin_mm: (f32, f32),
    /// One cell, caption band included.
    cell_mm: (f32, f32),
    /// Space between two cells.
    gutter_mm: f32,
    /// Height of the caption band at the bottom of a cell, possibly zero.
    caption_band_mm: f32,
}

impl ContactSheetSettings {
    /// Parses a preset's `settings_json`.
    ///
    /// Unknown fields are refused, like every other recipe in this crate: a
    /// preset written by a newer engine must not be applied by halves.
    pub fn parse(json: &str) -> Result<ContactSheetSettings, ExportError> {
        let settings: ContactSheetSettings =
            serde_json::from_str(json).map_err(|e| ExportError::InvalidSettings(e.to_string()))?;
        settings.validate()?;
        Ok(settings)
    }

    /// Serializes the recipe to its `settings_json` form.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("settings serialization cannot fail")
    }

    /// Validates the page and the grid, including that a cell is left with a
    /// positive image box once gutters and the caption band are taken out.
    pub fn validate(&self) -> Result<(), ExportError> {
        self.page.validate()?;
        if self.columns == 0 || self.rows == 0 {
            return Err(ExportError::InvalidSettings(
                "a contact sheet needs at least one column and one row".to_owned(),
            ));
        }
        if self.gutter_mm < 0.0 || self.caption_mm < 0.0 {
            return Err(ExportError::InvalidSettings(
                "gutter and caption sizes are millimeters, never negative".to_owned(),
            ));
        }
        self.layout()?;
        Ok(())
    }

    /// How many photographs one page holds.
    pub fn cells_per_page(&self) -> u32 {
        self.columns.saturating_mul(self.rows)
    }

    /// How many pages `count` photographs fill.
    pub fn pages_for(&self, count: usize) -> usize {
        let per_page = self.cells_per_page() as usize;
        count.div_ceil(per_page.max(1))
    }

    /// The whole page in pixels at the sheet's DPI — the raster the composer
    /// fills and the encoder writes.
    pub fn page_pixels(&self) -> Result<(u32, u32), ExportError> {
        let (w, h) = self.page.page_mm();
        Ok((self.mm_to_span(w), self.mm_to_span(h)))
    }

    /// The box one photograph is scaled to fit inside, in pixels — a cell
    /// minus its caption band. This is a contact sheet's `max_edge`: the
    /// engine scales each render into it and hands the result to
    /// [`compose_page`].
    pub fn image_box_pixels(&self) -> Result<(u32, u32), ExportError> {
        let layout = self.layout()?;
        Ok((
            self.mm_to_span(layout.cell_mm.0),
            self.mm_to_span(layout.cell_mm.1 - layout.caption_band_mm),
        ))
    }

    /// Millimeters to pixels at this sheet's DPI — a *position*, which is
    /// zero at the page's edge.
    fn mm_to_px(&self, mm: f32) -> u32 {
        ((mm / MM_PER_INCH) * self.page.dpi as f32).round().max(0.0) as u32
    }

    /// Millimeters to pixels at this sheet's DPI — a *length*, which is never
    /// zero: a cell one pixel tall is degenerate, a cell zero pixels tall is
    /// a division by nothing further down.
    fn mm_to_span(&self, mm: f32) -> u32 {
        self.mm_to_px(mm).max(1)
    }

    /// Whether captions are drawn at all.
    fn captions_on(&self) -> bool {
        self.caption != CaptionSource::None && self.caption_mm > 0.0
    }

    /// Resolves the grid, refusing a page the grid does not fit on.
    fn layout(&self) -> Result<Layout, ExportError> {
        let (printable_w, printable_h) = self.page.printable_area_mm()?;
        let gutters_w = self.gutter_mm * (self.columns - 1) as f32;
        let gutters_h = self.gutter_mm * (self.rows - 1) as f32;
        let cell_w = (printable_w - gutters_w) / self.columns as f32;
        let cell_h = (printable_h - gutters_h) / self.rows as f32;
        let caption_band_mm = if self.captions_on() {
            self.caption_mm * CAPTION_LEADING
        } else {
            0.0
        };
        if cell_w <= 0.0 || cell_h - caption_band_mm <= 0.0 {
            return Err(ExportError::InvalidSettings(format!(
                "a {}x{} grid with a {}mm gutter leaves no room on a \
                 {printable_w:.0}x{printable_h:.0}mm printable area",
                self.columns, self.rows, self.gutter_mm
            )));
        }
        Ok(Layout {
            origin_mm: (self.page.margins_mm.left_mm, self.page.margins_mm.top_mm),
            cell_mm: (cell_w, cell_h),
            gutter_mm: self.gutter_mm,
            caption_band_mm,
        })
    }

    /// Top-left corner, in pixels, of the image box of the cell at
    /// `index` on its page — cells flow left to right, then top to bottom.
    fn cell_origin_px(&self, layout: &Layout, index: u32) -> (u32, u32) {
        let column = index % self.columns;
        let row = index / self.columns;
        let x = layout.origin_mm.0 + column as f32 * (layout.cell_mm.0 + layout.gutter_mm);
        let y = layout.origin_mm.1 + row as f32 * (layout.cell_mm.1 + layout.gutter_mm);
        (self.mm_to_px(x), self.mm_to_px(y))
    }
}

/// One photograph placed on a sheet: pixels already scaled to fit
/// [`ContactSheetSettings::image_box_pixels`], and the caption to draw under
/// them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SheetCell<'a> {
    /// Width of `rgb8`, in pixels.
    pub width: u32,
    /// Height of `rgb8`, in pixels.
    pub height: u32,
    /// Interleaved RGB8 samples, `width * height * 3` of them.
    pub rgb8: &'a [u8],
    /// The caption; ignored when the recipe asks for none.
    pub caption: &'a str,
}

/// Composes one page: a white ground, every cell centred in its image box,
/// every caption drawn under it.
///
/// `cells` is in reading order, at most [`ContactSheetSettings::cells_per_page`]
/// of them, and a `None` is a photograph that failed to render — its cell
/// stays empty and the ones after it do **not** move up (ADR 0110 §6).
pub fn compose_page(
    settings: &ContactSheetSettings,
    cells: &[Option<SheetCell<'_>>],
) -> Result<Vec<u8>, ExportError> {
    settings.validate()?;
    let layout = settings.layout()?;
    if cells.len() > settings.cells_per_page() as usize {
        return Err(ExportError::InvalidSettings(format!(
            "{} cells for a page that holds {}",
            cells.len(),
            settings.cells_per_page()
        )));
    }
    let (page_w, page_h) = settings.page_pixels()?;
    let (box_w, box_h) = settings.image_box_pixels()?;
    let mut page = vec![u8::MAX; page_w as usize * page_h as usize * 3];

    for (index, cell) in cells.iter().enumerate() {
        let Some(cell) = cell else {
            continue;
        };
        let expected = cell.width as usize * cell.height as usize * 3;
        if cell.rgb8.len() != expected {
            return Err(ExportError::InvalidImage(format!(
                "cell {index} is {}x{} and needs {expected} samples, got {}",
                cell.width,
                cell.height,
                cell.rgb8.len()
            )));
        }
        let (box_x, box_y) = settings.cell_origin_px(&layout, index as u32);
        let offset_x = box_x + box_w.saturating_sub(cell.width) / 2;
        let offset_y = box_y + box_h.saturating_sub(cell.height) / 2;
        blit(&mut page, page_w, page_h, offset_x, offset_y, cell);

        if settings.captions_on() && !cell.caption.is_empty() {
            // The caption is glued to the **photograph**, not to the foot of
            // the cell: a landscape frame in a tall cell is centred, and a
            // label a third of a page below the thing it labels belongs to
            // nothing. The band reserved at the bottom of the cell is what
            // guarantees there is room for it wherever the image ends.
            draw_caption(
                &mut page,
                page_w,
                page_h,
                (box_x, offset_y + cell.height),
                box_w,
                settings.mm_to_span(settings.caption_mm) as f32,
                cell.caption,
            )?;
        }
    }
    Ok(page)
}

/// Copies a cell's pixels onto the page, clipping whatever falls outside it.
///
/// Clipping rather than refusing: the caller scales to the image box, and a
/// rounding of one pixel must not lose a whole sheet.
fn blit(page: &mut [u8], page_w: u32, page_h: u32, x: u32, y: u32, cell: &SheetCell<'_>) {
    for row in 0..cell.height {
        let target_y = y + row;
        if target_y >= page_h {
            break;
        }
        let width = cell.width.min(page_w.saturating_sub(x));
        if width == 0 {
            break;
        }
        let source = (row as usize * cell.width as usize) * 3;
        let target = (target_y as usize * page_w as usize + x as usize) * 3;
        let bytes = width as usize * 3;
        page[target..target + bytes].copy_from_slice(&cell.rgb8[source..source + bytes]);
    }
}

/// Draws one caption line, centred under a cell's image box, in black.
///
/// The typeface is ADR 0051's embedded one — the same reasoning as the
/// watermark's: a system font would make the same sheet different on another
/// machine. A line too wide for its cell is truncated with an ellipsis,
/// never scaled and never wrapped (ADR 0110 §5).
fn draw_caption(
    page: &mut [u8],
    page_w: u32,
    page_h: u32,
    band_origin: (u32, u32),
    band_width: u32,
    cap_height_px: f32,
    caption: &str,
) -> Result<(), ExportError> {
    let font = FontRef::try_from_slice(DEJAVU_SANS)
        .map_err(|e| ExportError::Encode(format!("embedded font is unusable: {e}")))?;
    // `caption_mm` is a cap height; a font's `px` scale is its em size, which
    // is taller. The conversion goes through the face's own metrics, exactly
    // as the watermark's does (ADR 0051 §3).
    let cap_ratio = {
        let unscaled = font.as_scaled(font.units_per_em().unwrap_or(1000.0));
        (unscaled.ascent() / unscaled.height()).clamp(0.1, 1.0)
    };
    let px = cap_height_px / cap_ratio;
    if px < 1.0 {
        return Ok(());
    }
    let scaled = font.as_scaled(px);
    let text = truncate_to_width(&font, px, caption, band_width as f32);

    let mut glyphs: Vec<Glyph> = Vec::new();
    let mut cursor = 0.0f32;
    let mut previous = None;
    for character in text.chars() {
        let id = font.glyph_id(character);
        if let Some(previous) = previous {
            cursor += scaled.kern(previous, id);
        }
        previous = Some(id);
        glyphs.push(id.with_scale_and_position(px, ab_glyph::point(cursor, scaled.ascent())));
        cursor += scaled.h_advance(id);
    }
    let origin_x = band_origin.0 as f32 + (band_width as f32 - cursor).max(0.0) / 2.0;
    // The band is `CAPTION_LEADING` cap heights tall; the line sits on the
    // baseline that centres it in that band.
    let origin_y = band_origin.1 as f32 + (cap_height_px * (CAPTION_LEADING - 1.0)) / 2.0;

    for glyph in glyphs {
        let Some(outlined) = font.outline_glyph(glyph) else {
            continue; // a space, or a character this face has no outline for
        };
        let bounds = outlined.px_bounds();
        outlined.draw(|gx, gy, coverage| {
            let x = origin_x + bounds.min.x + gx as f32;
            let y = origin_y + bounds.min.y + gy as f32;
            if x < 0.0 || y < 0.0 {
                return;
            }
            let (x, y) = (x as u32, y as u32);
            if x >= page_w || y >= page_h {
                return;
            }
            let alpha = coverage.clamp(0.0, 1.0);
            let offset = (y as usize * page_w as usize + x as usize) * 3;
            for channel in 0..3 {
                let under = f32::from(page[offset + channel]);
                page[offset + channel] = (under * (1.0 - alpha)).round().clamp(0.0, 255.0) as u8;
            }
        });
    }
    Ok(())
}

/// The longest prefix of `text` that fits in `max_width` pixels, with an
/// ellipsis when anything had to go.
fn truncate_to_width(font: &FontRef<'_>, px: f32, text: &str, max_width: f32) -> String {
    let scaled = font.as_scaled(px);
    let width = |s: &str| -> f32 {
        let mut total = 0.0;
        let mut previous = None;
        for character in s.chars() {
            let id = font.glyph_id(character);
            if let Some(previous) = previous {
                total += scaled.kern(previous, id);
            }
            previous = Some(id);
            total += scaled.h_advance(id);
        }
        total
    };
    if width(text) <= max_width {
        return text.to_owned();
    }
    let mut kept: Vec<char> = text.chars().collect();
    while !kept.is_empty() {
        kept.pop();
        let candidate: String = kept.iter().collect::<String>() + "…";
        if width(&candidate) <= max_width {
            return candidate;
        }
    }
    String::new()
}

/// Encodes composed pages as one multi-page PDF, every page the sheet's
/// physical paper size (ADR 0110 §4).
///
/// Each page raster covers the whole page — the margins are already white
/// ground in it — so a page is placed at the origin rather than centred the
/// way [`crate::encode_print`] centres a single photograph. An existing file
/// is refused, never overwritten.
pub fn encode_contact_sheet(
    path: &Path,
    pages: &[Vec<u8>],
    settings: &ContactSheetSettings,
) -> Result<(), ExportError> {
    settings.validate()?;
    if pages.is_empty() {
        return Err(ExportError::InvalidImage(
            "a contact sheet needs at least one page".to_owned(),
        ));
    }
    let (width, height) = settings.page_pixels()?;
    let expected = width as usize * height as usize * 3;
    for (index, page) in pages.iter().enumerate() {
        if page.len() != expected {
            return Err(ExportError::InvalidImage(format!(
                "page {index} is {}x{} and needs {expected} samples, got {}",
                width,
                height,
                page.len()
            )));
        }
    }
    if path.exists() {
        return Err(ExportError::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "{} already exists; contact sheets never overwrite",
                path.display()
            ),
        )));
    }

    let (page_w_mm, page_h_mm) = settings.page.page_mm();
    let mut doc = printpdf::PdfDocument::new("Leyline contact sheet");
    let mut rendered = Vec::with_capacity(pages.len());
    for page in pages {
        let image = printpdf::RawImage {
            pixels: printpdf::RawImageData::U8(page.clone()),
            width: width as usize,
            height: height as usize,
            data_format: printpdf::RawImageFormat::RGB8,
            tag: Vec::new(),
        };
        let id = doc.add_image(&image);
        let ops = vec![printpdf::Op::UseXobject {
            id,
            transform: printpdf::XObjectTransform {
                translate_x: Some(printpdf::Pt(0.0)),
                translate_y: Some(printpdf::Pt(0.0)),
                dpi: Some(settings.page.dpi as f32),
                ..Default::default()
            },
        }];
        rendered.push(printpdf::PdfPage::new(
            printpdf::Mm(page_w_mm),
            printpdf::Mm(page_h_mm),
            ops,
        ));
    }

    let mut warnings = Vec::new();
    let bytes = doc
        .with_pages(rendered)
        .save(&printpdf::PdfSaveOptions::default(), &mut warnings);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Margins, PaperSize};

    fn flat(width: u32, height: u32, color: [u8; 3]) -> Vec<u8> {
        color
            .iter()
            .cycle()
            .take(width as usize * height as usize * 3)
            .copied()
            .collect()
    }

    fn pixel(page: &[u8], page_w: u32, x: u32, y: u32) -> [u8; 3] {
        let offset = (y as usize * page_w as usize + x as usize) * 3;
        [page[offset], page[offset + 1], page[offset + 2]]
    }

    /// A page whose millimeters map to round pixel counts: 254 DPI is ten
    /// pixels per millimeter, so the arithmetic a test asserts on is the
    /// arithmetic a reader can do in their head.
    fn page(width_mm: f32, height_mm: f32) -> PrintSettings {
        PrintSettings {
            paper: PaperSize::Custom {
                width_mm,
                height_mm,
            },
            margins_mm: Margins {
                top_mm: 0.0,
                right_mm: 0.0,
                bottom_mm: 0.0,
                left_mm: 0.0,
            },
            dpi: 254,
            ..PrintSettings::default()
        }
    }

    /// A 3 x 1 grid with no captions and no margins: three 30 x 30 pixel
    /// cells on a 90 x 30 pixel page, easy to point at.
    fn strip() -> ContactSheetSettings {
        ContactSheetSettings {
            page: page(9.0, 3.0),
            columns: 3,
            rows: 1,
            gutter_mm: 0.0,
            caption: CaptionSource::None,
            caption_mm: 0.0,
        }
    }

    #[test]
    fn the_default_grid_is_twenty_cells_on_an_a4_page() {
        let settings = ContactSheetSettings::default();
        assert_eq!(settings.cells_per_page(), 20);
        assert_eq!(settings.pages_for(20), 1);
        assert_eq!(settings.pages_for(21), 2);
        assert_eq!(settings.pages_for(0), 0);
        // 190 x 277mm printable, 4 columns and 5 rows of 4mm gutters, a
        // 4.5mm caption band, at 200 DPI.
        assert_eq!(settings.image_box_pixels().unwrap(), (350, 376));
    }

    #[test]
    fn turning_captions_off_gives_their_band_back_to_the_image() {
        let captioned = ContactSheetSettings::default();
        let bare = ContactSheetSettings {
            caption: CaptionSource::None,
            ..ContactSheetSettings::default()
        };
        assert_eq!(
            captioned.image_box_pixels().unwrap().0,
            bare.image_box_pixels().unwrap().0
        );
        assert!(bare.image_box_pixels().unwrap().1 > captioned.image_box_pixels().unwrap().1);
    }

    #[test]
    fn a_grid_that_does_not_fit_its_page_is_refused_by_name() {
        let settings = ContactSheetSettings {
            gutter_mm: 100.0,
            ..ContactSheetSettings::default()
        };
        assert!(matches!(
            settings.validate(),
            Err(ExportError::InvalidSettings(_))
        ));
        let empty = ContactSheetSettings {
            columns: 0,
            ..ContactSheetSettings::default()
        };
        assert!(matches!(
            empty.validate(),
            Err(ExportError::InvalidSettings(_))
        ));
        let caption_taller_than_the_cell = ContactSheetSettings {
            caption_mm: 500.0,
            ..ContactSheetSettings::default()
        };
        assert!(matches!(
            caption_taller_than_the_cell.validate(),
            Err(ExportError::InvalidSettings(_))
        ));
    }

    #[test]
    fn settings_round_trip_and_refuse_newer_fields() {
        let settings = ContactSheetSettings {
            columns: 6,
            rows: 8,
            gutter_mm: 2.5,
            ..ContactSheetSettings::default()
        };
        assert_eq!(
            ContactSheetSettings::parse(&settings.to_json()).unwrap(),
            settings
        );
        assert!(matches!(
            ContactSheetSettings::parse(r#"{"columns":2,"crop_to_fill":true}"#),
            Err(ExportError::InvalidSettings(_))
        ));
        // The page is nested, never flattened (ADR 0110 §1).
        assert!(settings.to_json().contains(r#""page":{"#));
    }

    #[test]
    fn a_cell_that_failed_to_render_keeps_its_place() {
        let settings = strip();
        let (box_w, box_h) = settings.image_box_pixels().unwrap();
        let red = flat(box_w, box_h, [255, 0, 0]);
        let green = flat(box_w, box_h, [0, 255, 0]);
        let cells = vec![
            Some(SheetCell {
                width: box_w,
                height: box_h,
                rgb8: &red,
                caption: "",
            }),
            None,
            Some(SheetCell {
                width: box_w,
                height: box_h,
                rgb8: &green,
                caption: "",
            }),
        ];
        let page = compose_page(&settings, &cells).unwrap();
        let (page_w, _) = settings.page_pixels().unwrap();
        assert_eq!(pixel(&page, page_w, 1, 1), [255, 0, 0], "first cell");
        assert_eq!(
            pixel(&page, page_w, box_w + 1, 1),
            [255, 255, 255],
            "the hole left by the failure stays white"
        );
        assert_eq!(
            pixel(&page, page_w, 2 * box_w + 1, 1),
            [0, 255, 0],
            "the third photograph is still in the third cell"
        );
    }

    #[test]
    fn a_photograph_smaller_than_its_cell_is_centred_and_never_stretched() {
        let settings = strip();
        let (box_w, box_h) = settings.image_box_pixels().unwrap();
        let (small_w, small_h) = (box_w / 3, box_h / 3);
        let blue = flat(small_w, small_h, [0, 0, 255]);
        let page = compose_page(
            &settings,
            &[Some(SheetCell {
                width: small_w,
                height: small_h,
                rgb8: &blue,
                caption: "",
            })],
        )
        .unwrap();
        let (page_w, _) = settings.page_pixels().unwrap();
        let x = (box_w - small_w) / 2;
        let y = (box_h - small_h) / 2;
        assert_eq!(pixel(&page, page_w, x, y), [0, 0, 255], "top-left corner");
        assert_eq!(
            pixel(&page, page_w, x - 1, y),
            [255, 255, 255],
            "the letterbox around it stays paper"
        );
    }

    #[test]
    fn more_cells_than_the_page_holds_is_a_caller_error() {
        let settings = strip();
        let (box_w, box_h) = settings.image_box_pixels().unwrap();
        let pixels = flat(box_w, box_h, [1, 2, 3]);
        let cell = SheetCell {
            width: box_w,
            height: box_h,
            rgb8: &pixels,
            caption: "",
        };
        assert!(matches!(
            compose_page(&settings, &[Some(cell); 4]),
            Err(ExportError::InvalidSettings(_))
        ));
    }

    #[test]
    fn a_malformed_cell_buffer_is_refused() {
        let settings = strip();
        assert!(matches!(
            compose_page(
                &settings,
                &[Some(SheetCell {
                    width: 4,
                    height: 4,
                    rgb8: &[0u8; 7],
                    caption: "",
                })]
            ),
            Err(ExportError::InvalidImage(_))
        ));
    }

    #[test]
    fn a_caption_marks_the_band_under_its_photograph() {
        let settings = ContactSheetSettings {
            page: page(40.0, 30.0),
            columns: 1,
            rows: 1,
            gutter_mm: 0.0,
            caption: CaptionSource::Filename,
            caption_mm: 4.0,
        };
        let (box_w, box_h) = settings.image_box_pixels().unwrap();
        let white = flat(box_w, box_h, [255, 255, 255]);
        let page = compose_page(
            &settings,
            &[Some(SheetCell {
                width: box_w,
                height: box_h,
                rgb8: &white,
                caption: "IMG_0001",
            })],
        )
        .unwrap();
        let (page_w, page_h) = settings.page_pixels().unwrap();
        let band: Vec<u8> = (box_h..page_h)
            .flat_map(|y| (0..box_w).map(move |x| (x, y)))
            .map(|(x, y)| pixel(&page, page_w, x, y)[0])
            .collect();
        assert!(
            band.iter().any(|value| *value < 128),
            "the caption band carries ink"
        );
    }

    #[test]
    fn a_caption_wider_than_its_cell_is_truncated_rather_than_shrunk() {
        let font = FontRef::try_from_slice(DEJAVU_SANS).unwrap();
        let long = "a-very-long-original-file-name-from-a-camera";
        let short = truncate_to_width(&font, 20.0, long, 100.0);
        assert!(short.ends_with('…'));
        assert!(short.chars().count() < long.chars().count());
        // What fits is returned untouched, ellipsis and all.
        assert_eq!(truncate_to_width(&font, 20.0, "IMG_1", 10_000.0), "IMG_1");
    }

    #[test]
    fn encode_writes_one_pdf_page_per_raster_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sheet.pdf");
        let settings = strip();
        let (page_w, page_h) = settings.page_pixels().unwrap();
        let pages = vec![flat(page_w, page_h, [200, 200, 200]); 3];

        encode_contact_sheet(&path, &pages, &settings).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..5], b"%PDF-");
        assert_eq!(
            bytes.windows(11).filter(|w| *w == b"/Type/Page\n").count()
                + bytes.windows(11).filter(|w| *w == b"/Type/Page/").count()
                + bytes.windows(11).filter(|w| *w == b"/Type/Page ").count(),
            3,
            "three pages"
        );

        assert!(matches!(
            encode_contact_sheet(&path, &pages, &settings),
            Err(ExportError::Io(_))
        ));
    }

    #[test]
    fn encode_refuses_pages_that_are_not_the_paper_size() {
        let dir = tempfile::tempdir().unwrap();
        let settings = strip();
        assert!(matches!(
            encode_contact_sheet(&dir.path().join("a.pdf"), &[vec![0u8; 12]], &settings),
            Err(ExportError::InvalidImage(_))
        ));
        assert!(matches!(
            encode_contact_sheet(&dir.path().join("b.pdf"), &[], &settings),
            Err(ExportError::InvalidImage(_))
        ));
    }
}
