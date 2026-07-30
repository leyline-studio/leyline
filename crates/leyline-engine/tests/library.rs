//! Integration tests: the library facade (`docs/engine-api.md` §5).

use leyline_catalog::GridQuery;
use leyline_core::LeylineError;
use leyline_engine::{Event, ImportOptions, Library, Param, Value};

#[test]
fn create_open_and_work_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("MyLibrary");

    // Create: the §3 skeleton appears.
    let library = Library::create(&root, "My Library").unwrap();
    for sub in ["Photos", "Cache", "Exports", "Backups"] {
        assert!(root.join(sub).is_dir(), "{sub} should exist");
    }
    assert_eq!(library.catalog().library().unwrap().name, "My Library");
    // Creating again over the same catalog is refused.
    assert!(matches!(
        Library::create(&root, "Again"),
        Err(LeylineError::Io(_))
    ));
    drop(library);

    // Reopen and drive a full flow through the one handle.
    let library = Library::open(&root).unwrap();
    std::fs::write(dir.path().join("photo.png"), b"pixels").unwrap();
    let report = library
        .import(
            &dir.path().join("photo.png"),
            &ImportOptions {
                copy_files: true,
                recursive: false,
            },
            |_, _| {},
        )
        .unwrap();
    let registered = report.imported[0].registered;
    assert_eq!(library.catalog().count(&GridQuery::default()).unwrap(), 1);

    // Edit through the facade.
    {
        let mut session = library.edit(registered.version).unwrap();
        session.set(Param::Exposure, Value::Float(0.7)).unwrap();
        session.commit().unwrap();
    }
    assert_ne!(
        library.catalog().version_head(registered.version).unwrap(),
        registered.revision
    );

    // Classement passes through undecorated.
    library
        .catalog_mut()
        .set_rating(&[registered.version], Some(5))
        .unwrap();
}

#[test]
fn open_reports_missing_libraries() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        Library::open(&dir.path().join("nowhere")),
        Err(LeylineError::LibraryNotFound(_))
    ));
}

#[test]
fn read_only_handles_refuse_writes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("RO");
    drop(Library::create(&root, "RO").unwrap());

    let library = Library::open_read_only(&root).unwrap();
    assert!(library.catalog().is_read_only());
    assert!(matches!(
        library.catalog_mut().ensure_folder("Photos/New"),
        Err(LeylineError::Db(_))
    ));
}

#[test]
fn close_notifies_every_subscriber() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Closing"), "Closing").unwrap();

    // Two independent subscribers, and a second clone of the handle: all
    // three still share the one event stream.
    let first = library.subscribe();
    let other_clone = library.clone();
    let second = other_clone.subscribe();

    library.close().unwrap();

    assert_eq!(first.recv().unwrap(), Event::LibraryClosed);
    assert_eq!(second.recv().unwrap(), Event::LibraryClosed);

    // The other clone survives closing this one: `close` only notifies.
    assert!(other_clone.catalog().library().is_ok());
}

#[test]
fn reprocess_migrates_a_batch_and_notifies_subscribers() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Reprocess"), "Reprocess").unwrap();

    let source = dir.path().join("photo.png");
    std::fs::write(&source, b"not a real png, dimensions aren't needed here").unwrap();
    let report = library
        .import(
            &source,
            &ImportOptions {
                copy_files: true,
                recursive: false,
            },
            |_, _| {},
        )
        .unwrap();
    let registered = report.imported[0].registered;

    // Simulate a revision whose stage versions are stale: it activates the
    // gains stage without recording a version for it.
    library
        .catalog_mut()
        .connection()
        .execute(
            "UPDATE develop_revisions SET settings_json = '{\"schema\":1,\"exposure\":0.4}'
             WHERE id = ?1",
            [registered.revision.get()],
        )
        .unwrap();

    let events = library.subscribe();
    let migrated = library.reprocess(&[registered.version], |_, _| {}).unwrap();
    assert_eq!(migrated.reprocessed, vec![registered.version]);
    assert_eq!(migrated.already_current, vec![]);
    assert_eq!(migrated.failed.len(), 0);
    assert_eq!(
        events.recv().unwrap(),
        Event::VersionChanged {
            version_id: registered.version
        }
    );

    // A second pass finds nothing left to migrate — and notifies nothing.
    let already = library.reprocess(&[registered.version], |_, _| {}).unwrap();
    assert_eq!(already.reprocessed, vec![]);
    assert_eq!(already.already_current, vec![registered.version]);
    assert!(events.try_recv().is_err());
}

/// Screen soft proofing (ADR 0034, ADR 0051 §4): a view, and nothing else.
///
/// Proofing through sRGB itself is the one destination whose answer is known in
/// advance — the image comes back as it went in — which is exactly what makes
/// it the right test of the plumbing: any drift here would be the transform
/// misbuilt, not the profile disagreeing.
#[test]
fn a_soft_proof_transforms_the_view_and_leaves_everything_else_alone() {
    use leyline_core::PreviewKind;
    use leyline_engine::SoftProof;

    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), "Proof").unwrap();
    let photo = dir.path().join("photo.png");
    let pixels: Vec<u8> = (0..16 * 16 * 3).map(|i| (i % 251) as u8).collect();
    image::save_buffer(&photo, &pixels, 16, 16, image::ExtendedColorType::Rgb8).unwrap();
    let report = library
        .import(
            &photo,
            &ImportOptions {
                copy_files: true,
                recursive: false,
            },
            |_, _| {},
        )
        .unwrap();
    let asset = report.imported[0].registered.asset;

    // The destination profile is a file the user picks; sRGB's own bytes are
    // the one set of profile bytes the engine can produce itself.
    let profile = dir.path().join("srgb.icc");
    std::fs::write(&profile, leyline_color::srgb_icc_profile()).unwrap();

    let plain = library.preview(asset, PreviewKind::Small).unwrap();
    let plain_bytes = std::fs::read(&plain.path).unwrap();
    let proofed = library
        .preview_soft_proofed(
            asset,
            PreviewKind::Small,
            &SoftProof {
                profile: profile.clone(),
                intent: leyline_color::RenderingIntent::RelativeColorimetric,
                gamut_warning: false,
            },
        )
        .unwrap();

    // Proofing sRGB against sRGB is a round trip: same geometry, and samples
    // that moved by at most a rounding step.
    let reference = leyline_preview::Rgb8::load_png(&plain.path).unwrap();
    assert_eq!(
        (proofed.width(), proofed.height()),
        (reference.width(), reference.height())
    );
    let worst = proofed
        .data()
        .iter()
        .zip(reference.data())
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert!(worst <= 2, "sRGB proofed against sRGB drifted by {worst}");

    // And nothing was written: the cached preview file is untouched, and no
    // revision was created (the import's is still the only one).
    assert_eq!(std::fs::read(&plain.path).unwrap(), plain_bytes);
    let session = library.edit(report.imported[0].registered.version).unwrap();
    assert_eq!(session.history().unwrap().len(), 1);
    drop(session);

    // An unusable profile is an error, not a silently unproofed view.
    let junk = dir.path().join("junk.icc");
    std::fs::write(&junk, b"not a profile").unwrap();
    assert!(
        library
            .preview_soft_proofed(
                asset,
                PreviewKind::Small,
                &SoftProof {
                    profile: junk,
                    intent: leyline_color::RenderingIntent::RelativeColorimetric,
                    gamut_warning: true,
                },
            )
            .is_err()
    );
}
