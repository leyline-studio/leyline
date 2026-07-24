//! Print settings and rendering (ADR 0036): "an export with a physical
//! dimension and a destination profile". A print preset's `settings_json`
//! is exactly [`PrintSettings`], the same relationship [`crate::ExportSettings`]
//! has to an export preset (`docs/catalog.md` §27).
//!
//! No new render algorithm lives here: the engine decodes/develops/renders
//! exactly as it does for export, then scales to the physical target size
//! this module computes instead of an export's `max_edge`, optionally
//! transforms into a destination ICC profile (`leyline_color::OutputTransform`,
//! ADR 0027), and this module encodes the result as a single-page PDF sized
//! to the physical paper — the most portable hand-off to an OS print flow
//! (ADR 0036's named, deliberately deferred risk; see the engine's `print`
//! module doc comment for why this finalist was chosen).

use std::path::PathBuf;

use leyline_color::RenderingIntent;
use serde::{Deserialize, Serialize};

use crate::ExportError;

/// Paper size the page is cut to. `Custom` covers anything not named here —
/// print shops and consumer printers alike work off physical millimeters,
/// not a closed set of named sizes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PaperSize {
    /// 210 x 297 mm.
    A4,
    /// 297 x 420 mm.
    A3,
    /// 215.9 x 279.4 mm (US Letter).
    Letter,
    /// A caller-specified size in millimeters, portrait orientation.
    Custom {
        /// Portrait-orientation width, in millimeters.
        width_mm: f32,
        /// Portrait-orientation height, in millimeters.
        height_mm: f32,
    },
}

impl PaperSize {
    /// Portrait-orientation `(width_mm, height_mm)`.
    fn portrait_mm(self) -> (f32, f32) {
        match self {
            PaperSize::A4 => (210.0, 297.0),
            PaperSize::A3 => (297.0, 420.0),
            PaperSize::Letter => (215.9, 279.4),
            PaperSize::Custom {
                width_mm,
                height_mm,
            } => (width_mm, height_mm),
        }
    }
}

/// Page orientation, applied on top of [`PaperSize::portrait_mm`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    /// Height is the paper's longer edge.
    Portrait,
    /// Width is the paper's longer edge.
    Landscape,
}

/// Page margins, in millimeters, one per edge.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Margins {
    /// Top margin, in millimeters.
    pub top_mm: f32,
    /// Right margin, in millimeters.
    pub right_mm: f32,
    /// Bottom margin, in millimeters.
    pub bottom_mm: f32,
    /// Left margin, in millimeters.
    pub left_mm: f32,
}

impl Default for Margins {
    /// A modest 10mm border on every edge.
    fn default() -> Self {
        Margins {
            top_mm: 10.0,
            right_mm: 10.0,
            bottom_mm: 10.0,
            left_mm: 10.0,
        }
    }
}

/// One print recipe — and the `settings_json` of a print preset (ADR 0036,
/// parallel to [`crate::ExportSettings`]/`docs/catalog.md` §27).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PrintSettings {
    /// Paper cut size.
    pub paper: PaperSize,
    /// Page orientation.
    pub orientation: Orientation,
    /// Page margins.
    pub margins_mm: Margins,
    /// Target print resolution, in dots (pixels) per inch.
    pub dpi: u32,
    /// Destination ICC profile (printer/paper combination) to transform
    /// into before encoding. `None` leaves the pixels in sRGB, the render
    /// pipeline's working space.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<PathBuf>,
    /// Rendering intent used when `profile` is set.
    pub intent: RenderingIntent,
}

impl Default for PrintSettings {
    /// A4 portrait, 10mm margins, 300 DPI, no destination profile (sRGB).
    fn default() -> Self {
        PrintSettings {
            paper: PaperSize::A4,
            orientation: Orientation::Portrait,
            margins_mm: Margins::default(),
            dpi: 300,
            profile: None,
            intent: RenderingIntent::default(),
        }
    }
}

