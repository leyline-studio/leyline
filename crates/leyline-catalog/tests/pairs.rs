//! Integration tests: RAW+JPEG pairing (ADR 0079).
//!
//! The three terms of the criterion are tested by taking one away at a
//! time — a pair that forms on the conjunction and refuses on each of the
//! partial matches is the only evidence that the conjunction is doing the
//! work, and not one of its terms alone.

use leyline_catalog::{CameraInfo, Catalog, GridQuery, Metadata, NewAsset, Pairing};
use leyline_core::{AssetId, MediaType, Settings};

fn new_catalog(dir: &tempfile::TempDir) -> Catalog {
    Catalog::create(&dir.path().join("catalog.db"), "Pairs").unwrap()
}

/// Registers one file, with its body written into `metadata` the way an
/// import does — the criterion reads that row, not the asset.
fn add(
    catalog: &mut Catalog,
    folder: &str,
    filename: &str,
    media_type: MediaType,
    capture_date: Option<i64>,
    camera: Option<&str>,
) -> AssetId {
    add_with_offset(
        catalog,
        folder,
        filename,
        media_type,
        capture_date,
        camera,
        None,
    )
}

/// Same, stating the file's UTC offset — what a JPEG carrying
/// `OffsetTimeOriginal` gets at import and a RAW never does
/// (`docs/catalog.md` §9).
#[allow(clippy::too_many_arguments)]
fn add_with_offset(
    catalog: &mut Catalog,
    folder: &str,
    filename: &str,
    media_type: MediaType,
    capture_date: Option<i64>,
    camera: Option<&str>,
    capture_offset_minutes: Option<i32>,
) -> AssetId {
    let (stem, extension) = filename.rsplit_once('.').unwrap();
    let new = NewAsset {
        folder: catalog.ensure_folder(folder).unwrap(),
        filename: filename.to_owned(),
        extension: extension.to_ascii_lowercase(),
        media_type,
        file_size: 1,
        // Distinct per file: a duplicate checksum is a different question.
        checksum: std::array::from_fn(|i| (stem.len() + i + extension.len()) as u8),
        width: Some(6000),
        height: Some(4000),
        capture_date,
        capture_offset_minutes,
    };
    let registered = catalog.add_asset(&new, &Settings::default()).unwrap();
    if let Some(camera) = camera {
        catalog
            .set_metadata(
                registered.asset,
                &Metadata {
                    camera: Some(CameraInfo {
                        manufacturer: "Canon".to_owned(),
                        model: camera.to_owned(),
                    }),
                    ..Metadata::default()
                },
            )
            .unwrap();
    }
    registered.asset
}

/// How many photos the grid shows — the number that made ADR 0079 exist.
fn shown(catalog: &Catalog) -> u64 {
    catalog.count(&GridQuery::default()).unwrap()
}

#[test]
fn a_raw_adopts_the_jpeg_shot_with_it_across_folders() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    // The layout of the corpus that opened ADR 0079: JPEG at the root of
    // the shoot, RAW in a `raw/` subfolder, so the two sit in different
    // catalog folders.
    let jpeg = add(
        &mut catalog,
        "Photos",
        "5D4_2326.JPG",
        MediaType::Jpeg,
        Some(1_786_731_851_000),
        Some("EOS 5D Mark IV"),
    );
    let raw = add(
        &mut catalog,
        "Photos/raw",
        "5D4_2326.CR2",
        MediaType::Raw,
        Some(1_786_731_851_000),
        Some("EOS 5D Mark IV"),
    );

    // The RAW arrives second, as a sorted enumeration hands it over: the
    // direction that ADR 0079 §4 says is the common one.
    assert_eq!(
        catalog.pair_asset(raw).unwrap(),
        Pairing::Master(vec![jpeg])
    );
    assert_eq!(catalog.master_of(jpeg).unwrap(), Some(raw));
    assert_eq!(catalog.companions_of(raw).unwrap(), vec![jpeg]);
    assert_eq!(shown(&catalog), 1);
}

