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
        vec!["develop", &root, "1", "white-balance", "daylight"],
        vec!["develop", &root, "1", "lens-correction", "on"],
        vec!["develop", &root, "1", "noise-reduction", "20", "30"],
        vec!["develop", &root, "1", "sharpening", "40", "1.5"],
        vec!["develop", &root, "1", "crop", "10", "10", "80", "80"],
        vec!["develop", &root, "1", "crop", "reset"],
        vec!["develop", &root, "1", "highlight-reconstruction", "blend"],
        vec!["develop", &root, "1", "highlight-reconstruction", "clip"],
        vec!["develop", &root, "1", "perspective", "30", "-10"],
        vec!["develop", &root, "1", "perspective", "reset"],
    ] {
        let out = run(&args);
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        assert!(stdout(&out).starts_with("committed revision"));
    }

    let out = run(&["history", &root, "1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    // The initial revision plus one commit per develop call above.
    assert_eq!(stdout(&out).lines().count(), 14);
    assert!(stdout(&out).lines().next().unwrap().starts_with("HEAD"));
}

/// The measured white balance (ADR 0091): grey-world on the frame, or the
/// picker on one point; --dry-run proposes without committing.
#[test]
fn auto_wb_proposes_and_writes() {
    let dir = tempfile::tempdir().unwrap();
    let root = library_with_a_photo(&dir);

    let out = run(&["auto-wb", &root, "1", "--dry-run"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("temperature"));
    assert!(!stdout(&out).contains("committed"));

    let out = run(&["auto-wb", &root, "1", "--sample", "0.5,0.5"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("committed revision"));

    let out = run(&["auto-wb", &root, "1", "--sample", "nonsense"]);
    assert!(!out.status.success());
}

/// ADR 0105 §2–§3: the CLI half of the detector socket, exercised end to
/// end against a fake detector — no model, no weights, just a script that
/// speaks the protocol. `--from` is what makes this testable, and it is the
/// same flag a detector author uses before installing anything.
#[cfg(unix)]
#[test]
fn the_cli_runs_and_checks_a_detector() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let root = library_with_a_photo(&dir);

    // A detector that answers by copying the image it was handed: the right
    // size, and a range of values, which is all the protocol asks.
    let script = dir.path().join("fake.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\nimage=\"\"; out=\"\"\nwhile [ $# -gt 0 ]; do\n\
         case \"$1\" in\n --image) image=\"$2\"; shift 2;;\n \
         --out) out=\"$2\"; shift 2;;\n *) shift;;\n esac\ndone\ncp \"$image\" \"$out\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let manifests = dir.path().join("detectors");
    std::fs::create_dir_all(&manifests).unwrap();
    std::fs::write(
        manifests.join("fake.json"),
        format!(
            r#"{{"id":"fake","label":"Fake","command":{:?},
                "detections":[{{"id":"sky","label":"Sky"}}]}}"#,
            script.to_str().unwrap()
        ),
    )
    .unwrap();
    let from = manifests.to_str().unwrap().to_owned();

    // It is listed, with its detection's key.
    let listing = stdout(&run(&["detectors", "--from", &from]));
    assert!(listing.contains("fake:sky"), "{listing}");

    // It speaks the protocol, and the pass says what it does not check.
    let out = run(&["detect-check", "fake:sky", "--from", &from]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("quality of the segmentation is not checked"),
        "a pass must not read as an endorsement: {}",
        stdout(&out)
    );

    // And a detection lands as a local adjustment carrying the coverage.
    let out = run(&["detect", &root, "1", "fake:sky", "--from", &from]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("committed revision"),
        "{}",
        stdout(&out)
    );
    assert!(
        stdout(&out).contains("local adjustment 0"),
        "{}",
        stdout(&out)
    );

    // An unknown detector is named, not swallowed.
    let out = run(&["detect", &root, "1", "nosuch:sky", "--from", &from]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("nosuch"), "{}", stderr(&out));

    // A manifest that was read and thrown away is named, with the reason
    // (ADR 0105 §4). Written the way ADR 0073 §3 described it until
    // 2026-08-31 — the wrong field is exactly the mistake an author makes.
    std::fs::write(
        manifests.join("wrong-field.json"),
        r#"{"id":"wrong","label":"Wrong","command":"/bin/sh",
            "detectors":[{"id":"sky","label":"Sky"}]}"#,
    )
    .unwrap();
    let out = run(&["detectors", "--from", &from]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("fake:sky"), "{}", stdout(&out));
    let complaint = stderr(&out);
    assert!(complaint.contains("wrong-field.json"), "{complaint}");
    assert!(complaint.contains("detections"), "{complaint}");
    std::fs::remove_file(manifests.join("wrong-field.json")).unwrap();

    // A malformed key says what shape it wanted.
    let out = run(&["detect-check", "missing-colon", "--from", &from]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("<detector>:<detection>"),
        "{}",
        stderr(&out)
    );
}

