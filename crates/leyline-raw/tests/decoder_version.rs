//! The decoder is pinned, and an unvalidated decoder is loud (ADR 0086).
//!
//! `docs/pipeline.md` §5.1 names the decoder and its version among the terms
//! that must match for two renders to agree bit for bit. LibRaw is linked
//! dynamically (ADR 0004), so that term is a property of the machine: it can
//! move under the project without a single line of this repository changing,
//! and when it moves, pixels can move with it.
//!
//! This test is what makes that visible. It observes the **trigger** — the
//! decoder is one nobody has accepted — on every machine and with no fixture.
//! It deliberately does not claim to observe the **effect**: whether the
//! pixels of a given photograph actually moved is what `decode_manifest.rs`
//! answers, for whoever has a corpus to answer it with.
//!
//! Accepting a decoder is a deliberate act, spelled like the golden one:
//!
//! ```text
//! LEYLINE_BLESS_DECODER=1 cargo test -p leyline-raw --test decoder_version
//! ```

use std::path::PathBuf;

/// The accepted decoders, one version string per line.
///
/// Not JSON: it holds a list of strings, and a `serde_json` dependency bought
/// for a list of strings is a dependency bought for nothing.
const PIN: &str = "tests/decoder.txt";

fn pin_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(PIN)
}

/// Version strings only: blank lines and `#` comments carry the rationale and
/// are not data.
fn accepted(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_decoder_is_one_this_tree_has_been_validated_against() {
    let running = leyline_raw::decoder_version();
    let path = pin_path();

    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\n\
             run with LEYLINE_BLESS_DECODER=1 to record the decoder in use",
            path.display()
        )
    });
    let mut list = accepted(&text);

    if list.iter().any(|v| v == running) {
        return;
    }

    if std::env::var_os("LEYLINE_BLESS_DECODER").is_some() {
        // Append rather than replace: an entry someone accepted stays
        // accepted, exactly as blessing a golden render never rewrites one.
        list.push(running.to_owned());
        let kept = text.trim_end_matches('\n');
        std::fs::write(&path, format!("{kept}\n{running}\n")).expect("the pin is writable");
        return;
    }

    panic!(
        "\n\nThis RAW decoder is not one this tree has been validated against.\n\
         \n  running:  {running}\n  accepted: {}\n\n\
         Nothing in the repository is wrong, and this is not a flaky test.\n\
         LibRaw is linked dynamically (ADR 0004), so it moves when the machine\n\
         moves — and `docs/pipeline.md` §5.1 counts it among the terms that\n\
         must match for two renders to agree bit for bit (ADR 0086).\n\n\
         Renders made with an accepted decoder and renders made with this one\n\
         are not promised to be identical. What to do:\n\n\
         1. If you have RAW files, check whether pixels actually moved:\n   \
         make test-raw LEYLINE_TEST_RAW=<a raw file>\n   \
         which compares decoded pixels rather than a version string.\n\n\
         2. Accept this decoder deliberately, which appends it to the list:\n   \
         LEYLINE_BLESS_DECODER=1 cargo test -p leyline-raw --test decoder_version\n",
        list.join(", ")
    );
}

#[test]
fn the_decoder_reports_something_usable() {
    let version = leyline_raw::decoder_version();
    assert!(
        !version.is_empty() && version != "unknown",
        "LibRaw reported no version: {version:?}"
    );
    assert!(
        version.starts_with(|c: char| c.is_ascii_digit()),
        "a LibRaw version starts with a digit, got {version:?}"
    );
}

#[test]
fn comments_and_blank_lines_are_not_decoders() {
    let parsed = accepted("# a comment\n\n0.21.2-Release\n  \n# another\n0.21.4-Release\n");
    assert_eq!(parsed, ["0.21.2-Release", "0.21.4-Release"]);
}
