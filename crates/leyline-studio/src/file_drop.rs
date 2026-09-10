//! Files dragged from the desktop onto the window (ADR 0146 §4).
//!
//! Slint 1.13 has no drop target — it has no drag either, which ADR 0130 §1
//! had to work around from the inside. What Studio does have is the winit
//! backend (`unstable-winit-030`), and winit reports `HoveredFile`,
//! `DroppedFile` and `HoveredFileCancelled` on the UI thread. This module is
//! that handler, plus the one decision worth testing on its own: what a set
//! of dropped paths asks the import dialog for.
//!
//! A drop never imports. It opens the dialog ADR 0065 built, source filled
//! in — nothing is written by a gesture a pointer can make by accident.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use slint::winit_030::{CustomApplicationHandler, EventResult, winit};

/// What the import dialog is asked for by a drop: the folder to look in, and
/// the paths to tick inside it.
///
/// The ticks are **prefixes**: a dropped folder ticks everything under it, a
/// dropped file ticks itself. Empty means "this whole folder", the plain mode
/// of the dialog — which is what a single dropped folder means and the only
/// case where no scan is needed.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DropTarget {
    /// The folder the scan runs on.
    pub(crate) source: PathBuf,
    /// What to tick once it comes back; empty ticks nothing in particular.
    pub(crate) picks: Vec<PathBuf>,
}

/// Reads a drop.
///
/// `dirs` and `files` are the dropped paths, already split by the caller —
/// the only part of this that touches the filesystem.
///
/// One folder is the folder. Anything else — several folders, files, a mix —
/// is scanned from the nearest folder that contains them all, with only what
/// was dropped ticked: dropping three photographs and importing the eight
/// hundred beside them would be a lie about the gesture.
pub(crate) fn drop_target(dirs: &[PathBuf], files: &[PathBuf]) -> Option<DropTarget> {
    if dirs.len() == 1 && files.is_empty() {
        return Some(DropTarget {
            source: dirs[0].clone(),
            picks: Vec::new(),
        });
    }
    if dirs.is_empty() && files.is_empty() {
        return None;
    }
    let picks: Vec<PathBuf> = dirs.iter().chain(files.iter()).cloned().collect();
    let parents: Vec<&Path> = picks.iter().filter_map(|path| path.parent()).collect();
    let source = common_parent(&parents)?;
    Some(DropTarget { source, picks })
}

/// The deepest folder every one of `paths` is inside, or `None` when they
/// share none — two drives on Windows, `/` on Unix.
fn common_parent(paths: &[&Path]) -> Option<PathBuf> {
    let (first, rest) = paths.split_first()?;
    let mut common: PathBuf = (*first).to_path_buf();
    for path in rest {
        while !path.starts_with(&common) {
            if !common.pop() {
                return None;
            }
        }
    }
    // Relative paths can meet at the empty path, which is not a folder
    // anyone can scan. winit gives absolute ones; this is the guard, not the
    // expected case.
    (!common.as_os_str().is_empty()).then_some(common)
}

/// Whether a candidate the scan found is one of the dropped paths.
pub(crate) fn is_picked(path: &Path, picks: &[PathBuf]) -> bool {
    picks.iter().any(|pick| path.starts_with(pick))
}

/// The shared state between the winit handler and the window: what has been
/// dropped, and who to tell.
///
/// Same thread from end to end — winit's handler runs on the event loop,
/// which is the UI thread — so an `Rc` is all the sharing this needs.
#[derive(Default)]
pub(crate) struct FileDrops {
    pending: RefCell<Vec<PathBuf>>,
    hovering: Cell<bool>,
    #[allow(clippy::type_complexity)]
    deliver: RefCell<Option<Box<dyn Fn(Vec<PathBuf>)>>>,
    #[allow(clippy::type_complexity)]
    hover: RefCell<Option<Box<dyn Fn(bool)>>>,
}

impl FileDrops {
    /// Registers what the window does with a drop, and with a hover. Called
    /// once the window exists — every event before that is dropped on the
    /// floor, which is the right answer for one that arrives while Studio is
    /// still opening its catalog.
    pub(crate) fn on_drop(
        &self,
        deliver: impl Fn(Vec<PathBuf>) + 'static,
        hover: impl Fn(bool) + 'static,
    ) {
        *self.deliver.borrow_mut() = Some(Box::new(deliver));
        *self.hover.borrow_mut() = Some(Box::new(hover));
    }

