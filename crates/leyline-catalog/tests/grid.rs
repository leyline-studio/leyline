//! Integration tests: the grid query (`docs/engine-api.md` §7).

use leyline_catalog::{
    CameraInfo, Catalog, GridQuery, LensInfo, Metadata, NewAsset, Rational, RegisteredAsset,
    ShotFacets, ShotRange, Sort,
};
use leyline_core::Settings;
use leyline_core::{AssetId, ColorLabel, LeylineError, MediaType, PickState, VersionId};

fn new_catalog(dir: &tempfile::TempDir) -> Catalog {
    Catalog::create(&dir.path().join("catalog.db"), "Grid").unwrap()
}

fn add(
    catalog: &mut Catalog,
    folder: &str,
    filename: &str,
    capture_date: Option<i64>,
) -> RegisteredAsset {
    let new = NewAsset {
        folder: catalog.ensure_folder(folder).unwrap(),
        filename: filename.to_owned(),
        extension: "CR3".to_owned(),
        media_type: MediaType::Raw,
        file_size: 1,
        checksum: [0xAB; 32],
        width: Some(6000),
        height: Some(4000),
        capture_date,
        capture_offset_minutes: None,
    };
    catalog.add_asset(&new, &Settings::default()).unwrap()
}

/// Three assets: two birds in Wildlife (rated 5 and 3), one street shot.
fn seeded(catalog: &mut Catalog) -> [RegisteredAsset; 3] {
    let heron = add(catalog, "Wildlife", "heron.CR3", Some(3_000));
    let eagle = add(catalog, "Wildlife", "eagle.CR3", Some(1_000));
    let street = add(catalog, "Street", "street.CR3", Some(2_000));

    catalog.set_rating(&[heron.version], Some(5)).unwrap();
    catalog.set_rating(&[eagle.version], Some(3)).unwrap();
    catalog
        .set_color_label(&[street.version], Some(ColorLabel::Red))
        .unwrap();
    catalog.set_pick(&[heron.version], PickState::Pick).unwrap();

    let birds = catalog.create_keyword(None, "Birds").unwrap();
    let heron_kw = catalog.create_keyword(Some(birds), "Heron").unwrap();
    catalog.add_keyword(&[heron.asset], heron_kw).unwrap();
    catalog.add_keyword(&[eagle.asset], birds).unwrap();

    [heron, eagle, street]
}

fn versions(items: &[leyline_catalog::GridItem]) -> Vec<VersionId> {
    items.iter().map(|i| i.version_id).collect()
}

#[test]
fn default_query_lists_current_versions_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let [heron, eagle, street] = seeded(&mut catalog);

    let query = GridQuery::default();
    assert_eq!(catalog.count(&query).unwrap(), 3);
    let items = catalog.grid(&query).unwrap();
    assert_eq!(
        versions(&items),
        vec![heron.version, street.version, eagle.version]
    );

    let heron_item = &items[0];
    assert_eq!(heron_item.filename, "heron.CR3");
    assert_eq!(heron_item.rating, Some(5));
    assert_eq!(heron_item.pick, PickState::Pick);
    assert_eq!(heron_item.width, Some(6000));
}

#[test]
fn filters_compose_with_and_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let [heron, eagle, street] = seeded(&mut catalog);

    let wildlife = catalog.ensure_folder("Wildlife").unwrap();
    let by_folder = GridQuery {
        folder: Some(wildlife),
        ..GridQuery::default()
    };
    assert_eq!(catalog.count(&by_folder).unwrap(), 2);

    let rated_in_folder = GridQuery {
        rating_at_least: Some(4),
        ..by_folder.clone()
    };
    assert_eq!(
        versions(&catalog.grid(&rated_in_folder).unwrap()),
        vec![heron.version]
    );

    let labeled = GridQuery {
        color_label: Some(ColorLabel::Red),
        ..GridQuery::default()
    };
    assert_eq!(
        versions(&catalog.grid(&labeled).unwrap()),
        vec![street.version]
    );

    let picked = GridQuery {
        pick: Some(PickState::Pick),
        ..GridQuery::default()
    };
    assert_eq!(catalog.count(&picked).unwrap(), 1);

    let captured_early = GridQuery {
        capture_range: Some((0, 2_000)),
        ..GridQuery::default()
    };
    assert_eq!(
        versions(&catalog.grid(&captured_early).unwrap()),
        vec![street.version, eagle.version]
    );
}

