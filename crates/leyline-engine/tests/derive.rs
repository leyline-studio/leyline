//! Integration tests: derivation — an external processor's answer filed as a
//! new asset (ADR 0107).
//!
//! One property carries the whole design and is worth more than the rest put
//! together: **the derived file replaces the decode, not the development**.
//! An identity processor must therefore give back a photograph that renders
//! like its parent, with the parent's every setting still live on it. Both
//! halves are checked below, and the second is what a naive implementation
//! gets wrong — arriving at neutral settings and making the user redo every
//! slider.

#![cfg(unix)]

use std::path::{Path, PathBuf};

use leyline_core::{AssetId, SourceEncoding, VersionId};
use leyline_derive::{Operation, ProcessorSource};
use leyline_engine::{ImportOptions, Library, Param, Value};
use leyline_export::{ExportFormat, ExportSettings};

/// A library holding one photograph: a small sRGB PNG, since what is being
/// tested is the derivation and not the decoder.
fn library_with_a_photo(name: &str) -> (tempfile::TempDir, Library, AssetId, VersionId) {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("IMG_0001.png");
    // A gradient with a darker textured half: enough tonal range that a
    // difference in the pipeline would show somewhere.
    let image = image::RgbImage::from_fn(64, 48, |x, y| {
        let t = (x * 4).min(255) as u8;
        if y < 24 {
            image::Rgb([t, t / 2 + 40, 255 - t])
        } else {
            let n = ((x * 7 + y * 13) % 32) as u8;
            image::Rgb([40 + n, 60 + n, 90 + n])
        }
    });
    image.save(&source).unwrap();

    let library = Library::create(&dir.path().join("Library"), name).unwrap();
    let report = library
        .import(
            &source,
            &ImportOptions {
                copy_files: true,
                recursive: false,
                pair_companions: false,
                thumbnails: false,
            },
            |_, _| {},
        )
        .unwrap();
    let registered = report.imported[0].registered;
    (dir, library, registered.asset, registered.version)
}

/// A processor standing in for a real one, running `body`.
fn processor(dir: &Path, name: &str, body: &str) -> ProcessorSource {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(format!("{name}.sh"));
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    ProcessorSource {
        id: name.to_owned(),
        label: name.to_owned(),
        command: path,
        args: Vec::new(),
        operations: vec![Operation {
            id: "denoise".to_owned(),
            label: "Denoise".to_owned(),
        }],
    }
}

/// Copies `--image` to `--out`: the identity processor.
const COPY: &str = r#"image=""; out=""
while [ $# -gt 0 ]; do
  case "$1" in
    --image) image="$2"; shift 2;;
    --out) out="$2"; shift 2;;
    *) shift;;
  esac
done
cp "$image" "$out""#;

/// Exports a version to a PNG and reads the samples back.
fn exported(library: &Library, version: VersionId, out: &Path) -> (u32, u32, Vec<u8>) {
    std::fs::create_dir_all(out).unwrap();
    let report = library
        .export(
            &leyline_engine::ExportRequest {
                versions: vec![version],
                recipe: leyline_engine::ExportRecipe::Adhoc(ExportSettings {
                    format: ExportFormat::Png,
                    ..ExportSettings::default()
                }),
                destination_dir: out.to_path_buf(),
                concurrency: None,
            },
            |_, _| {},
        )
        .unwrap();
    let path: PathBuf = report.exported[0].path.clone();
    let image = image::open(&path).unwrap().to_rgb8();
    (image.width(), image.height(), image.into_raw())
}

/// **The invariant of ADR 0107 §4.** An identity processor gives back a
/// photograph that renders like its parent: the exchange happens at the one
/// rank where the buffer is the working space itself, so nothing between
/// there and the output has changed.
///
/// Not bit for bit, and the tolerance is the design rather than slack: the
/// exchange is sixteen bits (§3), so a linear sample makes a round trip
/// through a 1/65535 grid before the output transfer function stretches the
/// deep shadows. One 8-bit level is the whole budget that leaves.
#[test]
fn an_identity_processor_gives_back_the_same_photograph() {
    let (dir, library, _asset, version) = library_with_a_photo("DeriveIdentity");
    let source = processor(dir.path(), "copy", COPY);

    let (width, height, before) = exported(&library, version, &dir.path().join("before"));

    let derived = library.derive(version, &source, "denoise").unwrap();
    let derived_version = library.catalog().current_version(derived).unwrap();
    let (derived_width, derived_height, after) =
        exported(&library, derived_version, &dir.path().join("after"));

    assert_eq!((derived_width, derived_height), (width, height));
    let worst = before
        .iter()
        .zip(&after)
        .map(|(a, b)| i32::from(*a) - i32::from(*b))
        .map(i32::abs)
        .max()
        .unwrap();
    assert!(
        worst <= 1,
        "a derived photograph differs from its parent by {worst} levels; the exchange happens \
         before every operator, so nothing but the 16-bit quantization may move"
    );
    let exact = before.iter().zip(&after).filter(|(a, b)| a == b).count();
    assert!(
        exact * 100 / before.len() >= 95,
        "only {}% of samples came back identical",
        exact * 100 / before.len()
    );
}

