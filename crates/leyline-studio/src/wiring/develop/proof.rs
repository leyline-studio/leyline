//! Screen soft proofing (ADR 0034, ADR 0051 §4, ADR 0045 §4).
//!
//! A proof is a way of *looking* at the photo, so everything here lives in the
//! window's own state: picking a destination profile changes what the develop
//! view shows and nothing else. No revision, no preset, no cache entry — which
//! is also why this module has no `session.set` anywhere.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use super::refresh_develop;
use crate::app::{App, report_error};
use crate::ui::{DevelopState, StudioWindow};
use leyline_sdk::{RenderingIntent, SoftProof};
use slint::{ComponentHandle, Global, SharedString};

pub(super) fn wire_proof(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_browse_soft_proof(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let Some(path) = rfd::FileDialog::new()
                .add_filter("ICC profile", &["icc", "icm"])
                .pick_file()
            else {
                return;
            };
            let mut app = app.borrow_mut();
            let state = DevelopState::get(&window);
            app.soft_proof = Some(SoftProof {
                profile: path.clone(),
                intent: intent_of(state.get_soft_proof_intent()),
                gamut_warning: state.get_soft_proof_gamut_warning(),
            });
            state.set_soft_proof_name(SharedString::from(profile_name(&path)));
            reproof(&mut app, &window);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_clear_soft_proof(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            app.soft_proof = None;
            DevelopState::get(&window).set_soft_proof_name(SharedString::default());
            reproof(&mut app, &window);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_set_soft_proof_intent(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            DevelopState::get(&window).set_soft_proof_intent(index);
            if let Some(proof) = &mut app.soft_proof {
                proof.intent = intent_of(index);
            }
            reproof(&mut app, &window);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_toggle_soft_proof_gamut_warning(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let state = DevelopState::get(&window);
            let warn = !state.get_soft_proof_gamut_warning();
            state.set_soft_proof_gamut_warning(warn);
            if let Some(proof) = &mut app.soft_proof {
                proof.gamut_warning = warn;
            }
            reproof(&mut app, &window);
        });
    }
}

/// Re-renders the develop view after the proof changed. A failure — an
/// unreadable or unusable profile — is reported and drops the proof, rather
/// than leaving the view claiming to proof through something it could not
/// load.
fn reproof(app: &mut App, window: &StudioWindow) {
    if let Err(error) = refresh_develop(app, window) {
        app.soft_proof = None;
        DevelopState::get(window).set_soft_proof_name(SharedString::default());
        report_error(window, &error);
    }
}

/// The intent for a picker index — the four ICC intents, in the order the
/// panel lists them.
pub(crate) fn intent_of(index: i32) -> RenderingIntent {
    match index {
        0 => RenderingIntent::Perceptual,
        2 => RenderingIntent::Saturation,
        3 => RenderingIntent::AbsoluteColorimetric,
        // Relative colorimetric is the sensible default for proofing, and the
        // fallback for an index the panel could not have produced.
        _ => RenderingIntent::RelativeColorimetric,
    }
}

/// The file name of a profile path, for the panel's one-line label.
pub(crate) fn profile_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intents_follow_the_panels_order_and_default_to_relative() {
        assert_eq!(intent_of(0), RenderingIntent::Perceptual);
        assert_eq!(intent_of(1), RenderingIntent::RelativeColorimetric);
        assert_eq!(intent_of(2), RenderingIntent::Saturation);
        assert_eq!(intent_of(3), RenderingIntent::AbsoluteColorimetric);
        assert_eq!(intent_of(-1), RenderingIntent::RelativeColorimetric);
        assert_eq!(intent_of(99), RenderingIntent::RelativeColorimetric);
    }

    #[test]
    fn the_label_is_the_file_name_alone() {
        assert_eq!(
            profile_name(Path::new("/home/me/profiles/Epson P900 Luster.icc")),
            "Epson P900 Luster.icc"
        );
        assert_eq!(profile_name(Path::new("/")), "");
    }
}