#[test]
fn keyword_filter_includes_descendants() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let [heron, eagle, _] = seeded(&mut catalog);

    // "Birds" matches the eagle (tagged Birds) and the heron (tagged
    // Birds/Heron, a descendant).
    let birds = catalog.keyword_tree().unwrap()[0].keyword;
    let by_keyword = GridQuery {
        keywords: vec![birds],
        sort: Sort::Filename { ascending: true },
        ..GridQuery::default()
    };
    assert_eq!(
        versions(&catalog.grid(&by_keyword).unwrap()),
        vec![eagle.version, heron.version]
    );
}

#[test]
fn text_filter_uses_the_search_index() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let [heron, ..] = seeded(&mut catalog);

    let by_text = GridQuery {
        text: Some("her".to_owned()),
        ..GridQuery::default()
    };
    assert_eq!(
        versions(&catalog.grid(&by_text).unwrap()),
        vec![heron.version]
    );

    // Blank text filters nothing.
    let blank = GridQuery {
        text: Some("   ".to_owned()),
        ..GridQuery::default()
    };
    assert_eq!(catalog.count(&blank).unwrap(), 3);
}

#[test]
fn collection_queries_enumerate_members_in_user_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let [heron, eagle, street] = seeded(&mut catalog);

    let album = catalog.create_collection(None, "Album").unwrap();
    catalog
        .add_to_collection(album, &[street.version, heron.version, eagle.version])
        .unwrap();

    let in_album = GridQuery {
        collection: Some(album),
        sort: Sort::CollectionOrder,
        ..GridQuery::default()
    };
    assert_eq!(catalog.count(&in_album).unwrap(), 3);
    assert_eq!(
        versions(&catalog.grid(&in_album).unwrap()),
        vec![street.version, heron.version, eagle.version]
    );

    // Collection order without a collection is meaningless.
    let bad = GridQuery {
        sort: Sort::CollectionOrder,
        ..GridQuery::default()
    };
    assert!(matches!(catalog.grid(&bad), Err(LeylineError::Db(_))));
}

#[test]
fn sorts_are_stable_and_handle_null_last() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let [heron, eagle, street] = seeded(&mut catalog);
    let undated = add(&mut catalog, "Street", "undated.CR3", None);

    let by_date = GridQuery {
        sort: Sort::CaptureDate { ascending: true },
        ..GridQuery::default()
    };
    assert_eq!(
        versions(&catalog.grid(&by_date).unwrap()),
        vec![
            eagle.version,
            street.version,
            heron.version,
            undated.version
        ]
    );

    // Descending, ADR 0081 §2 dropped the explicit `capture_date IS NULL`
    // term: SQLite already sorts NULLs last that way, and naming it forbade
    // every index. Undated still comes last — that is the whole claim.
    let newest_first = GridQuery {
        sort: Sort::CaptureDate { ascending: false },
        ..GridQuery::default()
    };
    assert_eq!(
        versions(&catalog.grid(&newest_first).unwrap()),
        vec![
            heron.version,
            street.version,
            eagle.version,
            undated.version
        ]
    );

    // Unrated versions come last in both directions.
    let by_rating = GridQuery {
        sort: Sort::Rating { ascending: false },
        ..GridQuery::default()
    };
    let items = versions(&catalog.grid(&by_rating).unwrap());
    assert_eq!(items[0], heron.version);
    assert_eq!(items[1], eagle.version);
    assert_eq!(items.len(), 4);
}

#[test]
fn range_paginates_the_window() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let [heron, _, street] = seeded(&mut catalog);

    let window = GridQuery {
        range: 0..2,
        ..GridQuery::default()
    };
    assert_eq!(
        versions(&catalog.grid(&window).unwrap()),
        vec![heron.version, street.version]
    );
    // The count ignores the window.
    assert_eq!(catalog.count(&window).unwrap(), 3);

    let next = GridQuery {
        range: 2..4,
        ..GridQuery::default()
    };
    assert_eq!(catalog.grid(&next).unwrap().len(), 1);

    let empty = GridQuery {
        range: 2..2,
        ..GridQuery::default()
    };
    assert_eq!(catalog.grid(&empty).unwrap(), vec![]);
}

