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

/// A creative LUT is a referenced file (ADR 0053 §1), so it has the same
/// fail-closed contract as a camera profile: import copies it in, a revision
/// records its checksum, and a file that changed underneath is an error rather
/// than a render through something else.
#[test]
fn a_lut_is_imported_referenced_and_fails_closed_when_it_changes() {
    use leyline_core::{Lut, PreviewKind};

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Library");
    let library = Library::create(&root, "Looks").unwrap();
    let photo = dir.path().join("photo.png");
    image::save_buffer(
        &photo,
        &[128u8; 8 * 8 * 3],
        8,
        8,
        image::ExtendedColorType::Rgb8,
    )
    .unwrap();
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
    let registered = report.imported[0].registered;

    let source = dir.path().join("Warm.cube");
    std::fs::write(
        &source,
        "LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n",
    )
    .unwrap();
    let imported = library.import_lut(&source).unwrap();
    assert_eq!(imported.relative_path, "Profiles/LUT/Warm.cube");
    assert!(imported.checksum.starts_with("blake3:"));
    assert!(root.join("Profiles/LUT/Warm.cube").is_file());

    // A name already taken is refused, never overwritten.
    assert!(library.import_lut(&source).is_err());
    assert_eq!(library.luts().unwrap().len(), 1);

    {
        let mut session = library.edit(registered.version).unwrap();
        session
            .set(
                Param::Lut,
                Value::Lut(Some(Lut {
                    enabled: true,
                    path: imported.relative_path.clone(),
                    checksum: imported.checksum.clone(),
                    strength: 80,
                })),
            )
            .unwrap();
        session.commit().unwrap();
    }
    // It renders (the identity LUT above leaves the pixels alone, which is
    // what makes this about the plumbing rather than about a look).
    library
        .preview(registered.asset, PreviewKind::Small)
        .unwrap();

    // Now the file changes underneath: the recorded checksum no longer
    // matches, and rendering says so instead of quietly using the new look.
    std::fs::write(
        root.join("Profiles/LUT/Warm.cube"),
        "LUT_3D_SIZE 2\n1 1 1\n1 1 1\n1 1 1\n1 1 1\n1 1 1\n1 1 1\n1 1 1\n1 1 1\n",
    )
    .unwrap();
    let error = library
        .preview(registered.asset, PreviewKind::Medium)
        .unwrap_err();
    assert!(
        matches!(&error, LeylineError::LutFailed { path, .. } if path == "Profiles/LUT/Warm.cube"),
        "{error:?}"
    );
}

/// The cycle ADR 0060 exists for: a photo in the catalog cannot be
/// re-imported because its checksum is known, and becomes importable again
/// once removed. This is the remedy ADR 0043 §5 prescribes for a develop
/// library inherited from before the render-history collapse.
#[test]
fn removing_an_asset_frees_the_file_to_be_imported_again() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Library");
    let library = Library::create(&root, "Removal").unwrap();
    let source = dir.path().join("shot.png");
    image::save_buffer(
        &source,
        &[7u8; 4 * 4 * 3],
        4,
        4,
        image::ExtendedColorType::Rgb8,
    )
    .unwrap();
    let copy = ImportOptions {
        copy_files: true,
        recursive: false,
    };

    let asset = library.import(&source, &copy, |_, _| {}).unwrap().imported[0]
        .registered
        .asset;

    // Before the removal: the same bytes are refused as a duplicate,
    // which is exactly what blocked ADR 0043 §5's remedy.
    let blocked = library.import(&source, &copy, |_, _| {}).unwrap();
    assert!(blocked.imported.is_empty());
    assert_eq!(blocked.skipped.len(), 1);
    assert!(blocked.skipped[0].reason.contains("duplicate"));

    let events = library.subscribe();
    let report = library.remove_assets(&[asset]).unwrap();
    assert_eq!(report.removed, vec![asset]);
    // `remove` never touches a file, whatever else it does.
    assert!(report.trashed.is_empty());
    assert!(report.failed.is_empty());
    assert!(root.join("Photos/shot.png").is_file());
    assert_eq!(library.catalog().count(&GridQuery::default()).unwrap(), 0);
    assert!(matches!(
        events
            .try_iter()
            .find(|e| matches!(e, Event::AssetsRemoved { .. })),
        Some(Event::AssetsRemoved { asset_ids }) if asset_ids == vec![asset]
    ));

    // After the removal the checksum is free, so the copy the first import
    // left under `Photos/` re-imports by reference. Re-importing the
    // *outside* source in copy mode would still be refused, and rightly:
    // its destination file is already there. Repairing an inherited
    // library therefore means re-importing what the library already holds.
    let reference = ImportOptions {
        copy_files: false,
        recursive: false,
    };
    let again = library
        .import(&root.join("Photos/shot.png"), &reference, |_, _| {})
        .unwrap();
    assert_eq!(again.skipped, vec![]);
    assert_eq!(again.imported.len(), 1);
}

