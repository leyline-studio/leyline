//! Wires the two comparison views of `GridState` (ADR 0045 §4, ADR 0057).
//!
//! Compare and survey are ways of *looking*: they open no edit session, write
//! nothing, and read only previews that are already cached for the loupe and
//! for develop. What little state they need — which photo challenges which,
//! and what the survey still holds — lives in [`App`], never in the catalog.

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::{App, item_at};
use crate::ui::{GridState, StudioWindow, SurveyCell};
use crate::wiring::grid::{preview_image, refresh_multi_selected_cells};
use slint::{ComponentHandle, Global, ModelRc, SharedString, VecModel};

/// Connects the compare and survey views.
pub(crate) fn wire_views(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        GridState::get(window).on_compare_mode_changed(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            if !GridState::get(&window).get_compare_mode() {
                app.compare_candidate = None;
                clear_compare(&window);
                return;
            }
            // Entering: the selected photo defends its place against its
            // neighbour (ADR 0057 §3). The last photo of the grid has none
            // to its right, so the challenger comes from its left.
            let selected = GridState::get(&window).get_selected();
            let total = GridState::get(&window).get_total_cells();
            app.compare_candidate = neighbour(selected, total);
            show_compare(&mut app, &window);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        GridState::get(window).on_compare_candidate_step(move |delta| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let selected = GridState::get(&window).get_selected();
            let total = GridState::get(&window).get_total_cells();
            let from = app.compare_candidate.unwrap_or(selected);
            // Steps over the select rather than onto it: a photo never
            // challenges itself, and the arrow still moves by one.
            let mut next = from + delta;
            if next == selected {
                next += delta;
            }
            if next >= 0 && next < total {
                app.compare_candidate = Some(next);
                show_compare(&mut app, &window);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        GridState::get(window).on_compare_swap(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let selected = GridState::get(&window).get_selected();
            let Some(candidate) = app.compare_candidate else {
                return;
            };
            // Promotion: the challenger takes the place it was defending
            // against, and the photo it beat becomes the next challenger.
            GridState::get(&window).set_selected(candidate);
            app.compare_candidate = Some(selected);
            crate::wiring::grid::show_details(&mut app, &window, candidate);
            show_compare(&mut app, &window);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        GridState::get(window).on_survey_mode_changed(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            if !GridState::get(&window).get_survey_mode() {
                app.survey.clear();
                GridState::get(&window)
                    .set_survey_cells(ModelRc::from(Rc::new(VecModel::from(Vec::new()))));
                return;
            }
            // The multi-selection is the subject; a lone selection is a
            // survey of one, which is odd but honest — it is what was asked
            // for (ADR 0057 §4).
            app.survey = if app.multi_selected.is_empty() {
                usize::try_from(GridState::get(&window).get_selected())
                    .map(|index| vec![index])
                    .unwrap_or_default()
            } else {
                app.multi_selected.iter().copied().collect()
            };
            show_survey(&mut app, &window);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        GridState::get(window).on_survey_remove(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Ok(index) = usize::try_from(index) else {
                return;
            };
            if index >= app.survey.len() {
                return;
            }
            // Dropped from the *selection*, which is the whole point: the
            // photo stays in the catalog, in the grid and on disk.
            let dropped = app.survey.remove(index);
            app.multi_selected.remove(&dropped);
            refresh_multi_selected_cells(&app, &window);
            show_survey(&mut app, &window);
        });
    }
}

/// The grid index next to `selected`, preferring the one after it.
///
/// `None` when the grid holds fewer than two rows — there is then nothing to
/// compare against, and the panel says so rather than showing a photo twice.
fn neighbour(selected: i32, total: i32) -> Option<i32> {
    if selected < 0 || total < 2 {
        return None;
    }
    if selected + 1 < total {
        Some(selected + 1)
    } else {
        Some(selected - 1)
    }
}

/// Fills both sides of the compare view from the current select/candidate.
fn show_compare(app: &mut App, window: &StudioWindow) {
    let selected = GridState::get(window).get_selected();
    let (image, name) = photo_at(app, selected);
    GridState::get(window).set_compare_select_image(image);
    GridState::get(window).set_compare_select_name(name);

    let (image, name) = match app.compare_candidate {
        Some(index) => photo_at(app, index),
        None => (slint::Image::default(), SharedString::default()),
    };
    GridState::get(window).set_compare_candidate_image(image);
    GridState::get(window).set_compare_candidate_name(name);
}

/// Empties both sides, so a photo does not linger in memory — or on screen
/// behind a later view — once compare is left.
fn clear_compare(window: &StudioWindow) {
    GridState::get(window).set_compare_select_image(slint::Image::default());
    GridState::get(window).set_compare_candidate_image(slint::Image::default());
    GridState::get(window).set_compare_select_name(SharedString::default());
    GridState::get(window).set_compare_candidate_name(SharedString::default());
}

/// Rebuilds the survey model from `app.survey`, and leaves the view when the
/// last photo has been dropped from it.
fn show_survey(app: &mut App, window: &StudioWindow) {
    if app.survey.is_empty() {
        GridState::get(window).set_survey_mode(false);
        GridState::get(window).set_survey_cells(ModelRc::from(Rc::new(VecModel::from(Vec::new()))));
        return;
    }
    let cells: Vec<SurveyCell> = app
        .survey
        .clone()
        .into_iter()
        .map(|index| {
            let (image, filename) = photo_at(app, i32::try_from(index).unwrap_or(i32::MAX));
            SurveyCell { image, filename }
        })
        .collect();
    GridState::get(window).set_survey_cells(ModelRc::from(Rc::new(VecModel::from(cells))));
}

/// The preview and the file name of one grid row, both empty when the row is
/// outside the loaded window — scrolling far from the selection while compare
/// is open is rare, and an empty pane says more than a stale photo.
fn photo_at(app: &mut App, index: i32) -> (slint::Image, SharedString) {
    let Some((asset, filename)) =
        item_at(app, index).map(|item| (item.asset_id, item.filename.clone()))
    else {
        return (slint::Image::default(), SharedString::default());
    };
    (
        preview_image(app, asset),
        SharedString::from(filename.as_str()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_challenger_comes_from_the_right_except_at_the_end() {
        assert_eq!(neighbour(0, 4), Some(1));
        assert_eq!(neighbour(2, 4), Some(3));
        // Last row: nothing to its right, so the one before it challenges.
        assert_eq!(neighbour(3, 4), Some(2));
        // Nothing to compare with.
        assert_eq!(neighbour(0, 1), None);
        assert_eq!(neighbour(-1, 4), None);
    }
}
