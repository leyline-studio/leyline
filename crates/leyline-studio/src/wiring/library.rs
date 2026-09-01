//! Wires `LibraryState` (ADR 0045 §4).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use slint::{ComponentHandle, Global};

use crate::app::{App, report_error, selected_versions};
use crate::library::relaunch_into;
use crate::ui::{LibraryState, StudioWindow, Tr};

/// Connects the import and export dialogs.
/// Wires `LibraryState`: leaving the open library, and leaving the app.
///
/// All three callbacks are terminal — they either stop the event loop or
/// relaunch the process pointed at another library (`relaunch_into`) — so
/// none of them touches `App`.
pub(crate) fn wire_library(window: &StudioWindow, other_recent_libraries: Vec<PathBuf>) {
    let state = LibraryState::get(window);
    // File ▸ Quit (ADR 0020): stops the event loop, the same outcome as
    // closing the window from the OS chrome.
    state.on_quit(move || {
        let _ = slint::quit_event_loop();
    });
    {
        let handle = window.as_weak();
        state.on_open_library_requested(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                relaunch_into(&window, &folder);
            }
        });
    }
    {
        let handle = window.as_weak();
        state.on_open_recent_library(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let Some(path) = other_recent_libraries.get(index as usize) else {
                return;
            };
            relaunch_into(&window, path);
        });
    }
}

/// Wires Photo ▸ Process: the pixel socket's gesture (ADR 0107).
///
/// Discovered once, at wiring time — installing a processor is not something
/// that happens while the window is open, and a scan of a config directory
/// has no business running on every refresh. Nothing installed is the normal
/// state, and then the submenu does not exist.
///
/// Blocking, like the detector next to it: `leyline-derive` caps the run at
/// ten minutes so a wedged executable cannot hold the interface forever, and
/// a status line says what is happening meanwhile.
pub(crate) fn wire_processors(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    let state = LibraryState::get(window);
    state.set_processors(slint::ModelRc::new(slint::VecModel::from(
        crate::models::operation_rows(&leyline_sdk::derive::discover()),
    )));

    let app = Rc::clone(app);
    let handle = window.as_weak();
    state.on_derive_selected(move |key| {
        let Some(window) = handle.upgrade() else {
            return;
        };
        let tr = Tr::get(&window);
        let Some((source_id, operation)) = crate::models::split_detection_key(key.as_str()) else {
            return;
        };
        let Some(source) = leyline_sdk::derive::discover()
            .into_iter()
            .find(|source| source.id == source_id)
        else {
            report_error(
                &window,
                &format!("no processor named {source_id} is installed"),
            );
            return;
        };
        let mut app = app.borrow_mut();
        // One photo: a derivation writes a file and takes minutes, and a
        // batch of those is a job, not a menu entry. The focused cell is the
        // one acted on, as everywhere else the gesture is singular.
        let focused = crate::ui::GridState::get(&window).get_selected();
        let Some(version) = selected_versions(&app, focused).first().copied() else {
            LibraryState::get(&window).set_status_line(tr.invoke_select_photo_first());
            return;
        };
        LibraryState::get(&window).set_status_line(tr.invoke_deriving_ellipsis());
        match app.library.derive(version, &source, operation) {
            Ok(asset) => {
                let name = app
                    .library
                    .catalog()
                    .asset_details(asset)
                    .map(|details| details.filename)
                    .unwrap_or_default();
                // The new row first, the sentence about it second: `reload`
                // writes the photo count into the same status line, so
                // saying it before would say nothing.
                if let Err(error) = crate::wiring::grid::reload(&mut app, &window) {
                    report_error(&window, &error);
                    return;
                }
                LibraryState::get(&window)
                    .set_status_line(tr.invoke_derived(slint::SharedString::from(name)));
            }
            Err(error) => {
                LibraryState::get(&window).set_status_line(slint::SharedString::new());
                report_error(&window, &error.to_string());
            }
        }
    });
}
