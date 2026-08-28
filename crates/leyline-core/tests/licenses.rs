//! Every dependency's licence must flow into GPL-3.0-only (ADR 0084 §7, §8).
//!
//! Leyline is GPL-3.0-only, with the section 7 additional permission of
//! `LICENSE-EXCEPTION.md`. That constrains what may be linked into the open
//! binaries, and the constraint is easy to break by accident: a dependency
//! bump is one line, and a licence change upstream is none at all.
//!
//! [ADR 0073](../../../docs/adr/0073-external-mask-detectors.md) §5 is the
//! evidence that looking matters — the two most visible candidate models it
//! examined were both non-free, and only a deliberate check caught them. This
//! test applies the same criterion to the crate graph, mechanically, so the
//! rule holds without depending on a reviewer noticing.
//!
//! It asks Cargo rather than a checked-in list: a list would go stale in the
//! direction that hides violations.

use std::collections::BTreeSet;
use std::process::Command;

/// Licence identifiers that may appear in the graph.
///
/// Two families, and the reason each is here:
///
/// * **permissive** — flows one way into GPL-3 without conditions we cannot
///   meet;
/// * **copyleft that GPL-3 absorbs** — LGPL, MPL-2.0, and GPL-3 itself
///   (Slint's GPL option is what makes Leyline Studio GPLv3 in the first
///   place, `THIRD-PARTY-NOTICES.md`).
///
/// `Unicode-3.0`, `IJG`, `NCSA` and `BSL-1.0` are permissive licences carried
/// by specific dependencies; they are listed by name rather than by family so
/// that adding one is a deliberate act.
///
/// **AGPL is deliberately absent.** It is GPL-3-compatible in the sense that
/// the two can be combined, but the combined work is AGPL — which would
/// silently change what Leyline is. Anything non-commercial, evaluation-only
/// or source-available is absent for the obvious reason.
const ALLOWED: &[&str] = &[
    "0BSD",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "BSL-1.0",
    "CC0-1.0",
    "GPL-3.0-only",
    "IJG",
    "ISC",
    "LGPL-2.1-only",
    "LGPL-2.1-or-later",
    "LGPL-3.0-only",
    "LGPL-3.0-or-later",
    "MIT",
    "MIT-0",
    "MPL-2.0",
    "NCSA",
    "Unicode-3.0",
    "Unlicense",
    "Zlib",
];

/// Non-SPDX identifiers accepted by name, each with the reason it is here.
///
/// Slint is multi-licensed upstream; Leyline takes its **GPL-3.0-only**
/// option, which is why that identifier also appears in `ALLOWED` and why the
/// two proprietary alternatives alongside it in the same expression are
/// irrelevant rather than disqualifying.
const ALLOWED_REFS: &[&str] = &[
    "LicenseRef-Slint-Royalty-free-2.0",
    "LicenseRef-Slint-Software-3.0",
];

/// Splits an SPDX expression into the identifiers it mentions.
///
/// Deliberately crude: it does not evaluate `OR` against `AND`, it collects
/// every term. That is the conservative direction for `AND` (every term must
/// be acceptable, and every term is checked) and the permissive one for `OR`
/// — `MIT OR Apache-2.0` passes on either. A dual licence offering one
/// unacceptable option is not a problem as long as an acceptable one exists,
/// which is exactly the Slint case.
fn identifiers(expression: &str) -> BTreeSet<String> {
    expression
        .split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == '/')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .filter(|t| !matches!(*t, "OR" | "AND" | "WITH"))
        // `Apache-2.0 WITH LLVM-exception`: the exception only ever widens
        // what the licence permits, so the base identifier decides.
        .filter(|t| !t.ends_with("-exception"))
        .map(str::to_owned)
        .collect()
}

/// Whether an expression offers at least one acceptable option.
fn acceptable(expression: &str) -> bool {
    let terms = identifiers(expression);
    if terms.is_empty() {
        return false;
    }
    // `AND` means every term binds, `OR` means one suffices. Without parsing
    // the operators, accept when *some* term is allowed and no term is a
    // hard refusal — which is what the two lists encode between them.
    terms
        .iter()
        .any(|t| ALLOWED.contains(&t.as_str()) || ALLOWED_REFS.contains(&t.as_str()))
}

#[test]
fn every_dependency_licence_flows_into_gpl3() {
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--all-features",
            "--manifest-path",
            concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
        ])
        .output()
        .expect("cargo metadata runs");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata emits JSON");

    let mut offenders: Vec<String> = Vec::new();
    let mut unlicensed: Vec<String> = Vec::new();
    let mut checked = 0usize;
    for package in metadata["packages"]
        .as_array()
        .expect("metadata lists packages")
    {
        // Workspace members carry the licence this test is enforcing *for*;
        // `source: null` is what distinguishes them from registry crates.
        if package["source"].is_null() {
            continue;
        }
        checked += 1;
        let name = package["name"].as_str().unwrap_or("<unnamed>");
        match package["license"].as_str() {
            Some(expression) if acceptable(expression) => {}
            Some(expression) => offenders.push(format!("{name} — {expression}")),
            // A crate stating only a licence *file* has not been read here.
            // That is a human decision, not a parse: fail, and let whoever
            // adds it record what the file says.
            None => unlicensed.push(name.to_owned()),
        }
    }

    assert!(
        checked > 100,
        "only {checked} external packages seen — the graph was not resolved, \
         so this test proved nothing"
    );
    assert!(
        offenders.is_empty(),
        "dependencies whose licence does not flow into GPL-3.0-only \
         (ADR 0084 §7):\n  {}",
        offenders.join("\n  ")
    );
    assert!(
        unlicensed.is_empty(),
        "dependencies declaring no SPDX licence — read their licence file and \
         record the verdict in ADR 0084 §7:\n  {}",
        unlicensed.join("\n  ")
    );
}

#[test]
fn the_allow_list_refuses_what_it_is_meant_to_refuse() {
    // The test above passes trivially if `acceptable` says yes to everything.
    // These are the expressions that must not get through.
    for refused in [
        "AGPL-3.0-only",
        "AGPL-3.0-or-later",
        "LicenseRef-NVIDIA-Source-Code-License-NC",
        "CC-BY-NC-4.0",
        "BUSL-1.1",
        "Commercial",
        "",
    ] {
        assert!(
            !acceptable(refused),
            "{refused:?} must not be accepted (ADR 0084 §7)"
        );
    }
    // And the ones that must, including the dual expressions the real graph
    // is full of and the Slint case that mixes a GPL option with two
    // proprietary ones.
    for admitted in [
        "MIT",
        "MIT OR Apache-2.0",
        "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT",
        "(MIT OR Apache-2.0) AND Unicode-3.0",
        "GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0",
        "LGPL-2.1-only",
    ] {
        assert!(
            acceptable(admitted),
            "{admitted:?} must be accepted (ADR 0084 §7)"
        );
    }
}