/// The half a naive implementation gets wrong: the derived asset arrives
/// with its parent's development, not with neutral settings.
#[test]
fn a_derived_asset_inherits_its_parents_development() {
    let (dir, library, _asset, version) = library_with_a_photo("DeriveInherits");
    let source = processor(dir.path(), "copy", COPY);

    // Something unmistakable, and something that lives *after* the exchange
    // rank so it must survive as a setting rather than as pixels.
    {
        let mut session = library.edit(version).unwrap();
        session
            .set(Param::Exposure, Value::Float(1.5))
            .and_then(|()| session.set(Param::Contrast, Value::Int(40)))
            .and_then(|()| session.set(Param::Saturation, Value::Int(-30)))
            // The highlight shoulder is the trap: the exchange file has no
            // headroom above white, so "this is like an imported JPEG, its
            // white *is* white" reads as an argument for resetting it. It
            // is not one — the shoulder is the parent's own setting, it
            // applies to the parent's own data, and dropping it changes
            // every highlight in the picture. ADR 0107 §5 says three
            // changes, and this is how a fourth gets caught.
            .and_then(|()| session.set(Param::HighlightRolloff, Value::Int(35)))
            .unwrap();
        session.commit().unwrap();
    }

    let derived = library.derive(version, &source, "denoise").unwrap();
    let derived_version = library.catalog().current_version(derived).unwrap();
    let inherited = library.edit(derived_version).unwrap().settings().clone();

    assert_eq!(inherited.exposure, 1.5);
    assert_eq!(inherited.contrast, 40);
    assert_eq!(inherited.saturation, -30);
    // And the three changes ADR 0107 §5 makes, each for its own reason.
    assert_eq!(inherited.source_encoding, SourceEncoding::LinearWorkspace);
    assert_eq!(inherited.stages.get("input"), Some(&5));
    assert_eq!(inherited.camera_profile, None);
    assert_eq!(inherited.noise_reduction.luminance, 0);
    assert_eq!(inherited.output_rendering.highlight_rolloff, 35);
}

/// A derived asset is a photograph of its own — named after its operation,
/// sitting beside the original, and linked to it (ADR 0107 §5).
#[test]
fn a_derived_asset_is_filed_beside_its_parent_and_linked_to_it() {
    let (dir, library, asset, version) = library_with_a_photo("DeriveFiling");
    let source = processor(dir.path(), "copy", COPY);

    let derived = library.derive(version, &source, "denoise").unwrap();

    let catalog = library.catalog();
    assert_eq!(catalog.derived_from(derived).unwrap(), Some(asset));
    assert_eq!(catalog.derivatives_of(asset).unwrap(), vec![derived]);
    let details = catalog.asset_details(derived).unwrap();
    assert_eq!(details.filename, "IMG_0001-denoise.tif");
    assert_eq!(details.width, Some(64));
    assert_eq!(details.height, Some(48));
    drop(catalog);
    assert!(
        library
            .locate(derived)
            .unwrap()
            .parent()
            .unwrap()
            .join("IMG_0001.png")
            .is_file(),
        "the derived file sits beside the original"
    );
}

/// Deriving twice with the same operation would need the same name, and a
/// derivation never overwrites (ADR 0107 §5, the rule ADR 0100 set).
#[test]
fn a_second_derivation_under_the_same_name_is_refused() {
    let (dir, library, _asset, version) = library_with_a_photo("DeriveNoOverwrite");
    let source = processor(dir.path(), "copy", COPY);

    library.derive(version, &source, "denoise").unwrap();
    let error = library.derive(version, &source, "denoise").unwrap_err();
    assert!(error.to_string().contains("already exists"), "got {error}");
}

/// A processor that fails leaves nothing behind: no file, no asset, no row.
#[test]
fn a_failing_processor_leaves_the_library_untouched() {
    let (dir, library, asset, version) = library_with_a_photo("DeriveFailure");
    let source = processor(dir.path(), "angry", "echo model missing >&2; exit 3");

    let before = library.catalog().count(&Default::default()).unwrap();
    let error = library.derive(version, &source, "denoise").unwrap_err();

    assert!(error.to_string().contains("model missing"), "got {error}");
    assert_eq!(
        library.catalog().count(&Default::default()).unwrap(),
        before
    );
    assert!(library.catalog().derivatives_of(asset).unwrap().is_empty());
    assert!(
        !library
            .locate(asset)
            .unwrap()
            .parent()
            .unwrap()
            .join("IMG_0001-denoise.tif")
            .exists()
    );
}
