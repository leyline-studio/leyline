//! The tethered-capture bar (ADR 0087 §7) (ADR 0045 §4).
//!
//! Mirrors `ui/state/tether.slint` and `ui/panels/tether.slint`. Everything
//! here is a thin relay: the bar's controls enqueue commands that return at
//! once, and what the camera answers arrives as an event and lands in
//! `refresh_tether` — the interface thread never waits on USB
//! (ADR 0087 §1–2).

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::App;
use crate::ui::{StudioWindow, TetherControl, TetherState};
use leyline_sdk::TetherSetting;
use slint::{ComponentHandle, Global, Image, ModelRc, SharedString, VecModel};

/// The picker id the develop-settings field uses. Not a [`TetherSetting`]:
/// it is Leyline's own choice, not one of the camera's.
const PRESET_PICKER: &str = "preset";

pub(crate) fn wire_tether(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let app = Rc::clone(app);
        TetherState::get(window).on_capture(move || {
            app.borrow().library.tether_capture();
        });
    }
    {
        let app = Rc::clone(app);
        TetherState::get(window).on_set_live(move |on| {
            app.borrow().library.tether_live_view(on);
        });
    }
    {
        let app = Rc::clone(app);
        TetherState::get(window).on_disconnect(move || {
            app.borrow().library.tether_disconnect();
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        TetherState::get(window).on_open_picker(move |id| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let app = app.borrow();
            let choices: Vec<SharedString> = if id == PRESET_PICKER {
                // "None" first, so clearing the preset is one click and not
                // a trip back through the connect dialog.
                std::iter::once(SharedString::from(no_preset()))
                    .chain(
                        app.tether_presets
                            .iter()
                            .map(|(_, name)| SharedString::from(name.as_str())),
                    )
                    .collect()
            } else {
                TetherSetting::parse(id.as_str())
                    .and_then(|setting| {
                        app.library
                            .tether_settings()
                            .get(setting)
                            .map(|value| value.choices.clone())
                    })
                    .unwrap_or_default()
                    .into_iter()
                    .map(SharedString::from)
                    .collect()
            };
            TetherState::get(&window).set_picker_choices(ModelRc::new(VecModel::from(choices)));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        TetherState::get(window).on_choose(move |id, value| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let app = app.borrow();
            if id == PRESET_PICKER {
                let chosen = app
                    .tether_presets
                    .iter()
                    .find(|(_, name)| name.as_str() == value.as_str());
                app.library.tether_set_preset(chosen.map(|(id, _)| *id));
                TetherState::get(&window).set_preset_name(match chosen {
                    Some((_, name)) => SharedString::from(name.as_str()),
                    None => SharedString::default(),
                });
                return;
            }
            if let Some(setting) = TetherSetting::parse(id.as_str()) {
                app.library.tether_set(setting, value.as_str());
                // Optimistic, and corrected within one poll: the bar shows
                // the value the moment it is clicked, and the camera's own
                // `SettingsChanged` overwrites it — with the same value if
                // the body accepted, with the old one if it did not.
                set_controls(&app, &window);
            }
        });
    }
}

/// The label of "no develop preset", in the bar's picker.
///
/// A plain Rust string rather than a `Tr` helper because it must match, by
/// value, what `on_choose` compares against to mean "clear the preset".
fn no_preset() -> &'static str {
    "—"
}

/// Rebuilds the bar from what the camera last reported (ADR 0087 §2).
///
/// Called on `TetherSettingsChanged`, never on a timer: a bar that
/// repainted every 30 ms would flicker for a body that changed nothing.
pub(crate) fn refresh_tether(app: &App, window: &StudioWindow) {
    let settings = app.library.tether_settings();
    let state = TetherState::get(window);
    state.set_model(SharedString::from(settings.model.as_str()));
    state.set_can_capture(settings.can_capture);
    state.set_can_live_view(settings.can_live_view);
    set_controls(app, window);
}

/// The four exposure fields, in bar order, skipping what the body does not
/// expose.
fn set_controls(app: &App, window: &StudioWindow) {
    let settings = app.library.tether_settings();
    let controls: Vec<TetherControl> = TetherSetting::ALL
        .into_iter()
        .filter_map(|setting| {
            settings.get(setting).map(|value| TetherControl {
                id: SharedString::from(setting.as_str()),
                value: SharedString::from(value.value.as_str()),
                // A setting with no choices is one the body reports as free
                // text; there is nothing to offer in a picker, so it reads
                // like a read-only one.
                settable: !value.readonly && !value.choices.is_empty(),
            })
        })
        .collect();
    TetherState::get(window).set_controls(ModelRc::new(VecModel::from(controls)));
}

/// Pushes the newest live-view frame into the bar (ADR 0087 §6).
///
/// A frame that fails to decode is dropped rather than reported: the next
/// one is 50 ms away, and a viewfinder that stops to complain about one
/// bad frame is worse than one that skips it.
pub(crate) fn refresh_live_frame(app: &App, window: &StudioWindow) {
    let Some(bytes) = app.library.tether_live_frame() else {
        return;
    };
    let Ok(decoded) = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg) else {
        return;
    };
    let rgb = decoded.to_rgb8();
    let buffer = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::clone_from_slice(
        rgb.as_raw(),
        rgb.width(),
        rgb.height(),
    );
    TetherState::get(window).set_live_frame(Image::from_rgb8(buffer));
}
