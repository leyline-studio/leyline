//! Hostile documents — ADR 0152 §6: the settings parsers never panic on
//! arbitrary JSON.
//!
//! A `settings_json` or a `preset_json` arrives from a catalog someone else
//! wrote, an XMP sidecar, a CLI payload, an MCP client. Any of them may be
//! wrong; none of them may bring the process down. Parsing, validating,
//! fingerprinting and writing back must each answer with a value or an
//! error.
//!
//! Purely random JSON is almost always refused at the first field, which
//! proves little. So object keys are drawn mostly from the field names the
//! settings module itself declares — read from its source, so that a new
//! field joins the vocabulary without anyone remembering to add it — and
//! the generator reaches nested structures that really deserialize.
//!
//! Seeds are fixed so that `make check` stays deterministic.

use leyline_core::{PresetSettings, SETTINGS_KEYS, Settings};
use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
use serde_json::{Map, Number, Value};

/// Documents tried per parser.
const DOCUMENTS: u32 = 2048;

/// The field names `settings.rs` declares (`pub name:`), plus every stage
/// name the settings may cite.
fn vocabulary() -> Vec<String> {
    let mut words: Vec<String> = include_str!("../src/settings.rs")
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("pub "))
        .filter_map(|rest| rest.split_once(':').map(|(name, _)| name))
        .filter(|name| name.chars().all(|c| c.is_ascii_lowercase() || c == '_'))
        .map(str::to_owned)
        .chain(SETTINGS_KEYS.iter().map(|key| (*key).to_owned()))
        .collect();
    words.sort();
    words.dedup();
    words
}

/// A JSON number, biased towards the edges a range check forgets.
fn number() -> impl Strategy<Value = Value> {
    prop_oneof![
        (-3i64..=3).prop_map(Value::from),
        // Just past the ±100 of the sliders, and the ends of the `i32`
        // they are stored in.
        (-300i64..=300).prop_map(Value::from),
        Just(Value::from(i32::MAX)),
        Just(Value::from(i32::MIN)),
        any::<i64>().prop_map(Value::from),
        any::<u64>().prop_map(Value::from),
        (-2.0f64..2.0).prop_map(Value::from),
        any::<f64>().prop_filter_map("JSON has no NaN or infinity", |f| {
            Number::from_f64(f).map(Value::Number)
        }),
        Just(Value::from(f64::MAX)),
        Just(Value::from(-0.0f64)),
    ]
}

/// A JSON scalar; strings are often a word of `vocabulary`, which is what
/// the enumerations are spelled in.
fn leaf(vocabulary: Vec<String>) -> impl Strategy<Value = Value> {
    prop_oneof![
        1 => Just(Value::Null),
        1 => any::<bool>().prop_map(Value::from),
        3 => number(),
        1 => proptest::sample::select(vocabulary).prop_map(Value::from),
        1 => "\\PC{0,8}".prop_map(Value::from),
    ]
}

/// Any JSON value, with object keys biased towards `vocabulary`.
fn json(vocabulary: Vec<String>) -> impl Strategy<Value = Value> {
    let key = prop_oneof![
        4 => proptest::sample::select(vocabulary.clone()),
        1 => "[a-z_]{0,6}".prop_map(String::from),
    ];
    leaf(vocabulary).prop_recursive(5, 64, 8, move |inner| {
        prop_oneof![
            1 => proptest::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
            3 => proptest::collection::btree_map(key.clone(), inner, 0..8)
                .prop_map(|fields| Value::Object(fields.into_iter().collect::<Map<_, _>>())),
        ]
    })
}

/// A well-formed document with a few of its values replaced: the default
/// settings, serialized, then for each replacement a walk of one to four
/// levels down, choosing a field or an element at each, and whatever is
/// found there swapped for a generated value. This is what reaches the
/// range checks, which a random object almost never gets past the types to.
fn mutated(vocabulary: Vec<String>) -> impl Strategy<Value = Value> {
    let base: Value = serde_json::from_str(&Settings::default().to_json())
        .unwrap_or_else(|e| panic!("default settings serialize to JSON: {e}"));
    let replacement = (
        proptest::collection::vec(any::<proptest::sample::Index>(), 1..5),
        prop_oneof![3 => leaf(vocabulary.clone()), 1 => json(vocabulary)],
    );
    proptest::collection::vec(replacement, 1..6).prop_map(move |replacements| {
        let mut document = base.clone();
        for (walk, value) in replacements {
            replace(&mut document, &walk, value);
        }
        document
    })
}

/// Follows `walk` down from `node` as far as it goes, then puts `value`
/// where it stopped.
fn replace(node: &mut Value, walk: &[proptest::sample::Index], value: Value) {
    let Some((step, rest)) = walk.split_first() else {
        *node = value;
        return;
    };
    let child = match node {
        Value::Object(fields) if !fields.is_empty() => {
            let at = step.index(fields.len());
            fields.values_mut().nth(at)
        }
        Value::Array(items) if !items.is_empty() => {
            let at = step.index(items.len());
            items.get_mut(at)
        }
        _ => None,
    };
    match child {
        Some(child) => replace(child, rest, value),
        None => *node = value,
    }
}

/// Runs `property` on `DOCUMENTS` generated documents from a fixed seed.
fn check(property: impl Fn(&str)) {
    let config = Config {
        cases: DOCUMENTS,
        failure_persistence: None,
        ..Config::default()
    };
    let rng = TestRng::from_seed(RngAlgorithm::ChaCha, &[0x4a; 32]);
    let mut runner = TestRunner::new_with_rng(config, rng);
    runner
        .run(
            &prop_oneof![json(vocabulary()), mutated(vocabulary())],
            |document| {
                property(&document.to_string());
                Ok(())
            },
        )
        .unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn settings_never_panic_on_arbitrary_json() {
    check(|document| {
        let Ok(settings) = Settings::parse(document) else {
            return;
        };
        let _ = settings.validate();
        let _ = settings.fingerprint(SETTINGS_KEYS);
        let written = settings.to_json();
        assert!(
            Settings::parse(&written).is_ok(),
            "{document} parsed, but its own serialization {written} did not"
        );
    });
}

#[test]
fn presets_never_panic_on_arbitrary_json() {
    check(|document| {
        let Ok(preset) = PresetSettings::parse(document) else {
            return;
        };
        let written = preset.to_json();
        assert!(
            PresetSettings::parse(&written).is_ok(),
            "{document} parsed, but its own serialization {written} did not"
        );
    });
}
