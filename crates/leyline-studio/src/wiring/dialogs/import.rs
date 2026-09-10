//! Wires the import dialog (ADR 0045 §4).
//!
//! Mirrors `ui/dialogs/import.slint`. Two ways in: hand the engine a folder
//! and let it take everything, or look first and tick what to keep
//! (ADR 0065). The ticks live here, in Rust — the panel reports clicks and
//! displays what comes back.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::app::App;
use crate::file_drop::{FileDrops, drop_target};
use crate::format;
use crate::ui::{CandidateRow, DialogState, LibraryState, StudioWindow, Tr};
use leyline_sdk::{ImportCandidate, ImportOptions, ScanOptions};
use slint::{ComponentHandle, Global, ModelRc, SharedString, VecModel};

pub(crate) fn wire_import(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let handle = window.as_weak();
        DialogState::get(window).on_browse_import_source(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            // rfd's blocking API is fine to call directly from a Slint
            // callback: it runs synchronously on the calling thread and,
            // like the rest of this app's callbacks, we're already on the
            // UI thread here, so no extra thread hop / async wiring needed.
            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                DialogState::get(&window)
                    .set_import_source_text(SharedString::from(folder.to_string_lossy().as_ref()));
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_scan_import(move |source, recursive, exact| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            if source.is_empty() {
                DialogState::get(&window)
                    .set_dialog_result(Tr::get(&window).invoke_enter_source_folder());
                return;
            }
            // A previous list would otherwise stay on screen, describing a
            // folder nobody is looking at any more.
            clear_candidates(&mut app, &window);
            let job = app.library.scan_import_async(
                Path::new(source.as_str()),
                &ScanOptions {
                    recursive,
                    thumbnails: true,
                    exact,
                },
            );
            app.scan_job = Some(job);
            app.candidate_source = PathBuf::from(source.as_str());
            DialogState::get(&window)
                .set_dialog_result(Tr::get(&window).invoke_scanning_ellipsis());
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_toggle_candidate(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Ok(index) = usize::try_from(index) else {
                return;
            };
            if let Some((_, selected)) = app.candidates.get_mut(index) {
                *selected = !*selected;
            }
            publish_candidates(&app, &window);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_select_candidates(move |all| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            for (candidate, selected) in &mut app.candidates {
                // "All" means all the ones a fresh scan would have ticked:
                // a photo the library already holds stays out unless it is
                // ticked on purpose (ADR 0065 §3).
                *selected = all && !candidate.already_imported;
            }
            publish_candidates(&app, &window);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_run_import(move |source, copy, recursive, pair| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            if source.is_empty() {
                DialogState::get(&window)
                    .set_dialog_result(Tr::get(&window).invoke_enter_source_folder());
                return;
            }
            let options = ImportOptions {
                copy_files: copy,
                recursive,
                pair_companions: pair,
                // Studio has a grid to fill and an event loop to hear the
                // pass finish on (ADR 0082 §4).
                thumbnails: true,
            };
            let chosen: Vec<PathBuf> = app
                .candidates
                .iter()
                .filter(|(_, selected)| *selected)
                .map(|(candidate, _)| candidate.path.clone())
                .collect();
            // The ticks count only while the dialog is still showing the
            // list they belong to, and only for the folder it was made
            // from. Otherwise the dialog keeps its original meaning: the
            // whole folder, exactly as before ADR 0065.
            let scanned = DialogState::get(&window).get_import_scanned()
                && app.candidate_source == Path::new(source.as_str());
            let job = if !scanned || chosen.is_empty() {
                app.library
                    .import_async(Path::new(source.as_str()), &options)
            } else {
                let source = app.candidate_source.clone();
                app.library.import_files_async(&source, &chosen, &options)
            };
            app.import_job = Some(job);
            DialogState::get(&window)
                .set_dialog_result(Tr::get(&window).invoke_importing_ellipsis());
        });
    }
}

/// Wires what a drop on the window does (ADR 0146 §4).
///
/// It opens this dialog and never imports: nothing is written by a gesture a
/// pointer can make by accident, which is the rule ADR 0065 built the dialog
/// for in the first place.
pub(crate) fn wire_file_drop(app: &Rc<RefCell<App>>, window: &StudioWindow, drops: &Rc<FileDrops>) {
    let app = Rc::clone(app);
    let dropped = window.as_weak();
    let hovered = window.as_weak();
    drops.on_drop(
        move |paths| {
            let Some(window) = dropped.upgrade() else {
                return;
            };
            // The only part that asks the filesystem anything: which of the
            // dropped paths are folders.
            let (dirs, files): (Vec<PathBuf>, Vec<PathBuf>) =
                paths.into_iter().partition(|path| path.is_dir());
            let Some(target) = drop_target(&dirs, &files) else {
                return;
            };
            let source = SharedString::from(target.source.to_string_lossy().as_ref());
            let dialog = DialogState::get(&window);
            dialog.set_dialog_result(SharedString::default());
            dialog.set_import_source_text(source.clone());
            dialog.set_dialog(SharedString::from("import"));
            if target.picks.is_empty() {
                // One folder was dropped, and the folder is the answer: the
                // dialog stays in its "import this folder" mode.
                return;
            }
            // Anything else has to be looked at before it can be ticked. The
            // two switches are the dialog's own, not this gesture's — a drop
            // does not decide whether the scan recurses.
            dialog.invoke_scan_import(
                source,
                dialog.get_import_recursive(),
                dialog.get_import_exact(),
            );
            // After the call, not before: `scan-import` clears the previous
            // list, and clearing is where a stale set of ticks would die.
            app.borrow_mut().drop_picks = target.picks;
        },
        move |hovering| {
            if let Some(window) = hovered.upgrade() {
                LibraryState::get(&window).set_file_drop_hovering(hovering);
            }
        },
    );
}

/// Shows what a finished scan found, each line ticked unless the library
/// looks like it already holds it (ADR 0065 §3).
pub(crate) fn show_candidates(
    app: &mut App,
    window: &StudioWindow,
    candidates: Vec<ImportCandidate>,
) {
    // What a drop asked for, and only for the scan it started (ADR 0146 §4):
    // taken here, so a scan someone runs by hand afterwards ticks everything
    // it finds, as it always has.
    let picks = std::mem::take(&mut app.drop_picks);
    app.candidates = candidates
        .into_iter()
        .map(|candidate| {
            let selected = !candidate.already_imported
                && (picks.is_empty() || crate::file_drop::is_picked(&candidate.path, &picks));
            (candidate, selected)
        })
        .collect();
    DialogState::get(window).set_import_scanned(true);
    publish_candidates(app, window);
}

/// Drops the current list, back to "import this whole folder".
pub(crate) fn clear_candidates(app: &mut App, window: &StudioWindow) {
    app.candidates.clear();
    app.candidate_source = PathBuf::new();
    DialogState::get(window).set_import_scanned(false);
    publish_candidates(app, window);
}

/// Mirrors the candidate list into the dialog.
fn publish_candidates(app: &App, window: &StudioWindow) {
    let rows: Vec<CandidateRow> = app
        .candidates
        .iter()
        .map(|(candidate, selected)| CandidateRow {
            filename: SharedString::from(candidate.filename.as_str()),
            detail: SharedString::from(detail(candidate)),
            thumbnail: thumbnail_image(candidate),
            already: candidate.already_imported,
            selected: *selected,
        })
        .collect();
    let chosen = app.candidates.iter().filter(|(_, s)| *s).count();
    DialogState::get(window).set_import_chosen(i32::try_from(chosen).unwrap_or(i32::MAX));
    DialogState::get(window).set_import_candidates(ModelRc::from(Rc::new(VecModel::from(rows))));
}

/// The one-line description under a candidate's name: what took it and how
/// big it is, the two facts that separate a keeper from a stray file.
fn detail(candidate: &ImportCandidate) -> String {
    let size = format::file_size(candidate.file_size);
    match &candidate.camera {
        Some(camera) => format!("{camera} · {size}"),
        None => size,
    }
}

/// Decodes a candidate's embedded preview for display. A file that carries
/// none — or one whose preview will not decode — shows an empty frame: it
/// is still a file the user may want, and the name is right there.
fn thumbnail_image(candidate: &ImportCandidate) -> slint::Image {
    let Some(bytes) = candidate.thumbnail.as_deref() else {
        return slint::Image::default();
    };
    let Ok(decoded) = image::load_from_memory(bytes) else {
        return slint::Image::default();
    };
    let rgba = decoded.into_rgba8();
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        rgba.as_raw(),
        rgba.width(),
        rgba.height(),
    );
    slint::Image::from_rgba8(buffer)
}
