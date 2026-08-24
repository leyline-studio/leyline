//! Library ▸ Pair RAW+JPEG… (ADR 0079 §7).
//!
//! The menu entry does not pair. It counts what is pairable, says so, and
//! parks the action in `App::pending_confirm`; only the dialog's accept
//! reaches the engine, through [`crate::wiring::confirm`].
//!
//! The count comes first because the pass takes half the thumbnails of a
//! RAW+JPEG library out of the grid. That is the right outcome and a
//! frightening one to meet unannounced.

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::{App, PendingConfirm, report_error};
use crate::ui::{DialogState, LibraryState, StudioWindow, Tr};
use crate::wiring::grid::reload;
use slint::{ComponentHandle, Global, SharedString};

/// Connects `pair-library`.
pub(crate) fn wire_pairs(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    let app = Rc::clone(app);
    let handle = window.as_weak();
    LibraryState::get(window).on_pair_library(move || {
        let Some(window) = handle.upgrade() else {
            return;
        };
        let mut app = app.borrow_mut();
        let pairable = match app.library.catalog().pairable_count() {
            Ok(count) => count,
            Err(error) => {
                report_error(&window, &error.to_string());
                return;
            }
        };
        let tr = Tr::get(&window);
        if pairable == 0 {
            LibraryState::get(&window).set_status_line(tr.invoke_pair_library_none());
            return;
        }

        let state = DialogState::get(&window);
        state.set_confirm_title(
            tr.invoke_pair_library_title(i32::try_from(pairable).unwrap_or(i32::MAX)),
        );
        state.set_confirm_message(tr.invoke_pair_library_message());
        state.set_confirm_accept_text(tr.invoke_pair_library_accept());
        // Not destructive: no file moves, and every pair can be undone.
        state.set_confirm_danger(false);
        state.set_dialog_result(SharedString::new());
        state.set_dialog(SharedString::from("confirm"));

        app.pending_confirm = Some(PendingConfirm::PairLibrary);
    });
}

/// Runs the pass, once the user accepted.
pub(crate) fn run(app: &mut App, window: &StudioWindow) {
    let paired = match app.library.pair_assets() {
        Ok(paired) => paired,
        Err(error) => {
            DialogState::get(window).set_dialog(SharedString::new());
            report_error(window, &error.to_string());
            return;
        }
    };

    // The selection indexed rows that are about to shift: a companion just
    // left the grid, so every index after it names a different photo.
    app.multi_selected.clear();
    DialogState::get(window).set_dialog(SharedString::new());
    if let Err(error) = reload(app, window) {
        report_error(window, &error);
        return;
    }
    LibraryState::get(window).set_status_line(
        Tr::get(window).invoke_paired(i32::try_from(paired.len()).unwrap_or(i32::MAX)),
    );
}