    fn set_hovering(&self, hovering: bool) {
        if self.hovering.replace(hovering) == hovering {
            return;
        }
        if let Some(hover) = self.hover.borrow().as_ref() {
            hover(hovering);
        }
    }

    /// Hands over the batch, once winit has finished dispatching it.
    fn flush(&self) {
        let paths = std::mem::take(&mut *self.pending.borrow_mut());
        if paths.is_empty() {
            return;
        }
        self.set_hovering(false);
        // Borrowed for the call: the handler opens a dialog, and a dialog
        // callback that dropped files again would otherwise re-enter this.
        let deliver = self.deliver.borrow();
        if let Some(deliver) = deliver.as_ref() {
            deliver(paths);
        }
    }
}

/// The winit handler, installed on the backend before the first window
/// exists (see `run` in `main.rs`).
pub(crate) struct DropHandler {
    drops: Rc<FileDrops>,
}

impl DropHandler {
    pub(crate) fn new(drops: &Rc<FileDrops>) -> Self {
        Self {
            drops: Rc::clone(drops),
        }
    }
}

impl CustomApplicationHandler for DropHandler {
    fn window_event(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        _winit_window: Option<&winit::window::Window>,
        _slint_window: Option<&slint::Window>,
        event: &winit::event::WindowEvent,
    ) -> EventResult {
        match event {
            winit::event::WindowEvent::HoveredFile(_) => self.drops.set_hovering(true),
            winit::event::WindowEvent::HoveredFileCancelled => self.drops.set_hovering(false),
            winit::event::WindowEvent::DroppedFile(path) => {
                self.drops.pending.borrow_mut().push(path.clone());
            }
            // The overlay's only other way out. `HoveredFileCancelled` comes
            // from the *other* process, and a drag source that dies mid-drag
            // never sends it — seen under Xvfb, and the window then stays
            // dimmed with nothing able to clear it. During a real drag the
            // source holds the pointer grab and the target sees none of
            // these, so the first one that arrives means the drag is over.
            winit::event::WindowEvent::CursorMoved { .. }
            | winit::event::WindowEvent::CursorLeft { .. }
            | winit::event::WindowEvent::MouseInput { .. }
            | winit::event::WindowEvent::KeyboardInput { .. } => self.drops.set_hovering(false),
            _ => {}
        }
        // Never `Consume`: everything above is a notification, and consuming
        // it would take the event away from Slint's own handling.
        EventResult::Propagate
    }

    fn about_to_wait(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) -> EventResult {
        // winit sends one `DroppedFile` per file and no end-of-batch marker.
        // This runs once the burst has been dispatched, which is the marker.
        self.drops.flush();
        EventResult::Propagate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn one_folder_is_the_folder() {
        let target = drop_target(&[p("/photos/2026")], &[]).unwrap();
        assert_eq!(target.source, p("/photos/2026"));
        // Nothing ticked: the dialog stays in its "import this folder" mode.
        assert!(target.picks.is_empty());
    }

    #[test]
    fn files_are_scanned_from_their_folder_and_ticked() {
        let target = drop_target(&[], &[p("/photos/2026/a.cr2"), p("/photos/2026/b.cr2")]).unwrap();
        assert_eq!(target.source, p("/photos/2026"));
        assert_eq!(target.picks.len(), 2);
        assert!(is_picked(Path::new("/photos/2026/a.cr2"), &target.picks));
        // The eight hundred beside them are not.
        assert!(!is_picked(Path::new("/photos/2026/c.cr2"), &target.picks));
    }

    #[test]
    fn several_folders_meet_at_their_parent_and_tick_what_is_under_them() {
        let target = drop_target(&[p("/photos/2026/mars"), p("/photos/2026/avril")], &[]).unwrap();
        assert_eq!(target.source, p("/photos/2026"));
        assert!(is_picked(
            Path::new("/photos/2026/mars/a.cr2"),
            &target.picks
        ));
        assert!(!is_picked(
            Path::new("/photos/2026/mai/a.cr2"),
            &target.picks
        ));
    }

    #[test]
    fn a_mix_meets_where_it_can() {
        let target = drop_target(&[p("/photos/2026/mars")], &[p("/photos/2025/b.cr2")]).unwrap();
        assert_eq!(target.source, p("/photos"));
        assert!(is_picked(
            Path::new("/photos/2026/mars/a.cr2"),
            &target.picks
        ));
        assert!(!is_picked(Path::new("/photos/2025/c.cr2"), &target.picks));
    }

    #[test]
    fn nothing_dropped_asks_for_nothing() {
        assert!(drop_target(&[], &[]).is_none());
    }
}
