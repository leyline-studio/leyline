//! Copy/paste of develop settings between photos (ADR 0045 §4).
//!
//! Distinct from named presets: this captures the current photo's settings in
//! memory and applies them to the grid selection.
//!
//! Which categories it captures is the user's answer, asked once in Copy
//! Settings… and remembered after that (ADR 0132 §5): `Ctrl+C` takes the
//! stored set without a dialog, `Ctrl+Shift+C` opens the dialog, and paste
//! writes whatever was taken. There is no filter at paste — a captured set
//! carries its own `groups` list, and that is the only answer to "what does
//! this touch" (`docs/presets.md` §3.2).

use std::cell::RefCell;
use std::rc::Rc;

use super::refresh_develop;
use crate::app::{App, item_at, report_error, selected_versions};
use crate::preferences::SharedPreferences;
use crate::ui::{DevelopState, DialogState, GridState, PreferencesState, StudioWindow, Tr};
use crate::wiring::dialogs::preferences::with_preferences;
use crate::wiring::grid::reload;
use slint::{ComponentHandle, Global, SharedString};

/// Copy/paste develop settings between photos (distinct from named
/// presets, item 4 of the Lightroom/Darktable workflow-gap survey).
pub(crate) fn wire_settings_clipboard(
    app: &Rc<RefCell<App>>,
    window: &StudioWindow,
    preferences: &SharedPreferences,
) {
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_copy_settings(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mask = PreferencesState::get(&window).get_copy_groups();
            copy(&app, &window, mask);
        });
    }
    {
        let handle = window.as_weak();
        DevelopState::get(window).on_open_copy_dialog(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            DialogState::get(&window).set_dialog_result(SharedString::new());
            DialogState::get(&window).set_dialog(SharedString::from("copy-settings"));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        let preferences = preferences.clone();
        DevelopState::get(window).on_copy_settings_groups(move |mask| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            // Nothing chosen would copy nothing and say so nowhere: the
            // dialog stays up with the sentence instead of closing on a
            // gesture that did not happen.
            if mask == 0 {
                DialogState::get(&window)
                    .set_dialog_result(Tr::get(&window).invoke_pick_at_least_one_group());
                return;
            }
            if !copy(&app, &window, mask) {
                return;
            }
            // Stored only once the copy has actually landed (ADR 0132 §6),
            // and stored as the category names rather than the mask — see
            // `Preferences::copy_groups`.
            PreferencesState::get(&window).set_copy_groups(mask);
            let _ = with_preferences(&preferences, |file| {
                file.update(|values| values.copy_groups = Some(crate::groups::from_mask(mask)))
            });
            DialogState::get(&window).set_dialog(SharedString::new());
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_paste_settings(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some(clipboard) = app.dev_clipboard.clone() else {
                return;
            };
            let versions = selected_versions(&app, GridState::get(&window).get_selected());
            if versions.is_empty() {
                return;
            }
            let applied = app
                .library
                .apply_settings(&clipboard, &versions, |_, _| {})
                .map_err(|e| e.to_string())
                .and_then(|report| {
                    if report.failed.is_empty() {
                        Ok(())
                    } else {
                        Err(report
                            .failed
                            .iter()
                            .map(|f| f.reason.as_str())
                            .collect::<Vec<_>>()
                            .join("; "))
                    }
                })
                .and_then(|()| reload(&mut app, &window));
            if let Err(error) = applied {
                report_error(&window, &error);
            }
            // Re-render the develop canvas too when the pasted-onto set
            // includes the photo currently open there.
            if app
                .develop
                .is_some_and(|(_, version)| versions.contains(&version))
            {
                if let Err(error) = refresh_develop(&mut app, &window) {
                    report_error(&window, &error);
                }
            }
        });
    }
}

/// Captures `mask`'s categories from the focused photograph. `false` when
/// nothing was captured, so a caller that has a dialog open can leave it up.
fn copy(app: &Rc<RefCell<App>>, window: &StudioWindow, mask: i32) -> bool {
    let mut app = app.borrow_mut();
    let Some(version) =
        item_at(&app, GridState::get(window).get_selected()).map(|item| item.version_id)
    else {
        return false;
    };
    match app
        .library
        .capture_settings(version, &crate::groups::from_mask(mask))
    {
        Ok(settings) => {
            app.dev_clipboard = Some(settings);
            DevelopState::get(window).set_has_settings_clipboard(true);
            true
        }
        Err(error) => {
            report_error(window, &error.to_string());
            false
        }
    }
}
