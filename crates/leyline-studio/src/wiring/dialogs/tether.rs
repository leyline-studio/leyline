//! Wires the tethered-capture connect dialog (ADR 0038, ADR 0087 §7)
//! (ADR 0045 §4).
//!
//! Mirrors `ui/dialogs/tether.slint`. Only what is decided *before* a
//! session opens lives here — the session folder and the develop preset.
//! A session already running is the bar's business (`wiring/tether.rs`).

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::App;
use crate::ui::{DialogState, StudioWindow};
use leyline_sdk::TetherOptions;
use slint::{ComponentHandle, Global, ModelRc, SharedString, VecModel};

pub(crate) fn wire_tether(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_open_tether(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            // The preset list is read here rather than cached: presets are
            // created and deleted from Develop while a session is open, and
            // a stale list would offer one that no longer exists.
            app.tether_presets = app
                .library
                .presets()
                .unwrap_or_default()
                .into_iter()
                .map(|preset| (preset.preset, preset.name))
                .collect();
            let names: Vec<SharedString> = app
                .tether_presets
                .iter()
                .map(|(_, name)| SharedString::from(name.as_str()))
                .collect();
            let state = DialogState::get(&window);
            state.set_tether_presets(ModelRc::new(VecModel::from(names)));
            state.set_tether_status(SharedString::default());
            state.set_dialog(SharedString::from("tether"));
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
            let state = DialogState::get(&window);
            let index = state.get_tether_preset();
            let preset = usize::try_from(index)
                .ok()
                .and_then(|index| app.tether_presets.get(index))
                .map(|(id, _)| *id);
            let options = TetherOptions {
                session: state.get_tether_session().to_string(),
                preset,
            };
            match app.library.tether_connect(&options) {
                // The dialog closes on success: from here on the bar is the
                // interface, and leaving a modal in front of the live view
                // would hide the very thing the session exists to show.
                Ok(()) => {
                    app.tether_captured = 0;
                    app.tether_preset = preset;
                    state.set_tether_status(SharedString::default());
                    state.set_dialog(SharedString::default());
                }
                Err(error) => {
                    state.set_tether_status(SharedString::from(error.to_string()));
                }
            }
        });
    }
}
