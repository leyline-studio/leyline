//! Local adjustment edits: the tools that trace a mask and the sliders that
//! re-parameterize it (ADR 0029, ADR 0048, ADR 0049, ADR 0045 §4).
//!
//! The rules deciding what a gesture writes live in [`crate::masks`], free of
//! any Slint type; this module only moves values across the boundary and
//! commits, exactly as [`super::spot`] does for spot removal.

use std::cell::RefCell;
use std::rc::Rc;

use super::refresh_develop;
use crate::app::{App, report_error};
use crate::masks;
use crate::ui::{MaskState, StudioWindow};
use leyline_sdk::{Param, Value};
use slint::{ComponentHandle, Global};

pub(super) fn wire_masks(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        MaskState::get(window).on_add_mask(move |kind| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            commit(&mut app, &window, |current| {
                masks::add_mask(kind.as_str(), current)
            });
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        MaskState::get(window).on_remove_mask(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            commit(&mut app, &window, |current| {
                masks::remove_mask(index, current)
            });
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        MaskState::get(window).on_place_mask(
            move |selected, kind, px, py, mx, my, vw, vh, iw, ih| {
                let Some(window) = handle.upgrade() else {
                    return;
                };
                let mut app = app.borrow_mut();
                let Some(mask) = masks::drag_geometry(
                    kind.as_str(),
                    (f64::from(px), f64::from(py)),
                    (f64::from(mx), f64::from(my)),
                    (f64::from(vw), f64::from(vh)),
                    (f64::from(iw), f64::from(ih)),
                ) else {
                    return;
                };
                commit(&mut app, &window, |current| {
                    Some(masks::place_geometry(selected, mask, current))
                });
            },
        );
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        MaskState::get(window).on_paint_mask_dab(move |selected, mx, my, vw, vh, iw, ih| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let defaults = match brush_defaults(
                MaskState::get(&window).get_brush_size_text().as_str(),
                MaskState::get(&window).get_brush_flow_text().as_str(),
                MaskState::get(&window).get_brush_hardness_text().as_str(),
            ) {
                Ok(defaults) => defaults,
                Err(error) => {
                    report_error(&window, &error);
                    return;
                }
            };
            let Some(stroke) = masks::dab(
                (f64::from(mx), f64::from(my)),
                (f64::from(vw), f64::from(vh)),
                (f64::from(iw), f64::from(ih)),
                defaults,
            ) else {
                return;
            };
            commit(&mut app, &window, |current| {
                Some(masks::paint_dab(selected, stroke, current))
            });
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        MaskState::get(window).on_edit_mask(move |index, field, value| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let committed = (|| {
                let mut session = app.library.edit(version)?;
                let Some((param, value)) =
                    masks::edit_field(index, field.as_str(), f64::from(value), session.settings())
                else {
                    return Ok(());
                };
                session.set(param, value)?;
                session.commit().map(|_| ())
            })();
            if let Err(error) = committed
                .map_err(|e| e.to_string())
                .and_then(|()| refresh_develop(&mut app, &window))
            {
                report_error(&window, &error);
            }
        });
    }
}

/// Runs one local-adjustment decision against the current entries and commits
/// what it returns, then refreshes the develop view.
///
/// The four gestures share this shape — read the stored list, decide, commit,
/// refresh — and differ only in the decision, which is why it is a closure
/// here and a pure function in [`crate::masks`]. A decision returning `None`
/// (a degenerate drag, a row that no longer exists) commits nothing.
fn commit<F>(app: &mut App, window: &StudioWindow, decide: F)
where
    F: FnOnce(&[leyline_sdk::LocalAdjustment]) -> Option<(Param, Value)>,
{
    let Some((_, version)) = app.develop else {
        return;
    };
    let mut written = None;
    let committed = (|| {
        let mut session = app.library.edit(version)?;
        let Some((param, value)) = decide(&session.settings().local_adjustments) else {
            return Ok(());
        };
        // A gesture can create as well as modify, and what it created has to
        // become the selected row — otherwise the values of a mask that was
        // just traced would have nowhere to appear (ADR 0049 §2).
        written =
            masks::written_row(&param).filter(|_| !matches!(value, Value::LocalAdjustment(None)));
        session.set(param, value)?;
        session.commit().map(|_| ())
    })();
    if committed.is_ok() {
        if let Some(row) = written {
            MaskState::get(window).set_selected_mask(i32::try_from(row).unwrap_or(-1));
        }
    }
    if let Err(error) = committed
        .map_err(|e| e.to_string())
        .and_then(|()| refresh_develop(app, window))
    {
        report_error(window, &error);
    }
}

/// Parses the brush panel's size/flow/hardness percent fields into the
/// `[0, 1]` units [`leyline_sdk::BrushStroke`] stores — the defaults applied
/// to the *next* dab painted, part of no stored revision themselves, exactly
/// like [`super::spot::spot_defaults`].
pub(crate) fn brush_defaults(
    size: &str,
    flow: &str,
    hardness: &str,
) -> Result<(f64, f64, f64), String> {
    let size: f64 = size.parse().map_err(|_| format!("bad size {size:?}"))?;
    let flow: f64 = flow.parse().map_err(|_| format!("bad flow {flow:?}"))?;
    let hardness: f64 = hardness
        .parse()
        .map_err(|_| format!("bad hardness {hardness:?}"))?;
    Ok((size / 100.0, flow / 100.0, hardness / 100.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brush_defaults_converts_percent_to_unit_range() {
        assert_eq!(brush_defaults("8", "50", "40").unwrap(), (0.08, 0.5, 0.4));
    }

    #[test]
    fn brush_defaults_rejects_bad_input() {
        assert!(brush_defaults("wide", "50", "40").is_err());
        assert!(brush_defaults("8", "half", "40").is_err());
        assert!(brush_defaults("8", "50", "hard").is_err());
    }
}
