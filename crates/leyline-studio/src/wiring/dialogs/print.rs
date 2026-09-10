//! Wires the print dialog (ADR 0036, ADR 0045 §4).
//!
//! Mirrors `ui/dialogs/print.slint`, and owns the parsing of its fields into
//! a `PrintSettings`, the same shape as the export side.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::app::{App, report_error};
use crate::ui::{DialogState, GridState, StudioWindow, Tr};
use leyline_sdk::{
    Margins, Orientation, PaperSize, PrintRecipe, PrintRequest, PrintSettings, RenderingIntent,
};
use slint::{ComponentHandle, Global, ModelRc, SharedString, VecModel};

pub(crate) fn wire_print(app: &Rc<RefCell<App>>, window: &StudioWindow) {
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
            DialogState::get(&window).set_print_photo_count(
                i32::try_from(
                    crate::app::selected_versions(&app, GridState::get(&window).get_selected())
                        .len(),
                )
                .unwrap_or(0),
            );
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
                // The whole selection, like an export (ADR 0141 §4): the
                // engine has taken a list of versions since ADR 0036, and
                // the dialog was handing it one.
                let versions =
                    crate::app::selected_versions(&app, GridState::get(&window).get_selected());
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
                    versions,
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
}
