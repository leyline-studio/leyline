//! Wires `DevelopState`: the edit session, every adjustment, the revision
//! history and the settings clipboard (ADR 0045 §4).
//!
//! The largest wiring module, mirroring `ui/panels/develop.slint`. The rules
//! deciding *what* a slider does live in `crate::develop`, free of any Slint
//! type; this module only moves values across the boundary.

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::{App, item_at, report_error, selected_versions};
use crate::develop;
use crate::format;
use crate::models::{CURVE_CANVAS_SIZE, dev_model, rgb8_to_slint_image};
use crate::ui::{CurveMarker, DevelopState, GridState, LibraryState, StudioWindow, Tr};
use crate::wiring::dialogs::spot_defaults;
use crate::wiring::grid::reload;
use leyline_sdk::{AssetId, GridQuery, PreviewKind, SettingsGroup, VersionId};
use slint::{ComponentHandle, Global, ModelRc, SharedString, VecModel};

/// Develop settings groups copy/paste captures and applies — the same
/// default set a saved preset captures (`docs/presets.md` §3.1), Geometry
/// excluded since a crop/rotation is a per-photo judgment, not a
/// transferable style.
pub(crate) const CLIPBOARD_GROUPS: &[SettingsGroup] = &[
    SettingsGroup::WhiteBalance,
    SettingsGroup::Tone,
    SettingsGroup::Presence,
    SettingsGroup::LensCorrection,
    SettingsGroup::Detail,
];

/// Copy/paste develop settings between photos (distinct from named
/// presets, item 4 of the Lightroom/Darktable workflow-gap survey).
pub(crate) fn wire_settings_clipboard(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_copy_settings(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some(version) =
                item_at(&app, GridState::get(&window).get_selected()).map(|item| item.version_id)
            else {
                return;
            };
            match app.library.capture_settings(version, CLIPBOARD_GROUPS) {
                Ok(settings) => {
                    app.dev_clipboard = Some(settings);
                    DevelopState::get(&window).set_has_settings_clipboard(true);
                }
                Err(error) => report_error(&window, &error.to_string()),
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_paste_settings(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some(clipboard) = app.dev_clipboard.clone() else {
                return;
            };
            let versions = selected_versions(&app, GridState::get(&window).get_selected());
            if versions.is_empty() {
                return;
            }
            let applied = app
                .library
                .apply_settings(&clipboard, &versions, |_, _| {})
                .map_err(|e| e.to_string())
                .and_then(|report| {
                    if report.failed.is_empty() {
                        Ok(())
                    } else {
                        Err(report
                            .failed
                            .iter()
                            .map(|f| f.reason.as_str())
                            .collect::<Vec<_>>()
                            .join("; "))
                    }
                })
                .and_then(|()| reload(&mut app, &window));
            if let Err(error) = applied {
                report_error(&window, &error);
            }
            // Re-render the develop canvas too when the pasted-onto set
            // includes the photo currently open there.
            if app
                .develop
                .is_some_and(|(_, version)| versions.contains(&version))
            {
                if let Err(error) = refresh_develop(&mut app, &window) {
                    report_error(&window, &error);
                }
            }
        });
    }
}

