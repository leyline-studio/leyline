//! Tone curve edits (ADR 0030, ADR 0045 §4).

use std::cell::RefCell;
use std::rc::Rc;

use super::refresh_develop;
use crate::app::{App, report_error};
use crate::develop;
use crate::ui::{DevelopState, StudioWindow};
use slint::{ComponentHandle, Global};

pub(super) fn wire_curve(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    // The three parametric splits (ADR 0137 §4): shown while dragged,
    // committed on release, like every other drag in this module.
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_preview_split(move |which, percent| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            super::live_preview(&mut app, &window, |settings| {
                develop::split_action(which.as_str(), percent, settings)
            });
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_set_split(move |which, percent| {
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
                    develop::split_action(which.as_str(), percent, session.settings())
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
        DevelopState::get(window).on_develop_curve_click(move |mx, my, w, h| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let committed = (|| {
                let mut session = app.library.edit(version)?;
                // The curve canvas isn't letterboxed — it's its own square
                // widget — so this is a plain axis flip, not `letterbox_unit`.
                let click = (
                    f64::from(mx) / f64::from(w),
                    1.0 - f64::from(my) / f64::from(h),
                );
                let channel = DevelopState::get(&window).get_dev_curve_channel();
                let Some((param, value)) =
                    develop::curve_point(click, channel.as_str(), &session.settings().tone_curve)
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
        DevelopState::get(window).on_develop_curve_reset(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let channel = DevelopState::get(&window).get_dev_curve_channel();
            let committed = (|| {
                let mut session = app.library.edit(version)?;
                let (param, value) =
                    develop::reset_curve(channel.as_str(), &session.settings().tone_curve);
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
