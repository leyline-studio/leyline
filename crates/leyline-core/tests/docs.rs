//! What the documentation counts must match what the repository holds.
//!
//! `docs/readme.md` tells a newcomer how many structural decisions live in
//! `docs/adr/`. That number went stale silently — it said 79 while the
//! directory held 90 — because nothing read both sides. Same reasoning as
//! `licenses.rs`: a hand-maintained figure goes stale in the direction
//! nobody notices, so the figure is checked mechanically instead of by a
//! reviewer's memory.

use std::fs;
use std::path::Path;

/// The ADR count quoted in `docs/readme.md` equals the number of ADR files.
#[test]
fn readme_adr_count_matches_directory() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

    let adrs = fs::read_dir(root.join("docs/adr"))
        .expect("docs/adr must exist")
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            // ADRs are `NNNN-slug.md`; the index `README.md` is not one.
            name.ends_with(".md") && name[..4].chars().all(|c| c.is_ascii_digit())
        })
        .count();

    let readme =
        fs::read_to_string(root.join("docs/readme.md")).expect("docs/readme.md must exist");
    let quoted: usize = readme
        .split(" structural decisions")
        .next()
        .and_then(|before| before.rsplit(' ').next())
        .and_then(|n| n.parse().ok())
        .expect("docs/readme.md must quote a number before 'structural decisions'");

    assert_eq!(
        quoted, adrs,
        "docs/readme.md says {quoted} structural decisions, docs/adr/ holds {adrs}: \
         update the figure in docs/readme.md"
    );
}

/// The workspace's declared `rust-version` is the pinned toolchain's
/// major.minor (ADR 0168). It once said 1.85 while nothing built below
/// 1.97.1 and the dependency graph needed 1.88.
#[test]
fn declared_msrv_is_the_pinned_toolchain() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");

    let quoted_value = |text: &str, key: &str| -> String {
        text.lines()
            .find_map(|line| line.trim().strip_prefix(key))
            .and_then(|rest| rest.trim().strip_prefix('='))
            .map(|value| value.trim().trim_matches('"').to_owned())
            .unwrap_or_else(|| panic!("no `{key} = \"…\"` line"))
    };

    let manifest = fs::read_to_string(root.join("Cargo.toml")).expect("Cargo.toml must exist");
    let toolchain = fs::read_to_string(root.join("rust-toolchain.toml"))
        .expect("rust-toolchain.toml must exist");

    let declared = quoted_value(&manifest, "rust-version");
    let channel = quoted_value(&toolchain, "channel");
    let pinned_major_minor: String = channel.splitn(3, '.').take(2).collect::<Vec<_>>().join(".");

    assert_eq!(
        declared, pinned_major_minor,
        "Cargo.toml declares rust-version {declared}, rust-toolchain.toml pins {channel}: \
         they move together (ADR 0168)"
    );
}
