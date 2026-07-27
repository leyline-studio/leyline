//! Wires the watched-folder panel (ADR 0039, ADR 0045 §4).
//!
//! Mirrors `ui/dialogs/watch.slint`.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use crate::app::App;
use crate::ui::{DialogState, StudioWindow, Tr};
use slint::{ComponentHandle, Global, SharedString};

pub(crate) fn wire_watch(app: &Rc<RefCell<App>>, window: &StudioWindow) {
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
