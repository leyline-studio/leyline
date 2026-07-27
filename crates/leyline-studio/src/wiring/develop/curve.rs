//! Tone curve edits (ADR 0030, ADR 0045 §4).

use std::cell::RefCell;
use std::rc::Rc;

use super::refresh_develop;
use crate::app::{App, report_error};
use crate::develop;
use crate::ui::{DevelopState, StudioWindow};
use slint::{ComponentHandle, Global};

pub(super) fn wire_curve(app: &Rc<RefCell<App>>, window: &StudioWindow) {
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
                let Some((param, value)) =
                    develop::curve_point(click, &session.settings().tone_curve.points)
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
            let (param, value) = develop::reset_curve();
            let committed = (|| {
                let mut session = app.library.edit(version)?;
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
