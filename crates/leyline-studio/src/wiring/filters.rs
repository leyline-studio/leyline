//! Wires `FilterState`: the filter bar, the search box and classement
//! (ADR 0045 §4).

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::{
    App, item_at, report_error, selected_items, selected_versions, sort_from_parts, sort_key,
    sort_parts,
};
use crate::classify;
use crate::classify::Action;
use crate::preferences::SharedPreferences;
use crate::ui::{
    CollectionState, FilterState, FolderState, GridState, MapState, PreferencesState, StudioWindow,
};
use crate::undo::{Edit, Snapshot};
use crate::wiring::dialogs::preferences::with_preferences;
use crate::wiring::grid::{reload, show_details};
use crate::wiring::library::refresh_undo;
use leyline_sdk::{ColorLabel, GridQuery, PickState, ShotRange, VersionId};
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
        // What every one of them held a moment ago (ADR 0129 §1). Read from
        // the grid rows rather than from the catalog: they are the values
        // the cells are showing, which is what the photographer means by
        // "the way it was".
        let before = classement_of(&app, focused);
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
            return;
        }
        // After the reload, so the "after" half is what the catalog
        // actually holds and not what was asked for.
        let after = classement_of(&app, focused);
        app.undo.push(Edit {
            kind: match action {
                Action::Rate(_) => "rating",
                Action::Label(_) => "label",
                Action::Flag(_) => "flag",
            },
            before: Snapshot::Classement(before),
            after: Snapshot::Classement(after),
        });
        refresh_undo(&app, &window);
        // ADR 0131 §1: last, so nothing moves past a failed write, and the
        // photograph left on screen after an error is the one that did not
        // take the rating.
        if let Some(next) = advance_target(&app, &window, focused) {
            // The same steps `on_select` takes, minus the multi-selection
            // clear it does not need — the advance only happens when there
            // was none. Setting `selected` is what makes `browser.slint`'s
            // `followed-selection` scroll the new focus into view, which is
            // also what refills the loaded window of rows when the advance
            // walks off the end of it.
            GridState::get(&window).set_selected(next);
            show_details(&mut app, &window, next);
        }
    });
}

/// Where the selection goes after a classement has landed (ADR 0131 §1, §2),
/// or `None` when it stays put.
///
/// Every condition is read from state rather than from the call site: the
/// number row, the Photo menu and the grid's context menu all reach
/// `classify`, and they all mean the same thing.
fn advance_target(app: &App, window: &StudioWindow, focused: i32) -> Option<i32> {
    let grid = GridState::get(window);
    // The grid and the loupe are the culling surfaces; the other four views
    // each refuse for a reason of their own (§2). Develop is read from `App`
    // rather than from the flag, because what disqualifies it is the open
    // edit session, not the panel being visible.
    let elsewhere = grid.get_compare_mode()
        || grid.get_survey_mode()
        || MapState::get(window).get_map_mode()
        || app.develop.is_some();
    next_to_class(
        PreferencesState::get(window).get_advance_after_classement(),
        app.multi_selected.len(),
        elsewhere,
        focused,
        grid.get_total_cells(),
    )
}

/// The rule itself, with nothing to read it from (ADR 0131 §1).
///
/// `multi` is the size of the multi-selection and is compared against `1`
/// rather than `0`, so it reads the set exactly as [`selected_indices`]
/// does: one entry, or none, is a lone photograph.
fn next_to_class(on: bool, multi: usize, elsewhere: bool, focused: i32, total: i32) -> Option<i32> {
    if !on || multi > 1 || elsewhere || focused < 0 {
        return None;
    }
    let next = focused.checked_add(1)?;
    (next < total).then_some(next)
}

/// The classement of every selected row, as the grid currently shows it.
///
/// Through `selected_items`, so it covers the same photographs the action
/// does: with `Ctrl+A` the selection reaches past the loaded window
/// (ADR 0141 §2), and a snapshot that stopped at the window would undo forty
/// photographs out of thirty-eight thousand — silently, which is the worst
/// way for an undo to be wrong.
fn classement_of(
    app: &App,
    focused: i32,
) -> Vec<(VersionId, Option<u8>, Option<ColorLabel>, PickState)> {
    selected_items(app, focused)
        .into_iter()
        .map(|item| (item.version_id, item.rating, item.color_label, item.pick))
        .collect()
}

/// Connects the filter bar: stars, label dots, pick chips, sort cycling.
pub(crate) fn wire_filters(
    app: &Rc<RefCell<App>>,
    window: &StudioWindow,
    preferences: &SharedPreferences,
) {
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
        let preferences = preferences.clone();
        FilterState::get(window).on_choose_sort(move |key, ascending| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            // A key this build does not know is not an error and not a
            // reason to reorder anything: nothing happens (ADR 0148 §2).
            let Some(sort) = sort_from_parts(key, ascending) else {
                return;
            };
            let mut app = app.borrow_mut();
            if app.query.sort == sort {
                // Clicking the chip that is already lit reloads a grid that
                // would come back identical.
                return;
            }
            app.query.sort = sort;
            publish_sort(&window, sort);
            // Remembered across launches (ADR 0136 §5), by name. Silent on a
            // write failure, like every other remembered view setting.
            let _ = with_preferences(&preferences, |file| {
                file.update(|values| values.sort = Some(sort_key(sort).to_owned()))
            });
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

/// Mirrors an order into the two answers the filter bar shows (ADR 0148 §1).
pub(crate) fn publish_sort(window: &StudioWindow, sort: leyline_sdk::Sort) {
    let (key, ascending) = sort_parts(sort);
    let state = FilterState::get(window);
    state.set_sort_key(key);
    state.set_sort_ascending(ascending);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lone_photograph_steps_on_and_stops_at_the_end() {
        assert_eq!(next_to_class(true, 0, false, 0, 3), Some(1));
        assert_eq!(next_to_class(true, 1, false, 1, 3), Some(2));
        // The last photograph is where the run ends: no wrap (ADR 0131 §1).
        assert_eq!(next_to_class(true, 0, false, 2, 3), None);
        // Nothing focused, nothing to step from.
        assert_eq!(next_to_class(true, 0, false, -1, 3), None);
        assert_eq!(next_to_class(true, 0, false, i32::MAX, i32::MAX), None);
    }

    #[test]
    fn the_three_refusals() {
        // Off is the shipped default, and it is a refusal like any other.
        assert_eq!(next_to_class(false, 0, false, 0, 3), None);
        // A multi-selection is one decision about many photographs, and
        // "the next one" after it names nothing (§1).
        assert_eq!(next_to_class(true, 2, false, 0, 3), None);
        // Compare, survey, map, develop (§2).
        assert_eq!(next_to_class(true, 0, true, 0, 3), None);
    }
}
