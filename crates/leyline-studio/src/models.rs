//! Conversions from engine values into what the UI displays (ADR 0045 §4).
//!
//! The UI never interprets a catalog value itself, so every number reaching it
//! is formatted here first.

use std::rc::Rc;

use crate::app::SORTS;
use leyline_sdk::{ColorLabel, ExportReport, PrintReport, Settings, SkippedFile, Sort};
use slint::{ModelRc, SharedString, VecModel};

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
        perspective_vertical: settings.perspective.map_or(0, |p| p.vertical) as f32,
        perspective_horizontal: settings.perspective.map_or(0, |p| p.horizontal) as f32,
        crop_left: (crop.x * 100.0) as f32,
        crop_top: (crop.y * 100.0) as f32,
        crop_width: (crop.width * 100.0) as f32,
        crop_height: (crop.height * 100.0) as f32,
        nr_luminance: settings.noise_reduction.luminance as f32,
        nr_color: settings.noise_reduction.color as f32,
        sharpen_amount: settings.sharpening.amount as f32,
        sharpen_radius: settings.sharpening.radius as f32,
        vignette_amount: settings.vignette.amount as f32,
        vignette_midpoint: settings.vignette.midpoint as f32,
        vignette_roundness: settings.vignette.roundness as f32,
        vignette_feather: settings.vignette.feather as f32,
        grain_amount: settings.grain.amount as f32,
        grain_size: settings.grain.size as f32,
        grain_roughness: settings.grain.roughness as f32,
        monochrome: settings.monochrome,
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
        lut: settings.lut.as_ref().is_some_and(|lut| lut.enabled),
        lut_strength: settings.lut.as_ref().map_or(100, |lut| lut.strength) as f32,
        color_grading_shadows: zone_model(&settings.color_grading.shadows),
        color_grading_midtones: zone_model(&settings.color_grading.midtones),
        color_grading_highlights: zone_model(&settings.color_grading.highlights),
        highlight_reconstruction: SharedString::from(match settings.highlight_reconstruction {
            leyline_sdk::HighlightReconstruction::Clip => "clip",
            leyline_sdk::HighlightReconstruction::Blend => "blend",
            leyline_sdk::HighlightReconstruction::Rebuild => "rebuild",
        }),
        demosaic: SharedString::from(match settings.demosaic {
            leyline_sdk::Demosaic::Ahd => "ahd",
            leyline_sdk::Demosaic::Vng => "vng",
            leyline_sdk::Demosaic::Dcb => "dcb",
            leyline_sdk::Demosaic::Dht => "dht",
        }),
        color_grading_balance: settings.color_grading.balance as f32,
        color_grading_blending: settings.color_grading.blending as f32,
    }
}

/// Turns the detectors found on this machine into the panel's chips
/// (ADR 0073 §3), and encodes in each key the pair Rust needs to run it back.
///
/// The label is the detector's own, never translated: it alone knows what its
/// model was trained on. It is prefixed with the detector's name only when
/// more than one is installed — with a single one, "Leyline Assist · Ciel"
/// would repeat the same words on every chip for nothing.
pub(crate) fn detection_rows(
    sources: &[leyline_sdk::DetectorSource],
) -> Vec<crate::ui::DetectionRow> {
    let several = sources.len() > 1;
    sources
        .iter()
        .flat_map(|source| {
            source.detections.iter().map(move |detection| {
                let label = if several {
                    format!("{} · {}", source.label, detection.label)
                } else {
                    detection.label.clone()
                };
                crate::ui::DetectionRow {
                    key: SharedString::from(format!("{}/{}", source.id, detection.id)),
                    label: SharedString::from(label),
                }
            })
        })
        .collect()
}

/// Splits a chip's key back into the detector and the detection it names.
///
/// The detection identifier may itself hold a slash — it is the detector's
/// string, not ours — so the split is on the *first* separator only.
pub(crate) fn split_detection_key(key: &str) -> Option<(&str, &str)> {
    let (source, detection) = key.split_once('/')?;
    (!source.is_empty() && !detection.is_empty()).then_some((source, detection))
}

