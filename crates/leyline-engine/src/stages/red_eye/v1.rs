//! Red-eye correction v1 (ADR 0103) — rank 35, right after spot removal
//! and before every tonal stage.
//!
//! A disk placed by hand over a pupil, inside which each pixel is corrected
//! according to **how red it actually is**, through a smoothed threshold
//! (ADR 0103 §2). That test is the whole design: it is what makes an
//! imprecisely placed circle harmless, and therefore what makes a tool
//! without detection usable. The circle says where to look; the
//! red-dominance test says what to fix.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use leyline_core::RedEye;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{post_rotation_point_to_buffer, smoothstep01};
use crate::stages::spot_removal::v1::radial_coverage;

/// Below this red dominance a pixel is not a pupil and is left alone: a
/// skin tone the circle happens to cover scores well under it.
const RED_FLOOR: f32 = 0.2;

/// From this red dominance up, the correction is applied in full — a
/// saturated pupil must come out neutral, not nine tenths neutral.
const RED_FULL: f32 = 0.6;

/// Applies every red-eye correction, in list order.
pub(crate) fn red_eye(px: &mut Pixels, eyes: &[RedEye], rotation_degrees: f64) {
    for eye in eyes {
        apply_eye(px, eye, rotation_degrees);
    }
}

/// Corrects one disk. The geometry is spot removal's, exactly — the same
/// post-rotation referential (ADR 0026), the same radius normalization
/// against the larger dimension, the same radial falloff — so a circle
/// drawn by the same gesture lands in the same place.
fn apply_eye(px: &mut Pixels, eye: &RedEye, rotation_degrees: f64) {
    let (width, height) = (px.width, px.height);
    let (cx, cy) = post_rotation_point_to_buffer(width, height, rotation_degrees, eye.center);
    let radius_px = eye.radius * f64::from(width.max(height));
    if radius_px <= 0.0 {
        return;
    }
    let feather = eye.feather.clamp(0.0, 1.0);
    let darken = eye.darken.clamp(0.0, 1.0) as f32;

    let x0 = (cx - radius_px).floor().max(0.0) as usize;
    let y0 = (cy - radius_px).floor().max(0.0) as usize;
    let x1 = ((cx + radius_px).ceil() as i64).clamp(0, i64::from(width) - 1) as usize;
    let y1 = ((cy + radius_px).ceil() as i64).clamp(0, i64::from(height) - 1) as usize;
    if x0 >= width as usize || y0 >= height as usize || x1 < x0 || y1 < y0 {
        return;
    }

    for y in y0..=y1 {
        for x in x0..=x1 {
            let (px_x, px_y) = (x as f64 + 0.5, y as f64 + 0.5);
            let dist = ((px_x - cx).powi(2) + (px_y - cy).powi(2)).sqrt();
            if dist > radius_px {
                continue;
            }
            let coverage = radial_coverage(dist / radius_px, feather);
            if coverage <= 0.0 {
                continue;
            }
            let index = (y * width as usize + x) * 3;
            let (r, g, b) = (px.data[index], px.data[index + 1], px.data[index + 2]);
            // How red this pixel is: the excess of red over the larger of
            // the other two, relative to red. A neutral pixel scores 0 and
            // is left alone whatever the circle covers (ADR 0103 §2).
            let other = g.max(b);
            if r <= other || r <= 0.0 {
                continue;
            }
            let redness = ((r - other) / r).clamp(0.0, 1.0);
            // A smoothed threshold, not a proportion. Scaling the
            // correction by `redness` directly would leave a saturated
            // pupil visibly red (0.9 of the way is still a cast), while a
            // hard threshold would put a visible contour through the
            // gradient at an eye's rim. Below `RED_FLOOR` nothing is
            // touched — that is the clause protecting a skin tone the
            // circle happens to cover.
            let gate =
                smoothstep01(((redness - RED_FLOOR) / (RED_FULL - RED_FLOOR)).clamp(0.0, 1.0));
            if gate <= 0.0 {
                continue;
            }
            let strength = gate * coverage;
            // The cast goes first: red drops to the level of the other
            // channels, so the pupil stops being red before it is dark.
            let corrected = r + (other - r) * strength;
            // Then the darkening, applied to all three so the pupil reads
            // as a pupil rather than as a grey patch.
            let dim = 1.0 - darken * strength;
            px.data[index] = corrected * dim;
            px.data[index + 1] = g * dim;
            px.data[index + 2] = b * dim;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leyline_core::Point;

    /// A flat field of one colour, big enough for a disk to sit inside.
    fn field(rgb: [f32; 3]) -> Pixels {
        let (width, height) = (32u32, 32u32);
        let mut data = Vec::with_capacity((width * height * 3) as usize);
        for _ in 0..width * height {
            data.extend_from_slice(&rgb);
        }
        Pixels {
            width,
            height,
            data,
        }
    }

    fn centered(radius: f64, darken: f64) -> RedEye {
        RedEye {
            center: Point { x: 0.5, y: 0.5 },
            radius,
            // A hard edge, so the centre pixel's value is not diluted by
            // the falloff while the effect itself is under test.
            feather: 0.0,
            darken,
        }
    }

    fn center_pixel(px: &Pixels) -> [f32; 3] {
        let index = ((px.height as usize / 2) * px.width as usize + px.width as usize / 2) * 3;
        [px.data[index], px.data[index + 1], px.data[index + 2]]
    }

    /// The point of the tool: a red pupil stops being red, and darkens.
    #[test]
    fn a_red_pixel_loses_its_cast_and_darkens() {
        let mut px = field([0.60, 0.06, 0.06]);
        red_eye(&mut px, &[centered(0.3, 0.6)], 0.0);
        let [r, g, b] = center_pixel(&px);
        assert!(
            (r - g).abs() < 1e-4 && (r - b).abs() < 1e-4,
            "the cast must be gone, got {r} {g} {b}"
        );
        assert!(r < 0.06, "and the pupil must darken, got {r}");
    }

    /// ADR 0103 §2, and the reason the tool works without detection: a
    /// circle that overlaps skin does not paint a grey disk on it.
    #[test]
    fn a_neutral_pixel_inside_the_disk_is_left_alone() {
        let mut px = field([0.40, 0.40, 0.40]);
        let before = center_pixel(&px);
        red_eye(&mut px, &[centered(0.3, 1.0)], 0.0);
        assert_eq!(center_pixel(&px), before, "a neutral pixel must not move");
    }

    /// A skin tone is mildly red, so it is mildly corrected — not spared
    /// entirely, but nowhere near a pupil's treatment. What matters is the
    /// ratio between the two.
    #[test]
    fn a_skin_tone_is_barely_touched_next_to_a_pupil() {
        let moved = |rgb: [f32; 3]| {
            let mut px = field(rgb);
            let before = center_pixel(&px);
            red_eye(&mut px, &[centered(0.3, 0.6)], 0.0);
            let after = center_pixel(&px);
            (before[0] - after[0]).abs()
        };
        let skin = moved([0.35, 0.26, 0.22]);
        let pupil = moved([0.60, 0.06, 0.06]);
        assert!(
            pupil > skin * 4.0,
            "a pupil must be corrected far more than skin: {pupil} vs {skin}"
        );
    }

    /// Outside the disk nothing happens, whatever the pixel's colour.
    #[test]
    fn pixels_outside_the_disk_are_untouched() {
        let mut px = field([0.60, 0.06, 0.06]);
        let before = px.clone();
        red_eye(&mut px, &[centered(0.1, 1.0)], 0.0);
        // A corner is well outside a disk of radius 0.1 at the centre.
        assert_eq!(px.data[0..3], before.data[0..3]);
        assert_ne!(
            center_pixel(&px),
            center_pixel(&before),
            "but the centre moved"
        );
    }

    /// An empty list is the neutral, and a degenerate radius does nothing
    /// rather than panicking.
    #[test]
    fn the_neutral_cases_change_nothing() {
        let mut px = field([0.60, 0.06, 0.06]);
        let before = px.clone();
        red_eye(&mut px, &[], 0.0);
        assert_eq!(px.data, before.data);
        red_eye(&mut px, &[centered(0.0, 0.6)], 0.0);
        assert_eq!(px.data, before.data);
    }

    /// `darken` at 0 removes the cast and keeps the brightness — the two
    /// halves of the correction are separable, which is what the slider is
    /// for.
    #[test]
    fn darken_at_zero_only_removes_the_cast() {
        let mut px = field([0.60, 0.06, 0.06]);
        red_eye(&mut px, &[centered(0.3, 0.0)], 0.0);
        let [r, g, b] = center_pixel(&px);
        assert!((r - 0.06).abs() < 1e-4, "red drops to the others: {r}");
        assert!(
            (g - 0.06).abs() < 1e-6 && (b - 0.06).abs() < 1e-6,
            "{g} {b}"
        );
    }
}
