//! Keystone by drawn lines (ADR 0119, ADR 0045 §4).
//!
//! The one placed tool whose gesture writes nothing by itself: the guides
//! accumulate in the window's own state, and only *Straighten* turns them
//! into a revision. They render nothing, so ADR 0119 §5 keeps them out of
//! every revision, preset and fingerprint.

use std::cell::RefCell;
use std::rc::Rc;

use super::refresh_develop;
use crate::app::{App, report_error};
use crate::develop;
use crate::ui::{DevelopState, KeystoneGuide, StudioWindow};
use leyline_sdk::{Param, Value};
use slint::{ComponentHandle, Global, ModelRc, VecModel};

pub(super) fn wire_keystone(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        // Picking the tool clears the correction (ADR 0119 §4). A plain
        // edit, undone by the undo everything else is undone by — not a
        // mode with a cancel path of its own.
        DevelopState::get(window).on_develop_keystone_begin(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            app.keystone.clear();
            show(&window, &app);
            let Some((_, version)) = app.develop else {
                return;
            };
            let committed = (|| {
                let mut session = app.library.edit(version)?;
                if session.settings().perspective.is_none() {
                    return Ok(());
                }
                session.set(Param::Perspective, Value::Perspective(None))?;
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
        DevelopState::get(window).on_develop_keystone_drag(
            move |px, py, rx, ry, vw, vh, iw, ih| {
                let Some(window) = handle.upgrade() else {
                    return;
                };
                let mut app = app.borrow_mut();
                let Some((_, version)) = app.develop else {
                    return;
                };
                // The crop is undone on the way in, so the guides land in the
                // frame the `perspective` stage actually receives.
                let crop = match app.library.edit(version) {
                    Ok(session) => session.settings().crop.clone(),
                    Err(error) => {
                        report_error(&window, &error.to_string());
                        return;
                    }
                };
                let Some(drawn) = develop::keystone_line(
                    (f64::from(px), f64::from(py)),
                    (f64::from(rx), f64::from(ry)),
                    (f64::from(vw), f64::from(vh)),
                    (f64::from(iw), f64::from(ih)),
                    crop.as_ref(),
                ) else {
                    return;
                };
                app.keystone.push(drawn);
                show(&window, &app);
            },
        );
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_keystone_apply(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let lines: Vec<_> = app.keystone.iter().map(|(line, _)| *line).collect();
            let applied = (|| {
                let solution = app.library.keystone(version, &lines)?;
                let mut session = app.library.edit(version)?;
                session.set(
                    Param::Perspective,
                    Value::Perspective(Some(solution.perspective)),
                )?;
                session.commit().map(|_| ())
            })();
            if let Err(error) = applied.map_err(|e| e.to_string()) {
                report_error(&window, &error);
                return;
            }
            // The guides have said what they had to say, and they were never
            // part of the revision they produced.
            app.keystone.clear();
            show(&window, &app);
            if let Err(error) = refresh_develop(&mut app, &window) {
                report_error(&window, &error);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_keystone_clear(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            app.keystone.clear();
            show(&window, &app);
        });
    }
}

/// Mirrors the drawn guides into the panel's model, in the displayed frame's
/// own units — the letterbox is recomputed at paint time, so a window resize
/// moves them with the photograph.
fn show(window: &StudioWindow, app: &App) {
    let guides: Vec<KeystoneGuide> = app
        .keystone
        .iter()
        .map(|(_, view)| KeystoneGuide {
            x1: view[0] as f32,
            y1: view[1] as f32,
            x2: view[2] as f32,
            y2: view[3] as f32,
        })
        .collect();
    DevelopState::get(window).set_keystone_guides(ModelRc::new(VecModel::from(guides)));
}
