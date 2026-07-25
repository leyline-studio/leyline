//! End-to-end smoke tests of the CLI binary itself.
//!
//! The CLI is a thin `leyline-sdk` client with no logic of its own,
//! proving `docs/engine-api.md` §1's "API before GUI" claim — everything
//! Studio does, these commands do too. Each test shells out to the real
//! binary against a temp library, the same way a user would.

use std::path::Path;
use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_leyline-cli"))
        .args(args)
        .output()
        .expect("the cli binary should run")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A real 4×4 PNG the engine can decode and measure.
fn sample_png(path: &Path) {
    image::save_buffer(
        path,
        &[128u8; 4 * 4 * 3],
        4,
        4,
        image::ExtendedColorType::Rgb8,
    )
    .unwrap();
}

/// A fresh library at `<dir>/Lib` with one imported photo, version 1.
fn library_with_a_photo(dir: &tempfile::TempDir) -> String {
    let root = dir.path().join("Lib");
    let root_s = root.to_str().unwrap().to_owned();
    assert!(run(&["new", &root_s]).status.success());
    let photo = dir.path().join("photo.png");
    sample_png(&photo);
    let out = run(&["import", &root_s, photo.to_str().unwrap()]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("1 imported"));
    root_s
}

#[test]
fn new_creates_a_library_and_info_reports_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Lib");
    let out = run(&["new", root.to_str().unwrap(), "--name", "Test Lib"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("Test Lib"));

    let out = run(&["info", root.to_str().unwrap()]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("name:    Test Lib"));
    assert!(stdout(&out).contains("assets:  0"));
}

#[test]
fn develop_covers_every_param_kind() {
    let dir = tempfile::tempdir().unwrap();
    let root = library_with_a_photo(&dir);

    for args in [
        vec!["develop", &root, "1", "exposure", "0.5"],
        vec!["develop", &root, "1", "white-balance", "5000", "10"],
        vec!["develop", &root, "1", "white-balance", "none"],
        vec!["develop", &root, "1", "lens-correction", "on"],
        vec!["develop", &root, "1", "noise-reduction", "20", "30"],
        vec!["develop", &root, "1", "sharpening", "40", "1.5"],
        vec!["develop", &root, "1", "crop", "10", "10", "80", "80"],
        vec!["develop", &root, "1", "crop", "reset"],
    ] {
        let out = run(&args);
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        assert!(stdout(&out).starts_with("committed revision"));
    }

    let out = run(&["history", &root, "1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    // The initial revision plus one commit per develop call above.
    assert_eq!(stdout(&out).lines().count(), 9);
    assert!(stdout(&out).lines().next().unwrap().starts_with("HEAD"));
}

#[test]
fn camera_profile_import_list_and_reference_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let root = library_with_a_photo(&dir);
    // Import and listing never parse the profile — only rendering does —
    // so any bytes exercise this path (ADR 0035).
    let source = dir.path().join("Canon 60D.dcp");
    std::fs::write(&source, b"profile bytes").unwrap();

    let out = run(&["camera-profile", &root, source.to_str().unwrap()]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("Profiles/Camera/Canon 60D.dcp"));
    assert!(stdout(&out).contains("blake3:"));

    // Importing the same name twice is refused, never overwritten.
    let out = run(&["camera-profile", &root, source.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("never overwritten"));

    let out = run(&["camera-profiles", &root]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("1 camera profile(s)"));

    for args in [
        vec![
            "develop",
            &root,
            "1",
            "camera-profile",
            "Profiles/Camera/Canon 60D.dcp",
        ],
        vec!["develop", &root, "1", "camera-profile", "off"],
        vec!["develop", &root, "1", "camera-profile", "on"],
        vec!["develop", &root, "1", "camera-profile", "none"],
    ] {
        let out = run(&args);
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        assert!(stdout(&out).starts_with("committed revision"));
    }

    // A path that was never imported is refused rather than committed with
    // a checksum nothing on disk matches.
    let out = run(&["develop", &root, "1", "camera-profile", "Nope.dcp"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("unknown camera profile"));

    // `on`/`off` need something to toggle: the last committed state above
    // cleared the reference.
    let out = run(&["develop", &root, "1", "camera-profile", "on"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("no camera profile referenced"));
}

#[test]
fn develop_rejects_bad_input_without_writing_a_revision() {
    let dir = tempfile::tempdir().unwrap();
    let root = library_with_a_photo(&dir);

    let out = run(&["develop", &root, "1", "lens-correction", "maybe"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("on/off"));

    let out = run(&["develop", &root, "1", "sharpening", "40"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("expects"));

    let out = run(&["develop", &root, "1", "gamma", "1.0"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("unknown develop parameter"));

    // None of the rejected calls committed: only the initial revision.
    let out = run(&["history", &root, "1"]);
    assert_eq!(stdout(&out).lines().count(), 1);
}

#[test]
fn reprocess_reports_already_current_and_unknown_versions() {
    let dir = tempfile::tempdir().unwrap();
    let root = library_with_a_photo(&dir);

    let out = run(&["reprocess", &root, "1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("already current 1"));

    let out = run(&["reprocess", &root, "999"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("failed 1"));
    assert!(stdout(&out).contains("version 999"));

    let out = run(&["reprocess", &root]);
    assert!(!out.status.success());
}

#[test]
fn classement_commands_round_trip_into_ls() {
    let dir = tempfile::tempdir().unwrap();
    let root = library_with_a_photo(&dir);

    assert!(run(&["rate", &root, "4", "1"]).status.success());
    assert!(run(&["pick", &root, "pick", "1"]).status.success());
    assert!(run(&["label", &root, "red", "1"]).status.success());

    let out = run(&["ls", &root]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("****"));
    assert!(stdout(&out).contains("1 version(s)"));
}

#[test]
fn unknown_command_prints_usage_and_fails() {
    let out = run(&["bogus"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("unknown command"));
}
