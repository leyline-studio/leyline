//! The one accept of the generic confirmation dialog (ADR 0045 §4).
//!
//! `confirm-accept` carries no payload: what the user agreed to is parked in
//! `App::pending_confirm` when the dialog opens, and taken — not read — here.
//! Taken, because a second click must not run the action twice.
//!
//! This module knows the branches and nothing else. Each one lives in the
//! module that owns the feature: removal in [`crate::wiring::removal`],
//! pairing in [`crate::wiring::pairs`].

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::{App, PendingConfirm};
use crate::ui::{DialogState, StudioWindow};
use slint::{ComponentHandle, Global, SharedString};

/// Connects `confirm-accept` to whatever is pending.
pub(crate) fn wire_confirm(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    let app = Rc::clone(app);
    let handle = window.as_weak();
    DialogState::get(window).on_confirm_accept(move || {
        let Some(window) = handle.upgrade() else {
            return;
        };
        let mut app = app.borrow_mut();
        let Some(pending) = app.pending_confirm.take() else {
            DialogState::get(&window).set_dialog(SharedString::new());
            return;
        };
        match pending {
            PendingConfirm::Removal {
                assets,
                trash_files,
            } => crate::wiring::removal::run(&mut app, &window, &assets, trash_files),
            PendingConfirm::PairLibrary => crate::wiring::pairs::run(&mut app, &window),
        }
    });
}
