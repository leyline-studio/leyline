//! Wires the export dialog (ADR 0025, ADR 0045 §4).
//!
//! Mirrors `ui/dialogs/export.slint`, and owns the parsing that turns its
//! text fields into an `ExportSettings` — validated here so an unusable
//! recipe never reaches the engine.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::app::{App, report_error, selected_indices, selected_versions};
use crate::ui::{DialogState, GridState, StudioWindow, Tr};
use leyline_sdk::{
    ExportFormat, ExportRecipe, ExportRequest, ExportSettings, Watermark, WatermarkAnchor,
};
use slint::{ComponentHandle, Global, ModelRc, SharedString, VecModel};

pub(crate) fn wire_export(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let handle = window.as_weak();
        DialogState::get(window).on_browse_export_destination(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                DialogState::get(&window).set_export_destination_text(SharedString::from(
                    folder.to_string_lossy().as_ref(),
                ));
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_open_export(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let presets = match app.library.export_presets() {
                Ok(presets) => presets,
                Err(error) => {
                    report_error(&window, &error.to_string());
                    return;
                }
            };
            let names: Vec<SharedString> = presets
                .iter()
                .map(|preset| SharedString::from(preset.name.as_str()))
                .collect();
            app.presets = presets;
            DialogState::get(&window)
                .set_export_presets(ModelRc::from(Rc::new(VecModel::from(names))));
            DialogState::get(&window).set_export_photo_count(
                i32::try_from(selected_indices(&app, GridState::get(&window).get_selected()).len())
                    .unwrap_or(0),
            );
            DialogState::get(&window).set_export_preset(-1);
            DialogState::get(&window).set_export_format(0);
            DialogState::get(&window).set_export_quality_text(SharedString::from("90"));
            DialogState::get(&window).set_export_avif_speed_text(SharedString::from(
                leyline_sdk::DEFAULT_AVIF_SPEED.to_string().as_str(),
            ));
            DialogState::get(&window).set_export_max_edge_text(SharedString::default());
            DialogState::get(&window).set_export_preset_name(SharedString::default());
            DialogState::get(&window).set_dialog_result(SharedString::default());
            DialogState::get(&window).set_dialog(SharedString::from("export"));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_run_export(
            move |preset, destination, format, quality, avif_speed, max_edge| {
                let Some(window) = handle.upgrade() else {
                    return;
                };
                let mut app = app.borrow_mut();
                let versions = selected_versions(&app, GridState::get(&window).get_selected());
                if versions.is_empty() {
                    DialogState::get(&window)
                        .set_dialog_result(Tr::get(&window).invoke_select_photo_first());
                    return;
                }
                if destination.is_empty() {
                    DialogState::get(&window)
                        .set_dialog_result(Tr::get(&window).invoke_enter_destination_folder());
                    return;
                }
                let destination = PathBuf::from(destination.as_str());
                let stored = usize::try_from(preset)
                    .ok()
                    .and_then(|i| app.presets.get(i))
                    .map(|preset| preset.preset);
                let recipe = match stored {
                    Some(id) => ExportRecipe::Preset(id),
                    None => {
                        let watermark = match watermark_from_state(&window) {
                            Ok(watermark) => watermark,
                            Err(message) => {
                                DialogState::get(&window)
                                    .set_dialog_result(SharedString::from(message));
                                return;
                            }
                        };
                        let settings = match export_settings(
                            format,
                            &quality,
                            &avif_speed,
                            &max_edge,
                            watermark,
                        ) {
                            Ok(settings) => settings,
                            Err(message) => {
                                DialogState::get(&window)
                                    .set_dialog_result(SharedString::from(message));
                                return;
                            }
                        };
                        ExportRecipe::Adhoc(settings)
                    }
                };
                let job = app.library.export_async(ExportRequest {
                    versions,
                    recipe,
                    destination_dir: destination,
                    // The engine's default (ADR 0068 §1): Studio has no
                    // reason to know better than it does about this machine.
                    concurrency: None,
                });
                app.export_job = Some(job);
                DialogState::get(&window)
                    .set_dialog_result(Tr::get(&window).invoke_exporting_ellipsis());
            },
        );
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_run_save_export_preset(
            move |name, format, quality, avif_speed, max_edge| {
                let Some(window) = handle.upgrade() else {
                    return;
                };
                if name.trim().is_empty() {
                    DialogState::get(&window)
                        .set_dialog_result(Tr::get(&window).invoke_enter_a_name());
                    return;
                }
                let watermark = match watermark_from_state(&window) {
                    Ok(watermark) => watermark,
                    Err(message) => {
                        DialogState::get(&window).set_dialog_result(SharedString::from(message));
                        return;
                    }
                };
                let (name, settings) = match export_preset_request(
                    &name,
                    format,
                    &quality,
                    &avif_speed,
                    &max_edge,
                    watermark,
                ) {
                    Ok(request) => request,
                    Err(message) => {
                        DialogState::get(&window).set_dialog_result(SharedString::from(message));
                        return;
                    }
                };
                let mut app = app.borrow_mut();
                let saved = app
                    .library
                    .create_export_preset(&name, &settings)
                    .map_err(|e| e.to_string())
                    .and_then(|_| refresh_export_presets(&mut app, &window));
                match saved {
                    Ok(()) => {
                        DialogState::get(&window).set_export_preset_name(SharedString::default());
                        DialogState::get(&window)
                            .set_dialog_result(Tr::get(&window).invoke_preset_saved());
                    }
                    Err(error) => {
                        DialogState::get(&window).set_dialog_result(
                            Tr::get(&window).invoke_save_failed(SharedString::from(error)),
                        );
                    }
                }
            },
        );
    }
}

/// Reloads the export dialog's preset picker from the catalog (mirrors
/// `refresh_presets` for develop presets).
pub(crate) fn refresh_export_presets(app: &mut App, window: &StudioWindow) -> Result<(), String> {
    let presets = app.library.export_presets().map_err(|e| e.to_string())?;
    let names: Vec<SharedString> = presets
        .iter()
        .map(|preset| SharedString::from(preset.name.as_str()))
        .collect();
    app.presets = presets;
    DialogState::get(window).set_export_presets(ModelRc::from(Rc::new(VecModel::from(names))));
    Ok(())
}

/// Builds an ad-hoc `ExportSettings` from the export dialog's custom fields
/// (the `Custom` chip, as opposed to a stored preset).
pub(crate) fn export_settings(
    format: i32,
    quality: &str,
    avif_speed: &str,
    max_edge: &str,
    watermark: Option<Watermark>,
) -> Result<ExportSettings, String> {
    let format = match format {
        0 => ExportFormat::Jpeg,
        1 => ExportFormat::Png,
        2 => ExportFormat::Tiff,
        3 => ExportFormat::Webp,
        4 => ExportFormat::Avif,
        other => return Err(format!("unknown export format index {other}")),
    };
    let quality = quality
        .parse()
        .map_err(|_| format!("bad quality {quality:?}"))?;
    // The field only exists while AVIF is the chosen format, so a blank one
    // is the dialog not showing it, not the user clearing it.
    let avif_speed = if avif_speed.trim().is_empty() {
        leyline_sdk::DEFAULT_AVIF_SPEED
    } else {
        avif_speed
            .trim()
            .parse()
            .map_err(|_| format!("bad avif speed {avif_speed:?}"))?
    };
    let max_edge = if max_edge.trim().is_empty() {
        None
    } else {
        Some(
            max_edge
                .parse()
                .map_err(|_| format!("bad max edge {max_edge:?}"))?,
        )
    };
    let settings = ExportSettings {
        format,
        quality,
        avif_speed,
        max_edge,
        watermark,
    };
    // The module's promise, kept: an unusable recipe never reaches the
    // engine. It parsed here and it validates here, so the dialog can say
    // what is wrong while the field is still on screen — rather than the
    // export failing asynchronously, which is where a bad watermark colour
    // used to surface (ADR 0106 §1).
    settings.validate().map_err(|e| e.to_string())?;
    Ok(settings)
}

/// Builds a `(name, ExportSettings)` request for saving the export dialog's
/// custom fields as a reusable preset — trims and validates the name, then
/// reuses `export_settings` so the recipe validation isn't duplicated.
pub(crate) fn export_preset_request(
    name: &str,
    format: i32,
    quality: &str,
    avif_speed: &str,
    max_edge: &str,
    watermark: Option<Watermark>,
) -> Result<(String, ExportSettings), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("name is empty".to_owned());
    }
    let settings = export_settings(format, quality, avif_speed, max_edge, watermark)?;
    Ok((name.to_owned(), settings))
}

