//! Wires taking photos out of the catalog (ADR 0060), and the generic
//! confirmation dialog it needed (ADR 0045 §4).
//!
//! Neither menu entry acts. Both describe what they are about to do, park it
//! in [`App::pending_removal`], and open the confirmation dialog; only
//! `confirm-accept` reaches the engine. The wording is built here rather
//! than in Slint because only Rust knows how many photos the selection
//! resolves to.

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::{App, report_error, selected_assets};
use crate::ui::{DialogState, GridState, StudioWindow};
use crate::wiring::grid::reload;
use slint::{ComponentHandle, Global, SharedString};

/// Connects `remove-selected`, `delete-selected` and `confirm-accept`.
pub(crate) fn wire_removal(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    ask(app, window, false);
    ask(app, window, true);
    accept(app, window);
}

/// Arms one of the two menu entries: gather the selection, describe it,
/// open the dialog.
fn ask(app: &Rc<RefCell<App>>, window: &StudioWindow, trash_files: bool) {
    let app = Rc::clone(app);
    let handle = window.as_weak();
    let arm = move || {
        let Some(window) = handle.upgrade() else {
            return;
        };
        let mut app = app.borrow_mut();
        let focused = GridState::get(&window).get_selected();
        let assets = selected_assets(&app, focused);
        if assets.is_empty() {
            return;
        }

        let count = assets.len();
        let state = DialogState::get(&window);
        // The message says what happens to the *files*, because that is the
        // only question the two entries actually differ on, and the only
        // one worth reading twice.
        let tr = crate::ui::Tr::get(&window);
        let count = i32::try_from(count).unwrap_or(i32::MAX);
        let (title, message, accept) = if trash_files {
            (
                tr.invoke_delete_from_disk_title(count),
                tr.invoke_delete_from_disk_message(),
                tr.invoke_delete_from_disk_accept(),
            )
        } else {
            (
                tr.invoke_remove_from_catalog_title(count),
                tr.invoke_remove_from_catalog_message(),
                tr.invoke_remove_from_catalog_accept(),
            )
        };
        state.set_confirm_title(title);
        state.set_confirm_message(message);
        state.set_confirm_accept_text(accept);
        state.set_confirm_danger(trash_files);
        state.set_dialog_result(SharedString::new());
        state.set_dialog(SharedString::from("confirm"));

        app.pending_removal = Some((assets, trash_files));
    };
    if trash_files {
        GridState::get(window).on_delete_selected(arm);
    } else {
        GridState::get(window).on_remove_selected(arm);
    }
}

/// Runs whatever the dialog was asking about. The pending removal is taken,
/// not read: a second click must not remove a second time.
fn accept(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    let app = Rc::clone(app);
    let handle = window.as_weak();
    DialogState::get(window).on_confirm_accept(move || {
        let Some(window) = handle.upgrade() else {
            return;
        };
        let mut app = app.borrow_mut();
        let Some((assets, trash_files)) = app.pending_removal.take() else {
            DialogState::get(&window).set_dialog(SharedString::new());
            return;
        };

        let removal = if trash_files {
            app.library.delete_assets(&assets)
        } else {
            app.library.remove_assets(&assets)
        };
        let report = match removal {
            Ok(report) => report,
            Err(error) => {
                DialogState::get(&window).set_dialog(SharedString::new());
                report_error(&window, &error.to_string());
                return;
            }
        };

        // The selection indexed rows that no longer exist. Clearing it
        // before the reload keeps a later batch action from resolving stale
        // indices onto whatever slid into their place.
        app.multi_selected.clear();
        DialogState::get(&window).set_dialog(SharedString::new());
        if let Err(error) = reload(&mut app, &window) {
            report_error(&window, &error);
            return;
        }

        // A file that resisted is the one outcome the user cannot see for
        // themselves: the catalog looks right, and the photo is still on
        // disk. Say so rather than report a clean success.
        if !report.failed.is_empty() {
            let (path, reason) = &report.failed[0];
            report_error(
                &window,
                &format!(
                    "{} of {} file(s) could not be trashed — {}: {reason}",
                    report.failed.len(),
                    report.failed.len() + report.trashed.len(),
                    path.display()
                ),
            );
        }
    });
}