/// Connects the develop view: entering, sliders, undo / redo, leaving.
pub(crate) fn wire_develop(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_enter_develop(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((asset, version, filename)) =
                item_at(&app, GridState::get(&window).get_selected())
                    .map(|item| (item.asset_id, item.version_id, item.filename.clone()))
            else {
                return;
            };
            app.develop = Some((asset, version));
            match refresh_develop(&mut app, &window) {
                Ok(()) => {
                    DevelopState::get(&window)
                        .set_develop_filename(SharedString::from(filename.as_str()));
                    DevelopState::get(&window).set_develop_mode(true);
                }
                Err(error) => {
                    app.develop = None;
                    report_error(&window, &error);
                }
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_toggle_compare(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((asset, _version)) = app.develop else {
                return;
            };
            let showing_before = !DevelopState::get(&window).get_dev_compare();
            if showing_before && app.dev_before.is_none() {
                match app.library.preview_before(asset, PreviewKind::Small) {
                    Ok(rendered) => {
                        let image = rgb8_to_slint_image(&rendered);
                        DevelopState::get(&window).set_develop_image_before(image.clone());
                        app.dev_before = Some(image);
                    }
                    Err(error) => {
                        report_error(&window, &error.to_string());
                        return;
                    }
                }
            }
            DevelopState::get(&window).set_dev_compare(showing_before);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_exit_develop(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            app.develop = None;
            DevelopState::get(&window).set_develop_mode(false);
            // Edits invalidated the thumbnails: rebuild the grid.
            if let Err(error) = reload(&mut app, &window) {
                report_error(&window, &error);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_prev(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            develop_navigate(&mut app.borrow_mut(), &window, -1);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_next(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            develop_navigate(&mut app.borrow_mut(), &window, 1);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_switch(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            develop_switch_to(&mut app.borrow_mut(), &window, index);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_edit(move |slider, value| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let committed = (|| {
                let mut session = app.library.edit(version)?;
                let Some((param, value)) =
                    develop::action(slider.as_str(), f64::from(value), session.settings())
                else {
                    return Ok(());
                };
                session.set(param, value)?;
                session.commit().map(|_| ())
            })();
            if let Err(error) = committed
                .map_err(|e| e.to_string())
                .and_then(|()| refresh_develop(&mut app, &window))
            {
                report_error(&window, &error);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_edit_hsl_band(move |index, field, value| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let committed = (|| {
                let mut session = app.library.edit(version)?;
                let Some((param, value)) = develop::hsl_band_action(
                    index as usize,
                    field.as_str(),
                    f64::from(value),
                    session.settings(),
                ) else {
                    return Ok(());
                };
                session.set(param, value)?;
                session.commit().map(|_| ())
            })();
            if let Err(error) = committed
                .map_err(|e| e.to_string())
                .and_then(|()| refresh_develop(&mut app, &window))
            {
                report_error(&window, &error);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_edit_color_grading_zone(move |zone, field, value| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let committed = (|| {
                let mut session = app.library.edit(version)?;
                let Some((param, value)) = develop::color_grading_zone_action(
                    zone.as_str(),
                    field.as_str(),
                    f64::from(value),
                    session.settings(),
                ) else {
                    return Ok(());
                };
                session.set(param, value)?;
                session.commit().map(|_| ())
            })();
            if let Err(error) = committed
                .map_err(|e| e.to_string())
                .and_then(|()| refresh_develop(&mut app, &window))
            {
                report_error(&window, &error);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_crop_drag(move |px, py, rx, ry, vw, vh, iw, ih| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let committed = (|| {
                let mut session = app.library.edit(version)?;
                let Some((param, value)) = develop::drag_crop(
                    (f64::from(px), f64::from(py)),
                    (f64::from(rx), f64::from(ry)),
                    (f64::from(vw), f64::from(vh)),
                    (f64::from(iw), f64::from(ih)),
                    &session.settings().crop,
                ) else {
                    return Ok(());
                };
                session.set(param, value)?;
                session.commit().map(|_| ())
            })();
            if let Err(error) = committed
                .map_err(|e| e.to_string())
                .and_then(|()| refresh_develop(&mut app, &window))
            {
                report_error(&window, &error);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_curve_click(move |mx, my, w, h| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let committed = (|| {
                let mut session = app.library.edit(version)?;
                // The curve canvas isn't letterboxed — it's its own square
                // widget — so this is a plain axis flip, not `letterbox_unit`.
                let click = (
                    f64::from(mx) / f64::from(w),
                    1.0 - f64::from(my) / f64::from(h),
                );
                let Some((param, value)) =
                    develop::curve_point(click, &session.settings().tone_curve.points)
                else {
                    return Ok(());
                };
                session.set(param, value)?;
                session.commit().map(|_| ())
            })();
            if let Err(error) = committed
                .map_err(|e| e.to_string())
                .and_then(|()| refresh_develop(&mut app, &window))
            {
                report_error(&window, &error);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_curve_reset(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let (param, value) = develop::reset_curve();
            let committed = (|| {
                let mut session = app.library.edit(version)?;
                session.set(param, value)?;
                session.commit().map(|_| ())
            })();
            if let Err(error) = committed
                .map_err(|e| e.to_string())
                .and_then(|()| refresh_develop(&mut app, &window))
            {
                report_error(&window, &error);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_spot_click(move |sx, sy, tx, ty, vw, vh, iw, ih| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let radius_feather_opacity = match spot_defaults(
                DevelopState::get(&window).get_spot_radius_text().as_str(),
                DevelopState::get(&window).get_spot_feather_text().as_str(),
                DevelopState::get(&window).get_spot_opacity_text().as_str(),
            ) {
                Ok(defaults) => defaults,
                Err(error) => {
                    report_error(&window, &error);
                    return;
                }
            };
            let committed = (|| {
                let mut session = app.library.edit(version)?;
                let Some((param, value)) = develop::place_spot(
                    (f64::from(sx), f64::from(sy)),
                    (f64::from(tx), f64::from(ty)),
                    (f64::from(vw), f64::from(vh)),
                    (f64::from(iw), f64::from(ih)),
                    radius_feather_opacity,
                    &session.settings().spot_removal,
                ) else {
                    return Ok(());
                };
                session.set(param, value)?;
                session.commit().map(|_| ())
            })();
            if let Err(error) = committed
                .map_err(|e| e.to_string())
                .and_then(|()| refresh_develop(&mut app, &window))
            {
                report_error(&window, &error);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_spot_undo(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let committed = (|| {
                let mut session = app.library.edit(version)?;
                let Some((param, value)) =
                    develop::undo_last_spot(&session.settings().spot_removal)
                else {
                    return Ok(());
                };
                session.set(param, value)?;
                session.commit().map(|_| ())
            })();
            if let Err(error) = committed
                .map_err(|e| e.to_string())
                .and_then(|()| refresh_develop(&mut app, &window))
            {
                report_error(&window, &error);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_spot_reset(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let (param, value) = develop::reset_spots();
            let committed = (|| {
                let mut session = app.library.edit(version)?;
                session.set(param, value)?;
                session.commit().map(|_| ())
            })();
            if let Err(error) = committed
                .map_err(|e| e.to_string())
                .and_then(|()| refresh_develop(&mut app, &window))
            {
                report_error(&window, &error);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_undo(move || {
            if let Some(window) = handle.upgrade() {
                history_step(&mut app.borrow_mut(), &window, true);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_redo(move || {
            if let Some(window) = handle.upgrade() {
                history_step(&mut app.borrow_mut(), &window, false);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_checkout_history_row(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            if let Ok(index) = usize::try_from(index) {
                checkout_history_row(&mut app.borrow_mut(), &window, index);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_reprocess(move || {
            if let Some(window) = handle.upgrade() {
                reprocess_current(&mut app.borrow_mut(), &window);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_reprocess_library(move || {
            if let Some(window) = handle.upgrade() {
                reprocess_library(&mut app.borrow_mut(), &window);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_reprocess_selected(move || {
            if let Some(window) = handle.upgrade() {
                reprocess_selected(&mut app.borrow_mut(), &window);
            }
        });
    }
}

/// Migrates every version in the library to the engine's current stage
/// versions (`docs/engine-api.md` §10.4), regardless of the active grid
/// filter.
/// Reprocessing only rewrites `settings_json` — no pixels render during the
/// call — so this runs synchronously rather than as a tracked job, the same
/// way classement writes do.
pub(crate) fn reprocess_library(app: &mut App, window: &StudioWindow) {
    let query = GridQuery::default();
    let outcome = (|| {
        let count = app.library.catalog().count(&query)?;
        let all = GridQuery {
            range: 0..u32::try_from(count).unwrap_or(u32::MAX),
            ..query
        };
        let versions: Vec<VersionId> = app
            .library
            .catalog()
            .grid(&all)?
            .into_iter()
            .map(|item| item.version_id)
            .collect();
        app.library.reprocess(&versions, |_, _| {})
    })();
    match outcome {
        Ok(report) => {
            LibraryState::get(window).set_status_line(Tr::get(window).invoke_reprocessed(
                i32::try_from(report.reprocessed.len()).unwrap_or(i32::MAX),
                i32::try_from(report.already_current.len()).unwrap_or(i32::MAX),
                i32::try_from(report.failed.len()).unwrap_or(i32::MAX),
            ));
        }
        Err(error) => report_error(window, &error.to_string()),
    }
}

/// Grid context menu ▸ Reprocess (ADR 0021): the same `Library::reprocess`
/// call as `reprocess_library`/Shift+R, bounded to the single photo the
/// context menu was opened on rather than the whole catalog.
pub(crate) fn reprocess_selected(app: &mut App, window: &StudioWindow) {
    let Some(version) =
        item_at(app, GridState::get(window).get_selected()).map(|item| item.version_id)
    else {
        return;
    };
    match app.library.reprocess(&[version], |_, _| {}) {
        Ok(report) => {
            LibraryState::get(window).set_status_line(Tr::get(window).invoke_reprocessed(
                i32::try_from(report.reprocessed.len()).unwrap_or(i32::MAX),
                i32::try_from(report.already_current.len()).unwrap_or(i32::MAX),
                i32::try_from(report.failed.len()).unwrap_or(i32::MAX),
            ));
        }
        Err(error) => report_error(window, &error.to_string()),
    }
}

/// Moves the develop head one revision back or forward, then refreshes.
pub(crate) fn history_step(app: &mut App, window: &StudioWindow, undo: bool) {
    let Some((_, version)) = app.develop else {
        return;
    };
    let moved = (|| {
        let mut session = app.library.edit(version)?;
        if undo { session.undo() } else { session.redo() }
    })();
    if let Err(error) = moved
        .map_err(|e| e.to_string())
        .and_then(|_| refresh_develop(app, window))
    {
        report_error(window, &error);
    }
}

/// Jumps the open photo's develop history directly to `dev_history[index]`
/// — the history panel's click-to-jump, beyond undo/redo's one-step
/// movement.
pub(crate) fn checkout_history_row(app: &mut App, window: &StudioWindow, index: usize) {
    let Some((_, version)) = app.develop else {
        return;
    };
    let Some(revision) = app.dev_history.get(index).map(|row| row.revision) else {
        return;
    };
    let moved = (|| {
        let mut session = app.library.edit(version)?;
        session.checkout(revision)
    })();
    if let Err(error) = moved
        .map_err(|e| e.to_string())
        .and_then(|_| refresh_develop(app, window))
    {
        report_error(window, &error);
    }
}

/// Migrates the open photo to the engine's current stage versions
/// (`docs/engine-api.md` §10.4) — a new revision with the same parameter
/// values, so a photo imported before a rendering feature existed (e.g.
/// lens correction) can pick it up without the user touching a slider. A
/// no-op, silently, when the head is already current.
pub(crate) fn reprocess_current(app: &mut App, window: &StudioWindow) {
    let Some((_, version)) = app.develop else {
        return;
    };
    let reprocessed = (|| {
        let mut session = app.library.edit(version)?;
        session.reprocess()
    })();
    if let Err(error) = reprocessed
        .map_err(|e| e.to_string())
        .and_then(|_| refresh_develop(app, window))
    {
        report_error(window, &error);
    }
}

/// Renders the develop preview at the head settings and mirrors those
/// settings into the sliders.
/// Moves the develop view to the grid row `delta` away from the currently
/// selected one (±1), without leaving develop mode. A no-op past either
/// end of the grid, or if that row isn't in the currently loaded window
/// (`item_at`) — crossing a virtual-scroll window boundary while develop
/// is open is rare enough not to warrant reloading the grid for it.
pub(crate) fn develop_navigate(app: &mut App, window: &StudioWindow, delta: i32) {
    develop_switch_to(app, window, GridState::get(window).get_selected() + delta);
}

/// Switches develop to whole-grid index `next` directly, without leaving
/// develop mode — the filmstrip's click-to-switch, and what
/// [`develop_navigate`]'s ±1 arrow-key steps reduce to.
pub(crate) fn develop_switch_to(app: &mut App, window: &StudioWindow, next: i32) {
    if next < 0 || next >= GridState::get(window).get_total_cells() {
        return;
    }
    let Some((asset, version, filename)) =
        item_at(app, next).map(|item| (item.asset_id, item.version_id, item.filename.clone()))
    else {
        return;
    };
    GridState::get(window).set_selected(next);
    app.develop = Some((asset, version));
    match refresh_develop(app, window) {
        Ok(()) => {
            DevelopState::get(window).set_develop_filename(SharedString::from(filename.as_str()))
        }
        Err(error) => report_error(window, &error),
    }
}

pub(crate) fn refresh_develop(app: &mut App, window: &StudioWindow) -> Result<(), String> {
    let Some((asset, version)) = app.develop else {
        return Ok(());
    };
    let (settings, history) = {
        let session = app.library.edit(version).map_err(|e| e.to_string())?;
        let history = session.history().map_err(|e| e.to_string())?;
        (session.settings().clone(), history)
    };
    DevelopState::get(window).set_dev(dev_model(&settings));
    // Only the file name: the panel has no room for `Profiles/Camera/…`,
    // and that prefix is the same for every imported profile anyway.
    DevelopState::get(window).set_camera_profile_name(SharedString::from(
        settings
            .camera_profile
            .as_ref()
            .map_or("", |profile| {
                profile
                    .path
                    .rsplit('/')
                    .next()
                    .unwrap_or(profile.path.as_str())
            })
            .to_owned(),
    ));
    if app.dev_history_version != Some(version) {
        app.dev_history.clear();
        app.dev_history_version = Some(version);
    }
    for row in history.iter() {
        if !app
            .dev_history
            .iter()
            .any(|existing| existing.revision == row.revision)
        {
            app.dev_history.push(row.clone());
        }
    }
    app.dev_history.sort_by_key(|row| row.created_at);
    let current = history.first().map(|row| row.revision);
    let rows: Vec<SharedString> = app
        .dev_history
        .iter()
        .map(|row| SharedString::from(format::capture_date(row.created_at)))
        .collect();
    DevelopState::get(window).set_dev_history(ModelRc::from(Rc::new(VecModel::from(rows))));
    DevelopState::get(window).set_dev_history_current(
        i32::try_from(
            app.dev_history
                .iter()
                .position(|row| Some(row.revision) == current)
                .unwrap_or(0),
        )
        .unwrap_or(0),
    );
    let (path, markers) = develop::curve_layout(&settings.tone_curve.points, CURVE_CANVAS_SIZE);
    DevelopState::get(window).set_dev_curve_path(SharedString::from(path));
    DevelopState::get(window).set_dev_curve_points(ModelRc::from(Rc::new(VecModel::from(
        markers
            .into_iter()
            .map(|(x, y)| CurveMarker {
                x: x as f32,
                y: y as f32,
            })
            .collect::<Vec<_>>(),
    ))));
    DevelopState::get(window)
        .set_dev_spot_count(i32::try_from(settings.spot_removal.len()).unwrap_or(i32::MAX));
    let file = app
        .library
        .preview(asset, PreviewKind::Small)
        .map_err(|e| e.to_string())?;
    let image = slint::Image::load_from_path(&file.path)
        .map_err(|_| format!("cannot load preview {}", file.path.display()))?;
    DevelopState::get(window).set_develop_image(image);
    if let Ok(bins) = app.library.histogram(asset, PreviewKind::Small) {
        const CANVAS: (f64, f64) = (256.0, 90.0);
        let scale_max = bins.iter().flatten().copied().max().unwrap_or(0);
        DevelopState::get(window).set_dev_histogram_r(SharedString::from(
            develop::histogram_layout(&bins[0], scale_max, CANVAS.0, CANVAS.1),
        ));
        DevelopState::get(window).set_dev_histogram_g(SharedString::from(
            develop::histogram_layout(&bins[1], scale_max, CANVAS.0, CANVAS.1),
        ));
        DevelopState::get(window).set_dev_histogram_b(SharedString::from(
            develop::histogram_layout(&bins[2], scale_max, CANVAS.0, CANVAS.1),
        ));
    }
    // The develop target just changed (entered develop, or navigated to a
    // neighboring photo): any cached "before" render is for the wrong photo
    // now, and Compare Before/After starts back on "after" each time.
    app.dev_before = None;
    DevelopState::get(window).set_dev_compare(false);
    DevelopState::get(window).set_develop_image_before(slint::Image::default());
    Ok(())
}

/// Opens develop mode for an explicit `(asset, version)` pair rather than
/// the grid's current selection — `on_enter_develop`'s equivalent for a
/// jump that didn't come from clicking a grid cell (a map pin here; the
/// history panel's `checkout_history_row` has its own similar direct-jump
/// need but for a revision within the version already open).
pub(crate) fn enter_develop_for(
    app: &mut App,
    window: &StudioWindow,
    asset: AssetId,
    version: VersionId,
) {
    let filename = app
        .library
        .catalog()
        .asset_details(asset)
        .map(|details| details.filename)
        .unwrap_or_default();
    app.develop = Some((asset, version));
    match refresh_develop(app, window) {
        Ok(()) => {
            DevelopState::get(window).set_develop_filename(SharedString::from(filename.as_str()));
            DevelopState::get(window).set_develop_mode(true);
        }
        Err(error) => {
            app.develop = None;
            report_error(window, &error);
        }
    }
}
