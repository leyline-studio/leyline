//! Wires the import dialog (ADR 0045 §4).
//!
//! Mirrors `ui/dialogs/import.slint`.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use crate::app::App;
use crate::ui::{DialogState, StudioWindow, Tr};
use leyline_sdk::ImportOptions;
use slint::{ComponentHandle, Global, SharedString};

pub(crate) fn wire_import(app: &Rc<RefCell<App>>, window: &StudioWindow) {
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
}
