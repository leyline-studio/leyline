//! Wires `FilterState`: the filter bar, the search box and classement
//! (ADR 0045 §4).

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::{App, SORTS, item_at, report_error, selected_versions};
use crate::classify;
use crate::classify::Action;
use crate::ui::{FilterState, GridState, StudioWindow};
use crate::wiring::grid::reload;
use leyline_sdk::{ColorLabel, PickState};
use slint::{ComponentHandle, Global, SharedString};

/// Applies a classement key (`0`–`9`, `p`, `x`, `u`) to the selection — every
/// multi-selected photo when there is one, otherwise just the focused photo.
/// The *toggle* direction (e.g. re-rating 3 stars clears it) is decided from
/// the focused photo's own current label/pick alone, then that one resulting
/// value is applied to the whole selection — the same "last-active item
/// decides the toggle, batch gets the result" rule Lightroom uses.
pub(crate) fn wire_classify(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    let app = Rc::clone(app);
    let handle = window.as_weak();
    GridState::get(window).on_classify(move |key| {
        let Some(window) = handle.upgrade() else {
            return;
        };
        let mut app = app.borrow_mut();
        let focused = GridState::get(&window).get_selected();
        let Some((label, pick)) = item_at(&app, focused).map(|item| (item.color_label, item.pick))
        else {
            return;
        };
        let Some(action) = classify::from_key(key.as_str(), label, pick) else {
            return;
        };
        let versions = selected_versions(&app, focused);
        if versions.is_empty() {
            return;
        }
        let applied = match action {
            Action::Rate(rating) => app.library.set_rating(&versions, rating),
            Action::Label(label) => app.library.set_color_label(&versions, label),
            Action::Flag(pick) => app.library.set_pick(&versions, pick),
        };
        if let Err(error) = applied
            .map_err(|e| e.to_string())
            .and_then(|()| reload(&mut app, &window))
        {
            report_error(&window, &error);
        }
    });
}

/// Connects the filter bar: stars, label dots, pick chips, sort cycling.
pub(crate) fn wire_filters(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    let on_error = |window: &StudioWindow, result: Result<(), String>| {
        if let Err(error) = result {
            report_error(window, &error);
        }
    };

    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        FilterState::get(window).on_rating_filter(move |stars| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            app.query.rating_at_least =
                classify::toggle_rating_filter(app.query.rating_at_least, stars as u8);
            FilterState::get(&window)
                .set_filter_rating(app.query.rating_at_least.map_or(0, i32::from));
            on_error(&window, reload(&mut app, &window));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        FilterState::get(window).on_label_filter(move |value| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let Some(clicked) = ColorLabel::from_i64(i64::from(value)) else {
                return;
            };
            let mut app = app.borrow_mut();
            app.query.color_label = classify::toggle_label_filter(app.query.color_label, clicked);
            FilterState::get(&window)
                .set_filter_label(app.query.color_label.map_or(-1, |l| l.as_i64() as i32));
            on_error(&window, reload(&mut app, &window));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        FilterState::get(window).on_pick_filter(move |value| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let Some(clicked) = PickState::from_i64(i64::from(value)) else {
                return;
            };
            let mut app = app.borrow_mut();
            app.query.pick = classify::toggle_pick_filter(app.query.pick, clicked);
            FilterState::get(&window)
                .set_filter_pick(app.query.pick.map_or(-1, |p| p.as_i64() as i32));
            on_error(&window, reload(&mut app, &window));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        FilterState::get(window).on_search(move |text| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let text = text.trim();
            app.query.text = (!text.is_empty()).then(|| text.to_owned());
            on_error(&window, reload(&mut app, &window));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        FilterState::get(window).on_cycle_sort(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let next = SORTS
                .iter()
                .position(|(sort, _)| *sort == app.query.sort)
                .map_or(0, |i| (i + 1) % SORTS.len());
            app.query.sort = SORTS[next].0;
            FilterState::get(&window).set_sort_label(SharedString::from(SORTS[next].1));
            on_error(&window, reload(&mut app, &window));
        });
    }
}
