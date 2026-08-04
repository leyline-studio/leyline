//! Slider, HSL, color grading and crop edits (ADR 0045 §4).
//!
//! The rules deciding what each slider does live in `crate::develop`, free of
//! any Slint type; this module only carries released values across.

use std::cell::RefCell;
use std::rc::Rc;

use super::refresh_develop;
use crate::app::{App, report_error};
use crate::develop;
use crate::ui::{DevelopState, StudioWindow};
use slint::{ComponentHandle, Global};

pub(super) fn wire_adjustments(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_edit(move |slider, value| {
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
                    develop::action(slider.as_str(), f64::from(value), session.settings())
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
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        // The same edit, shown rather than applied (ADR 0074). It reuses
        // `develop::action`, so a slider cannot mean one thing while being
        // dragged and another when released.
        DevelopState::get(window).on_develop_preview(move |slider, value| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            super::live_preview(&mut app, &window, |settings| {
                develop::action(slider.as_str(), f64::from(value), settings)
            });
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        // ADR 0050: a mode name rather than a slider value, and the one edit
        // the engine can refuse outright — a revision pinned at `input: 1`
        // cannot express it, so the error reaches the status line instead of
        // the mode being dropped.
        DevelopState::get(window).on_develop_set_highlight_reconstruction(move |mode| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let committed = (|| {
                let mut session = app.library.edit(version)?;
                let Some((param, value)) = develop::highlight_reconstruction_action(mode.as_str())
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
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_set_demosaic(move |mode| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let committed = (|| {
                let mut session = app.library.edit(version)?;
                let Some((param, value)) = develop::demosaic_action(mode.as_str()) else {
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
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_preview_hsl_band(move |index, field, value| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            super::live_preview(&mut app, &window, |settings| {
                develop::hsl_band_action(index as usize, field.as_str(), f64::from(value), settings)
            });
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_edit_hsl_band(move |index, field, value| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let committed = (|| {
                let mut session = app.library.edit(version)?;
                let Some((param, value)) = develop::hsl_band_action(
                    index as usize,
                    field.as_str(),
                    f64::from(value),
                    session.settings(),
                ) else {
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
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_preview_color_grading_zone(
            move |zone, field, value| {
                let Some(window) = handle.upgrade() else {
                    return;
                };
                let mut app = app.borrow_mut();
                super::live_preview(&mut app, &window, |settings| {
                    develop::color_grading_zone_action(
                        zone.as_str(),
                        field.as_str(),
                        f64::from(value),
                        settings,
                    )
                });
            },
        );
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_edit_color_grading_zone(move |zone, field, value| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let committed = (|| {
                let mut session = app.library.edit(version)?;
                let Some((param, value)) = develop::color_grading_zone_action(
                    zone.as_str(),
                    field.as_str(),
                    f64::from(value),
                    session.settings(),
                ) else {
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
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_crop_drag(move |px, py, rx, ry, vw, vh, iw, ih| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let committed = (|| {
                let mut session = app.library.edit(version)?;
                let Some((param, value)) = develop::drag_crop(
                    (f64::from(px), f64::from(py)),
                    (f64::from(rx), f64::from(ry)),
                    (f64::from(vw), f64::from(vh)),
                    (f64::from(iw), f64::from(ih)),
                    &session.settings().crop,
                ) else {
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