/// ADR 0100: renaming moves the file and the catalog together, and refuses
/// to overwrite.
#[test]
fn rename_moves_the_file_and_refuses_to_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let root = library_with_a_photo(&dir);

    let out = run(&["rename", &root, "Heron-{seq}", "1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("Heron-0001.png"), "{}", stdout(&out));
    assert!(stdout(&out).contains("1 renamed"), "{}", stdout(&out));

    // A second photo, then a template that would collide with the first.
    let second = dir.path().join("second.png");
    image::save_buffer(
        &second,
        &[200u8; 4 * 4 * 3],
        4,
        4,
        image::ExtendedColorType::Rgb8,
    )
    .unwrap();
    assert!(
        run(&["import", &root, second.to_str().unwrap()])
            .status
            .success()
    );
    let out = run(&["rename", &root, "Heron-0001", "2"]);
    assert!(!out.status.success(), "overwriting must be refused");
    assert!(stderr(&out).contains("already exists"), "{}", stderr(&out));

    // An unknown placeholder is refused, and names the right one.
    let out = run(&["rename", &root, "{sequence}", "2"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("{seq}"), "{}", stderr(&out));
}

/// ADR 0099: what is written about a photo is kept apart from what its
/// file says, and shown back.
#[test]
fn describe_writes_reads_and_clears() {
    let dir = tempfile::tempdir().unwrap();
    let root = library_with_a_photo(&dir);

    // Nothing written yet: the listing is empty, not an error.
    let out = run(&["describe", &root, "1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out).trim(), "");

    let out = run(&[
        "describe",
        &root,
        "1",
        "--title",
        "Héron",
        "--copyright",
        "© 2026",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));

    let listing = stdout(&run(&["describe", &root, "1"]));
    assert!(listing.contains("Héron"), "{listing}");
    assert!(listing.contains("© 2026"), "{listing}");

    // A second write touches one field and leaves the other.
    assert!(
        run(&["describe", &root, "1", "--caption", "Au petit matin"])
            .status
            .success()
    );
    let listing = stdout(&run(&["describe", &root, "1"]));
    assert!(listing.contains("Héron"), "the title survives: {listing}");
    assert!(listing.contains("Au petit matin"), "{listing}");

    // The written creator is searchable.
    assert!(
        run(&["describe", &root, "1", "--creator", "Photographe"])
            .status
            .success()
    );
    // Text search is `ls --text` (ADR 0064), and it reads the index the
    // authored creator now feeds (ADR 0099 §2).
    let found = stdout(&run(&["ls", &root, "--text", "Photographe"]));
    assert!(!found.trim().is_empty(), "authored creator must be found");

    assert!(run(&["describe", &root, "1", "--clear"]).status.success());
    assert_eq!(stdout(&run(&["describe", &root, "1"])).trim(), "");
}

/// ADR 0095 §4: `--exact` marks the renamed copy the default scan misses,
/// and names the asset it duplicates.
#[test]
fn an_exact_scan_marks_a_renamed_copy() {
    let dir = tempfile::tempdir().unwrap();
    let root = library_with_a_photo(&dir);

    let backup = dir.path().join("Backup");
    std::fs::create_dir_all(&backup).unwrap();
    std::fs::copy(dir.path().join("photo.png"), backup.join("renamed.png")).unwrap();
    let backup = backup.to_str().unwrap().to_owned();

    // By name and size: nothing seen.
    let listing = stdout(&run(&["scan", &root, &backup]));
    assert!(listing.contains("0 already in the library"), "{listing}");

    // By fingerprint: seen, and named.
    let out = run(&["scan", &root, &backup, "--exact"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let listing = stdout(&out);
    assert!(listing.contains("1 already in the library"), "{listing}");
    assert!(listing.contains("same content as asset"), "{listing}");
}

/// Versions in the CLI (ADR 0094 §3): list, branch, switch — and a branch
/// carries the development its parent had at the moment it was made.
#[test]
fn versions_are_listable_branchable_and_switchable() {
    let dir = tempfile::tempdir().unwrap();
    let root = library_with_a_photo(&dir);

    // One version to start with, and it is the current one.
    let out = run(&["versions", &root, "1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out).lines().count(), 1);
    assert!(stdout(&out).starts_with('*'));

    // Develop, then branch: the branch is created and becomes current.
    assert!(
        run(&["develop", &root, "1", "exposure", "0.5"])
            .status
            .success()
    );
    let out = run(&["version-create", &root, "1", "Cool"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("now current"));

    let out = run(&["versions", &root, "1"]);
    let listing = stdout(&out);
    assert_eq!(listing.lines().count(), 2, "{listing}");
    assert!(listing.contains("Cool"));
    // The starred line is the new one, not the original.
    let starred = listing
        .lines()
        .find(|line| line.starts_with('*'))
        .expect("one version must be current");
    assert!(starred.contains("Cool"), "{listing}");

    // Switching back moves the pointer, and nothing else.
    let out = run(&["version-switch", &root, "1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let listing = stdout(&run(&["versions", &root, "1"]));
    assert!(
        listing
            .lines()
            .find(|line| line.starts_with('*'))
            .is_some_and(|line| !line.contains("Cool")),
        "{listing}"
    );

    // An auto-named branch does not need a name argument.
    let out = run(&["version-create", &root, "1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("Version 3"));
}

/// The range eyedropper's measurement (ADR 0093 §3): two numbers, no write.
#[test]
fn sample_range_prints_luminance_and_hue() {
    let dir = tempfile::tempdir().unwrap();
    let root = library_with_a_photo(&dir);
    let out = run(&["sample-range", &root, "1", "0.5,0.5"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("luminance"));
    assert!(stdout(&out).contains("hue"));
    let out = run(&["sample-range", &root, "1", "bad"]);
    assert!(!out.status.success());
}

/// Local adjustments take a stored `LocalAdjustment` verbatim (ADR 0049 §4).
/// What proves one landed is that the engine can then address it by index:
/// `rm 0` succeeds while it is there and is refused once it is gone.
#[test]
fn local_adjustments_are_appended_removed_and_reset() {
    let dir = tempfile::tempdir().unwrap();
    let root = library_with_a_photo(&dir);
    let radial = r#"{"mask":{"type":"radial","cx":0.5,"cy":0.5,"rx":0.3,"ry":0.2,
        "angle":0,"feather":0.5,"inverted":false},
        "opacity":1,"adjustments":{"exposure":-0.5}}"#;
    // A range term needs `local_adjustments` at v2 (ADR 0048 §5), which a
    // freshly imported photo pins.
    let gradient = r#"{"mask":{"type":"gradient","x0":0.0,"y0":0.0,"x1":0.0,"y1":1.0},
        "range":{"luminance":{"min":0.4,"max":1.0,"softness":0.1}},
        "opacity":0.8,"adjustments":{"shadows":25}}"#;

    for payload in [radial, gradient] {
        let out = run(&["develop", &root, "1", "local-adjustment", payload]);
        assert!(out.status.success(), "{}", stderr(&out));
        assert!(stdout(&out).starts_with("committed revision"));
    }

    // A brush payload from a file, the form a many-dab stroke needs.
    let file = dir.path().join("brush.json");
    std::fs::write(
        &file,
        r#"{"mask":{"type":"brush","strokes":[
            {"x":0.2,"y":0.3,"radius":0.05,"flow":0.5,"hardness":0.5}]},
            "opacity":1,"adjustments":{"clarity":40}}"#,
    )
    .unwrap();
    let out = run(&[
        "develop",
        &root,
        "1",
        "local-adjustment",
        &format!("@{}", file.display()),
    ]);
    assert!(out.status.success(), "{}", stderr(&out));

    let out = run(&["develop", &root, "1", "local-adjustment", "rm", "2"]);
    assert!(out.status.success(), "{}", stderr(&out));
    // Only two are left, so index 2 no longer addresses anything.
    let out = run(&["develop", &root, "1", "local-adjustment", "rm", "2"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("no local adjustment at index 2"));

    let out = run(&["develop", &root, "1", "local-adjustment", "reset"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let out = run(&["develop", &root, "1", "local-adjustment", "rm", "0"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("no local adjustment at index 0"));
}

#[test]
fn local_adjustments_refuse_a_malformed_or_out_of_range_payload() {
    let dir = tempfile::tempdir().unwrap();
    let root = library_with_a_photo(&dir);

    let out = run(&["develop", &root, "1", "local-adjustment", "{\"mask\":1}"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("bad local adjustment payload"));

    // Well-formed JSON, but `opacity` is out of range: `Settings::validate`
    // names it instead of storing it.
    let out = run(&[
        "develop",
        &root,
        "1",
        "local-adjustment",
        r#"{"mask":{"type":"everything"},"opacity":3,"adjustments":{}}"#,
    ]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("opacity"), "{}", stderr(&out));

    // Nothing above committed: only the initial revision.
    let out = run(&["history", &root, "1"]);
    assert_eq!(stdout(&out).lines().count(), 1);
}

/// A creative LUT (ADR 0053) follows the camera-profile shape: import, list,
/// reference by path, toggle, dose, remove.
#[test]
fn lut_import_list_and_reference_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let root = library_with_a_photo(&dir);
    let source = dir.path().join("Warm.cube");
    std::fs::write(
        &source,
        "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n",
    )
    .unwrap();

    let out = run(&["lut", &root, source.to_str().unwrap()]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("Profiles/LUT/Warm.cube"));
    assert!(stdout(&out).contains("blake3:"));

    // The same name twice is refused, never overwritten.
    let out = run(&["lut", &root, source.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("never overwritten"));

    let out = run(&["luts", &root]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("1 LUT(s)"));

    for args in [
        vec!["develop", &root, "1", "lut", "Profiles/LUT/Warm.cube"],
        vec!["develop", &root, "1", "lut-strength", "60"],
        vec!["develop", &root, "1", "lut", "off"],
        vec!["develop", &root, "1", "lut", "on"],
        vec!["develop", &root, "1", "lut", "none"],
    ] {
        let out = run(&args);
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        assert!(stdout(&out).starts_with("committed revision"));
    }

    // A path that was never imported is refused rather than stored with a
    // checksum nothing on disk matches.
    let out = run(&["develop", &root, "1", "lut", "Profiles/LUT/Nope.cube"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("unknown LUT"));

    // And the toggle and the dose need something to act on.
    let out = run(&["develop", &root, "1", "lut", "on"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("no LUT referenced"));
    let out = run(&["develop", &root, "1", "lut-strength", "50"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("no LUT referenced"));
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

    let out = run(&["develop", &root, "1", "highlight-reconstruction", "guess"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("clip/blend/rebuild"));

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

#[test]
fn remove_takes_the_asset_out_of_the_catalog_and_leaves_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = library_with_a_photo(&dir);

    assert!(stdout(&run(&["ls", &root])).contains("1 version(s)"));
    let out = run(&["remove", &root, "1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).contains("no file was touched"));

    assert!(stdout(&run(&["ls", &root])).contains("0 version(s)"));
    assert!(Path::new(&root).join("Photos/photo.png").is_file());
}

#[test]
fn delete_refuses_to_touch_a_file_without_yes() {
    let dir = tempfile::tempdir().unwrap();
    let root = library_with_a_photo(&dir);

    // The one CLI command that takes a photo off the disk, in a place with
    // no confirmation dialog: the guard is the command's whole safety.
    let out = run(&["delete", &root, "1"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("--yes"));
    assert!(stdout(&run(&["ls", &root])).contains("1 version(s)"));
    assert!(Path::new(&root).join("Photos/photo.png").is_file());
}

#[test]
fn demosaic_is_refused_until_the_revision_is_reprocessed() {
    let dir = tempfile::tempdir().unwrap();
    let root = library_with_a_photo(&dir);

    // The capability rule (ADR 0061 §2, ADR 0050 before it): a setting the
    // pinned stage version cannot express is refused *by name*, never
    // silently ignored.
    let out = run(&["develop", &root, "1", "demosaic", "dcb"]);
    if !out.status.success() {
        assert!(
            stderr(&out).contains("input version 3"),
            "the refusal must name the version needed: {}",
            stderr(&out)
        );
    }

    // The neutral value is always expressible, whatever the pinned version.
    let out = run(&["develop", &root, "1", "demosaic", "ahd"]);
    assert!(out.status.success(), "{}", stderr(&out));

    let out = run(&["develop", &root, "1", "demosaic", "amaze"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("ahd/vng/dcb/dht"));
}

#[test]
fn shot_filters_reach_the_grid_and_refuse_nonsense() {
    let dir = tempfile::tempdir().unwrap();
    let root = library_with_a_photo(&dir);

    // A PNG carries no EXIF, so it satisfies no shot criterion — the photo
    // listed without filters disappears behind any of them (ADR 0064 §1).
    assert!(stdout(&run(&["ls", &root])).contains("1 version(s)"));
    for filter in [
        ["--iso", "3200-"],
        ["--aperture", "-2.8"],
        ["--focal", "24-70"],
        ["--shutter", "-1/500"],
        ["--camera", "EOS 60D"],
        ["--lens", "EF 50mm f/1.8 STM"],
    ] {
        let out = run(&["ls", &root, filter[0], filter[1]]);
        assert!(out.status.success(), "{} {}", filter[0], stderr(&out));
        assert!(
            stdout(&out).contains("0 version(s)"),
            "{} {}",
            filter[0],
            stdout(&out)
        );
    }

    // A backwards interval and an unreadable one are told, not answered
    // with an empty list that would read as "the library has none".
    let out = run(&["ls", &root, "--iso", "3200-400"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("above maximum"), "{}", stderr(&out));

    let out = run(&["ls", &root, "--focal", "wide"]);
    assert!(!out.status.success());
    let message = stderr(&out);
    assert!(
        message.contains("--focal") && message.contains("cannot read"),
        "{message}"
    );
}

#[test]
fn facets_list_what_the_library_was_shot_with() {
    let dir = tempfile::tempdir().unwrap();
    let root = library_with_a_photo(&dir);

    // Nothing to offer here, and that is the honest answer: the only photo
    // has no EXIF at all.
    let out = run(&["facets", &root]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).is_empty(), "{}", stdout(&out));
}

#[test]
fn scan_lists_what_an_import_would_take_without_taking_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Lib");
    let root_s = root.to_str().unwrap().to_owned();
    assert!(run(&["new", &root_s]).status.success());

    let source = dir.path().join("Card");
    std::fs::create_dir(&source).unwrap();
    sample_png(&source.join("a.png"));
    std::fs::write(source.join("notes.txt"), b"not a photo").unwrap();
    let source_s = source.to_str().unwrap().to_owned();

    let out = run(&["scan", &root_s, &source_s]);
    assert!(out.status.success(), "{}", stderr(&out));
    let listing = stdout(&out);
    assert!(listing.contains("a.png"), "{listing}");
    assert!(!listing.contains("notes.txt"), "{listing}");
    assert!(listing.contains("1 candidate(s), 0 already"), "{listing}");

    // Nothing was written: the library is still empty.
    assert!(stdout(&run(&["ls", &root_s])).contains("0 version(s)"));

    // Once imported, the same scan says so — that mark is what a second
    // pass over the same card is for.
    assert!(run(&["import", &root_s, &source_s]).status.success());
    let listing = stdout(&run(&["scan", &root_s, &source_s]));
    assert!(listing.contains("1 candidate(s), 1 already"), "{listing}");
}

#[test]
fn import_only_takes_the_named_files_and_says_when_one_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Lib");
    let root_s = root.to_str().unwrap().to_owned();
    assert!(run(&["new", &root_s]).status.success());

    let source = dir.path().join("Card");
    std::fs::create_dir(&source).unwrap();
    // Two distinct images: identical bytes would make the second a
    // duplicate of the first, which is a different test.
    sample_png(&source.join("keep.png"));
    image::save_buffer(
        source.join("leave.png"),
        &[64u8; 8 * 8 * 3],
        8,
        8,
        image::ExtendedColorType::Rgb8,
    )
    .unwrap();
    let source_s = source.to_str().unwrap().to_owned();

    let out = run(&["import", &root_s, &source_s, "--only", "keep.png"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("1 imported, 0 skipped"),
        "{}",
        stdout(&out)
    );
    let listing = stdout(&run(&["ls", &root_s]));
    assert!(
        listing.contains("keep.png") && !listing.contains("leave.png"),
        "{listing}"
    );

    // A name that matches nothing is refused: the user asked for that photo.
    let out = run(&["import", &root_s, &source_s, "--only", "absent.png"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("no such file"), "{}", stderr(&out));
}
