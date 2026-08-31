//! `vignette` v1 (ADR 0090 §2): the vignette a photographer *adds*.
//!
//! **Frozen.** Published, therefore immutable: a revision citing
//! `vignette: 1` renders through this code forever (`docs/pipeline.md`
//! §5.1). Changing the falloff means a `v2`.
//!
//! Not to be confused with the other vignetting in this pipeline. `lens`
//! at rank 20 *removes* the darkening a lens produced, from a Lensfun
//! calibration, in the sensor's frame, before any geometric stage
//! (ADR 0017). This one *draws* one nobody's lens produced, on the frame
//! the photographer composed, after all of them — which is what rank 220,
//! immediately after `crop`, means (ADR 0090 §1).
//!
//! A multiplication in the linear working buffer, never a grey blended in
//! display space: `kernel::v1`'s own documentation sorts operators that way
//! — white balance, exposure and vignetting describe light itself — and it
//! is why a highlight two stops above white survives a −100 vignette still
//! above white, for `output_rendering` to roll off, instead of being
//! crushed onto a flat grey.

use leyline_core::Vignette;
use rayon::prelude::*;

use crate::pixels::Pixels;

/// Darkens (or brightens) the frame's edges.
///
/// The shape is a superellipse `(|u|^n + |v|^n)^(1/n)` on coordinates
/// normalized so the frame spans [-1, 1] each way, divided by its own
/// corner value so the corners sit at 1 whatever `n` is. Without that
/// division, moving `roundness` would move the corners' *brightness* as a
/// side effect of moving the shape.
///
/// The gain reaches ±3 EV at the ends of `amount`, a deliberate cap
/// (ADR 0090 §2): past it the last third of the slider drives a corner from
/// black to blacker.
///
/// Nothing here depends on the preview scale: every coordinate is
/// normalized to the frame, so a display preview and the export draw the
/// same shape (ADR 0041).
pub(crate) fn vignette(px: &mut Pixels, settings: &Vignette) {
    let amount = settings.amount as f32 / 100.0;
    let midpoint = (settings.midpoint as f32 / 100.0).clamp(0.0, 1.0);
    // n = 2 is the plain ellipse; 3^(r/100) reaches 6 at +100 (a squircle)
    // and is clamped at 1 below (a diamond) rather than allowed to go
    // concave.
    let n = (2.0 * 3.0f32.powf(settings.roundness as f32 / 100.0)).clamp(1.0, 6.0);
    let corner = 2.0f32.powf(1.0 / n);
    // A hard edge at feather 0, and never a zero-width smoothstep.
    let width = ((settings.feather as f32 / 100.0) * (1.0 - midpoint)).max(1e-4);

    let (width_px, height_px) = (px.width, px.height);
    // Rows carry a `y`, so the shared `kernel::v1::par_rows` — which hands
    // out anonymous rows — does not fit; the same `par_chunks_mut` the lens
    // stage's own vignetting uses does.
    px.data
        .par_chunks_mut(width_px as usize * 3)
        .enumerate()
        .for_each(|(y, row)| {
            let v = (2.0 * (y as f32 + 0.5) / height_px as f32) - 1.0;
            let vn = v.abs().powf(n);
            for (x, rgb) in row.chunks_exact_mut(3).enumerate() {
                let u = (2.0 * (x as f32 + 0.5) / width_px as f32) - 1.0;
                let d = (u.abs().powf(n) + vn).powf(1.0 / n) / corner;
                let t = smoothstep(midpoint, midpoint + width, d);
                let gain = (3.0 * amount * t).exp2();
                for sample in rgb {
                    *sample = (*sample * gain).max(0.0);
                }
            }
        });
}

/// Hermite ramp: 0 below `edge0`, 1 above `edge1`, smooth in between.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
