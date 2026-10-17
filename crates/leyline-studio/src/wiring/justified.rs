//! The justified layout of the grid (ADR 0161): the same cells as the
//! square grid, placed in rows of one height at each photograph's own shape.
//!
//! The square grid places cell *n* by arithmetic in Slint; this one cannot,
//! so Rust holds the layout of the whole query and does three things with
//! it: it turns the pixel band Slint scrolls to into the index window
//! `load_window` already knows how to serve (§4), it gives each loaded cell
//! its rectangle, and it answers `↑` / `↓` (§5).

use std::cell::RefCell;
use std::rc::Rc;

use leyline_sdk::{GridShape, JustifiedLayout, JustifyFrame, justify, step_vertical};
use slint::{ComponentHandle, Global};

use crate::app::{App, report_error, window_is_stale};
use crate::preferences::SharedPreferences;
use crate::ui::{GridState, StudioWindow};
use crate::wiring::dialogs::preferences::with_preferences;
use crate::wiring::grid::load_window;

/// Horizontal room a cell takes around its image: the 4px inset and the
/// 6px padding on each side, as `browser.slint` draws them. Laid out as the
/// gap between two image areas, so each image keeps its photograph's ratio.
const CELL_CHROME: f32 = 20.0;
/// The name-and-stars row under the image (ADR 0146 §2) and the 4px above
/// it, as `browser.slint` draws them.
const CELL_FOOT: f32 = 18.0;

/// The `grid_layout` value of the justified grid in `preferences.json`;
/// absence is the square grid (ADR 0161 §1).
pub(crate) const JUSTIFIED: &str = "justified";

/// What the justified layout holds while it is the grid's layout.
#[derive(Debug, Default)]
pub(crate) struct JustifiedGrid {
    /// Width over height of every photograph of the query, in grid order.
    aspects: Vec<f32>,
    /// The frame last reported by the grid; `None` until it has a width.
    frame: Option<JustifyFrame>,
    /// The rows of `aspects` in `frame`.
    layout: JustifiedLayout,
    /// The pixel band the viewport last showed: top, then height.
    band: (f32, f32),
}

impl JustifiedGrid {
    /// Recomputes the rows after the shapes or the frame changed.
    fn relayout(&mut self) {
        self.layout = self
            .frame
            .map(|frame| justify(&self.aspects, &frame))
            .unwrap_or_default();
    }

    /// The index window serving the current band, as `App::viewport` holds
    /// it: first index, then how many.
    fn viewport(&self) -> (usize, usize) {
        let range = self.layout.indices_in(self.band.0, self.band.1);
        (range.start, range.len().max(1))
    }

    /// The rectangle of grid index `index` — x, y, width, height — chrome
    /// included; zero for an index the layout does not hold.
    pub(crate) fn rectangle(&self, index: usize) -> (f32, f32, f32, f32) {
        let Some(row) = self.layout.row_of(index).map(|row| self.layout.rows[row]) else {
            return (0.0, 0.0, 0.0, 0.0);
        };
        let cell = self.layout.cells[index];
        (
            cell.x,
            row.y,
            cell.width + CELL_CHROME,
            row.image_height + CELL_FOOT + CELL_CHROME,
        )
    }
}

/// Reads the shapes of the whole query again and lays them out — on every
/// change of query, from `reload` (ADR 0161 §3). A no-op in the square
/// layout. Measured at 30 ms on 25 686 versions, which is why it runs here
/// rather than in the background (`docs/engine-api.md` §7).
pub(crate) fn refetch_shapes(app: &mut App, window: &StudioWindow) -> Result<(), String> {
    let Some(grid) = app.justified.as_mut() else {
        return Ok(());
    };
    let shapes = app
        .library
        .catalog()
        .grid_shapes(&app.query)
        .map_err(|e| e.to_string())?;
    grid.aspects = shapes.iter().map(GridShape::aspect).collect();
    grid.relayout();
    app.viewport = grid.viewport();
    publish_height(grid, window);
    Ok(())
}

fn publish_height(grid: &JustifiedGrid, window: &StudioWindow) {
    GridState::get(window).set_justified_height(grid.layout.height);
}

/// Switches the grid between its two layouts, as `GridState.justified` now
/// says, and remembers the choice (ADR 0161 §1).
fn switch_layout(
    app: &mut App,
    window: &StudioWindow,
    preferences: &SharedPreferences,
) -> Result<(), String> {
    let justified = GridState::get(window).get_justified();
    let _ = with_preferences(preferences, |file| {
        file.update(|values| values.grid_layout = justified.then(|| JUSTIFIED.to_owned()))
    });
    if justified == app.justified.is_some() {
        return Ok(());
    }
    app.justified = justified.then(JustifiedGrid::default);
    // The grid reports its frame and its band as soon as it sees the
    // switch; until then there is nothing to place.
    refetch_shapes(app, window)
}

