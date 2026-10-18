//! Hostile pixels — ADR 0152 §6: no stage panics on any image.
//!
//! The golden manifest already names every published `(stage, version)`
//! pair and a settings fragment that makes it run, so the replay here walks
//! the same entries. What changes is the buffer: instead of a smooth
//! synthetic photograph, each plan receives generated pixels — NaN,
//! infinities, negatives, huge values — at sizes down to zero, entering at
//! the stage rather than through `Pixels::from_raw`, which refuses a zero
//! dimension. A crop or a resampling can still shrink a buffer to nothing
//! further down, and the next stage has to survive it.
//!
//! The property is deliberately only "does not panic and keeps the buffer
//! well formed": what a stage *renders* from a NaN is §4's business, and
//! for every input that did not panic it is frozen by the golden.
//!
//! Seeds are fixed so that `make check` stays deterministic: the same
//! buffers every run, on every machine.

use std::collections::BTreeMap;

use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};

use super::golden::{
    Case, camera_source, canon_shot, cases, read_manifest, sample_lut, sample_profile,
};
use super::{Context, Stage, Version, plan};
use crate::pixels::Pixels;
use leyline_core::Settings;

/// Generated buffers per manifest entry. Bounded by the gate, not by
/// coverage: the lens and detail plans cost a few milliseconds each even on
/// five pixels, and this runs on every `make check`.
const CASES_PER_ENTRY: u32 = 16;

/// Edge of the largest generated image. Five is enough for every stage's
/// neighbourhood to be cut by a border on all sides at once.
const MAX_EDGE: u32 = 5;

/// One sample, weighted towards the values a stage is least likely to have
/// been written against.
fn sample() -> impl Strategy<Value = f32> {
    prop_oneof![
        3 => 0.0f32..1.5,
        1 => Just(f32::NAN),
        1 => Just(f32::INFINITY),
        1 => Just(f32::NEG_INFINITY),
        1 => Just(-1.0f32),
        1 => Just(0.0f32),
        1 => Just(-0.0f32),
        1 => Just(1.0e30f32),
        1 => Just(f32::MIN_POSITIVE),
    ]
}

/// A well-formed buffer — `width * height * 3` samples — of any size up to
/// [`MAX_EDGE`], zero included.
fn pixels() -> impl Strategy<Value = Pixels> {
    (0..=MAX_EDGE, 0..=MAX_EDGE).prop_flat_map(|(width, height)| {
        proptest::collection::vec(sample(), (width * height * 3) as usize).prop_map(move |data| {
            Pixels {
                width,
                height,
                data,
            }
        })
    })
}

/// The shapes a generator might be unlucky enough to skip, run explicitly
/// before the random ones.
fn fixed_pixels() -> Vec<Pixels> {
    let filled = |width: u32, height: u32, value: f32| Pixels {
        width,
        height,
        data: vec![value; (width * height * 3) as usize],
    };
    vec![
        filled(0, 0, 0.0),
        filled(0, 4, 0.0),
        filled(4, 0, 0.0),
        filled(1, 1, 0.5),
        filled(1, 1, f32::NAN),
        filled(1, 1, f32::INFINITY),
        filled(3, 3, f32::NAN),
        filled(1, 5, 0.5),
        filled(5, 1, 0.5),
    ]
}

/// Runs one plan on one buffer, checking after every stage that the buffer
/// still holds exactly `width * height * 3` samples.
fn develop(plan: &[(&Stage, &Version)], ctx: &Context<'_>, mut px: Pixels) -> Result<(), String> {
    for (stage, version) in plan {
        version.run(&mut px, ctx);
        let expected = px.width as usize * px.height as usize * 3;
        if px.data.len() != expected {
            return Err(format!(
                "{} v{} left {} samples for {}x{}",
                stage.name,
                version.version,
                px.data.len(),
                px.width,
                px.height
            ));
        }
    }
    Ok(())
}

#[test]
fn no_stage_panics_on_hostile_pixels() {
    let cases: BTreeMap<String, Case> = cases().into_iter().collect();
    let config = Config {
        cases: CASES_PER_ENTRY,
        failure_persistence: None,
        ..Config::default()
    };
    let shot = canon_shot();
    let profile = sample_profile();
    let look = sample_lut();
    let coverages = Default::default();
    let mut failures = Vec::new();
    for (key, entries) in read_manifest() {
        let case = &cases[&key];
        for entry in entries {
            let settings = Settings {
                stages: entry.stages.clone(),
                ..case.settings.clone()
            };
            let plan = plan(&settings).unwrap_or_else(|e| panic!("{key}: plan refused: {e}"));
            let ctx = Context {
                source: camera_source(),
                profile: None,
                settings: &settings,
                shot: Some(&shot),
                sensor: case.sensor.as_ref(),
                camera_profile: Some(&profile),
                lut: Some(&look),
                coverages: &coverages,
                scale: 1.0,
            };
            for px in fixed_pixels() {
                let shape = format!("{}x{}", px.width, px.height);
                match std::panic::catch_unwind(|| develop(&plan, &ctx, px)) {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => failures.push(format!("{key} on {shape}: {e}")),
                    Err(panic) => failures.push(format!(
                        "{key} on {shape}: panicked: {}",
                        panic_message(&*panic)
                    )),
                }
            }
            let rng = TestRng::from_seed(RngAlgorithm::ChaCha, &[0x52; 32]);
            let mut runner = TestRunner::new_with_rng(config.clone(), rng);
            if let Err(e) = runner.run(&pixels(), |px| {
                develop(&plan, &ctx, px).map_err(TestCaseError::fail)
            }) {
                failures.push(format!("{key}: {e}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} hostile buffers broke a plan (ADR 0152 §6):\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

/// The text of a caught panic, for the failure list.
fn panic_message(panic: &(dyn std::any::Any + Send)) -> &str {
    panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("(non-text panic)")
}
