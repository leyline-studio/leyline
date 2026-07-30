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
use leyline_sdk::{ExportFormat, ExportRecipe, ExportRequest, ExportSettings, Watermark};
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
            move |preset, destination, format, quality, max_edge, watermark| {
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
                        let settings =
                            match export_settings(format, &quality, &max_edge, &watermark) {
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
            move |name, format, quality, max_edge, watermark| {
                let Some(window) = handle.upgrade() else {
                    return;
                };
                if name.trim().is_empty() {
                    DialogState::get(&window)
                        .set_dialog_result(Tr::get(&window).invoke_enter_a_name());
                    return;
                }
                let (name, settings) =
                    match export_preset_request(&name, format, &quality, &max_edge, &watermark) {
                        Ok(request) => request,
                        Err(message) => {
                            DialogState::get(&window)
                                .set_dialog_result(SharedString::from(message));
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
    max_edge: &str,
    watermark_text: &str,
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
    let max_edge = if max_edge.trim().is_empty() {
        None
    } else {
        Some(
            max_edge
                .parse()
                .map_err(|_| format!("bad max edge {max_edge:?}"))?,
        )
    };
    // Only the line is typed here: its size, color, opacity and corner keep
    // the recipe's defaults (ADR 0051 §3), which a hand-written preset can
    // override. An empty line is the absence of a watermark, not an empty one,
    // which `Watermark::validate` refuses.
    let watermark = (!watermark_text.trim().is_empty()).then(|| Watermark {
        text: watermark_text.trim().to_owned(),
        ..Watermark::default()
    });
    Ok(ExportSettings {
        format,
        quality,
        max_edge,
        watermark,
    })
}

/// Builds a `(name, ExportSettings)` request for saving the export dialog's
/// custom fields as a reusable preset — trims and validates the name, then
/// reuses `export_settings` so the recipe validation isn't duplicated.
pub(crate) fn export_preset_request(
    name: &str,
    format: i32,
    quality: &str,
    max_edge: &str,
    watermark_text: &str,
) -> Result<(String, ExportSettings), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("name is empty".to_owned());
    }
    let settings = export_settings(format, quality, max_edge, watermark_text)?;
    Ok((name.to_owned(), settings))
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
            let settings = export_settings(index as i32, "80", "", "").unwrap();
            assert_eq!(
                settings,
                ExportSettings {
                    format,
                    quality: 80,
                    max_edge: None,
                    watermark: None,
                }
            );
        }
    }

    #[test]
    fn export_settings_parses_a_max_edge() {
        let settings = export_settings(0, "90", "2048", "").unwrap();
        assert_eq!(settings.max_edge, Some(2048));
    }

    #[test]
    fn export_settings_rejects_bad_input() {
        assert!(export_settings(5, "90", "", "").is_err());
        assert!(export_settings(0, "not a number", "", "").is_err());
        assert!(export_settings(0, "90", "not a number", "").is_err());
    }

    /// An empty line is the absence of a watermark; a typed one is trimmed
    /// and carries the recipe defaults (ADR 0051 §3).
    #[test]
    fn a_typed_watermark_line_becomes_a_decoration_and_a_blank_one_none() {
        assert_eq!(export_settings(0, "90", "", "   ").unwrap().watermark, None);
        let watermark = export_settings(0, "90", "", "  © 2026  ")
            .unwrap()
            .watermark
            .expect("a typed line is a watermark");
        assert_eq!(watermark.text, "© 2026");
        assert_eq!(
            (watermark.anchor, watermark.size, watermark.opacity),
            (
                leyline_sdk::WatermarkAnchor::BottomRight,
                Watermark::default().size,
                Watermark::default().opacity
            )
        );
    }

    #[test]
    fn export_preset_request_trims_the_name_and_reuses_export_settings() {
        let (name, settings) = export_preset_request("  Web  ", 0, "80", "2048", "").unwrap();
        assert_eq!(name, "Web");
        assert_eq!(
            settings,
            ExportSettings {
                format: ExportFormat::Jpeg,
                quality: 80,
                max_edge: Some(2048),
                watermark: None,
            }
        );
    }

    #[test]
    fn export_preset_request_rejects_a_blank_or_whitespace_only_name() {
        assert!(export_preset_request("", 0, "80", "", "").is_err());
        assert!(export_preset_request("   ", 0, "80", "", "").is_err());
    }

    #[test]
    fn export_preset_request_still_validates_the_recipe() {
        assert!(export_preset_request("Web", 0, "not a number", "", "").is_err());
        assert!(export_preset_request("Web", 5, "80", "", "").is_err());
    }
}