#[test]
fn a_jpeg_imported_after_its_raw_joins_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    let raw = add(
        &mut catalog,
        "Photos",
        "IMG_0001.CR2",
        MediaType::Raw,
        Some(1_000),
        Some("EOS 60D"),
    );
    let jpeg = add(
        &mut catalog,
        "Photos",
        "IMG_0001.JPG",
        MediaType::Jpeg,
        Some(1_000),
        Some("EOS 60D"),
    );

    assert_eq!(catalog.pair_asset(jpeg).unwrap(), Pairing::Companion(raw));
    assert_eq!(shown(&catalog), 1);
}

#[test]
fn each_term_of_the_criterion_is_load_bearing() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    let raw = add(
        &mut catalog,
        "Photos",
        "IMG_0001.CR2",
        MediaType::Raw,
        Some(1_000),
        Some("EOS 60D"),
    );
    // Same instant and body, different name — the second frame of a burst
    // at 1/500 s, which the corpus contains.
    add(
        &mut catalog,
        "Photos",
        "IMG_0002.JPG",
        MediaType::Jpeg,
        Some(1_000),
        Some("EOS 60D"),
    );
    // Same name and body, different instant — a second shoot years later.
    add(
        &mut catalog,
        "Other",
        "IMG_0001.JPG",
        MediaType::Jpeg,
        Some(9_000),
        Some("EOS 60D"),
    );
    // Same name and instant, different body — two cameras, both counting
    // from one.
    add(
        &mut catalog,
        "Third",
        "IMG_0001.JPG",
        MediaType::Jpeg,
        Some(1_000),
        Some("EOS 5D Mark IV"),
    );

    assert_eq!(catalog.pair_asset(raw).unwrap(), Pairing::None);
    assert_eq!(shown(&catalog), 4);
}

#[test]
fn a_file_without_a_capture_instant_never_pairs() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    let raw = add(
        &mut catalog,
        "Photos",
        "IMG_0001.CR2",
        MediaType::Raw,
        None,
        Some("EOS 60D"),
    );
    add(
        &mut catalog,
        "Photos",
        "IMG_0001.JPG",
        MediaType::Jpeg,
        None,
        Some("EOS 60D"),
    );

    assert_eq!(catalog.pair_asset(raw).unwrap(), Pairing::None);
    assert_eq!(shown(&catalog), 2);
}

#[test]
fn two_raws_of_one_shot_do_not_pair_with_each_other() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    let cr2 = add(
        &mut catalog,
        "Photos",
        "IMG_0001.CR2",
        MediaType::Raw,
        Some(1_000),
        Some("EOS 60D"),
    );
    add(
        &mut catalog,
        "Photos",
        "IMG_0001.DNG",
        MediaType::Dng,
        Some(1_000),
        Some("EOS 60D"),
    );

    assert_eq!(catalog.pair_asset(cr2).unwrap(), Pairing::None);
    assert_eq!(shown(&catalog), 2);
}

#[test]
fn a_third_file_of_the_same_shot_joins_the_same_master() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    let jpeg = add(
        &mut catalog,
        "Photos",
        "IMG_0001.JPG",
        MediaType::Jpeg,
        Some(1_000),
        Some("EOS 60D"),
    );
    let heif = add(
        &mut catalog,
        "Photos",
        "IMG_0001.HEIF",
        MediaType::Heif,
        Some(1_000),
        Some("EOS 60D"),
    );
    let raw = add(
        &mut catalog,
        "Photos",
        "IMG_0001.CR2",
        MediaType::Raw,
        Some(1_000),
        Some("EOS 60D"),
    );

    assert_eq!(
        catalog.pair_asset(raw).unwrap(),
        Pairing::Master(vec![jpeg, heif])
    );
    assert_eq!(shown(&catalog), 1);
}

#[test]
fn a_companion_never_becomes_a_master_in_turn() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    let jpeg = add(
        &mut catalog,
        "Photos",
        "IMG_0001.JPG",
        MediaType::Jpeg,
        Some(1_000),
        Some("EOS 60D"),
    );
    let raw = add(
        &mut catalog,
        "Photos",
        "IMG_0001.CR2",
        MediaType::Raw,
        Some(1_000),
        Some("EOS 60D"),
    );
    catalog.pair_asset(raw).unwrap();

    // A second RAW of the same shot arrives: the JPEG is taken, and the
    // chain that would form is refused rather than silently rewired.
    let second = add(
        &mut catalog,
        "Elsewhere",
        "IMG_0001.CR2",
        MediaType::Raw,
        Some(1_000),
        Some("EOS 60D"),
    );
    assert_eq!(catalog.pair_asset(second).unwrap(), Pairing::None);
    assert_eq!(catalog.master_of(jpeg).unwrap(), Some(raw));
}