/// `delete` differs from `remove` on exactly one axis: the file. The trash
/// is the system's, so this asserts the contract rather than the mechanism
/// — the file either left or was reported as resisting, never silently
/// left behind under a successful-looking report.
#[test]
fn deleting_an_asset_also_takes_its_file_and_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Library");
    let library = Library::create(&root, "Deletion").unwrap();
    let source = dir.path().join("gone.png");
    image::save_buffer(
        &source,
        &[3u8; 4 * 4 * 3],
        4,
        4,
        image::ExtendedColorType::Rgb8,
    )
    .unwrap();
    let asset = library
        .import(
            &source,
            &ImportOptions {
                copy_files: true,
                recursive: false,
            },
            |_, _| {},
        )
        .unwrap()
        .imported[0]
        .registered
        .asset;

    let file = root.join("Photos/gone.png");
    let sidecar = root.join("Photos/gone.xmp");
    std::fs::write(&sidecar, b"<x:xmpmeta/>").unwrap();

    let report = library.delete_assets(&[asset]).unwrap();
    assert_eq!(report.removed, vec![asset]);
    assert_eq!(library.catalog().count(&GridQuery::default()).unwrap(), 0);

    // Whatever the platform's trash does, a file that still exists must be
    // named in `failed` — never dropped from the report.
    for path in [&file, &sidecar] {
        assert!(
            !path.exists() || report.failed.iter().any(|(p, _)| p == path),
            "{} survived without being reported",
            path.display()
        );
    }
    assert_eq!(report.trashed.len() + report.failed.len(), 2);
}

/// The live view of ADR 0074: it shows uncommitted settings, and it writes
/// nothing while doing so.
///
/// Both halves matter. The first is the feature — a slider drag has to show
/// its own value, not the head's. The second is what makes it safe to call at
/// every mouse move: no revision, no preview row, no file. A regression on
/// either half is invisible in the interface until it has filled a disk or
/// lost an edit.
#[test]
fn a_live_preview_shows_uncommitted_settings_and_records_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Lib"), "Live").unwrap();
    let source = dir.path().join("Shoot");
    std::fs::create_dir(&source).unwrap();
    image::save_buffer(
        source.join("flat.png"),
        &[128u8; 16 * 16 * 3],
        16,
        16,
        image::ExtendedColorType::Rgb8,
    )
    .unwrap();
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

    let head = library
        .catalog()
        .current_head_revision(registered.asset)
        .unwrap();
    let previews_before = std::fs::read_dir(dir.path().join("Lib/Cache"))
        .map(|entries| entries.count())
        .unwrap_or(0);

    let neutral = library
        .preview_live(
            registered.asset,
            leyline_core::PreviewKind::Small,
            &leyline_core::Settings::default(),
        )
        .unwrap();
    let brightened = library
        .preview_live(
            registered.asset,
            leyline_core::PreviewKind::Small,
            &leyline_core::Settings {
                exposure: 2.0,
                ..leyline_core::Settings::default()
            },
        )
        .unwrap();

    // The value being tried is what is shown: +2 EV on a mid grey has to come
    // back visibly brighter than neutral.
    assert_eq!(neutral.data().len(), brightened.data().len());
    let (dark, light) = (neutral.data()[0], brightened.data()[0]);
    assert!(
        light > dark + 30,
        "live render ignored the settings: {dark} -> {light}"
    );

    // And nothing moved behind it.
    assert_eq!(
        library
            .catalog()
            .current_head_revision(registered.asset)
            .unwrap(),
        head,
        "a live render committed something"
    );
    assert!(
        library
            .catalog()
            .valid_preview(registered.asset, leyline_core::PreviewKind::Small)
            .unwrap()
            .is_none(),
        "a live render was recorded as the revision's valid preview"
    );
    assert_eq!(
        std::fs::read_dir(dir.path().join("Lib/Cache"))
            .map(|entries| entries.count())
            .unwrap_or(0),
        previews_before,
        "a live render wrote into the preview cache"
    );
}

