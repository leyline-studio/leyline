//! Wires the roots panel (ADR 0085 §8, ADR 0045 §4).
//!
//! Mirrors `ui/dialogs/roots.slint`.
//!
//! Both gestures that name a folder go through the native picker rather than
//! a text field. That is not a shortcut: a root is identified by the marker
//! *inside* the folder, so a typed path buys nothing except an easier way to
//! point at the wrong one — and `locate_root` refuses a folder whose marker
//! disagrees, which would turn a typo into an error message rather than into
//! a result.

use std::cell::RefCell;
use std::rc::Rc;

use slint::{ComponentHandle, Global, ModelRc, SharedString, VecModel};

use crate::app::App;
use crate::ui::{DialogState, RootRow, StudioWindow};

pub(crate) fn wire_roots(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_open_roots(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            DialogState::get(&window).set_roots_status(SharedString::default());
            refresh(&app, &window);
            DialogState::get(&window).set_dialog(SharedString::from("roots"));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_run_root_add(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let Some(folder) = rfd::FileDialog::new().pick_folder() else {
                return;
            };
            // The folder's own name is the default: the user picked it, so
            // it is already the name they have in mind, and a dialog asking
            // them to type it again would be asking twice.
            let name = folder
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| folder.to_string_lossy().into_owned());
            let outcome = app.borrow().library.add_root(&folder, &name);
            report(&window, outcome.map(|_| ()));
            refresh(&app, &window);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_run_root_locate(move |uuid| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let Some(folder) = rfd::FileDialog::new().pick_folder() else {
                return;
            };
            let outcome = app.borrow().library.locate_root(uuid.as_str(), &folder);
            report(&window, outcome);
            refresh(&app, &window);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_run_root_forget(move |uuid| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let outcome = app.borrow().library.forget_root(uuid.as_str());
            report(&window, outcome);
            refresh(&app, &window);
        });
    }
}

/// Puts an error where the user is looking, and clears it on success.
fn report(window: &StudioWindow, outcome: leyline_sdk::Result<()>) {
    let message = match outcome {
        Ok(()) => SharedString::default(),
        Err(error) => SharedString::from(error.to_string()),
    };
    DialogState::get(window).set_roots_status(message);
}

/// Re-reads the roots and their reachability.
///
/// Called after every gesture rather than patched in place: adding a root can
/// return one that already existed (a folder already carrying a marker), and
/// locating one changes a row the gesture did not name.
fn refresh(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    let statuses = match app.borrow().library.roots() {
        Ok(statuses) => statuses,
        Err(error) => {
            DialogState::get(window).set_roots_status(SharedString::from(error.to_string()));
            return;
        }
    };
    let rows: Vec<RootRow> = statuses
        .iter()
        .map(|status| RootRow {
            name: SharedString::from(status.root.name.as_str()),
            uuid: SharedString::from(status.root.uuid.as_str()),
            location: match &status.location {
                Some(path) => SharedString::from(path.display().to_string()),
                // Rust formats, the panel displays (ADR 0045 §2) — including
                // the sentence shown when there is no location to show.
                None => SharedString::from(crate::format::root_offline_location()),
            },
            online: status.is_online(),
            is_library: status.root.id == leyline_sdk::LIBRARY_ROOT,
        })
        .collect();
    DialogState::get(window).set_roots(ModelRc::from(Rc::new(VecModel::from(rows))));
}

/// The offline roots, as the grid needs them: a set to test each cell against.
pub(crate) fn offline_root_ids(app: &App) -> Vec<i64> {
    app.library
        .roots()
        .map(|statuses| {
            statuses
                .iter()
                .filter(|status| !status.is_online())
                .map(|status| status.root.id)
                .collect()
        })
        .unwrap_or_default()
}
