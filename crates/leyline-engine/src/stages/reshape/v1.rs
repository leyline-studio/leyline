//! Reshape v1 (ADR 0109) — rank 25, between `lens` and `spot_removal`.
//!
//! Every other stage changes what a pixel *is*; this one changes where it
//! **is**, locally. A handle grabs content at `from`, drops it at `to`, and
//! the pixels within `radius` of the destination follow, fading to nothing
//! at the edge of that circle.
//!
//! The stage is defined by its **inverse map** (ADR 0109 §3), which is the
//! whole of its implementation cost:
//!
//! ```text
//! source(p) = p + Σᵢ wᵢ(p) · strengthᵢ · (fromᵢ − toᵢ)
//! wᵢ(p)     = smoothstep falloff of ‖p − toᵢ‖ over radiusᵢ
//! ```
//!
//! The weight is centred on `to` and the offset points back toward `from`,
//! so at `p = to` the weight is 1 and the sample lands exactly on `from`:
//! what the user grabbed appears where they dropped it, **by construction**.
//! Nothing is inverted numerically, nothing is scattered, no accumulation
//! buffer exists. It is `rotate::v1`'s technique with a different source
//! formula — the fourth consumer of ADR 0026's backward remapping.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever.

use leyline_core::ReshapePoint;
use rayon::prelude::*;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{post_rotation_point_to_buffer, smoothstep01};

/// One handle, resolved into the buffer's own pixels.
struct Handle {
    /// The centre of influence, in buffer pixels.
    to: (f64, f64),
    /// Radius of influence, in buffer pixels.
    radius: f64,
    /// The displacement already scaled by `strength`.
    offset: (f64, f64),
}

/// Applies every handle at once (ADR 0109 §4: overlapping handles **add**).
///
/// `rotation_degrees` is what puts the stored, post-rotation coordinates
/// back into the buffer — the same mapping `spot_removal` and `red_eye`
/// call, and for the same reason.
pub(crate) fn reshape(px: &Pixels, points: &[ReshapePoint], rotation_degrees: f64) -> Pixels {
    let handles: Vec<Handle> = points
        .iter()
        .filter(|point| point.strength > 0.0 && point.radius > 0.0)
        .map(|point| {
            let from =
                post_rotation_point_to_buffer(px.width, px.height, rotation_degrees, point.from);
            let to = post_rotation_point_to_buffer(px.width, px.height, rotation_degrees, point.to);
            // A radius normalized against the buffer's larger dimension, the
            // convention `SpotRemoval` and `RedEye` already use.
            let larger = f64::from(px.width.max(px.height));
            Handle {
                to,
                radius: point.radius * larger,
                offset: (
                    (from.0 - to.0) * point.strength,
                    (from.1 - to.1) * point.strength,
                ),
            }
        })
        .collect();
    if handles.is_empty() {
        return px.clone();
    }

    // The output starts as the input: a pixel no handle reaches is not
    // resampled at all, so an untouched region stays bit-identical rather
    // than passing through a bilinear that would soften it (and would make
    // `strength = 0` differ from an empty list).
    let mut data = px.data.clone();
    let width = px.width as usize;
    data.par_chunks_mut(width * 3)
        .enumerate()
        .for_each(|(y, row)| {
            let py = y as f64 + 0.5;
            // Whole rows are outside every circle: skip them without
            // touching a pixel.
            if !handles.iter().any(|h| (py - h.to.1).abs() < h.radius) {
                return;
            }
            for (x, rgb_out) in row.chunks_exact_mut(3).enumerate() {
                let px_center = x as f64 + 0.5;
                let (mut dx, mut dy) = (0.0f64, 0.0f64);
                for handle in &handles {
                    let distance = (px_center - handle.to.0).hypot(py - handle.to.1);
                    if distance >= handle.radius {
                        continue;
                    }
                    // 1 at the centre, 0 at the rim, smooth in between —
                    // the same falloff masks and the sharpening edge mask
                    // use.
                    let weight = f64::from(smoothstep01(1.0 - (distance / handle.radius) as f32));
                    dx += weight * handle.offset.0;
                    dy += weight * handle.offset.1;
                }
                if dx == 0.0 && dy == 0.0 {
                    continue;
                }
                let rgb = sample_clamped(px, px_center + dx - 0.5, py + dy - 0.5);
                rgb_out.copy_from_slice(&rgb);
            }
        });

    Pixels {
        width: px.width,
        height: px.height,
        data,
    }
}