#[test]
fn the_retroactive_pass_counts_what_it_will_do_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    for n in 0..3 {
        add(
            &mut catalog,
            "Photos",
            &format!("IMG_000{n}.JPG"),
            MediaType::Jpeg,
            Some(1_000 + n),
            Some("EOS 60D"),
        );
        add(
            &mut catalog,
            "Photos/raw",
            &format!("IMG_000{n}.CR2"),
            MediaType::Raw,
            Some(1_000 + n),
            Some("EOS 60D"),
        );
    }
    // Nothing pairs on its own: the column exists, the pass has not run.
    assert_eq!(shown(&catalog), 6);
    assert_eq!(catalog.pairable_count().unwrap(), 3);

    assert_eq!(catalog.pair_all().unwrap().len(), 3);
    assert_eq!(shown(&catalog), 3);

    assert_eq!(catalog.pairable_count().unwrap(), 0);
    assert_eq!(catalog.pair_all().unwrap(), vec![]);
    assert_eq!(shown(&catalog), 3);
}

#[test]
fn unpairing_gives_the_photo_back_with_what_it_had() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    let jpeg = add(
        &mut catalog,
        "Photos",
        "IMG_0001.JPG",
        MediaType::Jpeg,
        Some(1_000),
        Some("EOS 60D"),
    );
    let raw = add(
        &mut catalog,
        "Photos",
        "IMG_0001.CR2",
        MediaType::Raw,
        Some(1_000),
        Some("EOS 60D"),
    );
    let rating = catalog.current_version(jpeg).unwrap();
    catalog.set_rating(&[rating], Some(4)).unwrap();
    catalog.pair_asset(raw).unwrap();
    assert_eq!(shown(&catalog), 1);

    // Detaching the master detaches its companions, and reports only the
    // rows that actually came back — not its own already-null one.
    assert_eq!(catalog.unpair_assets(&[raw]).unwrap(), 1);
    assert_eq!(shown(&catalog), 2);
    assert_eq!(catalog.master_of(jpeg).unwrap(), None);
    assert_eq!(
        catalog.asset_details(jpeg).unwrap().versions[0].rating,
        Some(4)
    );

    // And detaching what is already detached is not an error, it is zero.
    assert_eq!(catalog.unpair_assets(&[raw]).unwrap(), 0);
}

#[test]
fn a_removal_takes_the_companion_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    let jpeg = add(
        &mut catalog,
        "Photos",
        "IMG_0001.JPG",
        MediaType::Jpeg,
        Some(1_000),
        Some("EOS 60D"),
    );
    let raw = add(
        &mut catalog,
        "Photos",
        "IMG_0001.CR2",
        MediaType::Raw,
        Some(1_000),
        Some("EOS 60D"),
    );
    catalog.pair_asset(raw).unwrap();

    // What the engine expands before deleting — companion first, master
    // last. The order is the whole point: the other way round, the master's
    // cascade removes the companion's row before its turn, `delete_assets`
    // finds nothing under that id, and the JPEG stays on disk while the
    // report claims a clean removal.
    assert_eq!(catalog.with_companions(&[raw]).unwrap(), vec![jpeg, raw]);

    let deleted = catalog
        .delete_assets(&catalog.with_companions(&[raw]).unwrap())
        .unwrap();
    assert_eq!(deleted.assets, vec![jpeg, raw]);
    assert_eq!(deleted.file_paths.len(), 2);
    assert_eq!(shown(&catalog), 0);
}