impl PrintSettings {
    /// Parses a preset's `settings_json`.
    ///
    /// Unknown fields are refused: a preset written by a newer engine must
    /// not be applied partially (the philosophy of `docs/pipeline.md` §3.4).
    pub fn parse(json: &str) -> Result<PrintSettings, ExportError> {
        let settings: PrintSettings =
            serde_json::from_str(json).map_err(|e| ExportError::InvalidSettings(e.to_string()))?;
        settings.validate()?;
        Ok(settings)
    }

    /// Serializes the recipe to its `settings_json` form.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("settings serialization cannot fail")
    }

    /// Validates value ranges, including that the margins leave a positive
    /// printable area.
    pub fn validate(&self) -> Result<(), ExportError> {
        if self.dpi == 0 {
            return Err(ExportError::InvalidSettings(
                "dpi must be strictly positive".to_owned(),
            ));
        }
        self.printable_area_mm()?;
        Ok(())
    }

    /// The oriented page size, `(width_mm, height_mm)`.
    pub fn page_mm(&self) -> (f32, f32) {
        let (w, h) = self.paper.portrait_mm();
        match self.orientation {
            Orientation::Portrait => (w, h),
            Orientation::Landscape => (h, w),
        }
    }

    /// The printable area inside the margins, `(width_mm, height_mm)`.
    pub fn printable_area_mm(&self) -> Result<(f32, f32), ExportError> {
        let (page_w, page_h) = self.page_mm();
        let width = page_w - self.margins_mm.left_mm - self.margins_mm.right_mm;
        let height = page_h - self.margins_mm.top_mm - self.margins_mm.bottom_mm;
        if width <= 0.0 || height <= 0.0 {
            return Err(ExportError::InvalidSettings(format!(
                "margins leave no printable area on a {page_w}x{page_h}mm page"
            )));
        }
        Ok((width, height))
    }

    /// The printable area, in pixels at [`PrintSettings::dpi`] — the box
    /// the engine scales the rendered image to fit inside instead of an
    /// export's `max_edge` (ADR 0036).
    pub fn target_pixels(&self) -> Result<(u32, u32), ExportError> {
        let (width_mm, height_mm) = self.printable_area_mm()?;
        let mm_to_px = |mm: f32| -> u32 { ((mm / 25.4) * self.dpi as f32).round().max(1.0) as u32 };
        Ok((mm_to_px(width_mm), mm_to_px(height_mm)))
    }
}

fn mm_to_pt(mm: f32) -> printpdf::Pt {
    printpdf::Mm(mm).into_pt()
}

fn px_to_pt(px: u32, dpi: u32) -> printpdf::Pt {
    printpdf::Pt(px as f32 * 72.0 / dpi as f32)
}