/// Builds the watermark from the dialog's five fields, or `None` when there
/// is no line to draw.
///
/// An empty line is the *absence* of a watermark, not an empty one — which
/// `Watermark::validate` refuses. The decorations are parsed but not
/// range-checked here: `ExportSettings::validate` owns the ranges, and a
/// second copy of them in the interface would be a second place to correct
/// (ADR 0106 §1).
pub(crate) fn watermark_settings(
    text: &str,
    anchor: i32,
    size: &str,
    color: &str,
    opacity: &str,
) -> Result<Option<Watermark>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let anchor = match anchor {
        0 => WatermarkAnchor::BottomRight,
        1 => WatermarkAnchor::BottomLeft,
        2 => WatermarkAnchor::TopRight,
        3 => WatermarkAnchor::TopLeft,
        4 => WatermarkAnchor::Center,
        other => return Err(format!("unknown watermark anchor index {other}")),
    };
    Ok(Some(Watermark {
        text: text.to_owned(),
        anchor,
        size: size
            .trim()
            .parse()
            .map_err(|_| format!("bad watermark size {size:?}"))?,
        color: color.trim().to_owned(),
        opacity: opacity
            .trim()
            .parse()
            .map_err(|_| format!("bad watermark opacity {opacity:?}"))?,
        ..Watermark::default()
    }))
}

