//! The engine event pump (`docs/engine-api.md`) (ADR 0045 §4).
//!
//! Engine work is asynchronous: jobs are submitted from the wiring modules and
//! report back here, on a timer, where results are folded into `App` and
//! pushed to the UI.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use crate::app::{App, MAX_PREVIEW_JOBS, item_at, report_error};
use crate::models::{export_summary, import_summary, print_summary};
use crate::ui::{DialogState, GridState, StudioWindow, Tr};
use crate::wiring::folders::refresh_folders;
use crate::wiring::grid::{reload, show_details};
use crate::wiring::map::refresh_map_pins;
use leyline_sdk::{AssetId, Event, JobResult, PreviewKind};
use slint::{ComponentHandle, Global, Model, SharedString, Timer, TimerMode};

/// Starts the timer that pumps engine events into the UI — job progress,
/// finished imports and exports, freshly rendered thumbnails, added or
/// edited versions — and keeps a few thumbnail render jobs in flight off
/// the UI thread. The 30 ms interval is not a re-scan: it only drains the
/// `mpsc::Receiver` `Library::subscribe()` returns, because Slint's loop is
/// single-threaded and cannot be woken from the job threads that emit
/// events (`docs/engine-api.md` §3.2–3.3) — every UI update it triggers is
/// still driven by an event, never by the tick itself.
pub(crate) fn event_pump(app: &Rc<RefCell<App>>, window: &StudioWindow) -> Timer {
    let app = Rc::clone(app);
    let handle = window.as_weak();
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, Duration::from_millis(30), move || {
        let Some(window) = handle.upgrade() else {
            return;
        };
        let mut app = app.borrow_mut();
        while let Ok(event) = app.events.try_recv() {
            handle_event(&mut app, &window, event);
        }
        dispatch_thumbnails(&mut app);
    });
    timer
}

