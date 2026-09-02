//! Wires the contact-sheet dialog (ADR 0110, ADR 0045 §4).
//!
//! Mirrors `ui/dialogs/contact_sheet.slint`. Unlike the print dialog, the
//! callbacks take no arguments: a sheet has thirteen fields, all two-way
//! bound, and reading them back from `DialogState` is one lookup each
//! instead of thirteen positional parameters nothing checks the order of.
//!
//! The sheet is made of the **selection** (`selected_versions`), in grid
//! order, falling back to the focused photo — the same rule every batch
//! action in Studio follows.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::app::{App, report_error, selected_versions};
use crate::ui::{DialogState, GridState, StudioWindow, Tr};
use leyline_sdk::{
    CaptionSource, ContactSheetRecipe, ContactSheetRequest, ContactSheetSettings, Margins,
    Orientation, PaperSize, PrintSettings, RenderingIntent,
};
use slint::{ComponentHandle, Global, ModelRc, SharedString, VecModel};

pub(crate) fn wire_contact_sheet(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let handle = window.as_weak();
        DialogState::get(window).on_browse_sheet_destination(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            if let Some(file) = rfd::FileDialog::new()
                .add_filter("PDF", &["pdf"])
                .set_file_name("contact-sheet.pdf")
                .save_file()
            {
                DialogState::get(&window).set_sheet_destination_text(SharedString::from(
                    file.to_string_lossy().as_ref(),
                ));
            }
        });
    }
    {
        let handle = window.as_weak();
        DialogState::get(window).on_browse_sheet_profile(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            if let Some(file) = rfd::FileDialog::new()
                .add_filter("ICC profile", &["icc", "icm"])
                .pick_file()
            {
                DialogState::get(&window)
                    .set_sheet_profile_text(SharedString::from(file.to_string_lossy().as_ref()));
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_open_contact_sheet(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            if let Err(error) = refresh_sheet_presets(&mut app, &window) {
                report_error(&window, &error);
                return;
            }
            let defaults = ContactSheetSettings::default();
            let state = DialogState::get(&window);
            state.set_sheet_preset(-1);
            state.set_sheet_paper(0);
            state.set_sheet_orientation(0);
            state.set_sheet_margins_text(SharedString::from("10"));
            state.set_sheet_dpi_text(SharedString::from(defaults.page.dpi.to_string()));
            state.set_sheet_columns_text(SharedString::from(defaults.columns.to_string()));
            state.set_sheet_rows_text(SharedString::from(defaults.rows.to_string()));
            state.set_sheet_gutter_text(SharedString::from(format!("{}", defaults.gutter_mm)));
            state.set_sheet_captions(defaults.caption == CaptionSource::Filename);
            state.set_sheet_caption_size_text(SharedString::from(format!(
                "{}",
                defaults.caption_mm
            )));
            state.set_sheet_profile_text(SharedString::default());
            state.set_sheet_intent(1);
            state.set_sheet_preset_name(SharedString::default());
            state.set_dialog_result(SharedString::default());
            state.set_dialog(SharedString::from("contact-sheet"));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_run_contact_sheet(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let state = DialogState::get(&window);
            let versions = selected_versions(&app, GridState::get(&window).get_selected());
            if versions.is_empty() {
                state.set_dialog_result(Tr::get(&window).invoke_select_photo_first());
                return;
            }
            let destination = state.get_sheet_destination_text();
            if destination.trim().is_empty() {
                state.set_dialog_result(Tr::get(&window).invoke_enter_destination_folder());
                return;
            }
            let recipe = match usize::try_from(state.get_sheet_preset())
                .ok()
                .and_then(|index| app.sheet_presets.get(index))
                .map(|preset| preset.preset)
            {
                Some(id) => ContactSheetRecipe::Preset(id),
                None => match sheet_settings(&window) {
                    Ok(settings) => ContactSheetRecipe::Adhoc(settings),
                    Err(message) => {
                        state.set_dialog_result(SharedString::from(message));
                        return;
                    }
                },
            };
            let job = app.library.contact_sheet_async(ContactSheetRequest {
                versions,
                recipe,
                destination: PathBuf::from(destination.trim()),
            });
            app.sheet_job = Some(job);
            state.set_dialog_result(Tr::get(&window).invoke_printing_ellipsis());
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_run_save_sheet_preset(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let state = DialogState::get(&window);
            let name = state.get_sheet_preset_name().trim().to_owned();
            if name.is_empty() {
                state.set_dialog_result(Tr::get(&window).invoke_enter_a_name());
                return;
            }
            let settings = match sheet_settings(&window) {
                Ok(settings) => settings,
                Err(message) => {
                    state.set_dialog_result(SharedString::from(message));
                    return;
                }
            };
            let mut app = app.borrow_mut();
            let saved = app
                .library
                .create_contact_sheet_preset(&name, &settings)
                .map_err(|e| e.to_string())
                .and_then(|_| refresh_sheet_presets(&mut app, &window));
            match saved {
                Ok(()) => {
                    state.set_sheet_preset_name(SharedString::default());
                    state.set_dialog_result(Tr::get(&window).invoke_preset_saved());
                }
                Err(error) => {
                    state.set_dialog_result(
                        Tr::get(&window).invoke_save_failed(SharedString::from(error)),
                    );
                }
            }
        });
    }
}

/// Reloads the sheet dialog's preset picker from the catalog (ADR 0110,
/// mirrors `refresh_print_presets`).
pub(crate) fn refresh_sheet_presets(app: &mut App, window: &StudioWindow) -> Result<(), String> {
    let presets = app
        .library
        .contact_sheet_presets()
        .map_err(|e| e.to_string())?;
    let names: Vec<SharedString> = presets
        .iter()
        .map(|preset| SharedString::from(preset.name.as_str()))
        .collect();
    app.sheet_presets = presets;
    DialogState::get(window).set_sheet_presets(ModelRc::from(Rc::new(VecModel::from(names))));
    Ok(())
}

/// Reads the dialog's custom fields into a [`ContactSheetSettings`].
fn sheet_settings(window: &StudioWindow) -> Result<ContactSheetSettings, String> {
    let state = DialogState::get(window);
    sheet_settings_from(
        state.get_sheet_paper(),
        state.get_sheet_orientation(),
        &state.get_sheet_margins_text(),
        &state.get_sheet_dpi_text(),
        &state.get_sheet_profile_text(),
        state.get_sheet_intent(),
        &state.get_sheet_columns_text(),
        &state.get_sheet_rows_text(),
        &state.get_sheet_gutter_text(),
        state.get_sheet_captions(),
        &state.get_sheet_caption_size_text(),
    )
}

/// The parsing itself, taking values rather than a window so it is testable
/// without one — the shape `print_settings` established.
#[expect(clippy::too_many_arguments, reason = "a sheet has that many fields")]
fn sheet_settings_from(
    paper: i32,
    orientation: i32,
    margins: &str,
    dpi: &str,
    profile: &str,
    intent: i32,
    columns: &str,
    rows: &str,
    gutter: &str,
    captions: bool,
    caption_size: &str,
) -> Result<ContactSheetSettings, String> {
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
        .trim()
        .parse()
        .map_err(|_| format!("bad margins {margins:?}"))?;
    let dpi: u32 = dpi.trim().parse().map_err(|_| format!("bad dpi {dpi:?}"))?;
    let profile = if profile.trim().is_empty() {
        None
    } else {
        Some(PathBuf::from(profile.trim()))
    };
    let intent = match intent {
        0 => RenderingIntent::Perceptual,
        1 => RenderingIntent::RelativeColorimetric,
        2 => RenderingIntent::Saturation,
        3 => RenderingIntent::AbsoluteColorimetric,
        other => return Err(format!("unknown rendering intent index {other}")),
    };
    let settings = ContactSheetSettings {
        page: PrintSettings {
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
        },
        columns: columns
            .trim()
            .parse()
            .map_err(|_| format!("bad columns {columns:?}"))?,
        rows: rows
            .trim()
            .parse()
            .map_err(|_| format!("bad rows {rows:?}"))?,
        gutter_mm: gutter
            .trim()
            .parse()
            .map_err(|_| format!("bad gutter {gutter:?}"))?,
        caption: if captions {
            CaptionSource::Filename
        } else {
            CaptionSource::None
        },
        caption_mm: caption_size
            .trim()
            .parse()
            .map_err(|_| format!("bad caption size {caption_size:?}"))?,
    };
    // The engine would refuse it too, but a dialog says so before a job
    // starts rather than after one fails.
    settings.validate().map_err(|e| e.to_string())?;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(columns: &str, rows: &str) -> Result<ContactSheetSettings, String> {
        sheet_settings_from(0, 0, "10", "200", "", 1, columns, rows, "4", true, "3")
    }

    #[test]
    fn the_dialog_builds_the_page_and_the_grid_together() {
        let settings = parse("6", "8").unwrap();
        assert_eq!(settings.page.paper, PaperSize::A4);
        assert_eq!(settings.page.dpi, 200);
        assert_eq!((settings.columns, settings.rows), (6, 8));
        assert_eq!(settings.caption, CaptionSource::Filename);
    }

    #[test]
    fn captions_off_is_a_source_of_none_rather_than_a_zero_size() {
        let settings =
            sheet_settings_from(0, 0, "10", "200", "", 1, "4", "5", "4", false, "3").unwrap();
        assert_eq!(settings.caption, CaptionSource::None);
        assert_eq!(settings.caption_mm, 3.0);
    }

    #[test]
    fn a_grid_the_page_cannot_hold_is_refused_by_the_dialog() {
        // 40 columns of A4 leave less than a millimeter each once the
        // gutters are taken out.
        let error =
            sheet_settings_from(0, 0, "10", "200", "", 1, "40", "40", "10", true, "3").unwrap_err();
        assert!(!error.is_empty());
    }

    #[test]
    fn bad_numbers_are_named_rather_than_defaulted() {
        assert!(parse("not a number", "5").is_err());
        assert!(parse("4", "").is_err());
        assert!(sheet_settings_from(9, 0, "10", "200", "", 1, "4", "5", "4", true, "3").is_err());
        assert!(sheet_settings_from(0, 0, "10", "200", "", 9, "4", "5", "4", true, "3").is_err());
    }

    #[test]
    fn a_blank_profile_means_srgb_and_a_filled_one_is_kept() {
        assert_eq!(
            sheet_settings_from(0, 0, "10", "200", "  ", 1, "4", "5", "4", true, "3")
                .unwrap()
                .page
                .profile,
            None
        );
        assert_eq!(
            sheet_settings_from(
                0,
                0,
                "10",
                "200",
                "P/Baryta.icc",
                1,
                "4",
                "5",
                "4",
                true,
                "3"
            )
            .unwrap()
            .page
            .profile,
            Some(PathBuf::from("P/Baryta.icc"))
        );
    }
}