/// Bilinear sample with the **edge extended** (ADR 0109 §4).
///
/// A sample falling outside the frame reads the nearest edge pixel rather
/// than black: black would be a hole the user did not ask for and that
/// nothing in this program can fill.
fn sample_clamped(px: &Pixels, sx: f64, sy: f64) -> [f32; 3] {
    let (w, h) = (px.width as i64, px.height as i64);
    let x0f = sx.floor();
    let y0f = sy.floor();
    let tx = (sx - x0f) as f32;
    let ty = (sy - y0f) as f32;
    let clamp = |v: i64, max: i64| v.clamp(0, max - 1) as usize;
    let x0 = clamp(x0f as i64, w);
    let x1 = clamp(x0f as i64 + 1, w);
    let y0 = clamp(y0f as i64, h);
    let y1 = clamp(y0f as i64 + 1, h);

    let at = |x: usize, y: usize, c: usize| px.data[(y * px.width as usize + x) * 3 + c];
    let mut out = [0.0f32; 3];
    for (c, sample) in out.iter_mut().enumerate() {
        let top = at(x0, y0, c) * (1.0 - tx) + at(x1, y0, c) * tx;
        let bottom = at(x0, y1, c) * (1.0 - tx) + at(x1, y1, c) * tx;
        *sample = top * (1.0 - ty) + bottom * ty;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use leyline_core::Point;

    /// A flat field with one bright mark, so "where did the content go" has
    /// an answer a test can point at.
    fn marked(width: u32, height: u32, mark: (usize, usize)) -> Pixels {
        let mut data = vec![0.2f32; width as usize * height as usize * 3];
        let offset = (mark.1 * width as usize + mark.0) * 3;
        data[offset..offset + 3].copy_from_slice(&[1.0, 1.0, 1.0]);
        Pixels {
            width,
            height,
            data,
        }
    }

    fn brightness(px: &Pixels, x: usize, y: usize) -> f32 {
        let offset = (y * px.width as usize + x) * 3;
        px.data[offset]
    }

    fn handle(from: (f64, f64), to: (f64, f64), radius: f64, strength: f64) -> ReshapePoint {
        ReshapePoint {
            from: Point {
                x: from.0,
                y: from.1,
            },
            to: Point { x: to.0, y: to.1 },
            radius,
            strength,
        }
    }

    /// The property the whole design rests on (ADR 0109 §3): at `to`, the
    /// weight is 1, so the sample lands exactly on `from` — the content the
    /// user grabbed appears where they dropped it.
    #[test]
    fn what_was_grabbed_appears_where_it_was_dropped() {
        let px = marked(64, 64, (20, 32));
        // The mark sits at pixel (20, 32) of a 64x64 buffer.
        let out = reshape(
            &px,
            &[handle(
                (20.5 / 64.0, 32.5 / 64.0),
                (40.5 / 64.0, 32.5 / 64.0),
                0.4,
                1.0,
            )],
            0.0,
        );
        assert!(
            brightness(&out, 40, 32) > 0.9,
            "the mark should have arrived at 40: {}",
            brightness(&out, 40, 32)
        );
    }

    #[test]
    fn an_empty_list_and_a_zero_strength_are_the_photograph_itself() {
        let px = marked(48, 48, (10, 10));
        assert_eq!(reshape(&px, &[], 0.0), px);
        let zero = reshape(&px, &[handle((0.2, 0.2), (0.6, 0.6), 0.3, 0.0)], 0.0);
        assert_eq!(zero, px, "strength 0 is not a resampling, it is nothing");
    }

    #[test]
    fn beyond_the_radius_nothing_moves() {
        let px = marked(64, 64, (5, 5));
        let out = reshape(&px, &[handle((0.8, 0.8), (0.7, 0.7), 0.1, 1.0)], 0.0);
        // The mark is far outside that circle.
        assert!((brightness(&out, 5, 5) - 1.0).abs() < 1e-6);
        // And the untouched region is bit-identical, not merely close.
        for y in 0..20 {
            for x in 0..20 {
                assert_eq!(brightness(&out, x, y), brightness(&px, x, y));
            }
        }
    }

    /// The stated cost (ADR 0109 §4): two overlapping handles **add**, and
    /// the sum is what the map applies.
    #[test]
    fn overlapping_handles_add_their_displacements() {
        let px = marked(64, 64, (20, 32));
        let one = reshape(
            &px,
            &[handle(
                (20.5 / 64.0, 32.5 / 64.0),
                (30.5 / 64.0, 32.5 / 64.0),
                0.5,
                1.0,
            )],
            0.0,
        );
        let two = reshape(
            &px,
            &[
                handle(
                    (20.5 / 64.0, 32.5 / 64.0),
                    (30.5 / 64.0, 32.5 / 64.0),
                    0.5,
                    0.5,
                ),
                handle(
                    (20.5 / 64.0, 32.5 / 64.0),
                    (30.5 / 64.0, 32.5 / 64.0),
                    0.5,
                    0.5,
                ),
            ],
            0.0,
        );
        // Two half-strength copies of the same handle land where one full
        // one does.
        assert!(
            (brightness(&one, 30, 32) - brightness(&two, 30, 32)).abs() < 0.05,
            "{} vs {}",
            brightness(&one, 30, 32),
            brightness(&two, 30, 32)
        );
    }

    /// The other stated cost: the frame keeps its size and the edge is
    /// extended, never filled with black.
    #[test]
    fn a_sample_off_the_frame_reads_the_edge_rather_than_black() {
        // A bright column at x = 0, and a handle that drags content in from
        // beyond the left border.
        let mut px = marked(32, 32, (0, 16));
        for y in 0..32usize {
            let offset = (y * 32) * 3;
            px.data[offset..offset + 3].copy_from_slice(&[0.9, 0.9, 0.9]);
        }
        let out = reshape(&px, &[handle((0.0, 0.5), (0.3, 0.5), 0.4, 1.0)], 0.0);
        assert!(
            brightness(&out, 9, 16) > 0.5,
            "the edge was extended, not blackened: {}",
            brightness(&out, 9, 16)
        );
    }
}
