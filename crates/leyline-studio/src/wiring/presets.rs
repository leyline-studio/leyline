//! Wires develop presets (`docs/presets.md`) (ADR 0045 §4).

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::{App, report_error};
use crate::ui::{DevelopState, DialogState, StudioWindow, Tr};
use crate::wiring::develop::refresh_develop;
use leyline_sdk::SettingsGroup;
use slint::{ComponentHandle, Global, ModelRc, SharedString, VecModel};

/// Connects the develop presets panel: applying, deleting, and the
/// save-as-preset dialog (`docs/presets.md` §6).
pub(crate) fn wire_presets(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let handle = window.as_weak();
        DevelopState::get(window).on_open_preset_dialog(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            DialogState::get(&window).set_dialog_result(SharedString::default());
            DialogState::get(&window).set_dialog(SharedString::from("preset"));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_run_save_preset(
            move |name, white_balance, tone, presence, lens_correction, detail, geometry| {
                let Some(window) = handle.upgrade() else {
                    return;
                };
                let name = name.trim();
                if name.is_empty() {
                    DialogState::get(&window)
                        .set_dialog_result(Tr::get(&window).invoke_enter_a_name());
                    return;
                }
                let mut app = app.borrow_mut();
                let Some((_, version)) = app.develop else {
                    DialogState::get(&window)
                        .set_dialog_result(Tr::get(&window).invoke_open_photo_in_develop_first());
                    return;
                };
                let flags = [
                    (white_balance, SettingsGroup::WhiteBalance),
                    (tone, SettingsGroup::Tone),
                    (presence, SettingsGroup::Presence),
                    (lens_correction, SettingsGroup::LensCorrection),
                    (detail, SettingsGroup::Detail),
                    (geometry, SettingsGroup::Geometry),
                ];
                let groups: Vec<SettingsGroup> = flags
                    .into_iter()
                    .filter_map(|(on, group)| on.then_some(group))
                    .collect();
                if groups.is_empty() {
                    DialogState::get(&window)
                        .set_dialog_result(Tr::get(&window).invoke_pick_at_least_one_group());
                    return;
                }
                let saved = app
                    .library
                    .create_preset(name, version, &groups)
                    .map_err(|e| e.to_string())
                    .and_then(|_| refresh_presets(&mut app, &window));
                match saved {
                    Ok(()) => DialogState::get(&window).set_dialog(SharedString::default()),
                    Err(error) => {
                        DialogState::get(&window).set_dialog_result(
                            Tr::get(&window).invoke_save_failed(SharedString::from(error)),
                        );
                    }
                }
            },
        );
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_apply_preset(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let Some(preset) = usize::try_from(index)
                .ok()
                .and_then(|i| app.dev_presets.get(i))
                .map(|p| p.preset)
            else {
                return;
            };
            let applied = app
                .library
                .apply_preset(preset, &[version], |_, _| {})
                .map_err(|e| e.to_string());
            match applied {
                Ok(report) => {
                    if let Some(failed) = report.failed.first() {
                        report_error(&window, &failed.reason);
                    } else if let Err(error) = refresh_develop(&mut app, &window) {
                        report_error(&window, &error);
                    }
                }
                Err(error) => report_error(&window, &error),
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_delete_preset(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some(preset) = usize::try_from(index)
                .ok()
                .and_then(|i| app.dev_presets.get(i))
                .map(|p| p.preset)
            else {
                return;
            };
            let deleted = app
                .library
                .delete_preset(preset)
                .map_err(|e| e.to_string())
                .and_then(|()| refresh_presets(&mut app, &window));
            if let Err(error) = deleted {
                report_error(&window, &error);
            }
        });
    }
}

/// Reloads the develop sidebar's preset list from the catalog.
pub(crate) fn refresh_presets(app: &mut App, window: &StudioWindow) -> Result<(), String> {
    let presets = app.library.presets().map_err(|e| e.to_string())?;
    let names: Vec<SharedString> = presets
        .iter()
        .map(|preset| SharedString::from(preset.name.as_str()))
        .collect();
    app.dev_presets = presets;
    DevelopState::get(window).set_dev_presets(ModelRc::from(Rc::new(VecModel::from(names))));
    Ok(())
}
