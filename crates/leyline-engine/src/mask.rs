//! Mask rasterization and coverage blending for local (masked) adjustments
//! (ADR 0029).
//!
//! This module is pure geometry: it turns a [`Mask`] into a per-pixel
//! coverage plane (`[0, 1]`) and blends two buffers by that coverage. It
//! encodes **no** process-specific pixel-transform formula of its own — the
//! actual re-parameterized operators (exposure, contrast, white balance…)
//! stay inside each process module, called on their own already-frozen
//! functions (`stages::local_adjustments::v1` builds a fully re-adjusted copy
//! of the buffer, then asks this module how much of it to blend in per
//! pixel). That split is why this module is safe to share across every
//! future process version the same way `pixels.rs` already is: a later
//! process version that also needs masking will keep calling this same
//! module, and nothing here ever encodes a frozen version's render — only
//! `rotate`/`crop`/the tonal operators inside each `processN.rs` do that,
//! and those stay duplicated per module exactly as ADR 0028 requires.

use rayon::prelude::*;

use leyline_core::Mask;

use crate::pixels::Pixels;

/// The post-rotation canvas a mask's coordinates are normalized against
/// (ADR 0026): the axis-aligned bounding box `rotate` would produce for this
/// buffer at `degrees`, plus the analytic transform from a *buffer* pixel
/// (this stage runs before `rotate`) to that canvas's own pixel coordinates.
struct CanvasFrame {
    /// Canvas width in pixels.
    out_w: f64,
    /// Canvas height in pixels.
    out_h: f64,
    cos: f64,
    sin: f64,
    cx: f64,
    cy: f64,
    ocx: f64,
    ocy: f64,
}

impl CanvasFrame {
    fn new(width: u32, height: u32, degrees: f64) -> CanvasFrame {
        let radians = degrees.rem_euclid(360.0).to_radians();
        let (sin, cos) = radians.sin_cos();
        let (w, h) = (f64::from(width), f64::from(height));
        let out_w = (w * cos.abs() + h * sin.abs()).round().max(1.0);
        let out_h = (w * sin.abs() + h * cos.abs()).round().max(1.0);
        CanvasFrame {
            out_w,
            out_h,
            cos,
            sin,
            cx: w / 2.0,
            cy: h / 2.0,
            ocx: out_w / 2.0,
            ocy: out_h / 2.0,
        }
    }

    /// Maps a buffer pixel coordinate to its canvas-pixel coordinate — the
    /// exact analytic inverse of the rotation `rotate`/
    /// `post_rotation_point_to_buffer` apply per output pixel elsewhere in
    /// the process modules (that transform is a pure rotation, so its
    /// inverse is its transpose; no iterative solving needed).
    fn buffer_to_canvas(&self, sx: f64, sy: f64) -> (f64, f64) {
        let ax = sx - self.cx;
        let ay = sy - self.cy;
        let dx = self.cos * ax - self.sin * ay;
        let dy = self.sin * ax + self.cos * ay;
        (dx + self.ocx, dy + self.ocy)
    }
}

/// Radial falloff at a shape's rim, `t` the normalized distance from center
/// (0 at the center, 1 at the rim): full coverage out to `1 - feather`,
/// smoothstep-eased to 0 from there to the rim. Shared shape for the radial
/// mask's ellipse edge and each brush dab's edge — the same easing spot
/// removal (ADR 0032) uses for its disk edge, kept as its own copy here
/// since this module carries no dependency on any process module.
fn radial_falloff(t: f64, feather: f64) -> f64 {
    if t >= 1.0 {
        return 0.0;
    }
    let inner = 1.0 - feather.clamp(0.0, 1.0);
    if t <= inner {
        return 1.0;
    }
    let span = (1.0 - inner).max(1e-9);
    let f = ((t - inner) / span).clamp(0.0, 1.0);
    1.0 - f * f * (3.0 - 2.0 * f)
}

/// Coverage of a [`Mask::Radial`] ellipse at one canvas-pixel coordinate.
#[allow(clippy::too_many_arguments)]
fn radial_coverage_at(
    x: f64,
    y: f64,
    frame: &CanvasFrame,
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    angle: f64,
    feather: f64,
    inverted: bool,
) -> f64 {
    let (px, py) = (cx * frame.out_w, cy * frame.out_h);
    let (rxp, ryp) = ((rx * frame.out_w).max(1e-9), (ry * frame.out_h).max(1e-9));
    let dx = x - px;
    let dy = y - py;
    // Undoes the ellipse's own clockwise `angle` to evaluate distance in its
    // axis-aligned frame — the same "rotate by the inverse angle" idiom as
    // `CanvasFrame::buffer_to_canvas`, just for the mask's local rotation
    // rather than the image's.
    let (sin, cos) = angle.to_radians().sin_cos();
    let lx = cos * dx + sin * dy;
    let ly = -sin * dx + cos * dy;
    let t = (lx / rxp).hypot(ly / ryp);
    let coverage = radial_falloff(t, feather);
    if inverted { 1.0 - coverage } else { coverage }
}