/// Mirrors every stored local adjustment into the mask panel's model
/// (ADR 0049): one row per entry of `Settings::local_adjustments`, in list
/// order, since that order *is* the identity of an entry
/// (`Param::LocalAdjustment(index)`).
///
/// `[0, 1]` fields arrive as percent, the unit the panel's sliders use, and
/// each `Option` that is `None` arrives as its neutral value plus an `_on`
/// flag turned off — the mapping [`crate::masks::edit_field`] inverts.
pub(crate) fn mask_rows(settings: &Settings) -> Vec<crate::ui::MaskRow> {
    settings
        .local_adjustments
        .iter()
        .map(|entry| {
            let values = &entry.adjustments;
            let luminance = entry.range.as_ref().and_then(|range| range.luminance);
            let color = entry.range.as_ref().and_then(|range| range.color);
            let mut row = crate::ui::MaskRow {
                kind: SharedString::from(mask_kind(&entry.mask)),
                geometry: SharedString::from(mask_geometry(&entry.mask)),
                opacity: (entry.opacity * 100.0) as f32,
                feather: 0.0,
                inverted: false,
                wb_on: values.temperature.is_some() || values.tint.is_some(),
                temperature: values
                    .temperature
                    .unwrap_or_else(|| leyline_sdk::WhiteBalance::default().temperature)
                    as f32,
                tint: values.tint.unwrap_or(0) as f32,
                exposure: values.exposure.unwrap_or(0.0) as f32,
                contrast: values.contrast.unwrap_or(0) as f32,
                highlights: values.highlights.unwrap_or(0) as f32,
                shadows: values.shadows.unwrap_or(0) as f32,
                whites: values.whites.unwrap_or(0) as f32,
                blacks: values.blacks.unwrap_or(0) as f32,
                vibrance: values.vibrance.unwrap_or(0) as f32,
                saturation: values.saturation.unwrap_or(0) as f32,
                range_luminance_on: luminance.is_some(),
                lum_min: (luminance.unwrap_or_default().min * 100.0) as f32,
                lum_max: (luminance.unwrap_or_default().max * 100.0) as f32,
                lum_softness: (luminance.unwrap_or_default().softness * 100.0) as f32,
                range_color_on: color.is_some(),
                color_center: color.unwrap_or_default().center as f32,
                color_width: color.unwrap_or_default().width as f32,
                color_softness: color.unwrap_or_default().softness as f32,
                cx: 0.0,
                cy: 0.0,
                rx: 0.0,
                ry: 0.0,
                x0: 0.0,
                y0: 0.0,
                x1: 0.0,
                y1: 0.0,
                dabs: ModelRc::default(),
            };
            match &entry.mask {
                leyline_sdk::Mask::Radial {
                    cx,
                    cy,
                    rx,
                    ry,
                    feather,
                    inverted,
                    ..
                } => {
                    row.cx = *cx as f32;
                    row.cy = *cy as f32;
                    row.rx = *rx as f32;
                    row.ry = *ry as f32;
                    row.feather = (feather * 100.0) as f32;
                    row.inverted = *inverted;
                }
                leyline_sdk::Mask::Gradient { x0, y0, x1, y1 } => {
                    row.x0 = *x0 as f32;
                    row.y0 = *y0 as f32;
                    row.x1 = *x1 as f32;
                    row.y1 = *y1 as f32;
                }
                leyline_sdk::Mask::Brush { strokes } => {
                    row.dabs = ModelRc::from(Rc::new(VecModel::from(
                        strokes
                            .iter()
                            .map(|dab| crate::ui::MaskDab {
                                x: dab.x as f32,
                                y: dab.y as f32,
                                radius: dab.radius as f32,
                            })
                            .collect::<Vec<_>>(),
                    )));
                }
                // Nothing to place on the canvas: a stored coverage
                // (ADR 0070) has no handles, and no tool in Studio makes
                // one yet — the row exists so an extension's mask is
                // listed, selectable and adjustable like any other.
                leyline_sdk::Mask::Everything | leyline_sdk::Mask::Coverage { .. } => {}
            }
            row
        })
        .collect()
}

