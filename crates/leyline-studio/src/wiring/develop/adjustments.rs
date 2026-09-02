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
use leyline_sdk::{Param, Value};
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
        DevelopState::get(window).on_measure_tca(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let measured = (|| {
                // Same shape as Auto tone below, for the same reason
                // (ADR 0111 §2): the engine measures, an ordinary session
                // writes, and the photographer can undo it or drag the
                // sliders afterwards.
                let estimate = app.library.estimate_tca(version)?;
                if estimate.samples < leyline_sdk::TCA_MIN_SAMPLES {
                    return Ok(Some(estimate.samples));
                }
                let mut session = app.library.edit(version)?;
                let lens = leyline_sdk::LensCorrection {
                    tca_red: estimate.red,
                    tca_blue: estimate.blue,
                    ..session.settings().lens_correction.clone()
                };
                session.set(Param::LensCorrection, Value::LensCorrection(lens))?;
                session.commit().map(|_| None)
            })();
            match measured.map_err(|e| e.to_string()) {
                // Not a failure: this photograph has nothing to measure on,
                // and saying so is the honest answer (ADR 0111 §6).
                Ok(Some(samples)) => {
                    let message =
                        crate::ui::Tr::get(&window).invoke_tca_not_enough_edges(samples as i32);
                    crate::ui::LibraryState::get(&window).set_status_line(message);
                }
                Ok(None) => {
                    if let Err(error) = refresh_develop(&mut app, &window) {
                        report_error(&window, &error);
                    }
                }
                Err(error) => report_error(&window, &error),
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_run_auto_tone(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((asset, version)) = app.develop else {
                return;
            };
            let committed = (|| {
                // The engine only *proposes* (ADR 0088 §1). Writing the five
                // values through the ordinary session is what makes Auto one
                // undoable revision rather than a second way of developing a
                // photo — and it is why a second press, on a photo Auto
                // already answered for, is just another edit.
                let tone = app.library.auto_tone(asset)?;
                let mut session = app.library.edit(version)?;
                for (param, value) in [
                    (Param::Exposure, Value::Float(tone.exposure)),
                    (Param::Highlights, Value::Int(tone.highlights)),
                    (Param::Shadows, Value::Int(tone.shadows)),
                    (Param::Whites, Value::Int(tone.whites)),
                    (Param::Blacks, Value::Int(tone.blacks)),
                ] {
                    session.set(param, value)?;
                }
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
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        // Clipping toggles (ADR 0092 §2): "highlights" and "shadows" from
        // the histogram's triangles, "both" from J — which turns the pair
        // on together and off together, from either mixed state.
        DevelopState::get(window).on_toggle_clipping(move |which| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            match which.as_str() {
                "highlights" => app.clip_highlights = !app.clip_highlights,
                "shadows" => app.clip_shadows = !app.clip_shadows,
                "both" => {
                    let on = !(app.clip_highlights || app.clip_shadows);
                    app.clip_highlights = on;
                    app.clip_shadows = on;
                }
                _ => return,
            }
            if let Err(error) = refresh_develop(&mut app, &window) {
                report_error(&window, &error);
            }
        });
    }
}
