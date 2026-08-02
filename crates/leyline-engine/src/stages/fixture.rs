//! A stage with two versions, compiled only into test builds (ADR 0043 §7).
//!
//! Collapsing the pre-release history left every real operator at `v1`, so
//! nothing would exercise the mechanism that carries the project's central
//! guarantee: *a revision citing an older version keeps rendering through
//! that older version, even once a newer one exists*. Waiting for the first
//! real pixel fix to find out whether that works is not an option — it is
//! the one thing that must already work when it happens.
//!
//! So the registry permanently carries this: one stage, two versions, one
//! visibly different rendering each, and a different rank each so the
//! "position belongs to the version" rule of ADR 0042 §3 is exercised too.
//! It activates on a marker in [`Settings::extra`], which no real revision
//! carries, and it never reaches a release binary.

use leyline_core::Settings;

use super::{Context, Stage, Version};
use crate::pixels::Pixels;

/// Marker key in `Settings::extra` that activates the fixture stage.
pub(crate) const MARKER: &str = "__fixture_lift";

/// Name the fixture stage is recorded under in a `stages` map.
pub(crate) const NAME: &str = "__fixture_lift";

/// The lift each version applies, in working-buffer units.
pub(crate) const V1_LIFT: f32 = 0.10;
/// See [`V1_LIFT`] — deliberately far enough apart to be unmistakable in an
/// 8-bit output.
pub(crate) const V2_LIFT: f32 = 0.25;

/// The registry entry, appended to [`super::STAGES`] under `cfg(test)`.
pub(crate) static STAGES: &[Stage] = &[Stage {
    name: NAME,
    active: |settings: &Settings| settings.extra.contains_key(MARKER),
    // Driven by the `extra` passthrough rather than a named setting, so
    // that is what a checkpoint before it must depend on.
    reads: &["extra"],
    versions: &[
        Version {
            version: 1,
            rank: 65,

            space: super::Space::LinearRec2020,
            apply: |px: &mut Pixels, _: &Context<'_>| lift(px, V1_LIFT),
        },
        Version {
            version: 2,
            // Not 65: a new version is free to declare another position,
            // and revisions citing v1 keep v1's.
            rank: 66,

            space: super::Space::LinearRec2020,
            apply: |px: &mut Pixels, _: &Context<'_>| lift(px, V2_LIFT),
        },
    ],
}];

/// Raises every sample by `amount`, clamped — the simplest rendering whose
/// version is unmistakable from the output alone.
fn lift(px: &mut Pixels, amount: f32) {
    for sample in &mut px.data {
        *sample = (*sample + amount).clamp(0.0, 1.0);
    }
}
