//! Hostile rules — ADR 0152 §6: the catalog's rule parsers never panic on
//! arbitrary input.
//!
//! A smart collection's `rules_json` comes from a catalog file, a CLI
//! payload or an MCP client; a shot range is typed by hand. Both are parsed,
//! and smart rules are then *evaluated*: they become SQL, where a camera
//! name holding `%` or `'` or a keyword path made of slashes is exactly the
//! input nobody writes a test for. Each stage must answer with a value or an
//! error.
//!
//! Seeds are fixed so that `make check` stays deterministic.

use std::cell::Cell;

use leyline_catalog::{CameraInfo, Catalog, GridQuery, Metadata, NewAsset, ShotRange, SmartRules};
use leyline_core::{MediaType, Settings};
use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
use serde_json::{Map, Value};

/// Rule documents parsed per run. Every one that parses also creates a
/// collection and runs a query, which is what bounds the count.
const DOCUMENTS: u32 = 2048;

/// Shot ranges parsed per run: pure string work, so many.
const RANGES: u32 = 4096;

fn runner(cases: u32) -> TestRunner {
    let config = Config {
        cases,
        failure_persistence: None,
        ..Config::default()
    };
    TestRunner::new_with_rng(
        config,
        TestRng::from_seed(RngAlgorithm::ChaCha, &[0x37; 32]),
    )
}

/// A string a rule might carry: often one SQL or path syntax would read,
/// sometimes anything at all.
fn text() -> impl Strategy<Value = String> {
    prop_oneof![
        2 => proptest::sample::select(vec![
            "", "%", "_", "'", "\"", "\\", "/", "//", "Nature/Birds", "Nature/", "/Birds",
            "Canon EOS R5", "Canon", "' OR 1=1 --", "\0",
        ])
        .prop_map(str::to_owned),
        1 => "\\PC{0,12}",
    ]
}

/// Any JSON scalar, numbers biased towards the edges of the `u8` rating.
fn scalar() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::from),
        (-2i64..=300).prop_map(Value::from),
        any::<i64>().prop_map(Value::from),
        (-10.0f64..10.0).prop_map(Value::from),
        text().prop_map(Value::from),
    ]
}

/// A rules document: an object whose keys are mostly the criteria
/// `SmartRules` declares, each holding a value of roughly the right shape —
/// or of the wrong one.
fn document() -> impl Strategy<Value = Value> {
    let key = prop_oneof![
        4 => proptest::sample::select(vec!["rating", "camera", "keywords", "pick"])
            .prop_map(str::to_owned),
        1 => "[a-z_]{0,6}",
    ];
    let value = prop_oneof![
        2 => scalar(),
        1 => proptest::collection::btree_map(
            prop_oneof![Just("gte".to_owned()), "[a-z]{0,3}"],
            scalar(),
            0..3,
        )
        .prop_map(|fields| Value::Object(fields.into_iter().collect::<Map<_, _>>())),
        1 => proptest::collection::vec(prop_oneof![3 => text().prop_map(Value::from), 1 => scalar()], 0..4)
            .prop_map(Value::Array),
    ];
    prop_oneof![
        8 => proptest::collection::btree_map(key, value, 0..5)
            .prop_map(|fields| Value::Object(fields.into_iter().collect::<Map<_, _>>())),
        1 => scalar(),
    ]
}

/// A catalog with something for every criterion to match against.
fn populated_catalog(dir: &tempfile::TempDir) -> Catalog {
    let mut catalog = Catalog::create(&dir.path().join("catalog.db"), "Hostile")
        .unwrap_or_else(|e| panic!("create catalog: {e}"));
    let folder = catalog
        .ensure_folder("Photos")
        .unwrap_or_else(|e| panic!("create folder: {e}"));
    let new = NewAsset {
        folder,
        filename: "a.CR3".to_owned(),
        extension: "CR3".to_owned(),
        media_type: MediaType::Raw,
        file_size: 1,
        checksum: [0xAB; 32],
        width: None,
        height: None,
        capture_date: None,
        capture_offset_minutes: None,
    };
    let asset = catalog
        .add_asset(&new, &Settings::default())
        .unwrap_or_else(|e| panic!("add asset: {e}"));
    let camera = Metadata {
        camera: Some(CameraInfo {
            manufacturer: "Canon".to_owned(),
            model: "EOS R5".to_owned(),
        }),
        ..Metadata::default()
    };
    catalog
        .set_metadata(asset.asset, &camera)
        .unwrap_or_else(|e| panic!("set metadata: {e}"));
    let nature = catalog
        .create_keyword(None, "Nature")
        .unwrap_or_else(|e| panic!("create keyword: {e}"));
    let birds = catalog
        .create_keyword(Some(nature), "Birds")
        .unwrap_or_else(|e| panic!("create keyword: {e}"));
    catalog
        .add_keyword(&[asset.asset], birds)
        .unwrap_or_else(|e| panic!("tag asset: {e}"));
    catalog
}

#[test]
fn smart_rules_never_panic_on_arbitrary_json() {
    let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("temp dir: {e}"));
    let catalog = std::cell::RefCell::new(populated_catalog(&dir));
    let created = Cell::new(0u32);
    runner(DOCUMENTS)
        .run(&document(), |document| {
            let Ok(rules) = SmartRules::parse(&document.to_string()) else {
                return Ok(());
            };
            assert_eq!(
                SmartRules::parse(&rules.to_json()).ok(),
                Some(rules.clone()),
                "{document} parsed, but did not survive its own serialization"
            );
            let mut catalog = catalog.borrow_mut();
            created.set(created.get() + 1);
            let name = format!("smart {}", created.get());
            let Ok(collection) = catalog.create_smart_collection(None, &name, &rules) else {
                return Ok(());
            };
            let query = GridQuery {
                collection: Some(collection),
                ..GridQuery::default()
            };
            let _ = catalog.count(&query);
            let _ = catalog.grid(&query);
            Ok(())
        })
        .unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn shot_ranges_never_panic_on_arbitrary_text() {
    let range_text = prop_oneof![
        2 => "[0-9./ -]{0,10}",
        1 => "\\PC{0,10}",
        1 => proptest::sample::select(vec![
            "-", "--", "1/0", "0/0", "1/", "/2", "1e999", "-1e999", "NaN", "inf", "5-1",
            "18446744073709551616", "1/200-1/0",
        ])
        .prop_map(str::to_owned),
    ];
    runner(RANGES)
        .run(&range_text, |text| {
            let _ = ShotRange::parse(&text);
            Ok(())
        })
        .unwrap_or_else(|e| panic!("{e}"));
}
