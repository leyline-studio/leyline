//! Undo, redo, browsing the revision history, and reprocessing (ADR 0045 §4).
//!
//! Reprocessing sits here rather than in its own module because it is the same
//! act as checking out a revision: re-rendering what a revision describes.

use std::cell::RefCell;
use std::rc::Rc;

use super::refresh_develop;
use crate::app::{App, item_at, report_error};
use crate::ui::{DevelopState, GridState, LibraryState, StudioWindow, Tr};
use leyline_sdk::{GridQuery, VersionId};
use slint::{ComponentHandle, Global};

pub(super) fn wire_history(app: &Rc<RefCell<App>>, window: &StudioWindow) {
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
