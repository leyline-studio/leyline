//! What the GPU preview cannot run, and what that costs a drag
//! (ADR 0156 §4, `docs/measured-findings.md` §B6).
//!
//! A drag resumes from the deepest checkpoint still valid, and stays on the
//! GPU only if every active stage after it has a kernel. So a kernel-less
//! stage costs more the later it sits: `local_adjustments` (rank 160) sends
//! every drag before it to the CPU, `rotate` (200) every drag at all.

use std::collections::BTreeMap;

use leyline_core::{Crop, CurvePoint, ToneCurve};

use super::*;
use crate::gpu_preview::has_kernel;

/// The current stage versions after the first checkpoint with no GPU kernel.
///
/// ADR 0156's permanent tax, written down: a new stage version either gets a
/// kernel or joins this list, and joining it is a decision someone makes on
/// purpose, not something a drag discovers on a user's machine.
const KERNEL_LESS_AFTER_FIRST_CHECKPOINT: &[&str] =
    &["local_adjustments", "lut", "rotate", "perspective"];

/// Settings under which every kernel that exists is offered, each stage as
/// it is when active: a crop rectangle (no rectangle, nothing to cut) and a
/// master curve with points and no channel curve, the case the GPU covers.
fn probe_settings() -> Settings {
    let point = |x, y| CurvePoint { x, y };
    Settings {
        crop: Some(Crop {
            x: 0.1,
            y: 0.1,
            width: 0.8,
            height: 0.8,
        }),
        tone_curve: ToneCurve {
            points: vec![point(0.0, 0.0), point(128.0, 140.0), point(255.0, 255.0)],
            ..ToneCurve::default()
        },
        ..neutral_settings()
    }
}

/// For each checkpoint, the active stages after it the GPU has no kernel
/// for. A drag resuming from that checkpoint renders on the CPU whenever
/// the list is not empty.
fn blockers(settings: &Settings) -> Result<Vec<(u16, Vec<&'static str>)>> {
    let plan = plan(settings)?;
    Ok(CHECKPOINT_BEFORE_RANK
        .iter()
        .map(|&checkpoint| {
            let names = plan
                .iter()
                .filter(|(_, version)| version.rank >= checkpoint)
                .filter(|(stage, version)| !has_kernel(stage, version, settings, 1.0))
                .map(|(stage, _)| stage.name)
                .collect();
            (checkpoint, names)
        })
        .collect())
}

#[test]
fn the_stages_a_drag_cannot_cross_on_the_gpu_are_the_listed_ones() {
    let settings = probe_settings();
    let first = CHECKPOINT_BEFORE_RANK[0];
    let missing: Vec<&str> = STAGES
        .iter()
        .filter(|stage| stage.current().rank >= first)
        .filter(|stage| !has_kernel(stage, stage.current(), &settings, 1.0))
        .map(|stage| stage.name)
        .collect();
    assert_eq!(
        missing, KERNEL_LESS_AFTER_FIRST_CHECKPOINT,
        "a current stage version after rank {first} gained or lost its GPU kernel: \
         update KERNEL_LESS_AFTER_FIRST_CHECKPOINT and docs/measured-findings.md §B6"
    );
}

#[test]
fn a_rotation_sends_every_drag_to_the_cpu() {
    let mut settings = neutral_settings();
    settings.exposure = 0.5;
    assert!(
        blockers(&settings)
            .unwrap()
            .iter()
            .all(|(_, names)| names.is_empty())
    );

    settings.rotation = 2.0;
    assert!(
        blockers(&settings)
            .unwrap()
            .iter()
            .all(|(_, names)| names == &["rotate"])
    );
}

/// Measures a real catalogue: of its edited head revisions, how many would
/// drag on the GPU from each checkpoint, and which stage blocks the others.
/// Run on a copy: `LEYLINE_TEST_CATALOG=/copy/of/catalog.db cargo test -p
/// leyline-engine --lib gpu_coverage_of_a_real_catalog -- --ignored --nocapture`.
#[test]
#[ignore = "needs a catalog.db via LEYLINE_TEST_CATALOG"]
fn gpu_coverage_of_a_real_catalog() {
    let path = std::env::var("LEYLINE_TEST_CATALOG").expect("set LEYLINE_TEST_CATALOG");
    let db =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let mut statement = db
        .prepare(
            "SELECT r.settings_json FROM develop_versions v
             JOIN develop_revisions r ON r.id = v.head_revision_id",
        )
        .unwrap();
    let heads: Vec<String> = statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<std::result::Result<_, _>>()
        .unwrap();

    let (mut edited, mut unreadable) = (0usize, 0usize);
    let mut stays_on_gpu = [0usize; CHECKPOINT_BEFORE_RANK.len()];
    let mut blocked_by: BTreeMap<&str, usize> = BTreeMap::new();
    for json in &heads {
        let Ok(settings) = Settings::parse(json) else {
            unreadable += 1;
            continue;
        };
        let Ok(per_checkpoint) = blockers(&settings) else {
            unreadable += 1;
            continue;
        };
        // An import revision runs only the input and output stages: no
        // slider has moved, so there is no drag to measure.
        let plan = plan(&settings).unwrap();
        if plan
            .iter()
            .all(|(stage, _)| matches!(stage.name, "input" | "output_rendering"))
        {
            continue;
        }
        edited += 1;
        for (index, (_, names)) in per_checkpoint.iter().enumerate() {
            if names.is_empty() {
                stays_on_gpu[index] += 1;
            }
        }
        for name in &per_checkpoint[0].1 {
            *blocked_by.entry(name).or_default() += 1;
        }
    }

    println!(
        "{path}: {} head revisions, {edited} edited, {unreadable} unreadable",
        heads.len()
    );
    for (checkpoint, count) in CHECKPOINT_BEFORE_RANK.iter().zip(stays_on_gpu) {
        println!("  drag from checkpoint {checkpoint}: {count}/{edited} on the GPU");
    }
    for (name, count) in &blocked_by {
        println!("  blocked by {name}: {count}/{edited}");
    }
}
