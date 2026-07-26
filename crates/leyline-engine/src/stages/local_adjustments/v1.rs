//! Local adjustments v1 (ADR 0029) — rank 160.
//!
//! Introduced by process 8. Each adjustment rasterizes its mask, develops a
//! full copy of the buffer through the global operators, and blends that
//! copy back through the mask's coverage.
//!
//! The operators it reaches for are named explicitly, at the versions it was
//! defined against — `gains::v2`, `contrast::v1`, … — and that binding is
//! part of what is frozen here: a later `contrast::v2` would leave this
//! module calling `contrast::v1`, as it must.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use leyline_core::{LocalAdjustment, WhiteBalance};

use crate::mask;
use crate::pixels::Pixels;
use crate::stages::contrast::v1::contrast;
use crate::stages::gains::v2::linear_gains;
use crate::stages::highlights_shadows::v1::highlights_shadows;
use crate::stages::kernel::v1::saturate;
use crate::stages::whites_blacks::v1::whites_blacks;

/// Applies every local adjustment, in list order: later entries composite
/// on top of the buffer earlier ones already wrote, exactly like spot
/// removal's list-order rule above.
pub(crate) fn local_adjustments(
    px: &mut Pixels,
    adjustments: &[LocalAdjustment],
    rotation_degrees: f64,
) {
    for adjustment in adjustments {
        apply_local_adjustment(px, adjustment, rotation_degrees);
    }
}

/// Re-adjusts a full copy of `px` with this module's own operator functions
/// — re-parameterized by `adjustment.adjustments`, falling back to `px`'s
/// *global* setting for whichever of the restricted fields the entry
/// doesn't set (so a mask that only sets `contrast` still sees the photo's
/// existing global exposure/white-balance as its starting point, not the
/// as-shot neutral) — then blends that copy back into `px` by the mask's
/// rasterized coverage times `opacity` (`mask::blend_by_coverage`).
///
/// Takes the *global* values it falls back to from `px`'s own state at
/// call time in the pipeline: since this stage runs once, after every
/// global tonal/color operator above has already applied, `px` already
/// reflects the global settings — this function only needs `adjustment`'s
/// overrides, not a second copy of the global `Settings`.
pub(crate) fn apply_local_adjustment(
    px: &mut Pixels,
    adjustment: &LocalAdjustment,
    rotation_degrees: f64,
) {
    let coverage =
        mask::rasterize_coverage(&adjustment.mask, px.width, px.height, rotation_degrees);
    let values = &adjustment.adjustments;
    let mut adjusted = px.clone();
    if values.temperature.is_some() || values.tint.is_some() || values.exposure.is_some() {
        let wb = if values.temperature.is_some() || values.tint.is_some() {
            Some(WhiteBalance {
                temperature: values.temperature.unwrap_or(6500),
                tint: values.tint.unwrap_or(0),
            })
        } else {
            None
        };
        linear_gains(&mut adjusted, wb.as_ref(), values.exposure.unwrap_or(0.0));
    }
    if let Some(v) = values.contrast {
        contrast(&mut adjusted, v);
    }
    if values.highlights.is_some() || values.shadows.is_some() {
        highlights_shadows(
            &mut adjusted,
            values.highlights.unwrap_or(0),
            values.shadows.unwrap_or(0),
        );
    }
    if values.whites.is_some() || values.blacks.is_some() {
        whites_blacks(
            &mut adjusted,
            values.whites.unwrap_or(0),
            values.blacks.unwrap_or(0),
        );
    }
    if let Some(v) = values.vibrance {
        saturate(&mut adjusted, v, true);
    }
    if let Some(v) = values.saturation {
        saturate(&mut adjusted, v, false);
    }
    mask::blend_by_coverage(px, &adjusted, &coverage, adjustment.opacity);
}
