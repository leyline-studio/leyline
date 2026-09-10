//! The engine event pump (`docs/engine-api.md`) (ADR 0045 §4).
//!
//! Engine work is asynchronous: jobs are submitted from the wiring modules and
//! report back here, on a timer, where results are folded into `App` and
//! pushed to the UI.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use crate::app::{App, MAX_PREVIEW_JOBS, item_at, report_error};
use crate::models::{BatchOutcome, export_outcome, print_outcome};
use crate::ui::{DialogState, GridState, LibraryState, StudioWindow, TetherState, Tr};
use crate::wiring::filters::refresh_shot_facets;
use crate::wiring::folders::refresh_folders;
use crate::wiring::grid::{reload, show_details};
use crate::wiring::jobs::{Task, finish_task, show_task};
use crate::wiring::map::refresh_map_pins;
use crate::wiring::tether::{refresh_live_frame, refresh_tether};
use leyline_sdk::{AssetId, Event, JobId, JobResult, PreviewKind};
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

/// Appends "stopped; what was done is kept" to a batch's summary when it
/// did not reach the end of its list (ADR 0139 §4). The summary itself
/// already names what was done, which is the half that matters first.
fn with_stop(window: &StudioWindow, message: SharedString, cancelled: bool) -> SharedString {
    if cancelled {
        SharedString::from(format!(
            "{message}{}",
            Tr::get(window).invoke_stopped_suffix()
        ))
    } else {
        message
    }
}

