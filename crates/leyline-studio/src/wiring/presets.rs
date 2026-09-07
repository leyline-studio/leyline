//! Wires develop presets and the panel they live in (`docs/presets.md`,
//! ADR 0058) (ADR 0045 §4).

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use crate::app::{App, report_error};
use crate::ui::{DevelopState, DialogState, PresetRow, StudioWindow, Tr};
use crate::wiring::develop::refresh_develop;
use leyline_sdk::{Preset, PresetFolderId, PresetSettings, PreviewKind};
use slint::{ComponentHandle, Global, ModelRc, SharedString, TimerMode, VecModel};

/// How long a preset must stay hovered before it is rendered (ADR 0058 §4).
///
/// A quarter of a second: long enough that running the pointer down the list
/// renders nothing, short enough that stopping on one feels like an answer
/// rather than a wait.
const TRIAL_DELAY: Duration = Duration::from_millis(250);

/// Connects the develop presets panel: applying, filing, starring, deleting,
/// the trial render, and the save-as-preset dialog (`docs/presets.md` §6).
pub(crate) fn wire_presets(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_open_preset_dialog(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            // The folder list is read as the dialog opens, not cached: one
            // can be created from inside the dialog itself.
            set_folder_names(&app.borrow(), &window);
            DialogState::get(&window).set_dialog_result(SharedString::default());
            DialogState::get(&window).set_dialog(SharedString::from("preset"));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_run_save_preset(move |name, mask, folder| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let name = name.trim();
            if name.is_empty() {
                DialogState::get(&window).set_dialog_result(Tr::get(&window).invoke_enter_a_name());
                return;
            }
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                DialogState::get(&window)
                    .set_dialog_result(Tr::get(&window).invoke_open_photo_in_develop_first());
                return;
            };
            let groups = crate::groups::from_mask(mask);
            if groups.is_empty() {
                DialogState::get(&window)
                    .set_dialog_result(Tr::get(&window).invoke_pick_at_least_one_group());
                return;
            }
            let filed = usize::try_from(folder)
                .ok()
                .and_then(|index| app.preset_folders.get(index))
                .map(|folder| folder.folder);
            let saved = app
                .library
                .create_preset(name, version, &groups)
                .and_then(|preset| {
                    // Two calls rather than one because filing is not a
                    // property of capturing: `create_preset` records
                    // *what* the preset is, `file_preset` where it goes.
                    if filed.is_some() {
                        app.library.file_preset(preset, filed)?;
                    }
                    Ok(())
                })
                .map_err(|e| e.to_string())
                .and_then(|()| refresh_presets(&mut app, &window));
            match saved {
                Ok(()) => DialogState::get(&window).set_dialog(SharedString::default()),
                Err(error) => {
                    DialogState::get(&window).set_dialog_result(
                        Tr::get(&window).invoke_save_failed(SharedString::from(error)),
                    );
                }
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_new_preset_folder(move |name| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let name = name.trim();
            if name.is_empty() {
                return;
            }
            let mut app = app.borrow_mut();
            let created = app
                .library
                .create_preset_folder(name)
                .map_err(|e| e.to_string())
                .and_then(|_| refresh_presets(&mut app, &window));
            match created {
                Ok(()) => set_folder_names(&app, &window),
                Err(error) => DialogState::get(&window).set_dialog_result(
                    Tr::get(&window).invoke_save_failed(SharedString::from(error)),
                ),
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_apply_preset(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            // Applying settles the trial one way or the other: the photo is
            // about to actually become what the hover was showing.
            stop_trial(&app, &window);
            let Some((_, version)) = app.develop else {
                return;
            };
            let Some(preset) = preset_at(&app, index) else {
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
            stop_trial(&app, &window);
            let Some(preset) = preset_at(&app, index) else {
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
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_favourite_preset(move |index, on| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some(preset) = preset_at(&app, index) else {
                return;
            };
            let starred = app
                .library
                .favourite_preset(preset, on)
                .map_err(|e| e.to_string())
                .and_then(|()| refresh_presets(&mut app, &window));
            if let Err(error) = starred {
                report_error(&window, &error);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_update_preset(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                report_error(&window, "open a photo in develop first");
                return;
            };
            let Some(stored) = usize::try_from(index)
                .ok()
                .and_then(|i| app.dev_presets.get(i))
                .cloned()
            else {
                return;
            };
            // Re-captured through the groups the preset already declares:
            // updating a "tone only" preset from a photo whose white balance
            // was also moved must not quietly widen what it touches
            // (`docs/presets.md` §3.1).
            let groups = match PresetSettings::parse(&stored.preset_json) {
                Ok(fields) => fields.groups,
                Err(error) => {
                    report_error(&window, &error.to_string());
                    return;
                }
            };
            let updated = app
                .library
                .update_preset(stored.preset, version, &groups)
                .map_err(|e| e.to_string())
                .and_then(|_| refresh_presets(&mut app, &window));
            if let Err(error) = updated {
                report_error(&window, &error);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_delete_preset_folder(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some(folder) = usize::try_from(index)
                .ok()
                .and_then(|i| app.preset_folders.get(i))
                .map(|folder| folder.folder)
            else {
                return;
            };
            let deleted = app
                .library
                .delete_preset_folder(folder)
                .map_err(|e| e.to_string())
                .and_then(|()| refresh_presets(&mut app, &window));
            if let Err(error) = deleted {
                report_error(&window, &error);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_toggle_preset_folder(move |row| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            // The root is a folder with no id, which is why the set is
            // keyed on an `Option` and why the row's key is looked up
            // rather than read off its `index`.
            let Some(&key) = usize::try_from(row)
                .ok()
                .and_then(|row| app.preset_rows.get(row))
            else {
                return;
            };
            if !app.preset_expanded.remove(&key) {
                app.preset_expanded.insert(key);
            }
            if let Err(error) = refresh_presets(&mut app, &window) {
                report_error(&window, &error);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_filter_presets(move |text| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            app.preset_filter = text.to_string();
            if let Err(error) = refresh_presets(&mut app, &window) {
                report_error(&window, &error);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_trial_preset(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let borrowed = app.borrow();
            if index < 0 {
                stop_trial(&borrowed, &window);
                return;
            }
            let app = Rc::clone(&app);
            let handle = window.as_weak();
            borrowed
                .preset_trial
                .start(TimerMode::SingleShot, TRIAL_DELAY, move || {
                    let Some(window) = handle.upgrade() else {
                        return;
                    };
                    let app = app.borrow();
                    render_trial(&app, &window, index);
                });
        });
    }
}

/// Renders the open photo with one preset laid over it, and shows it
/// (ADR 0058 §4). Writes nothing, anywhere.
fn render_trial(app: &App, window: &StudioWindow, index: i32) {
    let Some((asset, _)) = app.develop else {
        return;
    };
    let Some(preset) = preset_at(app, index) else {
        return;
    };
    // A trial that fails renders nothing rather than reporting: the pointer
    // is merely passing over a list, and an error dialog would be a strange
    // answer to a gesture nobody made deliberately.
    let Ok(image) = app
        .library
        .preset_preview(asset, PreviewKind::Small, preset)
    else {
        return;
    };
    let state = DevelopState::get(window);
    state.set_dev_trial_image(crate::models::rgb8_to_slint_image(&image));
    state.set_dev_trial_active(true);
}

/// Ends the trial: the pending render is cancelled and the viewer goes back
/// to the photo — which never changed, so there is nothing to undo.
fn stop_trial(app: &App, window: &StudioWindow) {
    app.preset_trial.stop();
    DevelopState::get(window).set_dev_trial_active(false);
}

/// The preset an `index` from the panel or the menu bar names.
fn preset_at(app: &App, index: i32) -> Option<leyline_sdk::PresetId> {
    usize::try_from(index)
        .ok()
        .and_then(|i| app.dev_presets.get(i))
        .map(|preset| preset.preset)
}

/// The folder names the save dialog offers, in `app.preset_folders` order.
fn set_folder_names(app: &App, window: &StudioWindow) {
    let names: Vec<SharedString> = app
        .preset_folders
        .iter()
        .map(|folder| SharedString::from(folder.name.as_str()))
        .collect();
    DevelopState::get(window).set_preset_folders(ModelRc::from(Rc::new(VecModel::from(names))));
}

/// Reloads the develop presets from the catalog: the flat list the menu bar
/// addresses, and the panel's rows.
pub(crate) fn refresh_presets(app: &mut App, window: &StudioWindow) -> Result<(), String> {
    let presets = app.library.presets().map_err(|e| e.to_string())?;
    let folders = app.library.preset_folders().map_err(|e| e.to_string())?;
    let names: Vec<SharedString> = presets
        .iter()
        .map(|preset| SharedString::from(preset.name.as_str()))
        .collect();
    app.dev_presets = presets;
    app.preset_folders = folders;
    DevelopState::get(window).set_dev_presets(ModelRc::from(Rc::new(VecModel::from(names))));
    let (rows, keys) = panel_rows(app, window);
    app.preset_rows = keys;
    DevelopState::get(window).set_dev_preset_rows(ModelRc::from(Rc::new(VecModel::from(rows))));
    Ok(())
}

/// Flattens folders and presets into the panel's rows, and the folder key of
/// each row beside them so a toggle knows what it opened.
///
/// The root comes first and holds what is filed nowhere; then each folder,
/// by name. Inside a folder, favourites first (ADR 0058 §2), then by name —
/// the order the catalog already returns them in is not it, because
/// favourites are a flag and not a position.
fn panel_rows(app: &App, window: &StudioWindow) -> (Vec<PresetRow>, Vec<Option<PresetFolderId>>) {
    let filter = app.preset_filter.trim().to_lowercase();
    let matches =
        |preset: &Preset| filter.is_empty() || preset.name.to_lowercase().contains(&filter);

    let mut rows = Vec::new();
    let mut keys = Vec::new();
    let mut groups: Vec<(Option<PresetFolderId>, String, i32)> = vec![(
        None,
        Tr::get(window).invoke_preset_root_folder().to_string(),
        -1,
    )];
    groups.extend(
        app.preset_folders
            .iter()
            .enumerate()
            .map(|(index, folder)| {
                (
                    Some(folder.folder),
                    folder.name.clone(),
                    i32::try_from(index).unwrap_or(-1),
                )
            }),
    );

    for (key, name, folder_index) in groups {
        let mut members: Vec<(usize, &Preset)> = app
            .dev_presets
            .iter()
            .enumerate()
            .filter(|(_, preset)| preset.folder == key && matches(preset))
            .collect();
        members.sort_by(|(_, a), (_, b)| {
            b.favourite
                .cmp(&a.favourite)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        // An empty root row would be a header over nothing on a fresh
        // library; an empty *folder* still shows, because ADR 0058 §2 made
        // folders exist independently of what is in them.
        if key.is_none() && members.is_empty() {
            continue;
        }
        // A search hides the folders it found nothing in: the answer to
        // "where is it" should not be a list of everywhere it is not.
        if !filter.is_empty() && members.is_empty() {
            continue;
        }
        // A search opens what it searched: hiding a match inside a
        // collapsed folder would make the filter look broken.
        let expanded = !filter.is_empty() || app.preset_expanded.contains(&key);
        rows.push(PresetRow {
            name: SharedString::from(name.as_str()),
            summary: SharedString::default(),
            is_folder: true,
            expanded,
            count: i32::try_from(members.len()).unwrap_or(0),
            favourite: false,
            index: folder_index,
        });
        keys.push(key);
        if !expanded {
            continue;
        }
        for (index, preset) in members {
            rows.push(PresetRow {
                name: SharedString::from(preset.name.as_str()),
                summary: SharedString::from(summary(window, &preset.preset_json)),
                is_folder: false,
                expanded: false,
                count: 0,
                favourite: preset.favourite,
                index: i32::try_from(index).unwrap_or(-1),
            });
            keys.push(key);
        }
    }
    (rows, keys)
}

/// What a preset changes, in plain terms (ADR 0058 §3) — "Exposure +0.35 ·
/// Contrast +12 · Temperature 5200 K".
///
/// Only the fields it actually carries, and at most four of them: the line
/// is two hundred pixels wide, and a summary that elides in the middle of
/// its third value says less than one that stops after its third value.
fn summary(window: &StudioWindow, preset_json: &str) -> String {
    let Ok(fields) = PresetSettings::parse(preset_json) else {
        return String::new();
    };
    let tr = Tr::get(window);
    let label = |id: &str| tr.invoke_preset_field(SharedString::from(id)).to_string();
    let mut parts: Vec<String> = Vec::new();

    if let Some(wb) = &fields.white_balance {
        parts.push(match wb {
            Some(wb) => format!("{} {:.0} K", label("temperature"), wb.temperature),
            // `Some(None)` is a preset that puts white balance *back* to as
            // shot — a real instruction, and one worth naming.
            None => format!("{} {}", label("wb"), label("as-shot")),
        });
    }
    // Zero is filtered here for the same reason it is filtered below: a
    // group is captured whole, so a "tone only" preset carries all six of
    // its fields and most of them are zero. "Exposure +0.00" is not what
    // the preset does, it is what it happens to contain.
    if let Some(v) = fields.exposure.filter(|v| *v != 0.0) {
        parts.push(format!("{} {v:+.2}", label("exposure")));
    }
    for (value, id) in [
        (fields.contrast, "contrast"),
        (fields.highlights, "highlights"),
        (fields.shadows, "shadows"),
        (fields.whites, "whites"),
        (fields.blacks, "blacks"),
        (fields.vibrance, "vibrance"),
        (fields.saturation, "saturation"),
    ] {
        if let Some(v) = value.filter(|v| *v != 0) {
            parts.push(format!("{} {v:+}", label(id)));
        }
    }
    for (present, id) in [
        (fields.lens_correction.is_some(), "lens"),
        (fields.noise_reduction.is_some(), "noise"),
        (fields.sharpening.is_some(), "sharpening"),
        (fields.crop.is_some(), "crop"),
    ] {
        if present {
            parts.push(label(id));
        }
    }
    if let Some(v) = fields.rotation.filter(|v| *v != 0.0) {
        parts.push(format!("{} {v:+.1}°", label("rotation")));
    }

    parts.truncate(4);
    parts.join(" · ")
}

/// The panel's expanded-folder set, reset to what a fresh library shows.
pub(crate) fn default_expanded() -> HashSet<Option<PresetFolderId>> {
    // The root open, the folders closed: what is filed nowhere is what a
    // library that has never made a folder has, and it should not need a
    // click to be seen.
    HashSet::from([None])
}