/// Coverage of a [`Mask::Gradient`] at one canvas-pixel coordinate: full at
/// `(x0, y0)`, linearly easing to none at `(x1, y1)`, constant along lines
/// perpendicular to that axis. `Settings::validate` guarantees the two
/// endpoints differ, so the projection below never divides by zero.
fn gradient_coverage_at(
    x: f64,
    y: f64,
    frame: &CanvasFrame,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
) -> f64 {
    let (px0, py0) = (x0 * frame.out_w, y0 * frame.out_h);
    let (px1, py1) = (x1 * frame.out_w, y1 * frame.out_h);
    let (vx, vy) = (px1 - px0, py1 - py0);
    let len2 = vx * vx + vy * vy;
    let t = ((x - px0) * vx + (y - py0) * vy) / len2;
    (1.0 - t).clamp(0.0, 1.0)
}

/// Coverage of a [`Mask::Brush`] at one canvas-pixel coordinate: dabs
/// accumulate with "over" compositing (each additional dab covers a
/// fraction `flow` of what remains uncovered), so repeatedly stroking the
/// same spot approaches full coverage without ever exceeding it — the usual
/// painting-tool behavior, applied in stroke order.
fn brush_coverage_at(
    x: f64,
    y: f64,
    frame: &CanvasFrame,
    strokes: &[leyline_core::BrushStroke],
) -> f64 {
    let scale = frame.out_w.max(frame.out_h);
    let mut coverage = 0.0;
    for stroke in strokes {
        let (px, py) = (stroke.x * frame.out_w, stroke.y * frame.out_h);
        let radius_px = (stroke.radius * scale).max(1e-9);
        let dist = (x - px).hypot(y - py);
        let dab = radial_falloff(dist / radius_px, 1.0 - stroke.hardness.clamp(0.0, 1.0))
            * stroke.flow.clamp(0.0, 1.0);
        coverage += dab * (1.0 - coverage);
    }
    coverage
}

/// Rasterizes one mask's coverage over the whole (still-unrotated) working
/// buffer: for every buffer pixel, maps it into the post-rotation canvas
/// frame (ADR 0026) and evaluates the mask there.
pub(crate) fn rasterize_coverage(
    mask: &Mask,
    width: u32,
    height: u32,
    rotation_degrees: f64,
) -> Vec<f32> {
    let frame = CanvasFrame::new(width, height, rotation_degrees);
    let mut coverage = vec![0.0f32; width as usize * height as usize];
    coverage
        .par_chunks_mut(width as usize)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, cell) in row.iter_mut().enumerate() {
                let (cx, cy) = frame.buffer_to_canvas(x as f64 + 0.5, y as f64 + 0.5);
                *cell = match mask {
                    Mask::Radial {
                        cx: mx,
                        cy: my,
                        rx,
                        ry,
                        angle,
                        feather,
                        inverted,
                    } => radial_coverage_at(
                        cx, cy, &frame, *mx, *my, *rx, *ry, *angle, *feather, *inverted,
                    ),
                    Mask::Gradient { x0, y0, x1, y1 } => {
                        gradient_coverage_at(cx, cy, &frame, *x0, *y0, *x1, *y1)
                    }
                    Mask::Brush { strokes } => brush_coverage_at(cx, cy, &frame, strokes),
                } as f32;
            }
        });
    coverage
}