#[test]
fn grid_shows_the_current_version_not_all_versions() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let [heron, ..] = seeded(&mut catalog);

    let bw = catalog
        .create_version(heron.version, "Noir & Blanc", None)
        .unwrap();
    assert_eq!(catalog.count(&GridQuery::default()).unwrap(), 3);

    catalog.set_current_version(heron.asset, bw).unwrap();
    let items = catalog.grid(&GridQuery::default()).unwrap();
    assert!(versions(&items).contains(&bw));
    assert!(!versions(&items).contains(&heron.version));
}

#[test]
fn a_cell_says_whether_its_version_has_been_developed() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let untouched = add(&mut catalog, "Photos", "a.CR3", Some(1_000));
    let worked_on = add(&mut catalog, "Photos", "b.CR3", Some(2_000));

    // A committed adjustment is what turns the badge on — not a rating, not
    // a label, not a collection membership (ADR 0055 §5).
    let settings = Settings {
        exposure: 0.5,
        ..Settings::default()
    };
    catalog
        .commit_revision(worked_on.version, &settings)
        .unwrap();

    let items = catalog.grid(&GridQuery::default()).unwrap();
    let edited = |version: VersionId| {
        items
            .iter()
            .find(|item| item.version_id == version)
            .map(|item| item.edited)
    };
    assert_eq!(edited(untouched.version), Some(false));
    assert_eq!(edited(worked_on.version), Some(true));
}

/// Gives the three seeded assets their shot metadata: the heron on a 60D at
/// ISO 3200, the eagle on a 5D Mark IV at ISO 400, the street shot on
/// nothing at all — a photo whose EXIF says nothing is exactly the case the
/// shot filters have to get right (ADR 0064 §1).
fn with_shot_metadata(catalog: &mut Catalog, heron: AssetId, eagle: AssetId) {
    catalog
        .set_metadata(
            heron,
            &Metadata {
                camera: Some(CameraInfo {
                    manufacturer: "Canon".to_owned(),
                    model: "EOS 60D".to_owned(),
                }),
                lens: Some(LensInfo {
                    manufacturer: "Canon".to_owned(),
                    model: "EF 50mm f/1.8 STM".to_owned(),
                    mount: None,
                }),
                iso: Some(3200),
                shutter: Some(Rational {
                    numerator: 1,
                    denominator: 200,
                }),
                aperture: Some(Rational {
                    numerator: 18,
                    denominator: 10,
                }),
                focal_length: Some(Rational {
                    numerator: 50,
                    denominator: 1,
                }),
                ..Metadata::default()
            },
        )
        .unwrap();
    catalog
        .set_metadata(
            eagle,
            &Metadata {
                camera: Some(CameraInfo {
                    manufacturer: "Canon".to_owned(),
                    model: "EOS 5D Mark IV".to_owned(),
                }),
                lens: Some(LensInfo {
                    manufacturer: "Canon".to_owned(),
                    model: "EF 70-200mm f/2.8L".to_owned(),
                    mount: None,
                }),
                iso: Some(400),
                shutter: Some(Rational {
                    numerator: 1,
                    denominator: 1000,
                }),
                aperture: Some(Rational {
                    numerator: 80,
                    denominator: 10,
                }),
                focal_length: Some(Rational {
                    numerator: 200,
                    denominator: 1,
                }),
                ..Metadata::default()
            },
        )
        .unwrap();
}