/// Which kind of batch a job id belongs to, or `None` for the jobs the bar
/// says nothing about — a thumbnail render, a derivation.
fn task_of(app: &App, job: JobId) -> Option<Task> {
    let known = [
        (app.import_job, Task::Import),
        (app.scan_job, Task::Scan),
        (app.export_job, Task::Export),
        (app.print_job, Task::Print),
        (app.sheet_job, Task::ContactSheet),
        (app.reprocess_job, Task::Reprocess),
        (app.cull_job, Task::Cull),
    ];
    known
        .into_iter()
        .find(|(id, _)| *id == Some(job))
        .map(|(_, task)| task)
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
            let (done_u64, total_u64) = (done, total);
            let done = i32::try_from(done).unwrap_or(i32::MAX);
            let total = i32::try_from(total).unwrap_or(i32::MAX);
            // The bar and the count say the same thing in two registers: a
            // proportion answers "how long", a count answers "how much".
            // Seventeen thousand photos need both.
            if app.import_job == Some(job_id)
                || app.export_job == Some(job_id)
                || app.print_job == Some(job_id)
                || app.sheet_job == Some(job_id)
            {
                let state = DialogState::get(window);
                state.set_job_progress(if total > 0 {
                    done as f32 / total as f32
                } else {
                    0.0
                });
                state.set_job_caption(SharedString::from(format!("{done} / {total}")));
                state.set_job_done(false);
            }
            if app.cull_job == Some(job_id) {
                // The status line, not the dialog: culling has no dialog,
                // because the window stays usable while it runs and what
                // it produces is a change of what the grid shows.
                LibraryState::get(window)
                    .set_status_line(Tr::get(window).invoke_culling_progress(done, total));
            }
            // The corner of the window, whatever started the job and
            // whether or not its dialog is still open (ADR 0139 §4).
            if let Some(task) = task_of(app, job_id) {
                show_task(app, window, job_id, task, done_u64, total_u64);
            }
            if app.import_job == Some(job_id) {
                DialogState::get(window)
                    .set_dialog_result(Tr::get(window).invoke_importing_progress(done, total));
            } else if app.export_job == Some(job_id) {
                DialogState::get(window)
                    .set_dialog_result(Tr::get(window).invoke_exporting_progress(done, total));
            } else if app.print_job == Some(job_id) {
                DialogState::get(window)
                    .set_dialog_result(Tr::get(window).invoke_printing_progress(done, total));
            } else if app.sheet_job == Some(job_id) {
                DialogState::get(window)
                    .set_dialog_result(Tr::get(window).invoke_sheet_progress(done, total));
            }
        }
        Event::JobFinished { job_id, result } => {
            finish_task(app, window, job_id);
            // Whatever the outcome, the dialog stops waiting: the bar reads
            // full and the way out is spelled out rather than guessed at.
            if app.import_job == Some(job_id)
                || app.export_job == Some(job_id)
                || app.print_job == Some(job_id)
                || app.sheet_job == Some(job_id)
            {
                let state = DialogState::get(window);
                state.set_job_progress(1.0);
                state.set_job_caption(SharedString::new());
                state.set_job_done(true);
            }
            if app.scan_job == Some(job_id) {
                // A scan is not a job the dialog's progress bar owns: it
                // wrote nothing, and what it produced is a list to look at,
                // not an outcome to announce (ADR 0065 §1).
                app.scan_job = None;
                match result {
                    JobResult::Scan(report) => {
                        let found = i32::try_from(report.candidates.len()).unwrap_or(i32::MAX);
                        let stopped = report.cancelled;
                        crate::wiring::dialogs::import::show_candidates(
                            app,
                            window,
                            report.candidates,
                        );
                        let tr = Tr::get(window);
                        DialogState::get(window).set_dialog_result(if stopped {
                            tr.invoke_scan_stopped(found)
                        } else {
                            tr.invoke_scan_found(found)
                        });
                    }
                    JobResult::Failed(reason) => {
                        DialogState::get(window)
                            .set_dialog_result(SharedString::from(reason.as_str()));
                    }
                    _ => {}
                }
            } else if app.import_job == Some(job_id) {
                app.import_job = None;
                DialogState::get(window).set_dialog_result(match result {
                    JobResult::Import(report) => {
                        let imported = i32::try_from(report.imported.len()).unwrap_or(i32::MAX);
                        let summary = match report.skipped.first() {
                            None => Tr::get(window).invoke_imported(imported),
                            Some(first) => Tr::get(window).invoke_imported_with_skips(
                                imported,
                                i32::try_from(report.skipped.len()).unwrap_or(i32::MAX),
                                SharedString::from(first.reason.as_str()),
                            ),
                        };
                        with_stop(window, summary, report.cancelled)
                    }
                    JobResult::Failed(reason) => {
                        Tr::get(window).invoke_import_failed(SharedString::from(reason))
                    }
                    _ => return,
                });
                // No explicit reload here: a successful import already
                // emitted `AssetsAdded` (handled below), and an import that
                // added nothing (every file skipped) leaves the grid as-is.
                //
                // The candidate list did its job and is released with the
                // choice it served (ADR 0065, Conséquences); the dialog goes
                // back to meaning "this whole folder".
                crate::wiring::dialogs::import::clear_candidates(app, window);
            } else if app.export_job == Some(job_id) {
                app.export_job = None;
                DialogState::get(window).set_dialog_result(match result {
                    JobResult::Export(report) => {
                        let summary = match export_outcome(&report) {
                            BatchOutcome::Done(path) => Tr::get(window)
                                .invoke_exported_to(SharedString::from(path.display().to_string())),
                            BatchOutcome::Failed(reason) => {
                                Tr::get(window).invoke_export_failed(SharedString::from(reason))
                            }
                            BatchOutcome::Nothing => Tr::get(window).invoke_nothing_to_export(),
                        };
                        with_stop(window, summary, report.cancelled)
                    }
                    JobResult::Failed(reason) => {
                        Tr::get(window).invoke_export_failed(SharedString::from(reason))
                    }
                    _ => return,
                });
            } else if app.print_job == Some(job_id) {
                app.print_job = None;
                DialogState::get(window).set_dialog_result(match result {
                    JobResult::Print(report) => {
                        let summary = match print_outcome(&report) {
                            BatchOutcome::Done(path) => Tr::get(window)
                                .invoke_printed_to(SharedString::from(path.display().to_string())),
                            BatchOutcome::Failed(reason) => {
                                Tr::get(window).invoke_print_failed(SharedString::from(reason))
                            }
                            BatchOutcome::Nothing => Tr::get(window).invoke_nothing_to_print(),
                        };
                        with_stop(window, summary, report.cancelled)
                    }
                    JobResult::Failed(reason) => {
                        Tr::get(window).invoke_print_failed(SharedString::from(reason))
                    }
                    _ => return,
                });
            } else if app.sheet_job == Some(job_id) {
                app.sheet_job = None;
                DialogState::get(window).set_dialog_result(match result {
                    JobResult::ContactSheet(report) => {
                        let written = Tr::get(window).invoke_sheet_written(
                            SharedString::from(report.path.display().to_string()),
                            i32::try_from(report.placed).unwrap_or(i32::MAX),
                            i32::try_from(report.pages).unwrap_or(i32::MAX),
                        );
                        if report.failed.is_empty() {
                            written
                        } else {
                            // An empty cell is not a failed sheet: the file
                            // exists, and what is missing is named (ADR 0110 §6).
                            let holes = Tr::get(window).invoke_sheet_empty_cells(
                                i32::try_from(report.failed.len()).unwrap_or(i32::MAX),
                            );
                            SharedString::from(format!("{written} {holes}"))
                        }
                    }
                    JobResult::Failed(reason) => {
                        Tr::get(window).invoke_sheet_failed(SharedString::from(reason))
                    }
                    _ => return,
                });
            } else if app.reprocess_job == Some(job_id) {
                // The status line and not a dialog: reprocessing has no
                // dialog of its own, and the window stayed usable while it
                // ran (ADR 0139 §5).
                app.reprocess_job = None;
                match result {
                    JobResult::Reprocess(report) => {
                        let summary = Tr::get(window).invoke_reprocessed(
                            i32::try_from(report.reprocessed.len()).unwrap_or(i32::MAX),
                            i32::try_from(report.already_current.len()).unwrap_or(i32::MAX),
                            i32::try_from(report.failed.len()).unwrap_or(i32::MAX),
                        );
                        LibraryState::get(window).set_status_line(with_stop(
                            window,
                            summary,
                            report.cancelled,
                        ));
                    }
                    JobResult::Failed(reason) => report_error(window, &reason),
                    _ => {}
                }
            } else if app.cull_job == Some(job_id) {
                app.cull_job = None;
                match result {
                    JobResult::Cull(proposal) => {
                        let rejects = proposal.rejects();
                        let bursts = proposal.bursts();
                        if rejects.is_empty() {
                            LibraryState::get(window)
                                .set_status_line(Tr::get(window).invoke_culling_found_nothing());
                            return;
                        }
                        // The grid is narrowed to exactly what is proposed,
                        // so the verdicts are looked at before any of them
                        // is applied (ADR 0084 §2).
                        app.query.assets = proposal
                            .entries
                            .iter()
                            .filter(|entry| !matches!(entry.verdict, leyline_sdk::Verdict::Keep))
                            .map(|entry| entry.asset)
                            .collect();
                        app.proposal = Some(*proposal);
                        LibraryState::get(window).set_reviewing_proposal(true);
                        if let Err(error) = crate::wiring::grid::reload(app, window) {
                            report_error(window, &error);
                            return;
                        }
                        // The grid now holds the keeper *and* the frame
                        // proposed against it, side by side — which is the
                        // comparison the photographer has to make — and the
                        // proposed ones are selected. So the sentence below
                        // is literally true: X rejects exactly those, and
                        // deselecting one takes it out of the verdict.
                        // ADR 0084 §1's "the keystrokes the photographer
                        // would have typed", arriving as keystrokes.
                        let rejected: std::collections::BTreeSet<usize> = app
                            .items
                            .iter()
                            .enumerate()
                            .filter(|(_, item)| rejects.contains(&item.version_id))
                            .map(|(index, _)| app.window_start + index)
                            .collect();
                        app.multi_selected = rejected;
                        crate::wiring::grid::refresh_multi_selected_cells(app, window);
                        LibraryState::get(window).set_status_line(
                            Tr::get(window).invoke_culling_proposal(
                                i32::try_from(rejects.len()).unwrap_or(i32::MAX),
                                i32::try_from(bursts).unwrap_or(i32::MAX),
                            ),
                        );
                    }
                    JobResult::Failed(reason) => {
                        LibraryState::get(window).set_status_line(SharedString::new());
                        report_error(window, &reason);
                    }
                    _ => {}
                }
            } else if app.derive_job == Some(job_id) {
                app.derive_job = None;
                match result {
                    JobResult::Derive(asset) => {
                        let name = app
                            .library
                            .catalog()
                            .asset_details(asset)
                            .map(|details| details.filename)
                            .unwrap_or_default();
                        // The new row first, the sentence about it second:
                        // `reload` writes the photo count into the same
                        // status line, so saying it before would say
                        // nothing.
                        if let Err(error) = crate::wiring::grid::reload(app, window) {
                            report_error(window, &error);
                            return;
                        }
                        LibraryState::get(window)
                            .set_status_line(Tr::get(window).invoke_derived(name.into()));
                    }
                    JobResult::Failed(reason) => {
                        LibraryState::get(window).set_status_line(SharedString::new());
                        report_error(window, &reason);
                    }
                    _ => {}
                }
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
                TetherState::get(window)
                    .set_captured_count(i32::try_from(app.tether_captured).unwrap_or(0));
                TetherState::get(window)
                    .set_last_captured(SharedString::from(details.filename.as_str()));
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
            // Newly imported photos can bring a body or a lens the library
            // had never seen (ADR 0064 §3).
            if let Err(error) = refresh_shot_facets(app, window) {
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
        // Photos left the library: a body or a lens may have left with the
        // last of its photos, and a filter list still offering it would send
        // the user to an empty grid.
        Event::AssetsRemoved { .. } => {
            if let Err(error) = refresh_shot_facets(app, window) {
                report_error(window, &error);
            }
        }
        Event::TetherConnected => {
            app.tether_connected = true;
            app.tether_captured = 0;
            let preset = app
                .tether_preset
                .and_then(|wanted| {
                    app.tether_presets
                        .iter()
                        .find(|(id, _)| *id == wanted)
                        .map(|(_, name)| SharedString::from(name.as_str()))
                })
                .unwrap_or_default();
            let state = TetherState::get(window);
            state.set_connected(true);
            state.set_captured_count(0);
            state.set_last_captured(SharedString::default());
            state.set_status(SharedString::default());
            state.set_live(false);
            state.set_preset_name(preset);
            state.set_session(DialogState::get(window).get_tether_session());
            DialogState::get(window).set_tether_connected(true);
            // The first settings read happens on the session's own thread
            // and has usually already landed by now; asking for it here
            // rather than waiting for the next change is what stops the bar
            // appearing empty for its first two seconds.
            refresh_tether(app, window);
        }
        Event::TetherDisconnected { reason } => {
            app.tether_connected = false;
            let state = TetherState::get(window);
            state.set_connected(false);
            state.set_live(false);
            state.set_picker(SharedString::default());
            DialogState::get(window).set_tether_connected(false);
            // An unplug says so in the dialog, where the next Connect is:
            // the bar it would otherwise report into has just gone away.
            DialogState::get(window).set_tether_status(match reason {
                Some(reason) => SharedString::from(reason),
                None => SharedString::default(),
            });
        }
        // The body changed — set from the bar, or turned on the camera
        // itself (ADR 0087 §2).
        Event::TetherSettingsChanged => {
            TetherState::get(window).set_status(SharedString::default());
            refresh_tether(app, window);
        }
        Event::TetherLiveFrame => refresh_live_frame(app, window),
        // Never a modal, and never a disconnect: the session is still
        // running and the next frame still has to be firable.
        Event::TetherCommandFailed { message } => {
            TetherState::get(window).set_status(SharedString::from(message));
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