/// ADR 0082 §4 and §5, in one query: a warming pass must start with the
/// photos the grid shows first, and must not spend a second on a companion —
/// which no grid ever draws (ADR 0079 §5). Both facts come from the same
/// clause, so they cannot drift apart.
#[test]
fn grid_order_sorts_like_the_grid_and_drops_companions() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    let raw = add(
        &mut catalog,
        "Photos",
        "IMG_1.CR3",
        MediaType::Raw,
        Some(2_000),
        Some("Canon EOS 5D Mark IV"),
    );
    let jpeg = add(
        &mut catalog,
        "Photos",
        "IMG_1.JPG",
        MediaType::Jpeg,
        Some(2_000),
        Some("Canon EOS 5D Mark IV"),
    );
    let older = add(
        &mut catalog,
        "Photos",
        "IMG_0.CR3",
        MediaType::Raw,
        Some(1_000),
        Some("Canon EOS 5D Mark IV"),
    );
    let undated = add(
        &mut catalog,
        "Photos",
        "IMG_9.CR3",
        MediaType::Raw,
        None,
        Some("Canon EOS 5D Mark IV"),
    );
    assert_eq!(
        catalog.pair_asset(raw).unwrap(),
        Pairing::Master(vec![jpeg])
    );

    // Newest capture first, undated last, and the companion gone — the
    // default grid order, applied to the subset a pass was handed.
    assert_eq!(
        catalog.grid_order(&[undated, jpeg, older, raw]).unwrap(),
        vec![raw, older, undated]
    );

    // An id that names nothing simply does not come back.
    assert_eq!(
        catalog.grid_order(&[AssetId::new(9_999)]).unwrap(),
        Vec::new()
    );
    assert_eq!(catalog.grid_order(&[]).unwrap(), Vec::new());
}

/// The defect that made ADR 0079 find nothing on a real shoot.
///
/// `capture_date` holds two different readings depending on whether the file
/// stated its offset (`docs/catalog.md` §9): a true UTC instant when it did,
/// the camera's bare wall clock stored as if UTC when it did not. A RAW never
/// states one, and most bodies write `OffsetTimeOriginal` into the JPEG — so
/// the two files of one shot land exactly one offset apart, and a criterion
/// comparing the column directly can never be satisfied. Measured on a 5D
/// Mark IV shoot: 16 RAW+JPEG pairs, 0 found.
#[test]
fn a_jpeg_that_states_its_offset_still_pairs_with_its_raw() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    // 2025-04-14 11:00:58 on the body's clock, in Paris summer time. The RAW
    // stores the reading itself; the JPEG stores the instant it denotes,
    // which is two hours earlier.
    let wall_clock = 1_744_628_458_000;
    let offset_minutes = 120;

    let jpeg = add_with_offset(
        &mut catalog,
        "2025_04_14",
        "5D4_8204.JPG",
        MediaType::Jpeg,
        Some(wall_clock - i64::from(offset_minutes) * 60_000),
        Some("Canon EOS 5D Mark IV"),
        Some(offset_minutes),
    );
    let raw = add(
        &mut catalog,
        "2025_04_14/raw",
        "5D4_8204.CR2",
        MediaType::Raw,
        Some(wall_clock),
        Some("Canon EOS 5D Mark IV"),
    );

    assert_eq!(
        catalog.pair_asset(raw).unwrap(),
        Pairing::Master(vec![jpeg]),
        "the RAW must adopt the JPEG of the same shot even though only the \
         JPEG could state its offset"
    );
    assert_eq!(shown(&catalog), 1, "one shot is one photo in the grid");
}

/// The offset is normalised away, not ignored: two files an hour apart on the
/// same clock are two different shots, whatever their offsets say.
#[test]
fn two_different_instants_do_not_pair_because_of_their_offsets() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    let wall_clock = 1_744_628_458_000;
    let jpeg = add_with_offset(
        &mut catalog,
        "shoot",
        "5D4_8204.JPG",
        MediaType::Jpeg,
        // One hour later on the wall clock, once its offset is added back.
        Some(wall_clock + 3_600_000 - 120 * 60_000),
        Some("Canon EOS 5D Mark IV"),
        Some(120),
    );
    let raw = add(
        &mut catalog,
        "shoot/raw",
        "5D4_8204.CR2",
        MediaType::Raw,
        Some(wall_clock),
        Some("Canon EOS 5D Mark IV"),
    );

    assert_eq!(catalog.pair_asset(raw).unwrap(), Pairing::None);
    assert_eq!(catalog.pair_asset(jpeg).unwrap(), Pairing::None);
    assert_eq!(shown(&catalog), 2);
}
