//! Spot removal edits (ADR 0032, ADR 0045 §4).

use std::cell::RefCell;
use std::rc::Rc;

use super::refresh_develop;
use crate::app::{App, report_error};
use crate::develop;
use crate::ui::{DevelopState, StudioWindow};
use slint::{ComponentHandle, Global};

pub(super) fn wire_spot(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_develop_spot_click(move |sx, sy, tx, ty, vw, vh, iw, ih| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let radius_feather_opacity = match spot_defaults(
                DevelopState::get(&window).get_spot_radius_text().as_str(),
                DevelopState::get(&window).get_spot_feather_text().as_str(),
                DevelopState::get(&window).get_spot_opacity_text().as_str(),
            ) {
                Ok(defaults) => defaults,
                Err(error) => {
                    report_error(&window, &error);
                    return;
                }
            };
            let committed = (|| {
                let mut session = app.library.edit(version)?;
                let Some((param, value)) = develop::place_spot(
                    (f64::from(sx), f64::from(sy)),
                    (f64::from(tx), f64::from(ty)),
                    (f64::from(vw), f64::from(vh)),
                    (f64::from(iw), f64::from(ih)),
                    radius_feather_opacity,
                    &session.settings().spot_removal,
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
        DevelopState::get(window).on_develop_spot_undo(move || {
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
                    develop::undo_last_spot(&session.settings().spot_removal)
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
        DevelopState::get(window).on_develop_spot_reset(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let (param, value) = develop::reset_spots();
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

/// Parses the spot-removal panel's radius/feather/opacity percent fields
/// into the `[0, 1]` units `SpotRemoval` stores — the defaults applied to
/// the *next* spot placed, not part of any stored revision themselves.
pub(crate) fn spot_defaults(
    radius: &str,
    feather: &str,
    opacity: &str,
) -> Result<(f64, f64, f64), String> {
    let radius: f64 = radius
        .parse()
        .map_err(|_| format!("bad radius {radius:?}"))?;
    let feather: f64 = feather
        .parse()
        .map_err(|_| format!("bad feather {feather:?}"))?;
    let opacity: f64 = opacity
        .parse()
        .map_err(|_| format!("bad opacity {opacity:?}"))?;
    Ok((radius / 100.0, feather / 100.0, opacity / 100.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spot_defaults_converts_percent_to_unit_range() {
        assert_eq!(spot_defaults("5", "50", "100").unwrap(), (0.05, 0.5, 1.0));
    }

    #[test]
    fn spot_defaults_rejects_bad_input() {
        assert!(spot_defaults("not a number", "50", "100").is_err());
        assert!(spot_defaults("5", "not a number", "100").is_err());
        assert!(spot_defaults("5", "50", "not a number").is_err());
    }
}