/// Reacts to one engine event on the UI thread.
pub(crate) fn handle_event(app: &mut App, window: &StudioWindow, event: Event) {
    match event {
        Event::PreviewReady {
            asset_id,
            kind: PreviewKind::Thumbnail,
        } => {
            set_thumbnail_cell(app, asset_id);
        }
        // The loupe's own render, queued rather than run inline so the
        // window never freezes on a first look at a photo.
        Event::PreviewReady { asset_id, kind } if kind == crate::wiring::grid::LOUPE_KIND => {
            crate::wiring::grid::loupe_preview_ready(app, window, asset_id);
        }
        Event::JobProgress {
            job_id,
            done,
            total,
        } => {
            let done = i32::try_from(done).unwrap_or(i32::MAX);
            let total = i32::try_from(total).unwrap_or(i32::MAX);
            if app.import_job == Some(job_id) {
                DialogState::get(window)
                    .set_dialog_result(Tr::get(window).invoke_importing_progress(done, total));
            } else if app.export_job == Some(job_id) {
                DialogState::get(window)
                    .set_dialog_result(Tr::get(window).invoke_exporting_progress(done, total));
            } else if app.print_job == Some(job_id) {
                DialogState::get(window)
                    .set_dialog_result(Tr::get(window).invoke_printing_progress(done, total));
            }
        }
        Event::JobFinished { job_id, result } => {
            if app.import_job == Some(job_id) {
                app.import_job = None;
                DialogState::get(window).set_dialog_result(match result {
                    JobResult::Import(report) => {
                        SharedString::from(import_summary(report.imported.len(), &report.skipped))
                    }
                    JobResult::Failed(reason) => {
                        Tr::get(window).invoke_import_failed(SharedString::from(reason))
                    }
                    _ => return,
                });
                // No explicit reload here: a successful import already
                // emitted `AssetsAdded` (handled below), and an import that
                // added nothing (every file skipped) leaves the grid as-is.
            } else if app.export_job == Some(job_id) {
                app.export_job = None;
                DialogState::get(window).set_dialog_result(match result {
                    JobResult::Export(report) => SharedString::from(export_summary(&report)),
                    JobResult::Failed(reason) => {
                        Tr::get(window).invoke_export_failed(SharedString::from(reason))
                    }
                    _ => return,
                });
            } else if app.print_job == Some(job_id) {
                app.print_job = None;
                DialogState::get(window).set_dialog_result(match result {
                    JobResult::Print(report) => SharedString::from(print_summary(&report)),
                    JobResult::Failed(reason) => {
                        Tr::get(window).invoke_print_failed(SharedString::from(reason))
                    }
                    _ => return,
                });
            } else if app.preview_jobs.remove(&job_id)
                && let JobResult::Failed(reason) = result
            {
                // Release builds have no console (`windows_subsystem =
                // "windows"`) to catch a bare `eprintln!`, so a thumbnail
                // render failure needs to reach the status line or it's
                // invisible — e.g. a missing LibRaw DLL dependency would
                // silently leave every thumbnail blank with no clue why.
                report_error(window, &reason);
            }
        }
        Event::AssetsChanged { asset_ids } => {
            // Another writer touched assets: refresh the side panel when
            // the selected photo is among them.
            let selected = GridState::get(window).get_selected();
            if let Some(asset) = item_at(app, selected).map(|item| item.asset_id)
                && asset_ids.contains(&asset)
            {
                show_details(app, window, selected);
            }
        }
        Event::VersionChanged { version_id } => {
            // A version's head moved — rating/label/pick, a develop commit,
            // a preset application or a reprocess (`docs/engine-api.md`
            // §3.2) — any of which can invalidate the cached thumbnail
            // (`cached_preview` follows the head revision id) or the row's
            // filter/sort position. Reload only when the version is one of
            // the loaded rows, and not while develop is open: the develop
            // view refreshes itself on every commit, the grid isn't visible,
            // and `on_exit_develop` reloads it unconditionally on the way
            // back out.
            if app.develop.is_none()
                && app.items.iter().any(|item| item.version_id == version_id)
                && let Err(error) = reload(app, window)
            {
                report_error(window, &error);
            }
        }
        Event::AssetsAdded { ref asset_ids } => {
            // A tethered shot or a watched-folder import lands here too
            // (`docs/adr/0038`, `docs/adr/0039`): both are ordinary imports
            // under the hood, so each panel's "last" line is filled in from
            // this same event rather than a dedicated one.
            if app.tether_connected
                && let Some(&asset) = asset_ids.last()
                && let Ok(details) = app.library.catalog().asset_details(asset)
            {
                app.tether_captured += 1;
                DialogState::get(window)
                    .set_tether_captured_count(i32::try_from(app.tether_captured).unwrap_or(0));
                DialogState::get(window)
                    .set_tether_last_captured(SharedString::from(details.filename.as_str()));
            }
            if app.watch_active
                && let Some(&asset) = asset_ids.last()
                && let Ok(details) = app.library.catalog().asset_details(asset)
            {
                app.watch_imported += 1;
                DialogState::get(window)
                    .set_watch_imported_count(i32::try_from(app.watch_imported).unwrap_or(0));
                DialogState::get(window)
                    .set_watch_last_imported(SharedString::from(details.filename.as_str()));
            }
            // New assets — our own import, or another writer's — can change
            // both the total count and the visible window; skip while
            // develop is open for the same reason as `VersionChanged`.
            if app.develop.is_none()
                && let Err(error) = reload(app, window)
            {
                report_error(window, &error);
            }
            // An import can also create folders (ADR 0055 §2), and a sidebar
            // whose tree stops at the last relaunch would send the user
            // looking for photos it just told them arrived.
            if let Err(error) = refresh_folders(app, window) {
                report_error(window, &error);
            }
            // Map mode fetches pins once on entry, but tethered capture
            // and watched-folder import both add assets while it stays
            // open (`docs/adr/0038`, `docs/adr/0039`) — and a GPS-tagged
            // one belongs on the map right away, not on the next entry.
            if app.map.is_some() {
                refresh_map_pins(app, window);
            }
        }
        Event::TetherConnected => {
            app.tether_connected = true;
            app.tether_captured = 0;
            DialogState::get(window).set_tether_connected(true);
            DialogState::get(window).set_tether_captured_count(0);
            DialogState::get(window).set_tether_last_captured(SharedString::default());
        }
        Event::TetherDisconnected { reason } => {
            app.tether_connected = false;
            DialogState::get(window).set_tether_connected(false);
            DialogState::get(window).set_tether_status(match reason {
                Some(reason) => SharedString::from(reason),
                None => SharedString::default(),
            });
        }
        Event::WatchStarted { .. } => {
            app.watch_active = true;
            app.watch_imported = 0;
            DialogState::get(window).set_watch_active(true);
            DialogState::get(window).set_watch_imported_count(0);
            DialogState::get(window).set_watch_last_imported(SharedString::default());
        }
        Event::WatchStopped { reason } => {
            app.watch_active = false;
            DialogState::get(window).set_watch_active(false);
            DialogState::get(window).set_watch_status(match reason {
                Some(reason) => SharedString::from(reason),
                None => SharedString::default(),
            });
        }
        _ => {}
    }
}

/// Fills the grid cell of an asset with its freshly cached thumbnail.
pub(crate) fn set_thumbnail_cell(app: &mut App, asset: AssetId) {
    let Some(index) = app.items.iter().position(|item| item.asset_id == asset) else {
        return; // scrolled out of the loaded window meanwhile
    };
    let Ok(Some(file)) = app.library.cached_preview(asset, PreviewKind::Thumbnail) else {
        return;
    };
    let Ok(image) = slint::Image::load_from_path(&file.path) else {
        return;
    };
    if let Some(mut cell) = app.cells.row_data(index) {
        cell.thumbnail = image;
        app.cells.set_row_data(index, cell);
    }
}

/// Keeps up to [`MAX_PREVIEW_JOBS`] thumbnail renders in flight, visible
/// rows first (the pending queue is ordered that way by `load_window`).
pub(crate) fn dispatch_thumbnails(app: &mut App) {
    while app.preview_jobs.len() < MAX_PREVIEW_JOBS {
        let Some(index) = app.pending.pop_front() else {
            return;
        };
        let Some(asset) = app.items.get(index).map(|item| item.asset_id) else {
            continue; // the loaded window moved since this row was queued
        };
        let job = app.library.preview_async(asset, PreviewKind::Thumbnail);
        app.preview_jobs.insert(job);
    }
}