/// The grid's width or the row height changed: lay the rows out again on
/// the shapes already held — never the query (ADR 0161 §6).
fn frame_changed(app: &mut App, window: &StudioWindow, width: f32, cell_size: f32) {
    let Some(grid) = app.justified.as_mut() else {
        return;
    };
    let frame = (width > CELL_CHROME).then(|| JustifyFrame {
        width: width - CELL_CHROME,
        // The slider sets the square cell's outer size; the row's image
        // height is that minus the same chrome, so one number means "how
        // big" in both layouts.
        row_height: (cell_size - CELL_CHROME).max(1.0),
        gap: CELL_CHROME,
        foot: CELL_FOOT,
    });
    if frame == grid.frame {
        return;
    }
    grid.frame = frame;
    grid.relayout();
    app.viewport = grid.viewport();
    publish_height(grid, window);
    // Every loaded cell has moved, stale window or not.
    if let Err(error) = load_window(app, window) {
        report_error(window, &error);
    }
}

/// The viewport scrolled or changed height (ADR 0161 §4).
fn viewport_moved(app: &mut App, window: &StudioWindow, top: f32, height: f32) {
    let Some(grid) = app.justified.as_mut() else {
        return;
    };
    grid.band = (top, height);
    app.viewport = grid.viewport();
    let loaded = app.window_start..app.window_start + app.items.len();
    if window_is_stale(app.viewport, &loaded, app.total)
        && let Err(error) = load_window(app, window)
    {
        report_error(window, &error);
    }
}

/// Restores the stored layout before the first `reload`, so the grid is not
/// drawn square and then redrawn justified.
pub(crate) fn restore_layout(
    app: &mut App,
    window: &StudioWindow,
    preferences: &SharedPreferences,
) {
    let stored = with_preferences(preferences, |file| file.values().grid_layout.clone());
    let justified = stored.as_deref() == Some(JUSTIFIED);
    GridState::get(window).set_justified(justified);
    app.justified = justified.then(JustifiedGrid::default);
}

pub(crate) fn wire_justified(
    app: &Rc<RefCell<App>>,
    window: &StudioWindow,
    preferences: &SharedPreferences,
) {
    let state = GridState::get(window);
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        let preferences = preferences.clone();
        state.on_justified_changed(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            if let Err(error) = switch_layout(&mut app.borrow_mut(), &window, &preferences) {
                report_error(&window, &error);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        state.on_justified_frame_changed(move |width, cell_size| {
            if let Some(window) = handle.upgrade() {
                frame_changed(&mut app.borrow_mut(), &window, width, cell_size);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        state.on_justified_viewport_moved(move |top, height| {
            if let Some(window) = handle.upgrade() {
                viewport_moved(&mut app.borrow_mut(), &window, top, height);
            }
        });
    }
    // The three questions Slint asks while it scrolls or moves the focus.
    // `try_borrow`: they are pure callbacks, evaluated whenever Slint needs
    // them, and must answer rather than panic if that is mid-update.
    {
        let app = Rc::clone(app);
        state.on_justified_step(move |index, direction| {
            let Ok(app) = app.try_borrow() else {
                return index;
            };
            match (app.justified.as_ref(), usize::try_from(index)) {
                (Some(grid), Ok(from)) => {
                    i32::try_from(step_vertical(&grid.layout, from, direction)).unwrap_or(index)
                }
                // No focus yet: the first photograph, as `↓` does square.
                _ => index.max(0),
            }
        });
    }
    {
        let app = Rc::clone(app);
        state.on_justified_cell_top(move |index| cell_span(&app, index).0);
    }
    {
        let app = Rc::clone(app);
        state.on_justified_cell_bottom(move |index| cell_span(&app, index).1);
    }
}

/// Top and bottom of grid index `index`, for scrolling it into view.
fn cell_span(app: &Rc<RefCell<App>>, index: i32) -> (f32, f32) {
    let Ok(app) = app.try_borrow() else {
        return (0.0, 0.0);
    };
    match (app.justified.as_ref(), usize::try_from(index)) {
        (Some(grid), Ok(index)) => {
            let (_, y, _, height) = grid.rectangle(index);
            (y, y + height)
        }
        _ => (0.0, 0.0),
    }
}

/// The rectangle a loaded cell is drawn at, for `load_window`: zero in the
/// square layout, where Slint places cells itself.
pub(crate) fn cell_rectangle(app: &App, index: usize) -> (f32, f32, f32, f32) {
    app.justified
        .as_ref()
        .map_or((0.0, 0.0, 0.0, 0.0), |grid| grid.rectangle(index))
}