/// Reads the watermark's five fields off the dialog's state (ADR 0106 §3).
///
/// They travel together, so they are read together rather than threaded one
/// by one through a callback whose argument order would be the only thing
/// keeping it correct.
fn watermark_from_state(window: &StudioWindow) -> Result<Option<Watermark>, String> {
    let state = DialogState::get(window);
    watermark_settings(
        &state.get_export_watermark_text(),
        state.get_export_watermark_anchor(),
        &state.get_export_watermark_size_text(),
        &state.get_export_watermark_color_text(),
        &state.get_export_watermark_opacity_text(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_settings_parses_every_format_and_blank_max_edge() {
        for (index, format) in [
            ExportFormat::Jpeg,
            ExportFormat::Png,
            ExportFormat::Tiff,
            ExportFormat::Webp,
            ExportFormat::Avif,
        ]
        .into_iter()
        .enumerate()
        {
            let settings = export_settings(index as i32, "80", "9", "", None).unwrap();
            assert_eq!(
                settings,
                ExportSettings {
                    format,
                    quality: 80,
                    avif_speed: 9,
                    max_edge: None,
                    watermark: None,
                }
            );
        }
    }

    /// The AVIF field only exists in the dialog while AVIF is the chosen
    /// format (ADR 0067 §1), so a blank one means "not shown" and takes the
    /// default rather than failing to parse.
    #[test]
    fn a_blank_avif_speed_falls_back_to_the_default() {
        assert_eq!(
            export_settings(4, "90", "  ", "", None).unwrap().avif_speed,
            leyline_sdk::DEFAULT_AVIF_SPEED
        );
        assert_eq!(
            export_settings(4, "90", " 10 ", "", None)
                .unwrap()
                .avif_speed,
            10
        );
        assert!(export_settings(4, "90", "fast", "", None).is_err());
    }

    #[test]
    fn export_settings_parses_a_max_edge() {
        let settings = export_settings(0, "90", "9", "2048", None).unwrap();
        assert_eq!(settings.max_edge, Some(2048));
    }

    #[test]
    fn export_settings_rejects_bad_input() {
        assert!(export_settings(5, "90", "9", "", None).is_err());
        assert!(export_settings(0, "not a number", "9", "", None).is_err());
        assert!(export_settings(0, "90", "9", "not a number", None).is_err());
    }

    /// An empty line is the absence of a watermark; a typed one is trimmed.
    #[test]
    fn a_typed_watermark_line_becomes_a_decoration_and_a_blank_one_none() {
        assert_eq!(
            watermark_settings("   ", 0, "3", "#FFFFFF", "0.7").unwrap(),
            None
        );
        let watermark = watermark_settings("  © 2026  ", 0, "3", "#FFFFFF", "0.7")
            .unwrap()
            .expect("a typed line is a watermark");
        assert_eq!(watermark.text, "© 2026");
        assert_eq!(
            (watermark.anchor, watermark.size, watermark.opacity),
            (
                WatermarkAnchor::BottomRight,
                Watermark::default().size,
                Watermark::default().opacity
            )
        );
    }

    /// The four decorations reach the recipe, which is the whole point of
    /// ADR 0106: before it, every watermark Studio could produce was white,
    /// 3 %, 70 % opaque, bottom-right.
    #[test]
    fn the_four_decorations_reach_the_recipe() {
        let watermark = watermark_settings("©", 4, "7.5", "#101010", "0.25")
            .unwrap()
            .expect("a typed line is a watermark");
        assert_eq!(watermark.anchor, WatermarkAnchor::Center);
        assert_eq!(watermark.size, 7.5);
        assert_eq!(watermark.color, "#101010");
        assert_eq!(watermark.opacity, 0.25);
    }

    /// Every corner the dialog draws maps to the corner it names, in the
    /// order the chips are in — an off-by-one here would silently move a
    /// copyright line to the wrong side of every export.
    #[test]
    fn every_corner_chip_maps_to_its_anchor() {
        for (index, anchor) in [
            WatermarkAnchor::BottomRight,
            WatermarkAnchor::BottomLeft,
            WatermarkAnchor::TopRight,
            WatermarkAnchor::TopLeft,
            WatermarkAnchor::Center,
        ]
        .into_iter()
        .enumerate()
        {
            let watermark = watermark_settings("©", index as i32, "3", "#FFFFFF", "0.7")
                .unwrap()
                .unwrap();
            assert_eq!(watermark.anchor, anchor, "chip {index}");
        }
        assert!(watermark_settings("©", 5, "3", "#FFFFFF", "0.7").is_err());
    }

    /// The dialog parses, the recipe validates. A colour the engine refuses
    /// must come back as a refusal, not as a silently ignored field.
    #[test]
    fn an_unparseable_decoration_is_refused_and_an_invalid_one_too() {
        assert!(watermark_settings("©", 0, "big", "#FFFFFF", "0.7").is_err());
        assert!(watermark_settings("©", 0, "3", "#FFFFFF", "opaque").is_err());

        let watermark = watermark_settings("©", 0, "3", "rouge", "0.7").unwrap();
        assert!(
            export_settings(0, "90", "9", "", watermark).is_err(),
            "a colour the engine refuses must not reach an export"
        );
    }

    #[test]
    fn export_preset_request_trims_the_name_and_reuses_export_settings() {
        let (name, settings) =
            export_preset_request("  Web  ", 0, "80", "9", "2048", None).unwrap();
        assert_eq!(name, "Web");
        assert_eq!(
            settings,
            ExportSettings {
                format: ExportFormat::Jpeg,
                quality: 80,
                avif_speed: 9,
                max_edge: Some(2048),
                watermark: None,
            }
        );
    }

    #[test]
    fn export_preset_request_rejects_a_blank_or_whitespace_only_name() {
        assert!(export_preset_request("", 0, "80", "9", "", None).is_err());
        assert!(export_preset_request("   ", 0, "80", "9", "", None).is_err());
    }

    #[test]
    fn export_preset_request_still_validates_the_recipe() {
        assert!(export_preset_request("Web", 0, "not a number", "9", "", None).is_err());
        assert!(export_preset_request("Web", 5, "80", "9", "", None).is_err());
    }
}
