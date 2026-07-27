//! Conversions from engine values into what the UI displays (ADR 0045 §4).
//!
//! The UI never interprets a catalog value itself, so every number reaching it
//! is formatted here first.

use std::rc::Rc;

use crate::app::SORTS;
use leyline_sdk::{ColorLabel, ExportReport, PrintReport, Settings, SkippedFile, Sort};
use slint::{ModelRc, VecModel};

/// One line summing up an import batch for the dialog.
pub(crate) fn import_summary(imported: usize, skipped: &[SkippedFile]) -> String {
    match skipped {
        [] => format!("{imported} imported."),
        [first, ..] => format!(
            "{imported} imported, {} skipped ({}).",
            skipped.len(),
            first.reason
        ),
    }
}

/// One line summing up an export batch for the dialog.
pub(crate) fn export_summary(report: &ExportReport) -> String {
    match (report.exported.first(), report.failed.first()) {
        (Some(done), _) => format!("Exported to {}.", done.path.display()),
        (None, Some(failed)) => format!("Export failed: {}", failed.reason),
        (None, None) => "Nothing to export.".to_owned(),
    }
}

/// One line summing up a print batch for the dialog (ADR 0036).
pub(crate) fn print_summary(report: &PrintReport) -> String {
    match (report.printed.first(), report.failed.first()) {
        (Some(done), _) => format!("Printed to {}.", done.path.display()),
        (None, Some(failed)) => format!("Print failed: {}", failed.reason),
        (None, None) => "Nothing to print.".to_owned(),
    }
}

/// Converts an in-memory RGB8 render into a displayable Slint image, without
/// going through a file — used only for the before/after comparison's
/// "before" half, which [`leyline_sdk::Library::preview_before`]
/// deliberately never writes to the preview cache.
pub(crate) fn rgb8_to_slint_image(image: &leyline_sdk::Rgb8) -> slint::Image {
    let buffer = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::clone_from_slice(
        image.data(),
        image.width(),
        image.height(),
    );
    slint::Image::from_rgb8(buffer)
}

/// The tone-curve graph's canvas size, pixels square — matches the fixed
/// `220px` `Rectangle`/`Path` dimensions in `ui/studio.slint`'s Tone Curve
/// section.
pub(crate) const CURVE_CANVAS_SIZE: f64 = 220.0;

/// Mirrors pipeline settings into the develop slider model.
pub(crate) fn dev_model(settings: &Settings) -> crate::ui::DevSettings {
    let wb = settings.white_balance.clone().unwrap_or_default();
    let crop = settings.crop.clone().unwrap_or(leyline_sdk::Crop {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    });
    crate::ui::DevSettings {
        exposure: settings.exposure as f32,
        contrast: settings.contrast as f32,
        highlights: settings.highlights as f32,
        shadows: settings.shadows as f32,
        whites: settings.whites as f32,
        blacks: settings.blacks as f32,
        clarity: settings.clarity as f32,
        texture: settings.texture as f32,
        dehaze: settings.dehaze as f32,
        highlight_rolloff: settings.output_rendering.highlight_rolloff as f32,
        vibrance: settings.vibrance as f32,
        saturation: settings.saturation as f32,
        wb_temp: wb.temperature as f32,
        wb_tint: wb.tint as f32,
        rotation: settings.rotation as f32,
        crop_left: (crop.x * 100.0) as f32,
        crop_top: (crop.y * 100.0) as f32,
        crop_width: (crop.width * 100.0) as f32,
        crop_height: (crop.height * 100.0) as f32,
        nr_luminance: settings.noise_reduction.luminance as f32,
        nr_color: settings.noise_reduction.color as f32,
        sharpen_amount: settings.sharpening.amount as f32,
        sharpen_radius: settings.sharpening.radius as f32,
        lens_correction: settings.lens_correction.enabled,
        camera_profile: settings
            .camera_profile
            .as_ref()
            .is_some_and(|profile| profile.enabled),
        hsl: ModelRc::from(Rc::new(VecModel::from(
            settings
                .hsl
                .iter()
                .map(|band| crate::ui::HslBandValues {
                    hue: band.hue as f32,
                    saturation: band.saturation as f32,
                    luminance: band.luminance as f32,
                })
                .collect::<Vec<_>>(),
        ))),
        color_grading_shadows: zone_model(&settings.color_grading.shadows),
        color_grading_midtones: zone_model(&settings.color_grading.midtones),
        color_grading_highlights: zone_model(&settings.color_grading.highlights),
        color_grading_balance: settings.color_grading.balance as f32,
        color_grading_blending: settings.color_grading.blending as f32,
    }
}

/// Mirrors one color grading zone into the Slint model.
pub(crate) fn zone_model(
    zone: &leyline_sdk::ColorGradingZone,
) -> crate::ui::ColorGradingZoneValues {
    crate::ui::ColorGradingZoneValues {
        hue: zone.hue as f32,
        saturation: zone.saturation as f32,
        luminance: zone.luminance as f32,
    }
}

/// The label shown on the sort button for a sort order.
pub(crate) fn sort_label(sort: Sort) -> &'static str {
    SORTS
        .iter()
        .find(|(candidate, _)| *candidate == sort)
        .map_or(SORTS[0].1, |(_, label)| label)
}

/// The dot color of a catalog color label.
pub(crate) fn label_color(label: Option<ColorLabel>) -> slint::Color {
    match label {
        Some(ColorLabel::Red) => slint::Color::from_rgb_u8(0xE0, 0x4F, 0x4F),
        Some(ColorLabel::Yellow) => slint::Color::from_rgb_u8(0xE0, 0xC0, 0x4F),
        Some(ColorLabel::Green) => slint::Color::from_rgb_u8(0x6F, 0xBF, 0x6F),
        Some(ColorLabel::Blue) => slint::Color::from_rgb_u8(0x5F, 0x8F, 0xDF),
        Some(ColorLabel::Purple) => slint::Color::from_rgb_u8(0xA0, 0x6F, 0xDF),
        None => slint::Color::default(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use leyline_sdk::{ExportedVersion, FailedExport, VersionId};

    use super::*;

    #[test]
    fn import_summaries_count_and_explain() {
        assert_eq!(import_summary(3, &[]), "3 imported.");
        let skipped = vec![
            SkippedFile {
                path: PathBuf::from("/photos/a.xmp"),
                reason: "unsupported file type".to_owned(),
            },
            SkippedFile {
                path: PathBuf::from("/photos/b.xmp"),
                reason: "unsupported file type".to_owned(),
            },
        ];
        assert_eq!(
            import_summary(1, &skipped),
            "1 imported, 2 skipped (unsupported file type)."
        );
    }

    #[test]
    fn export_summaries_show_the_file_or_the_failure() {
        let mut report = ExportReport::default();
        assert_eq!(export_summary(&report), "Nothing to export.");
        report.failed.push(FailedExport {
            version: VersionId::new(7),
            reason: "no such version".to_owned(),
        });
        assert_eq!(export_summary(&report), "Export failed: no such version");
        report.exported.push(ExportedVersion {
            version: VersionId::new(7),
            path: PathBuf::from("/out/photo.jpg"),
        });
        assert_eq!(export_summary(&report), "Exported to /out/photo.jpg.");
    }
}
