//! Entering, leaving and moving around the develop view (ADR 0045 §4).
//!
//! Everything that changes *which* photo is being edited, plus the
//! Before/After compare toggle — none of it touches the settings themselves.

use std::cell::RefCell;
use std::rc::Rc;

use super::refresh_develop;
use crate::app::{App, item_at, report_error};
use crate::models::rgb8_to_slint_image;
use crate::ui::{DevelopState, GridState, StudioWindow};
use crate::wiring::grid::reload;
use leyline_sdk::PreviewKind;
use slint::{ComponentHandle, Global, SharedString};

pub(super) fn wire_session(app: &Rc<RefCell<App>>, window: &StudioWindow) {
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
