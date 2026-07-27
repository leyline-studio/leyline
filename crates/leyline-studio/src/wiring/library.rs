//! Wires `LibraryState` (ADR 0045 §4).

use slint::{ComponentHandle, Global};
use std::path::PathBuf;

use crate::library::relaunch_into;
use crate::ui::{LibraryState, StudioWindow};

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
