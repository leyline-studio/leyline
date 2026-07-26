//! Spot removal v1 (ADR 0031) — rank 30, before every tonal stage.
//!
//! Introduced by process 7. Clones a source disk onto a target disk, both
//! expressed in post-rotation normalized coordinates, so a spot placed on
//! the rotated image lands where the user put it.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use leyline_core::SpotRemoval;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{bilinear, post_rotation_point_to_buffer};

/// Applies every clone patch, in list order: later entries see the pixels
/// earlier entries already wrote, since each reads from a fresh snapshot of
/// `px` taken at the start of its own turn.
pub(crate) fn spot_removal(px: &mut Pixels, spots: &[SpotRemoval], rotation_degrees: f64) {
    for spot in spots {
        apply_spot(px, spot, rotation_degrees);
    }
}

/// Clones the disk around `spot.source` onto the disk around `spot.target`.
/// Both points are stored normalized in the post-rotation, pre-crop
/// referential of ADR 0026; `rotation_degrees` maps them back to this
/// still-unrotated buffer via [`post_rotation_point_to_buffer`]. The radius
/// is normalized against the buffer's larger dimension so the disk stays
/// circular in physical pixels regardless of aspect ratio (an implementation
/// constant, not fixed by the ADR).
pub(crate) fn apply_spot(px: &mut Pixels, spot: &SpotRemoval, rotation_degrees: f64) {
    let (width, height) = (px.width, px.height);
    let (tx, ty) = post_rotation_point_to_buffer(width, height, rotation_degrees, spot.target);
    let (sx, sy) = post_rotation_point_to_buffer(width, height, rotation_degrees, spot.source);
    let radius_px = spot.radius * f64::from(width.max(height));
    if radius_px <= 0.0 {
        return;
    }
    let (dx, dy) = (sx - tx, sy - ty);
    let feather = spot.feather.clamp(0.0, 1.0);
    let opacity = spot.opacity.clamp(0.0, 1.0) as f32;

    let x0 = (tx - radius_px).floor().max(0.0) as usize;
    let y0 = (ty - radius_px).floor().max(0.0) as usize;
    let x1 = ((tx + radius_px).ceil() as i64).clamp(0, i64::from(width) - 1) as usize;
    let y1 = ((ty + radius_px).ceil() as i64).clamp(0, i64::from(height) - 1) as usize;
    if x0 >= width as usize || y0 >= height as usize || x1 < x0 || y1 < y0 {
        return;
    }

    // A snapshot taken once per spot: every pixel this turn writes reads
    // from the state before this spot, so the pass is well-defined even when
    // source and target disks overlap.
    let before = px.clone();
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (cx, cy) = (x as f64 + 0.5, y as f64 + 0.5);
            let dist = ((cx - tx).powi(2) + (cy - ty).powi(2)).sqrt();
            if dist > radius_px {
                continue;
            }
            let coverage = radial_coverage(dist / radius_px, feather) * opacity;
            if coverage <= 0.0 {
                continue;
            }
            let Some(sample) = bilinear(&before, cx + dx, cy + dy) else {
                continue;
            };
            let index = (y * width as usize + x) * 3;
            for (c, source) in sample.into_iter().enumerate() {
                let background = px.data[index + c];
                px.data[index + c] = background + (source - background) * coverage;
            }
        }
    }
}

/// Radial falloff at the disk's edge: full coverage out to `1 - feather` of
/// the normalized radius `t` (0 at the center, 1 at the rim), smoothstep-
/// eased to 0 from there to the rim. `feather = 0` is a hard edge;
/// `feather = 1` eases across the whole disk.
pub(crate) fn radial_coverage(t: f64, feather: f64) -> f32 {
    if t >= 1.0 {
        return 0.0;
    }
    let inner = 1.0 - feather;
    if t <= inner {
        return 1.0;
    }
    let span = (1.0 - inner).max(1e-9);
    let f = ((t - inner) / span).clamp(0.0, 1.0) as f32;
    1.0 - f * f * (3.0 - 2.0 * f)
}
