//! Wires the tethered-capture panel (ADR 0038, ADR 0045 §4).
//!
//! Mirrors `ui/dialogs/tether.slint`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::App;
use crate::ui::{DialogState, StudioWindow};
use slint::{ComponentHandle, Global, SharedString};

pub(crate) fn wire_tether(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_open_tether(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let app = app.borrow();
            DialogState::get(&window).set_tether_connected(app.tether_connected);
            DialogState::get(&window).set_tether_status(SharedString::default());
            DialogState::get(&window)
                .set_tether_captured_count(i32::try_from(app.tether_captured).unwrap_or(0));
            DialogState::get(&window).set_tether_last_captured(SharedString::default());
            DialogState::get(&window).set_dialog(SharedString::from("tether"));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_run_tether_connect(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            match app.library.tether_connect() {
                Ok(()) => {
                    app.tether_captured = 0;
                    DialogState::get(&window).set_tether_status(SharedString::default());
                }
                Err(error) => DialogState::get(&window)
                    .set_tether_status(SharedString::from(error.to_string())),
            }
        });
    }
    {
        let app = Rc::clone(app);
        DialogState::get(window).on_run_tether_disconnect(move || {
            app.borrow().library.tether_disconnect();
        });
    }
}
