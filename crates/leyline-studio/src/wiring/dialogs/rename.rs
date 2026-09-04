//! Renaming files on disk (ADR 0100, ADR 0045 §4).
//!
//! The one dialog whose confirmation moves the user's own files, so the
//! wiring reports what happened rather than closing silently: a batch that
//! refused half its names has to say which half.

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::{App, selected_indices};
use crate::ui::{DialogState, GridState, StudioWindow, Tr};
use slint::{ComponentHandle, Global, SharedString};

pub(crate) fn wire_rename(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let handle = window.as_weak();
        DialogState::get(window).on_open_rename(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            DialogState::get(&window).set_dialog_result(SharedString::default());
            DialogState::get(&window).set_dialog(SharedString::from("rename"));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_run_rename(move |template| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let assets: Vec<_> = selected_indices(&app, GridState::get(&window).get_selected())
                .into_iter()
                .filter_map(|index| app.items.get(index).map(|item| item.asset_id))
                .collect();
            if assets.is_empty() {
                DialogState::get(&window)
                    .set_dialog_result(Tr::get(&window).invoke_select_photo_first());
                return;
            }
            // The template is remembered: renaming a second batch the same
            // way is the common case, and retyping it is the annoyance.
            DialogState::get(&window).set_rename_template(template.clone());

            let report = match app.library.rename(&assets, template.as_str()) {
                Ok(report) => report,
                Err(error) => {
                    DialogState::get(&window)
                        .set_dialog_result(SharedString::from(error.to_string()));
                    return;
                }
            };
            // Reported, never silently closed: a refusal is the outcome the
            // user most needs to see (ADR 0100 §2).
            let renamed = i32::try_from(report.renamed.len()).unwrap_or(i32::MAX);
            let message = match report.failed.first() {
                None => Tr::get(&window).invoke_renamed(renamed),
                Some(first) => Tr::get(&window).invoke_renamed_with_refusals(
                    renamed,
                    i32::try_from(report.failed.len()).unwrap_or(i32::MAX),
                    SharedString::from(first.reason.as_str()),
                ),
            };
            DialogState::get(&window).set_dialog_result(message);
            if report.failed.is_empty() {
                DialogState::get(&window).set_dialog(SharedString::default());
            }
            if let Err(error) = crate::wiring::grid::reload(&mut app, &window) {
                crate::app::report_error(&window, &error);
            }
        });
    }
}