/// The proxy cache of ADR 0076 changes no pixels — the only way it could go
/// wrong.
///
/// A cache keyed on too little serves the buffer of the wrong picture, or of
/// the wrong size class, and nothing about that is visible in a hit rate or a
/// timing: the render succeeds and shows something else. So the test
/// interleaves two assets across two size classes on one warm `Library` and
/// demands byte equality against the same render on a `Library` whose cache
/// this render is the first thing to touch.
///
/// The reference libraries only ever `open` — they never import. Import
/// renders a thumbnail, which fills the proxy cache at `Thumbnail`'s size
/// before the test asks for anything: a reference that imported first would
/// be warm too, in the same way, and would agree with a broken cache instead
/// of catching it.
#[test]
fn a_warm_proxy_cache_renders_exactly_what_a_cold_one_does() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("Shoot");
    std::fs::create_dir(&source).unwrap();
    // Bigger than `Small`'s 1024 px, so a proxy is really built, and two
    // pictures a swapped buffer could not be mistaken for one another.
    //
    // Deliberately high-frequency: `preview_live` scales its *output* to the
    // size class too, so a proxy built at the wrong size still comes back at
    // the right dimensions. Only detail that a reduction destroys — and
    // destroys differently depending on when it happens — makes the swap
    // visible in the samples. A smooth gradient here would pass whatever the
    // cache served.
    for (name, seed) in [("dark.png", 1u32), ("light.png", 7u32)] {
        let pixels: Vec<u8> = (0..1600u32 * 1200)
            .flat_map(|i| {
                let n = (i.wrapping_mul(2_654_435_761).wrapping_add(seed)) >> 13;
                [n as u8, (n >> 5) as u8, (n >> 11) as u8]
            })
            .collect();
        image::save_buffer(
            source.join(name),
            &pixels,
            1600,
            1200,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
    }

    // Imported once, into one library on disk; every handle below reopens it,
    // so each starts with empty caches over identical content.
    let root = dir.path().join("Library");
    let assets = {
        let library = Library::create(&root, "Proxy").unwrap();
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
        let mut assets: Vec<_> = report
            .imported
            .iter()
            .map(|f| (f.registered.asset, f.relative_path.clone()))
            .collect();
        // Import order is not guaranteed; key the pair by path instead.
        assets.sort_by(|a, b| a.1.cmp(&b.1));
        assets
    };

    let settings = leyline_core::Settings {
        exposure: 0.7,
        sharpening: leyline_core::Sharpening {
            amount: 60,
            radius: 1.5,
        },
        ..leyline_core::Settings::default()
    };
    let kinds = [
        leyline_core::PreviewKind::Small,
        leyline_core::PreviewKind::Thumbnail,
    ];

    // One library, every combination twice and interleaved: by the second
    // pass every proxy is a hit, and each hit had a chance to be the other
    // one's.
    let mut rendered = Vec::new();
    {
        let warm = Library::open(&root).unwrap();
        for _ in 0..2 {
            for kind in kinds {
                for (asset, relative) in &assets {
                    rendered.push((
                        *asset,
                        relative.clone(),
                        kind,
                        warm.preview_live(*asset, kind, &settings).unwrap(),
                    ));
                }
            }
        }
    }

    for (asset, relative, kind, warm_image) in rendered {
        // A handle per render: this render is the only thing its caches ever
        // saw.
        let cold = Library::open(&root).unwrap();
        let cold_image = cold.preview_live(asset, kind, &settings).unwrap();
        assert_eq!(
            (warm_image.width(), warm_image.height()),
            (cold_image.width(), cold_image.height()),
            "{relative} at {kind:?} came back a different size when cached"
        );
        assert!(
            warm_image.data() == cold_image.data(),
            "{relative} at {kind:?} was served another render's proxy"
        );
    }
}

/// What a slider drag actually costs, on a real RAW (ADR 0074 §3):
///
/// ```text
/// LEYLINE_TEST_RAW=/path/to/file.CR2 cargo test --release -p leyline-engine \
///     --test library live_preview -- --ignored --nocapture
/// ```
///
/// The first render pays the decode; the ones after it are what the finger
/// feels, and they are the number the 40 ms budget has to fit inside.
#[test]
#[ignore = "needs a real RAW file via LEYLINE_TEST_RAW"]
fn live_preview_keeps_up_with_a_finger() {
    let raw = std::env::var("LEYLINE_TEST_RAW").expect("set LEYLINE_TEST_RAW");
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Lib"), "Live").unwrap();
    let report = library
        .import(
            std::path::Path::new(&raw),
            &ImportOptions {
                // Copied, not referenced: a referenced file has to already
                // live under the library root, and the corpus does not.
                copy_files: true,
                recursive: false,
            },
            |_, _| {},
        )
        .unwrap();
    let asset = report
        .imported
        .first()
        .unwrap_or_else(|| panic!("import refused it: {:?}", report.skipped))
        .registered
        .asset;

    let render = |settings: &leyline_core::Settings| {
        let started = std::time::Instant::now();
        library
            .preview_live(asset, leyline_core::PreviewKind::Small, settings)
            .unwrap();
        started.elapsed()
    };

    // Both ends of the pipeline, because the stage cache makes them differ by
    // an order of magnitude: exposure sits at rank 40, so a move replays
    // nearly everything; sharpening is the last operator, so a move replays
    // only itself (ADR 0041 §3).
    for (name, drag) in [
        (
            "exposition (tête de pipeline)",
            &(|i| leyline_core::Settings {
                exposure: f64::from(i) * 0.05,
                ..leyline_core::Settings::default()
            }) as &dyn Fn(i32) -> leyline_core::Settings,
        ),
        ("accentuation (fin de pipeline)", &|i| {
            leyline_core::Settings {
                exposure: 0.4,
                sharpening: leyline_core::Sharpening {
                    amount: i,
                    radius: 1.0,
                },
                ..leyline_core::Settings::default()
            }
        }),
    ] {
        let cold = render(&drag(0));
        let frames: Vec<std::time::Duration> = (1..=20).map(|i| render(&drag(i))).collect();
        let total: std::time::Duration = frames.iter().sum();
        println!(
            "{name} — froid {:?}, puis {} images : moyenne {:?}, pire {:?}",
            cold,
            frames.len(),
            total / frames.len() as u32,
            frames.iter().max().unwrap()
        );
    }
}
