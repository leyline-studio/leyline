//! Integration tests: tether sessions (`docs/adr/0038-tethered-capture.md`,
//! `docs/adr/0087-tethered-capture-bar.md`).
//!
//! No USB camera is attached in CI or a plain dev machine, so these only
//! cover what's true without hardware: a clean error instead of a panic or
//! a hang, that `tether_disconnect` never requires a session to exist, and
//! that every command of the capture bar is safe to issue with nothing
//! connected.

use leyline_core::{CameraSettings, LeylineError, TetherSetting};
use leyline_engine::{Library, TetherOptions};

fn open_test_library(name: &str) -> (tempfile::TempDir, Library) {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), name).unwrap();
    (dir, library)
}

#[test]
fn tether_connect_without_a_camera_returns_a_tether_error() {
    let (_dir, library) = open_test_library("TetherNoCamera");
    let result = library.tether_connect(&TetherOptions::default());
    assert!(matches!(result, Err(LeylineError::Tether(_))));
}

/// A session name that cannot become a folder is refused *before* the USB
/// connection is attempted (ADR 0087 §4) — and refused as bad settings, not
/// as a camera problem, so the message sends the user to the field they
/// typed in rather than to the cable.
#[test]
fn a_session_name_that_is_a_path_is_refused_before_the_camera() {
    let (_dir, library) = open_test_library("TetherBadSession");
    let result = library.tether_connect(&TetherOptions {
        session: "../escape".to_owned(),
        preset: None,
    });
    assert!(matches!(result, Err(LeylineError::InvalidSettings(_))));
}

/// Every command of the bar is safe to issue with nothing connected: the
/// bar is hidden then, but an event arriving a tick late must not be able
/// to panic the interface (ADR 0087 §1).
#[test]
fn the_bar_commands_are_no_ops_without_a_session() {
    let (_dir, library) = open_test_library("TetherNoSessionCommands");
    assert_eq!(library.tether_settings(), CameraSettings::default());
    assert!(library.tether_live_frame().is_none());
    library.tether_capture();
    library.tether_set(TetherSetting::Iso, "800");
    library.tether_live_view(true);
    library.tether_set_preset(None);
}

#[test]
fn tether_disconnect_without_a_session_is_a_no_op() {
    let (_dir, library) = open_test_library("TetherNoSession");
    library.tether_disconnect();
}