/// The mask kind the panel switches its labels and controls on.
fn mask_kind(mask: &leyline_sdk::Mask) -> &'static str {
    match mask {
        leyline_sdk::Mask::Radial { .. } => "radial",
        leyline_sdk::Mask::Gradient { .. } => "gradient",
        leyline_sdk::Mask::Brush { .. } => "brush",
        leyline_sdk::Mask::Everything => "everything",
        leyline_sdk::Mask::Coverage { .. } => "coverage",
    }
}

/// One mask's geometry in a row's worth of numbers: position and size for a
/// radial, both ends for a gradient, dab count and size for a brush. Symbols
/// and percentages only — nothing to translate, and nothing for the UI to
/// interpret.
fn mask_geometry(mask: &leyline_sdk::Mask) -> String {
    let percent = |value: f64| (value * 100.0).round();
    match mask {
        leyline_sdk::Mask::Radial { cx, cy, rx, ry, .. } => format!(
            "({}, {}) · {}×{} %",
            percent(*cx),
            percent(*cy),
            percent(*rx),
            percent(*ry)
        ),
        leyline_sdk::Mask::Gradient { x0, y0, x1, y1 } => format!(
            "({}, {}) → ({}, {}) %",
            percent(*x0),
            percent(*y0),
            percent(*x1),
            percent(*y1)
        ),
        // The checksum's first bytes: enough to tell two stored masks apart
        // in a list, and the only identity a coverage has.
        leyline_sdk::Mask::Coverage { checksum, .. } => format!(
            "⛁ {}",
            checksum
                .trim_start_matches("blake3:")
                .chars()
                .take(8)
                .collect::<String>()
        ),
        leyline_sdk::Mask::Brush { strokes } => format!(
            "×{} · ⌀ {} %",
            strokes.len(),
            strokes.last().map_or(0.0, |dab| percent(dab.radius))
        ),
        leyline_sdk::Mask::Everything => String::new(),
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

    fn source(id: &str, label: &str, detections: &[(&str, &str)]) -> leyline_sdk::DetectorSource {
        leyline_sdk::DetectorSource {
            id: id.to_owned(),
            label: label.to_owned(),
            command: std::path::PathBuf::from("/bin/true"),
            args: Vec::new(),
            detections: detections
                .iter()
                .map(|(id, label)| leyline_sdk::Detection {
                    id: (*id).to_owned(),
                    label: (*label).to_owned(),
                })
                .collect(),
        }
    }

    /// A single detector names its detections and nothing else: prefixing
    /// every chip with the same word would spend the panel's width on no
    /// information at all.
    #[test]
    fn one_detector_labels_its_detections_plainly() {
        let rows = detection_rows(&[source("assist", "Leyline Assist", &[("sky", "Ciel")])]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "Ciel");
        assert_eq!(rows[0].key, "assist/sky");
    }

    /// With two installed, the name is what tells two "Ciel" apart.
    #[test]
    fn several_detectors_are_told_apart_by_name() {
        let rows = detection_rows(&[
            source("a", "Assist", &[("sky", "Ciel")]),
            source("b", "Autre", &[("sky", "Ciel"), ("subject", "Sujet")]),
        ]);
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, ["Assist · Ciel", "Autre · Ciel", "Autre · Sujet"]);
        assert_eq!(rows[2].key, "b/subject");
    }

    /// The key survives a detection identifier holding a slash, which is the
    /// detector's string to choose, not ours.
    #[test]
    fn a_key_splits_on_its_first_separator_only() {
        assert_eq!(split_detection_key("assist/sky"), Some(("assist", "sky")));
        assert_eq!(
            split_detection_key("assist/people/faces"),
            Some(("assist", "people/faces"))
        );
        assert_eq!(split_detection_key("nothing"), None);
        assert_eq!(split_detection_key("/sky"), None);
        assert_eq!(split_detection_key("assist/"), None);
    }

    #[test]
    fn mask_rows_mirror_every_kind_with_its_geometry_in_words() {
        use leyline_sdk::{BrushStroke, LocalAdjustment, LocalAdjustmentValues, Mask};

        let entry = |mask: Mask| LocalAdjustment {
            mask,
            range: None,
            opacity: 1.0,
            adjustments: LocalAdjustmentValues::default(),
        };
        let settings = Settings {
            local_adjustments: vec![
                entry(Mask::Radial {
                    cx: 0.5,
                    cy: 0.25,
                    rx: 0.3,
                    ry: 0.2,
                    angle: 0.0,
                    feather: 0.4,
                    inverted: true,
                }),
                entry(Mask::Gradient {
                    x0: 0.0,
                    y0: 0.8,
                    x1: 0.0,
                    y1: 0.2,
                }),
                entry(Mask::Brush {
                    strokes: vec![BrushStroke {
                        x: 0.1,
                        y: 0.2,
                        radius: 0.08,
                        flow: 0.5,
                        hardness: 0.5,
                    }],
                }),
                entry(Mask::Everything),
            ],
            ..Settings::default()
        };
        let rows = mask_rows(&settings);
        let kinds: Vec<&str> = rows.iter().map(|row| row.kind.as_str()).collect();
        assert_eq!(kinds, ["radial", "gradient", "brush", "everything"]);
        // Two percentages side by side are parenthesized rather than
        // comma-joined: a bare "50,25" reads as one decimal number.
        assert_eq!(rows[0].geometry, "(50, 25) · 30×20 %");
        assert_eq!(rows[1].geometry, "(0, 80) → (0, 20) %");
        assert_eq!(rows[2].geometry, "×1 · ⌀ 8 %");
        assert_eq!(rows[3].geometry, "");
        // Radial-only fields reach the panel as percent, and the other kinds
        // leave them at zero rather than showing a stale feather.
        assert_eq!((rows[0].feather, rows[0].inverted), (40.0, true));
        assert_eq!((rows[1].feather, rows[1].inverted), (0.0, false));
        assert_eq!(slint::Model::row_count(&rows[2].dabs), 1);
    }

    /// Every `None` reaches the panel as its neutral value with the matching
    /// flag off — the inverse of `masks::edit_field`'s "neutral = absent"
    /// (ADR 0049 §3).
    #[test]
    fn unset_values_arrive_neutral_and_flagged_off() {
        use leyline_sdk::{
            ColorRange, LocalAdjustment, LocalAdjustmentValues, LuminanceRange, Mask, RangeMask,
        };

        let settings = Settings {
            local_adjustments: vec![LocalAdjustment {
                mask: Mask::Everything,
                range: None,
                opacity: 0.5,
                adjustments: LocalAdjustmentValues::default(),
            }],
            ..Settings::default()
        };
        let row = &mask_rows(&settings)[0];
        assert_eq!(row.opacity, 50.0);
        assert!(!row.wb_on && !row.range_luminance_on && !row.range_color_on);
        assert_eq!(
            (row.exposure, row.contrast, row.saturation),
            (0.0, 0.0, 0.0)
        );
        assert_eq!(
            row.temperature,
            leyline_sdk::WhiteBalance::default().temperature as f32
        );

        let settings = Settings {
            local_adjustments: vec![LocalAdjustment {
                mask: Mask::Everything,
                range: Some(RangeMask {
                    luminance: Some(LuminanceRange {
                        min: 0.4,
                        max: 0.9,
                        softness: 0.1,
                    }),
                    color: Some(ColorRange {
                        center: 210.0,
                        width: 30.0,
                        softness: 15.0,
                    }),
                }),
                opacity: 1.0,
                adjustments: LocalAdjustmentValues {
                    temperature: Some(4800),
                    tint: Some(-10),
                    exposure: Some(-1.5),
                    ..LocalAdjustmentValues::default()
                },
            }],
            ..Settings::default()
        };
        let row = &mask_rows(&settings)[0];
        assert!(row.wb_on && row.range_luminance_on && row.range_color_on);
        assert_eq!(
            (row.temperature, row.tint, row.exposure),
            (4800.0, -10.0, -1.5)
        );
        assert_eq!((row.lum_min, row.lum_max), (40.0, 90.0));
        assert_eq!(row.color_center, 210.0);
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
