//! Wires `DialogState`: import, export, print, tethering and watched folders
//! (ADR 0045 §4).

use std::cell::RefCell;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;

use crate::app::{App, item_at, report_error, selected_indices, selected_versions};
use crate::ui::{DialogState, GridState, StudioWindow, Tr};
use leyline_sdk::{
    ExportFormat, ExportRecipe, ExportRequest, ExportSettings, ImportOptions, Margins, Orientation,
    PaperSize, PrintRecipe, PrintRequest, PrintSettings, RenderingIntent,
};
use slint::{ComponentHandle, Global, ModelRc, SharedString, VecModel};

pub(crate) fn wire_dialogs(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let handle = window.as_weak();
        DialogState::get(window).on_browse_import_source(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            // rfd's blocking API is fine to call directly from a Slint
            // callback: it runs synchronously on the calling thread and,
            // like the rest of this app's callbacks, we're already on the
            // UI thread here, so no extra thread hop / async wiring needed.
            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                DialogState::get(&window)
                    .set_import_source_text(SharedString::from(folder.to_string_lossy().as_ref()));
            }
        });
    }
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
        DialogState::get(window).on_run_import(move |source, copy, recursive| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            if source.is_empty() {
                DialogState::get(&window)
                    .set_dialog_result(Tr::get(&window).invoke_enter_source_folder());
                return;
            }
            let options = ImportOptions {
                copy_files: copy,
                recursive,
            };
            let job = app
                .library
                .import_async(Path::new(source.as_str()), &options);
            app.import_job = Some(job);
            DialogState::get(&window)
                .set_dialog_result(Tr::get(&window).invoke_importing_ellipsis());
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
            move |preset, destination, format, quality, max_edge| {
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
                        let settings = match export_settings(format, &quality, &max_edge) {
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
            move |name, format, quality, max_edge| {
                let Some(window) = handle.upgrade() else {
                    return;
                };
                if name.trim().is_empty() {
                    DialogState::get(&window)
                        .set_dialog_result(Tr::get(&window).invoke_enter_a_name());
                    return;
                }
                let (name, settings) =
                    match export_preset_request(&name, format, &quality, &max_edge) {
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
    {
        let handle = window.as_weak();
        DialogState::get(window).on_browse_print_destination(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                DialogState::get(&window).set_print_destination_text(SharedString::from(
                    folder.to_string_lossy().as_ref(),
                ));
            }
        });
    }
    {
        let handle = window.as_weak();
        DialogState::get(window).on_browse_print_profile(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            if let Some(file) = rfd::FileDialog::new()
                .add_filter("ICC profile", &["icc", "icm"])
                .pick_file()
            {
                DialogState::get(&window)
                    .set_print_profile_text(SharedString::from(file.to_string_lossy().as_ref()));
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_open_print(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let presets = match app.library.print_presets() {
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
            app.print_presets = presets;
            DialogState::get(&window)
                .set_print_presets(ModelRc::from(Rc::new(VecModel::from(names))));
            DialogState::get(&window).set_print_preset(-1);
            DialogState::get(&window).set_print_paper(0);
            DialogState::get(&window).set_print_orientation(0);
            DialogState::get(&window).set_print_margins_text(SharedString::from("10"));
            DialogState::get(&window).set_print_dpi_text(SharedString::from("300"));
            DialogState::get(&window).set_print_profile_text(SharedString::default());
            DialogState::get(&window).set_print_intent(1);
            DialogState::get(&window).set_print_copies_text(SharedString::from("1"));
            DialogState::get(&window).set_print_preset_name(SharedString::default());
            DialogState::get(&window).set_dialog_result(SharedString::default());
            DialogState::get(&window).set_dialog(SharedString::from("print"));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_run_print(
            move |preset,
                  destination,
                  paper,
                  orientation,
                  margins,
                  dpi,
                  profile,
                  intent,
                  copies| {
                let Some(window) = handle.upgrade() else {
                    return;
                };
                let mut app = app.borrow_mut();
                let Some(version) = item_at(&app, GridState::get(&window).get_selected())
                    .map(|item| item.version_id)
                else {
                    DialogState::get(&window)
                        .set_dialog_result(Tr::get(&window).invoke_select_photo_first());
                    return;
                };
                if destination.is_empty() {
                    DialogState::get(&window)
                        .set_dialog_result(Tr::get(&window).invoke_enter_destination_folder());
                    return;
                }
                let destination_dir = PathBuf::from(destination.as_str());
                let stored = usize::try_from(preset)
                    .ok()
                    .and_then(|i| app.print_presets.get(i))
                    .map(|preset| preset.preset);
                let recipe = match stored {
                    Some(id) => PrintRecipe::Preset(id),
                    None => {
                        let settings = match print_settings(
                            paper,
                            orientation,
                            &margins,
                            &dpi,
                            &profile,
                            intent,
                        ) {
                            Ok(settings) => settings,
                            Err(message) => {
                                DialogState::get(&window)
                                    .set_dialog_result(SharedString::from(message));
                                return;
                            }
                        };
                        PrintRecipe::Adhoc(settings)
                    }
                };
                let copies: u32 = match copies.parse() {
                    Ok(copies) => copies,
                    Err(_) => {
                        DialogState::get(&window).set_dialog_result(SharedString::from(format!(
                            "bad copies {copies:?}"
                        )));
                        return;
                    }
                };
                let job = app.library.print_async(PrintRequest {
                    versions: vec![version],
                    recipe,
                    destination_dir,
                    copies,
                });
                app.print_job = Some(job);
                DialogState::get(&window)
                    .set_dialog_result(Tr::get(&window).invoke_printing_ellipsis());
            },
        );
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_run_save_print_preset(
            move |name, paper, orientation, margins, dpi, profile, intent| {
                let Some(window) = handle.upgrade() else {
                    return;
                };
                if name.trim().is_empty() {
                    DialogState::get(&window)
                        .set_dialog_result(Tr::get(&window).invoke_enter_a_name());
                    return;
                }
                let (name, settings) = match print_preset_request(
                    &name,
                    paper,
                    orientation,
                    &margins,
                    &dpi,
                    &profile,
                    intent,
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
                    .create_print_preset(&name, &settings)
                    .map_err(|e| e.to_string())
                    .and_then(|_| refresh_print_presets(&mut app, &window));
                match saved {
                    Ok(()) => {
                        DialogState::get(&window).set_print_preset_name(SharedString::default());
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
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_open_tether(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let app = app.borrow();
            DialogState::get(&window).set_tether_connected(app.tether_connected);
            DialogState::get(&window).set_tether_status(SharedString::default());
            DialogState::get(&window)
                .set_tether_captured_count(i32::try_from(app.tether_captured).unwrap_or(0));
            DialogState::get(&window).set_tether_last_captured(SharedString::default());
            DialogState::get(&window).set_dialog(SharedString::from("tether"));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_run_tether_connect(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            match app.library.tether_connect() {
                Ok(()) => {
                    app.tether_captured = 0;
                    DialogState::get(&window).set_tether_status(SharedString::default());
                }
                Err(error) => DialogState::get(&window)
                    .set_tether_status(SharedString::from(error.to_string())),
            }
        });
    }
    {
        let app = Rc::clone(app);
        DialogState::get(window).on_run_tether_disconnect(move || {
            app.borrow().library.tether_disconnect();
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_open_watch(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let app = app.borrow();
            DialogState::get(&window).set_watch_active(app.watch_active);
            DialogState::get(&window).set_watch_status(SharedString::default());
            DialogState::get(&window)
                .set_watch_imported_count(i32::try_from(app.watch_imported).unwrap_or(0));
            DialogState::get(&window).set_watch_last_imported(SharedString::default());
            DialogState::get(&window).set_dialog(SharedString::from("watch"));
        });
    }
    {
        let handle = window.as_weak();
        DialogState::get(window).on_browse_watch_folder(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                DialogState::get(&window)
                    .set_watch_folder_text(SharedString::from(folder.to_string_lossy().as_ref()));
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_run_watch_start(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let app = app.borrow_mut();
            let folder = DialogState::get(&window).get_watch_folder_text();
            if folder.is_empty() {
                DialogState::get(&window)
                    .set_watch_status(Tr::get(&window).invoke_enter_source_folder());
                return;
            }
            match app.library.watch_start(Path::new(folder.as_str())) {
                Ok(()) => DialogState::get(&window).set_watch_status(SharedString::default()),
                Err(error) => DialogState::get(&window)
                    .set_watch_status(SharedString::from(error.to_string())),
            }
        });
    }
    {
        let app = Rc::clone(app);
        DialogState::get(window).on_run_watch_stop(move || {
            app.borrow().library.watch_stop();
        });
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

/// Parses the spot-removal panel's radius/feather/opacity percent fields
/// into the `[0, 1]` units `SpotRemoval` stores — the defaults applied to
/// the *next* spot placed, not part of any stored revision themselves.
pub(crate) fn spot_defaults(
    radius: &str,
    feather: &str,
    opacity: &str,
) -> Result<(f64, f64, f64), String> {
    let radius: f64 = radius
        .parse()
        .map_err(|_| format!("bad radius {radius:?}"))?;
    let feather: f64 = feather
        .parse()
        .map_err(|_| format!("bad feather {feather:?}"))?;
    let opacity: f64 = opacity
        .parse()
        .map_err(|_| format!("bad opacity {opacity:?}"))?;
    Ok((radius / 100.0, feather / 100.0, opacity / 100.0))
}

/// Builds an ad-hoc `ExportSettings` from the export dialog's custom fields
/// (the `Custom` chip, as opposed to a stored preset).
pub(crate) fn export_settings(
    format: i32,
    quality: &str,
    max_edge: &str,
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
    Ok(ExportSettings {
        format,
        quality,
        max_edge,
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
) -> Result<(String, ExportSettings), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("name is empty".to_owned());
    }
    let settings = export_settings(format, quality, max_edge)?;
    Ok((name.to_owned(), settings))
}

/// Reloads the print dialog's preset picker from the catalog (ADR 0036,
/// mirrors `refresh_export_presets`).
pub(crate) fn refresh_print_presets(app: &mut App, window: &StudioWindow) -> Result<(), String> {
    let presets = app.library.print_presets().map_err(|e| e.to_string())?;
    let names: Vec<SharedString> = presets
        .iter()
        .map(|preset| SharedString::from(preset.name.as_str()))
        .collect();
    app.print_presets = presets;
    DialogState::get(window).set_print_presets(ModelRc::from(Rc::new(VecModel::from(names))));
    Ok(())
}

/// Builds an ad-hoc `PrintSettings` from the print dialog's custom fields
/// (ADR 0036, mirrors `export_settings`). `margins` is a single mm value
/// applied to all four edges — the dialog's simplification of the CLI's
/// `--margins <mm>` flag.
pub(crate) fn print_settings(
    paper: i32,
    orientation: i32,
    margins: &str,
    dpi: &str,
    profile: &str,
    intent: i32,
) -> Result<PrintSettings, String> {
    let paper = match paper {
        0 => PaperSize::A4,
        1 => PaperSize::A3,
        2 => PaperSize::Letter,
        other => return Err(format!("unknown paper index {other}")),
    };
    let orientation = match orientation {
        0 => Orientation::Portrait,
        1 => Orientation::Landscape,
        other => return Err(format!("unknown orientation index {other}")),
    };
    let margin: f32 = margins
        .parse()
        .map_err(|_| format!("bad margins {margins:?}"))?;
    let dpi: u32 = dpi.parse().map_err(|_| format!("bad dpi {dpi:?}"))?;
    let profile = if profile.trim().is_empty() {
        None
    } else {
        Some(PathBuf::from(profile))
    };
    let intent = match intent {
        0 => RenderingIntent::Perceptual,
        1 => RenderingIntent::RelativeColorimetric,
        2 => RenderingIntent::Saturation,
        3 => RenderingIntent::AbsoluteColorimetric,
        other => return Err(format!("unknown rendering intent index {other}")),
    };
    Ok(PrintSettings {
        paper,
        orientation,
        margins_mm: Margins {
            top_mm: margin,
            right_mm: margin,
            bottom_mm: margin,
            left_mm: margin,
        },
        dpi,
        profile,
        intent,
    })
}

/// Builds a `(name, PrintSettings)` request for saving the print dialog's
/// custom fields as a reusable preset (ADR 0036, mirrors
/// `export_preset_request`).
pub(crate) fn print_preset_request(
    name: &str,
    paper: i32,
    orientation: i32,
    margins: &str,
    dpi: &str,
    profile: &str,
    intent: i32,
) -> Result<(String, PrintSettings), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("name is empty".to_owned());
    }
    let settings = print_settings(paper, orientation, margins, dpi, profile, intent)?;
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
            let settings = export_settings(index as i32, "80", "").unwrap();
            assert_eq!(
                settings,
                ExportSettings {
                    format,
                    quality: 80,
                    max_edge: None,
                }
            );
        }
    }

    #[test]
    fn export_settings_parses_a_max_edge() {
        let settings = export_settings(0, "90", "2048").unwrap();
        assert_eq!(settings.max_edge, Some(2048));
    }

    #[test]
    fn export_settings_rejects_bad_input() {
        assert!(export_settings(5, "90", "").is_err());
        assert!(export_settings(0, "not a number", "").is_err());
        assert!(export_settings(0, "90", "not a number").is_err());
    }

    #[test]
    fn export_preset_request_trims_the_name_and_reuses_export_settings() {
        let (name, settings) = export_preset_request("  Web  ", 0, "80", "2048").unwrap();
        assert_eq!(name, "Web");
        assert_eq!(
            settings,
            ExportSettings {
                format: ExportFormat::Jpeg,
                quality: 80,
                max_edge: Some(2048),
            }
        );
    }

    #[test]
    fn export_preset_request_rejects_a_blank_or_whitespace_only_name() {
        assert!(export_preset_request("", 0, "80", "").is_err());
        assert!(export_preset_request("   ", 0, "80", "").is_err());
    }

    #[test]
    fn export_preset_request_still_validates_the_recipe() {
        assert!(export_preset_request("Web", 0, "not a number", "").is_err());
        assert!(export_preset_request("Web", 5, "80", "").is_err());
    }

    #[test]
    fn print_settings_parses_paper_orientation_and_intent() {
        let settings = print_settings(1, 1, "15", "600", "", 2).unwrap();
        assert_eq!(
            settings,
            PrintSettings {
                paper: PaperSize::A3,
                orientation: Orientation::Landscape,
                margins_mm: Margins {
                    top_mm: 15.0,
                    right_mm: 15.0,
                    bottom_mm: 15.0,
                    left_mm: 15.0,
                },
                dpi: 600,
                profile: None,
                intent: RenderingIntent::Saturation,
            }
        );
    }

    #[test]
    fn print_settings_treats_a_blank_profile_as_srgb() {
        let settings = print_settings(0, 0, "10", "300", "  ", 1).unwrap();
        assert_eq!(settings.profile, None);
    }

    #[test]
    fn print_settings_carries_a_non_blank_profile_path() {
        let settings = print_settings(0, 0, "10", "300", "Profiles/Baryta.icc", 1).unwrap();
        assert_eq!(settings.profile, Some(PathBuf::from("Profiles/Baryta.icc")));
    }

    #[test]
    fn print_settings_rejects_bad_input() {
        assert!(print_settings(9, 0, "10", "300", "", 1).is_err());
        assert!(print_settings(0, 9, "10", "300", "", 1).is_err());
        assert!(print_settings(0, 0, "not a number", "300", "", 1).is_err());
        assert!(print_settings(0, 0, "10", "not a number", "", 1).is_err());
        assert!(print_settings(0, 0, "10", "300", "", 9).is_err());
    }

    #[test]
    fn print_preset_request_trims_the_name_and_reuses_print_settings() {
        let (name, settings) =
            print_preset_request("  Postcard  ", 2, 0, "5", "300", "", 1).unwrap();
        assert_eq!(name, "Postcard");
        assert_eq!(settings.paper, PaperSize::Letter);
    }

    #[test]
    fn print_preset_request_rejects_a_blank_or_whitespace_only_name() {
        assert!(print_preset_request("", 0, 0, "10", "300", "", 1).is_err());
        assert!(print_preset_request("   ", 0, 0, "10", "300", "", 1).is_err());
    }

    #[test]
    fn print_preset_request_still_validates_the_recipe() {
        assert!(print_preset_request("Web", 0, 0, "not a number", "300", "", 1).is_err());
        assert!(print_preset_request("Web", 9, 0, "10", "300", "", 1).is_err());
    }

    #[test]
    fn spot_defaults_converts_percent_to_unit_range() {
        assert_eq!(spot_defaults("5", "50", "100").unwrap(), (0.05, 0.5, 1.0));
    }

    #[test]
    fn spot_defaults_rejects_bad_input() {
        assert!(spot_defaults("not a number", "50", "100").is_err());
        assert!(spot_defaults("5", "not a number", "100").is_err());
        assert!(spot_defaults("5", "50", "not a number").is_err());
    }
}
