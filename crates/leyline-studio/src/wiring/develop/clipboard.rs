//! Copy/paste of develop settings between photos (ADR 0045 §4).
//!
//! Distinct from named presets: this captures the current photo's settings in
//! memory and applies them to the grid selection.

use std::cell::RefCell;
use std::rc::Rc;

use super::refresh_develop;
use crate::app::{App, item_at, report_error, selected_versions};
use crate::ui::{DevelopState, GridState, StudioWindow};
use crate::wiring::grid::reload;
use leyline_sdk::SettingsGroup;
use slint::{ComponentHandle, Global};

/// Develop settings groups copy/paste captures and applies — the same
/// default set a saved preset captures (`docs/presets.md` §3.1), Geometry
/// excluded since a crop/rotation is a per-photo judgment, not a
/// transferable style.
pub(crate) const CLIPBOARD_GROUPS: &[SettingsGroup] = &[
    SettingsGroup::WhiteBalance,
    SettingsGroup::Tone,
    SettingsGroup::Presence,
    SettingsGroup::LensCorrection,
    SettingsGroup::Detail,
];

/// Copy/paste develop settings between photos (distinct from named
/// presets, item 4 of the Lightroom/Darktable workflow-gap survey).
pub(crate) fn wire_settings_clipboard(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_copy_settings(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some(version) =
                item_at(&app, GridState::get(&window).get_selected()).map(|item| item.version_id)
            else {
                return;
            };
            match app.library.capture_settings(version, CLIPBOARD_GROUPS) {
                Ok(settings) => {
                    app.dev_clipboard = Some(settings);
                    DevelopState::get(&window).set_has_settings_clipboard(true);
                }
                Err(error) => report_error(&window, &error.to_string()),
            }
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
