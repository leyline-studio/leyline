//! The task bar: what a long batch is doing, and the button that stops it
//! (ADR 0139 §4).
//!
//! One job at a time — the most recent one to report — because the question
//! the corner of the window answers is "what is this busy with, and can I
//! stop it". The dialogs keep their own progress line: this is what survives
//! closing them.

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::App;
use crate::ui::{JobsState, StudioWindow, Tr};
use leyline_sdk::JobId;
use slint::{ComponentHandle, Global, SharedString};

/// Which batch is running, for the label the bar shows and for whether its
/// Cancel button exists at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Task {
    Import,
    Scan,
    Export,
    Print,
    /// Not cancellable: one PDF, and half a PDF is nothing to keep
    /// (ADR 0139 §3).
    ContactSheet,
    Reprocess,
    /// Not cancellable: a proposal over part of a shoot would propose
    /// rejections while staying silent about the rest (ADR 0139 §3).
    Cull,
}

impl Task {
    /// Whether the engine listens for a cancellation on this kind of job.
    fn cancellable(self) -> bool {
        !matches!(self, Self::ContactSheet | Self::Cull)
    }

    /// The translated name the bar shows.
    fn label(self, window: &StudioWindow) -> SharedString {
        let tr = Tr::get(window);
        match self {
            Self::Import => tr.invoke_task_import(),
            Self::Scan => tr.invoke_task_scan(),
            Self::Export => tr.invoke_task_export(),
            Self::Print => tr.invoke_task_print(),
            Self::ContactSheet => tr.invoke_task_sheet(),
            Self::Reprocess => tr.invoke_task_reprocess(),
            Self::Cull => tr.invoke_task_cull(),
        }
    }
}

/// Wires the bar's one button.
pub(crate) fn wire_jobs(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    let app = Rc::clone(app);
    let handle = window.as_weak();
    JobsState::get(window).on_cancel(move || {
        let Some(window) = handle.upgrade() else {
            return;
        };
        let app = app.borrow();
        let Some(job) = app.task_job else {
            return;
        };
        app.library.cancel_job(job);
        // The batch stops at its next checkpoint, which is one photograph
        // away at worst — saying so is what keeps the click from looking
        // ignored while the current file finishes.
        JobsState::get(&window).set_cancelling(true);
    });
}

/// Shows a job's progress in the corner of the window, taking the bar over
/// from whatever was there before.
pub(crate) fn show_task(
    app: &mut App,
    window: &StudioWindow,
    job: JobId,
    task: Task,
    done: u64,
    total: u64,
) {
    let state = JobsState::get(window);
    if app.task_job != Some(job) {
        app.task_job = Some(job);
        state.set_label(task.label(window));
        state.set_cancellable(task.cancellable());
        state.set_cancelling(false);
    }
    state.set_done(i32::try_from(done).unwrap_or(i32::MAX));
    state.set_total(i32::try_from(total).unwrap_or(i32::MAX));
    state.set_running(true);
}

/// Takes the bar down when the job it was showing ends. A job that ends
/// while the bar shows a *different* one leaves it alone: the newer batch is
/// still running and is still what the window is busy with.
pub(crate) fn finish_task(app: &mut App, window: &StudioWindow, job: JobId) {
    if app.task_job != Some(job) {
        return;
    }
    app.task_job = None;
    let state = JobsState::get(window);
    state.set_running(false);
    state.set_cancelling(false);
}
