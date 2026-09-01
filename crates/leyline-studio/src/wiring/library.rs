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
        // A job, not a call: a derivation runs a model over every pixel of
        // the original and takes minutes (ADR 0107 §2.1). Doing it here
        // would freeze the window for the whole run — the detector next
        // door can be synchronous because it works on a preview and comes
        // back in seconds; this one cannot.
        LibraryState::get(&window).set_status_line(tr.invoke_deriving_ellipsis());
        app.derive_job = Some(app.library.derive_async(version, &source, operation));
    });
}

/// Wires Library ▸ Assisted Culling… and its way out (ADR 0084).
///
/// The run covers **what the grid currently holds** rather than the whole
/// library: the photographer has already said what they are looking at, and
/// culling a folder they are not in is work nobody asked for.
///
/// What comes back is shown, never applied. The grid is narrowed to exactly
/// the proposed frames — the one filter that names rows rather than
/// describing them — so the verdicts can be looked at, and the keystroke
/// that applies them is the reject key the photographer already uses. That
/// is §2 word for word: a wrongly rejected photograph does not look wrong,
/// it looks absent, so it has to be seen before it goes.
pub(crate) fn wire_culling(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        LibraryState::get(window).on_cull_library(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let assets: Vec<leyline_sdk::AssetId> = match app.library.catalog().grid(&app.query) {
                Ok(items) => items.into_iter().map(|item| item.asset_id).collect(),
                Err(error) => {
                    report_error(&window, &error.to_string());
                    return;
                }
            };
            if assets.is_empty() {
                return;
            }
            LibraryState::get(&window).set_status_line(Tr::get(&window).invoke_culling_ellipsis());
            app.cull_job = Some(
                app.library
                    .cull_async(assets, leyline_sdk::CullOptions::default()),
            );
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        LibraryState::get(window).on_discard_proposal(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            discard_proposal(&mut app.borrow_mut(), &window);
        });
    }
}

/// Puts the grid back to what it was showing and drops the proposal.
///
/// The menu's way out: it reloads, because the user asked for the grid
/// back and nothing else is going to fetch it.
pub(crate) fn discard_proposal(app: &mut App, window: &StudioWindow) {
    if !forget_proposal(app, window) {
        return;
    }
    if let Err(error) = crate::wiring::grid::reload(app, window) {
        report_error(window, &error);
    }
}

/// Drops the proposal **without** reloading — for a caller about to reload
/// anyway, which is every filter change.
///
/// Returns whether there was one, so the menu's version knows whether it
/// has anything to reload for.
pub(crate) fn forget_proposal(app: &mut App, window: &StudioWindow) -> bool {
    if app.proposal.take().is_none() {
        return false;
    }
    app.query.assets.clear();
    LibraryState::get(window).set_reviewing_proposal(false);
    true
}