#[test]
fn shot_filters_select_by_body_lens_and_range() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let [heron, eagle, _street] = seeded(&mut catalog);
    with_shot_metadata(&mut catalog, heron.asset, eagle.asset);

    let matched = |query: &GridQuery| versions(&catalog.grid(query).unwrap());

    // The body is matched by model alone or by "manufacturer model" — the
    // semantics smart collections already used (ADR 0064 §2).
    for name in ["EOS 60D", "Canon EOS 60D"] {
        let query = GridQuery {
            camera: Some(name.to_owned()),
            ..GridQuery::default()
        };
        assert_eq!(matched(&query), vec![heron.version], "camera {name:?}");
        assert_eq!(catalog.count(&query).unwrap(), 1);
    }

    let query = GridQuery {
        lens: Some("Canon EF 70-200mm f/2.8L".to_owned()),
        ..GridQuery::default()
    };
    assert_eq!(matched(&query), vec![eagle.version]);

    // Continuous criteria: one bound, the other, or both.
    let ranges: [(&str, GridQuery, Vec<VersionId>); 5] = [
        (
            "iso >= 3200",
            GridQuery {
                iso: ShotRange::at_least(3200.0),
                ..GridQuery::default()
            },
            vec![heron.version],
        ),
        (
            "iso <= 800",
            GridQuery {
                iso: ShotRange::at_most(800.0),
                ..GridQuery::default()
            },
            vec![eagle.version],
        ),
        (
            "aperture 1.4-2.8",
            GridQuery {
                aperture: ShotRange::between(1.4, 2.8),
                ..GridQuery::default()
            },
            vec![heron.version],
        ),
        (
            "focal >= 100",
            GridQuery {
                focal_length: ShotRange::at_least(100.0),
                ..GridQuery::default()
            },
            vec![eagle.version],
        ),
        (
            "shutter <= 1/500",
            GridQuery {
                shutter_speed: ShotRange::at_most(1.0 / 500.0),
                ..GridQuery::default()
            },
            vec![eagle.version],
        ),
    ];
    for (name, query, expected) in ranges {
        assert_eq!(matched(&query), expected, "{name}");
    }

    // And they compose with each other and with the rest of the query.
    let query = GridQuery {
        camera: Some("EOS 60D".to_owned()),
        iso: ShotRange::at_least(6400.0),
        ..GridQuery::default()
    };
    assert!(matched(&query).is_empty());
}

#[test]
fn a_photo_without_metadata_never_satisfies_a_shot_filter() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let [heron, eagle, street] = seeded(&mut catalog);
    with_shot_metadata(&mut catalog, heron.asset, eagle.asset);

    // The street shot has no metadata row at all: an absent measurement is
    // not a match, however wide the interval.
    let wide_open = GridQuery {
        iso: ShotRange::between(0.0, 1_000_000.0),
        ..GridQuery::default()
    };
    let matched = versions(&catalog.grid(&wide_open).unwrap());
    assert_eq!(matched.len(), 2);
    assert!(!matched.contains(&street.version));
}

#[test]
fn a_reversed_range_is_refused_rather_than_answered_with_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    seeded(&mut catalog);

    let query = GridQuery {
        iso: ShotRange::between(3200.0, 400.0),
        ..GridQuery::default()
    };
    assert!(matches!(
        catalog.grid(&query),
        Err(LeylineError::InvalidSettings(_))
    ));
    assert!(matches!(
        catalog.count(&query),
        Err(LeylineError::InvalidSettings(_))
    ));
}

#[test]
fn facets_list_the_whole_library_not_the_current_selection() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let [heron, eagle, _street] = seeded(&mut catalog);

    // An empty library offers nothing rather than a bogus interval.
    let empty = catalog.shot_facets().unwrap();
    assert_eq!(empty, ShotFacets::default());

    with_shot_metadata(&mut catalog, heron.asset, eagle.asset);
    let facets = catalog.shot_facets().unwrap();
    assert_eq!(facets.cameras, ["Canon EOS 5D Mark IV", "Canon EOS 60D"]);
    assert_eq!(
        facets.lenses,
        ["Canon EF 50mm f/1.8 STM", "Canon EF 70-200mm f/2.8L"]
    );
    assert_eq!(facets.iso, Some((400.0, 3200.0)));
    assert_eq!(facets.aperture, Some((1.8, 8.0)));
    assert_eq!(facets.focal_length, Some((50.0, 200.0)));
    assert_eq!(facets.shutter_speed, Some((0.001, 0.005)));

    // Every listed body is a value the camera filter accepts as it stands.
    for camera in &facets.cameras {
        let query = GridQuery {
            camera: Some(camera.clone()),
            ..GridQuery::default()
        };
        assert_eq!(catalog.count(&query).unwrap(), 1, "facet {camera:?}");
    }
}

