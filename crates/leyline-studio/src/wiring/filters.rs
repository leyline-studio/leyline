//! Wires `FilterState`: the filter bar, the search box and classement
//! (ADR 0045 §4).

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::{App, SORTS, item_at, report_error, selected_versions};
use crate::classify;
use crate::classify::Action;
use crate::ui::{CollectionState, FilterState, FolderState, GridState, StudioWindow};
use crate::wiring::grid::reload;
use leyline_sdk::{ColorLabel, GridQuery, PickState, ShotRange};
use slint::{ComponentHandle, Global, Model, ModelRc, SharedString, VecModel};

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
            // A proposal narrows the grid by *naming* photographs; every
            // filter narrows it by describing them. Leaving both in place
            // would show the intersection of a set the user chose and one
            // they had forgotten about (ADR 0084 §2).
            crate::wiring::library::forget_proposal(&mut app, &window);
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
            // A proposal narrows the grid by *naming* photographs; every
            // filter narrows it by describing them. Leaving both in place
            // would show the intersection of a set the user chose and one
            // they had forgotten about (ADR 0084 §2).
            crate::wiring::library::forget_proposal(&mut app, &window);
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
            // A proposal narrows the grid by *naming* photographs; every
            // filter narrows it by describing them. Leaving both in place
            // would show the intersection of a set the user chose and one
            // they had forgotten about (ADR 0084 §2).
            crate::wiring::library::forget_proposal(&mut app, &window);
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
        FilterState::get(window).on_camera_filter(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let clicked = FilterState::get(&window)
                .get_shot_cameras()
                .row_data(usize::try_from(index).unwrap_or(usize::MAX));
            // Clicking the active body turns the filter off, the same way
            // the label dots and the pick chips just above already behave.
            app.query.camera = match (clicked, app.query.camera.take()) {
                (Some(clicked), Some(active)) if active == clicked.as_str() => None,
                (Some(clicked), _) => Some(clicked.to_string()),
                (None, active) => active,
            };
            publish_shot_filters(&app, &window);
            on_error(&window, reload(&mut app, &window));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        FilterState::get(window).on_lens_filter(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let clicked = FilterState::get(&window)
                .get_shot_lenses()
                .row_data(usize::try_from(index).unwrap_or(usize::MAX));
            app.query.lens = match (clicked, app.query.lens.take()) {
                (Some(clicked), Some(active)) if active == clicked.as_str() => None,
                (Some(clicked), _) => Some(clicked.to_string()),
                (None, active) => active,
            };
            publish_shot_filters(&app, &window);
            on_error(&window, reload(&mut app, &window));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        FilterState::get(window).on_shot_range(move |which, text| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            // An unreadable interval is said, not swallowed: an empty grid
            // would read as "the library holds none of those".
            let range = match ShotRange::parse(text.as_str()) {
                Ok(range) => range,
                Err(error) => {
                    report_error(&window, &error.to_string());
                    return;
                }
            };
            match which.as_str() {
                "iso" => app.query.iso = range,
                "aperture" => app.query.aperture = range,
                "focal" => app.query.focal_length = range,
                "shutter" => app.query.shutter_speed = range,
                _ => return,
            }
            publish_shot_filters(&app, &window);
            on_error(&window, reload(&mut app, &window));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        FilterState::get(window).on_clear_shot_filters(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            // Also the way back when the last photo of a body has been
            // removed: its chip is gone from the list, so nothing else could
            // turn that filter off any more.
            app.query.camera = None;
            app.query.lens = None;
            app.query.iso = ShotRange::default();
            app.query.aperture = ShotRange::default();
            app.query.focal_length = ShotRange::default();
            app.query.shutter_speed = ShotRange::default();
            publish_shot_filters(&app, &window);
            on_error(&window, reload(&mut app, &window));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        // The way back from a grid narrowed to nothing (ADR 0054 §1). Several
        // criteria may be on at once — a folder *and* three stars *and* a
        // search — so the empty grid cannot point at the one chip to click;
        // this drops the lot and republishes every mirror of it.
        GridState::get(window).on_clear_criteria(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            crate::wiring::library::forget_proposal(&mut app, &window);
            // Sort and the row window are not criteria: the order the
            // photographer chose survives showing everything again.
            let sort = app.query.sort;
            let range = app.query.range.clone();
            app.query = GridQuery {
                sort,
                range,
                ..GridQuery::default()
            };
            app.keyword_filter = None;
            let filters = FilterState::get(&window);
            filters.set_filter_rating(0);
            filters.set_filter_label(-1);
            filters.set_filter_pick(-1);
            filters.set_filter_keyword_label(SharedString::default());
            publish_shot_filters(&app, &window);
            FolderState::get(&window).set_active_folder(-1);
            CollectionState::get(&window).set_active_collection(-1);
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

/// Sends the bodies and lenses the library was shot with to the filter panel
/// (ADR 0064 §3).
///
/// Called when the library's contents change — an import, a removal — and
/// not on every keystroke: the lists describe the whole library, so nothing
/// a filter does can alter them.
pub(crate) fn refresh_shot_facets(app: &App, window: &StudioWindow) -> Result<(), String> {
    let facets = app
        .library
        .catalog()
        .shot_facets()
        .map_err(|e| e.to_string())?;
    let shared = |values: &[String]| {
        let rows: Vec<SharedString> = values.iter().map(SharedString::from).collect();
        ModelRc::from(Rc::new(VecModel::from(rows)))
    };
    FilterState::get(window).set_shot_cameras(shared(&facets.cameras));
    FilterState::get(window).set_shot_lenses(shared(&facets.lenses));
    publish_shot_filters(app, window);
    Ok(())
}

/// Mirrors the query's shot filters into the panel: which chip is lit, and
/// how many criteria are on — the count is what a folded panel shows, so a
/// narrowed grid never looks unfiltered.
fn publish_shot_filters(app: &App, window: &StudioWindow) {
    let state = FilterState::get(window);
    let index = |value: Option<&String>, list: ModelRc<SharedString>| {
        value
            .and_then(|value| list.iter().position(|row| row.as_str() == value))
            .and_then(|i| i32::try_from(i).ok())
            .unwrap_or(-1)
    };
    state.set_shot_camera(index(app.query.camera.as_ref(), state.get_shot_cameras()));
    state.set_shot_lens(index(app.query.lens.as_ref(), state.get_shot_lenses()));
    state.set_shot_count(shot_count(&app.query));
}

/// How many shot criteria the query carries.
fn shot_count(query: &GridQuery) -> i32 {
    let ranges = [
        query.iso,
        query.aperture,
        query.focal_length,
        query.shutter_speed,
    ];
    let discrete = i32::from(query.camera.is_some()) + i32::from(query.lens.is_some());
    let continuous = ranges.iter().filter(|r| !r.is_unbounded()).count();
    discrete + i32::try_from(continuous).unwrap_or(0)
}
