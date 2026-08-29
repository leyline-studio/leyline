//! Reference decodes over real RAW files (ADR 0086 §4).
//!
//! `decoder_version.rs` observes that the decoder *changed*. This observes
//! whether that change actually moved any pixels — the question a version
//! string cannot answer.
//!
//! It ships with **no entries**, and that is deliberate. A RAW fixture in the
//! repository would carry a licence question and a size cost, and would pin
//! one camera's code path as though it were the decoder. Instead the manifest
//! is keyed by the **checksum of the input file**, so any maintainer builds
//! their own reference set out of their own photographs, and it stays valid
//! across machines and checkouts:
//!
//! ```text
//! make test-raw LEYLINE_TEST_RAW=/path/to/IMG_1234.CR2
//! ```
//!
//! The first run over a given file records it. Later runs verify it, and a
//! mismatch means the decoder now renders that photograph differently.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

const MANIFEST: &str = "tests/decodes.json";

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(MANIFEST)
}

fn digest(bytes: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(bytes).to_hex())
}

/// One recorded decode. Dimensions ride along so a failure can say *how* the
/// decode differs before anyone reaches for a hex diff.
///
/// `decoder` is recorded but **not compared** — see [`pixels`]. It says which
/// LibRaw produced the entry, which is context for a failure and provenance
/// for the entry; it is not part of what the entry asserts.
fn entry(image: &leyline_raw::RawImage, decoder: &str) -> Value {
    let mut map = Map::new();
    map.insert("decoder".into(), Value::String(decoder.to_owned()));
    map.insert("width".into(), Value::from(image.width));
    map.insert("height".into(), Value::from(image.height));
    map.insert("bits".into(), Value::from(image.bits));
    map.insert("digest".into(), Value::String(digest(&image.data)));
    Value::Object(map)
}

/// The part of an entry that is the claim: the pixels and their shape.
///
/// Comparing whole entries was wrong, and wrong in the way that mattered most.
/// This file exists to answer *did the pixels move* — the question a version
/// string cannot answer — and an entry carrying the decoder's name can never
/// match across two decoders, even when the two produce byte-identical
/// output. Measured on 2026-08-30: LibRaw 0.21.2 and 0.21.4 decode the same
/// CR2 to the same BLAKE3, and the comparison still failed, reporting a
/// difference in the one field that was never the point.
fn pixels(entry: &Value) -> Vec<Option<&Value>> {
    ["digest", "width", "height", "bits"]
        .iter()
        .map(|key| entry.get(*key))
        .collect()
}

fn read_manifest(path: &Path) -> BTreeMap<String, Value> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).expect("the manifest is valid JSON"),
        Err(_) => BTreeMap::new(),
    }
}

/// Decodes a real RAW and pins the result against its own checksum.
///
/// Run with `make test-raw LEYLINE_TEST_RAW=/path/to/file.ext`.
#[test]
#[ignore = "needs a real RAW file via LEYLINE_TEST_RAW"]
fn a_real_raw_decodes_to_the_pixels_it_decoded_to_before() {
    let raw = std::env::var("LEYLINE_TEST_RAW").expect("set LEYLINE_TEST_RAW");
    let path = Path::new(&raw);
    let source = std::fs::read(path).expect("the RAW file is readable");
    let key = digest(&source);

    let decoded =
        leyline_raw::decode(path, &leyline_raw::DecodeParams::default()).expect("the file decodes");
    let decoder = leyline_raw::decoder_version();
    let observed = entry(&decoded.image, decoder);

    let manifest_path = manifest_path();
    let mut manifest = read_manifest(&manifest_path);

    match manifest.get(&key) {
        None => {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            eprintln!(
                "recording a reference decode for {name}\n  \
                 key:     {key}\n  \
                 decoder: {decoder}\n  \
                 digest:  {}\n\
                 It is written to {}; commit it if this corpus is yours to pin.",
                observed["digest"].as_str().unwrap_or_default(),
                manifest_path.display()
            );
            manifest.insert(key, observed);
            let json = serde_json::to_string_pretty(&manifest).unwrap();
            std::fs::write(&manifest_path, format!("{json}\n")).expect("the manifest is writable");
        }
        Some(pinned) => {
            assert_eq!(
                pixels(pinned),
                pixels(&observed),
                "\n\nThis RAW file decodes to different pixels than when it was pinned.\n\
                 \n  file:            {}\n  key:             {key}\n  \
                 pinned decoder:  {}\n  running decoder: {decoder}\n\n\
                 The input is byte-identical — the key is its checksum — so the\n\
                 difference is in the decoder, not in the photograph. This is\n\
                 exactly the change `docs/pipeline.md` §5.1 counts as breaking\n\
                 bit-for-bit reproducibility (ADR 0086) — and unlike a change of\n\
                 version string this is the *effect* rather than the trigger:\n\
                 renders made before and after are not the same image.\n\n\
                 Decide whether that is acceptable, and if it is, delete this\n\
                 entry and let the next run re-record it.\n",
                path.display(),
                pinned["decoder"].as_str().unwrap_or("?"),
            );
        }
    }
}
