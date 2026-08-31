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
