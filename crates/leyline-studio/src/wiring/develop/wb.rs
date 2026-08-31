//! White-balance picker, Auto and fixed presets (ADR 0091, ADR 0045 §4).
//!
//! All three end the same way: one `WhiteBalance` written through the
//! ordinary session — the engine proposes, the revision history records.

use std::cell::RefCell;
use std::rc::Rc;

use super::refresh_develop;
use crate::app::{App, report_error};
use crate::develop;
use crate::ui::{DevelopState, StudioWindow};
use leyline_sdk::{Param, Value, WHITE_BALANCE_PRESETS, WhiteBalance};
use slint::{ComponentHandle, Global};

pub(super) fn wire_wb(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_wb_click(move |vx, vy, vw, vh, iw, ih| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((asset, version)) = app.develop else {
                return;
            };
            let Some((x, y)) = develop::letterbox_unit(
                (f64::from(vx), f64::from(vy)),
                (f64::from(vw), f64::from(vh)),
                (f64::from(iw), f64::from(ih)),
            ) else {
                return;
            };
            let committed = (|| {
                let wb = app.library.neutralize_wb(asset, x, y)?;
                write_wb(&mut app, version, Some(wb))
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
        DevelopState::get(window).on_run_auto_wb(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((asset, version)) = app.develop else {
                return;
            };
            let committed = (|| {
                let wb = app.library.auto_wb(asset)?;
                write_wb(&mut app, version, Some(wb))
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
        DevelopState::get(window).on_apply_wb_preset(move |name| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            // "as-shot" clears the override — the true neutral of the
            // model (ADR 0091 §4); anything else is a table entry.
            let wb = match name.as_str() {
                "as-shot" => None,
                key => match WHITE_BALANCE_PRESETS.iter().find(|p| p.name == key) {
                    Some(preset) => Some(WhiteBalance {
                        temperature: preset.temperature,
                        tint: preset.tint,
                    }),
                    None => return,
                },
            };
            if let Err(error) = write_wb(&mut app, version, wb)
                .map_err(|e| e.to_string())
                .and_then(|()| refresh_develop(&mut app, &window))
            {
                report_error(&window, &error);
            }
        });
    }
}

/// One white balance, one revision.
fn write_wb(
    app: &mut App,
    version: leyline_sdk::VersionId,
    wb: Option<WhiteBalance>,
) -> leyline_sdk::Result<()> {
    let mut session = app.library.edit(version)?;
    session.set(Param::WhiteBalance, Value::WhiteBalance(wb))?;
    session.commit().map(|_| ())
}