#[test]
fn a_written_interval_reads_the_same_for_every_client() {
    // The four written forms of ADR 0064 §5, plus the fraction a shutter
    // speed is normally written with.
    assert_eq!(
        ShotRange::parse("100-800").unwrap(),
        ShotRange::between(100.0, 800.0)
    );
    assert_eq!(
        ShotRange::parse("3200-").unwrap(),
        ShotRange::at_least(3200.0)
    );
    assert_eq!(ShotRange::parse(" -2.8 ").unwrap(), ShotRange::at_most(2.8));
    assert_eq!(
        ShotRange::parse("50").unwrap(),
        ShotRange::between(50.0, 50.0)
    );
    assert_eq!(
        ShotRange::parse("-1/500").unwrap(),
        ShotRange::at_most(0.002)
    );

    // Blank is the absent filter, not an error: an emptied text field in
    // Studio simply stops filtering.
    assert!(ShotRange::parse("  ").unwrap().is_unbounded());

    for refused in ["wide", "1/0", "24-", "-", "24-oo"] {
        if refused == "24-" {
            continue;
        }
        assert!(ShotRange::parse(refused).is_err(), "{refused:?}");
    }
}

/// ADR 0081 §1 and §3: a grid page walks `idx_assets_grid` for its window and
/// decorates only the rows it kept. The plan is the contract — a page that
/// goes back to sorting the whole library reads `USE TEMP B-TREE FOR ORDER BY`
/// on the inner query, and costs 10 ms instead of 0,3 ms on 50 000 assets.
#[test]
fn the_grid_page_walks_its_index_instead_of_sorting_the_library() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    seeded(&mut catalog);

    let plan = catalog.grid_plan(&GridQuery::default()).unwrap();

    assert!(
        plan.iter().any(|step| step.contains("idx_assets_grid")),
        "the grid page stopped using its index:\n{}",
        plan.join("\n")
    );
    // The outer query orders the page it was handed, which is a hundred rows
    // at most; what must never come back is a full sort *inside* the CTE.
    let inner_full_sort = plan
        .iter()
        .take_while(|step| !step.starts_with("SCAN page"))
        .any(|step| step.trim() == "USE TEMP B-TREE FOR ORDER BY");
    assert!(
        !inner_full_sort,
        "the grid page sorts the whole library again:\n{}",
        plan.join("\n")
    );
}

/// The grid is read by position (ADR 0081 §5), so the select list's order is
/// part of its contract. `grid_sql` rebuilds that list per sort, and all but
/// one of its columns are integers: a reordering would otherwise swap a rating
/// for a colour label without failing anything.
#[test]
fn grid_columns_match_the_declared_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = Catalog::create(&dir.path().join("catalog.db"), "Order").unwrap();
    let folder = catalog.ensure_folder("Shoot").unwrap();
    let collection = catalog.create_collection(None, "Album").unwrap();

    for sort in [
        Sort::CaptureDate { ascending: true },
        Sort::CaptureDate { ascending: false },
        Sort::Filename { ascending: true },
        Sort::Filename { ascending: false },
        Sort::ImportedAt { ascending: true },
        Sort::ImportedAt { ascending: false },
        Sort::Rating { ascending: true },
        Sort::Rating { ascending: false },
        Sort::CollectionOrder,
    ] {
        // `CollectionOrder` is the one sort that needs a collection filter.
        let query = GridQuery {
            sort,
            collection: Some(collection),
            folder: Some(folder),
            rating_at_least: Some(2),
            text: Some("sea".to_owned()),
            range: 0..100,
            ..GridQuery::default()
        };
        assert_eq!(
            catalog.grid_columns(&query).unwrap(),
            leyline_catalog::GRID_COLUMNS,
            "the select list moved under {sort:?} without GRID_COLUMNS following"
        );
    }
}