/// Encodes a rendered, already-scaled RGB8 image as a single-page PDF sized
/// to `settings`' physical paper, the image centered in the printable area.
///
/// The pixels are written as-is: any scaling to [`PrintSettings::target_pixels`]
/// and any ICC destination transform already happened. An existing file is
/// refused, never overwritten — the same never-overwrite rule an export
/// applies.
pub fn encode_print(
    path: &std::path::Path,
    width: u32,
    height: u32,
    rgb8: &[u8],
    settings: &PrintSettings,
) -> Result<(), ExportError> {
    settings.validate()?;
    let expected = width as usize * height as usize * 3;
    if width == 0 || height == 0 || rgb8.len() != expected {
        return Err(ExportError::InvalidImage(format!(
            "{width}x{height} RGB needs {expected} samples, got {}",
            rgb8.len()
        )));
    }
    if path.exists() {
        return Err(ExportError::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("{} already exists; prints never overwrite", path.display()),
        )));
    }

    let (page_w_mm, page_h_mm) = settings.page_mm();
    let (printable_w_mm, printable_h_mm) = settings.printable_area_mm()?;

    let image = printpdf::RawImage {
        pixels: printpdf::RawImageData::U8(rgb8.to_vec()),
        width: width as usize,
        height: height as usize,
        data_format: printpdf::RawImageFormat::RGB8,
        tag: Vec::new(),
    };

    let mut doc = printpdf::PdfDocument::new("Leyline print");
    let image_id = doc.add_image(&image);

    let image_w_pt = px_to_pt(width, settings.dpi);
    let image_h_pt = px_to_pt(height, settings.dpi);
    let printable_w_pt = mm_to_pt(printable_w_mm);
    let printable_h_pt = mm_to_pt(printable_h_mm);
    let left_pt = mm_to_pt(settings.margins_mm.left_mm);
    let bottom_pt = mm_to_pt(settings.margins_mm.bottom_mm);

    let translate_x = printpdf::Pt(left_pt.0 + (printable_w_pt.0 - image_w_pt.0) / 2.0);
    let translate_y = printpdf::Pt(bottom_pt.0 + (printable_h_pt.0 - image_h_pt.0) / 2.0);

    let page_ops = vec![printpdf::Op::UseXobject {
        id: image_id,
        transform: printpdf::XObjectTransform {
            translate_x: Some(translate_x),
            translate_y: Some(translate_y),
            dpi: Some(settings.dpi as f32),
            ..Default::default()
        },
    }];
    let page = printpdf::PdfPage::new(printpdf::Mm(page_w_mm), printpdf::Mm(page_h_mm), page_ops);

    let mut warnings = Vec::new();
    let bytes = doc
        .with_pages(vec![page])
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

    fn gradient(width: u32, height: u32) -> Vec<u8> {
        let mut data = Vec::new();
        for y in 0..height {
            for x in 0..width {
                data.extend_from_slice(&[(x * 20) as u8, (y * 20) as u8, 128]);
            }
        }
        data
    }

    #[test]
    fn a4_portrait_target_pixels_at_300_dpi() {
        let settings = PrintSettings::default();
        // 210 - 20 = 190mm wide, 297 - 20 = 277mm tall, at 300dpi.
        assert_eq!(settings.target_pixels().unwrap(), (2244, 3272));
    }

    #[test]
    fn landscape_swaps_the_oriented_page_size() {
        let settings = PrintSettings {
            orientation: Orientation::Landscape,
            ..PrintSettings::default()
        };
        assert_eq!(settings.page_mm(), (297.0, 210.0));
    }

    #[test]
    fn margins_that_exceed_the_page_are_rejected() {
        let settings = PrintSettings {
            margins_mm: Margins {
                top_mm: 200.0,
                right_mm: 200.0,
                bottom_mm: 200.0,
                left_mm: 200.0,
            },
            ..PrintSettings::default()
        };
        assert!(matches!(
            settings.validate(),
            Err(ExportError::InvalidSettings(_))
        ));
    }

    #[test]
    fn settings_round_trip_and_refuse_newer_fields() {
        let settings = PrintSettings {
            paper: PaperSize::Custom {
                width_mm: 100.0,
                height_mm: 150.0,
            },
            dpi: 720,
            ..PrintSettings::default()
        };
        assert_eq!(PrintSettings::parse(&settings.to_json()).unwrap(), settings);
        assert!(matches!(
            PrintSettings::parse(r#"{"paper":"a4","copies":3}"#),
            Err(ExportError::InvalidSettings(_))
        ));
    }

    #[test]
    fn encode_print_writes_a_pdf_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.pdf");
        let pixels = gradient(8, 6);
        let settings = PrintSettings::default();

        encode_print(&path, 8, 6, &pixels, &settings).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..5], b"%PDF-", "PDF header");

        assert!(matches!(
            encode_print(&path, 8, 6, &pixels, &settings),
            Err(ExportError::Io(_))
        ));
    }

    #[test]
    fn encode_print_rejects_malformed_buffers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.pdf");
        assert!(matches!(
            encode_print(&path, 8, 6, &[0u8; 10], &PrintSettings::default()),
            Err(ExportError::InvalidImage(_))
        ));
    }
}