/// Blends `adjusted` into `base` in place, per pixel, by `coverage[pixel] *
/// opacity` — the `output = lerp(buffer, apply_local_operators(...),
/// coverage)` composition of ADR 0029.
pub(crate) fn blend_by_coverage(
    base: &mut Pixels,
    adjusted: &Pixels,
    coverage: &[f32],
    opacity: f64,
) {
    let opacity = opacity.clamp(0.0, 1.0) as f32;
    let width = base.width as usize;
    base.data
        .par_chunks_mut(width * 3)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, rgb) in row.chunks_exact_mut(3).enumerate() {
                let amount = coverage[y * width + x] * opacity;
                if amount <= 0.0 {
                    continue;
                }
                let base_index = (y * width + x) * 3;
                for (c, sample) in rgb.iter_mut().enumerate() {
                    *sample += (adjusted.data[base_index + c] - *sample) * amount;
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radial_falloff_is_full_inside_the_core_and_zero_past_the_rim() {
        assert_eq!(radial_falloff(0.0, 0.4), 1.0);
        assert_eq!(radial_falloff(1.0, 0.4), 0.0);
        assert_eq!(radial_falloff(1.5, 0.4), 0.0);
    }

    #[test]
    fn radial_coverage_is_full_at_the_center_and_fades_by_the_rim() {
        let frame = CanvasFrame::new(100, 100, 0.0);
        let center =
            radial_coverage_at(50.0, 42.0, &frame, 0.5, 0.42, 0.30, 0.22, 0.0, 0.40, false);
        assert_eq!(center, 1.0);
        let rim = radial_coverage_at(
            50.0 + 30.0,
            42.0,
            &frame,
            0.5,
            0.42,
            0.30,
            0.22,
            0.0,
            0.40,
            false,
        );
        assert_eq!(rim, 0.0);
    }

    #[test]
    fn radial_inverted_flips_center_and_far_field() {
        let frame = CanvasFrame::new(100, 100, 0.0);
        let center = radial_coverage_at(50.0, 50.0, &frame, 0.5, 0.5, 0.2, 0.2, 0.0, 0.0, true);
        assert_eq!(center, 0.0);
        let far = radial_coverage_at(0.0, 0.0, &frame, 0.5, 0.5, 0.2, 0.2, 0.0, 0.0, true);
        assert_eq!(far, 1.0);
    }

    #[test]
    fn gradient_is_full_at_the_start_and_zero_at_the_end() {
        let frame = CanvasFrame::new(100, 100, 0.0);
        let start = gradient_coverage_at(50.0, 0.0, &frame, 0.5, 0.0, 0.5, 0.5);
        assert_eq!(start, 1.0);
        let end = gradient_coverage_at(50.0, 50.0, &frame, 0.5, 0.0, 0.5, 0.5);
        assert_eq!(end, 0.0);
        let past_end = gradient_coverage_at(50.0, 90.0, &frame, 0.5, 0.0, 0.5, 0.5);
        assert_eq!(past_end, 0.0);
        let mid = gradient_coverage_at(50.0, 25.0, &frame, 0.5, 0.0, 0.5, 0.5);
        assert!((mid - 0.5).abs() < 1e-9);
    }

    #[test]
    fn brush_single_dab_matches_radial_falloff() {
        let frame = CanvasFrame::new(100, 100, 0.0);
        let strokes = vec![leyline_core::BrushStroke {
            x: 0.5,
            y: 0.5,
            radius: 0.1,
            flow: 1.0,
            hardness: 0.5,
        }];
        let center = brush_coverage_at(50.0, 50.0, &frame, &strokes);
        assert_eq!(center, 1.0);
        let far = brush_coverage_at(0.0, 0.0, &frame, &strokes);
        assert_eq!(far, 0.0);
    }

    #[test]
    fn overlapping_dabs_never_exceed_full_coverage() {
        let frame = CanvasFrame::new(100, 100, 0.0);
        let strokes: Vec<leyline_core::BrushStroke> = (0..10)
            .map(|_| leyline_core::BrushStroke {
                x: 0.5,
                y: 0.5,
                radius: 0.1,
                flow: 0.9,
                hardness: 1.0,
            })
            .collect();
        let coverage = brush_coverage_at(50.0, 50.0, &frame, &strokes);
        assert!(coverage <= 1.0);
        assert!(coverage > 0.99);
    }

    #[test]
    fn buffer_to_canvas_is_identity_at_zero_rotation() {
        let frame = CanvasFrame::new(100, 50, 0.0);
        let (cx, cy) = frame.buffer_to_canvas(30.0, 35.0);
        assert!((cx - 30.0).abs() < 1e-9);
        assert!((cy - 35.0).abs() < 1e-9);
    }

    #[test]
    fn buffer_to_canvas_round_trips_through_the_process_modules_inverse() {
        // `buffer_to_canvas` must be the exact analytic inverse of
        // `post_rotation_point_to_buffer` (copied identically into every
        // process module, e.g. `process8.rs`): composing forward then
        // backward returns the original buffer pixel, for several angles.
        for degrees in [0.0, 37.0, 90.0, 180.0, 271.0] {
            let (width, height) = (100u32, 50u32);
            let frame = CanvasFrame::new(width, height, degrees);
            for (sx, sy) in [(0.0, 0.0), (99.0, 49.0), (42.3, 17.9)] {
                let (cx, cy) = frame.buffer_to_canvas(sx, sy);
                let point = crate::stages::kernel::v1::post_rotation_point_to_buffer(
                    width,
                    height,
                    degrees,
                    leyline_core::Point {
                        x: cx / frame.out_w,
                        y: cy / frame.out_h,
                    },
                );
                assert!(
                    (point.0 - sx).abs() < 1e-6,
                    "x at {degrees} degrees: {point:?} vs {sx}"
                );
                assert!(
                    (point.1 - sy).abs() < 1e-6,
                    "y at {degrees} degrees: {point:?} vs {sy}"
                );
            }
        }
    }

    #[test]
    fn rasterize_coverage_neutral_mask_stays_in_bounds() {
        let mask = Mask::Radial {
            cx: 0.5,
            cy: 0.5,
            rx: 0.3,
            ry: 0.3,
            angle: 0.0,
            feather: 0.2,
            inverted: false,
        };
        let coverage = rasterize_coverage(&mask, 20, 10, 0.0);
        assert_eq!(coverage.len(), 200);
        for value in coverage {
            assert!((0.0..=1.0).contains(&value));
        }
    }
}
