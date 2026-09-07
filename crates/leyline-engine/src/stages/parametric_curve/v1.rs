//! Parametric tone curve v1 (ADR 0137) — rank 75, just before the point
//! curve, which is the precise instrument and therefore has the last word.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use leyline_core::{CurvePoint, ParametricCurve};

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{display_curve, in_display};
use crate::stages::tone_curve::v1::{build_curve_lut, curve_lookup};

/// How far a region slider at ±100 lifts the centre of its region, in
/// display units.
///
/// Frozen with this version: it is the whole strength of the control, and
/// changing it changes every rendered pixel.
const REACH: f64 = 0.25;

/// Applies the parametric curve, through the same lookup table the point
/// curve uses — the six control points of [`control_points`] handed to the
/// monotone interpolant that has rendered pixels since process 6.
pub(crate) fn parametric_curve(px: &mut Pixels, curve: &ParametricCurve) {
    let table = build_curve_lut(&control_points(curve));
    in_display(px, |px| {
        display_curve(px, |x| curve_lookup(&table, x));
    });
}

/// The six control points a parametric curve describes (ADR 0137 §3).
///
/// A slider lifts the **centre** of its region, not its boundary — which is
/// what makes the sliders and the splits two different controls rather than
/// two names for one. Both ends stay pinned: black is black and white is
/// white, and no region slider can lift the black point.
///
/// The `y` values are clamped into `[0, 1]` and then forced non-decreasing
/// in one forward pass, so the data reaching the interpolant is monotone and
/// the curve therefore is. A tone curve that goes back on itself solarises,
/// and no combination of four sliders should be able to ask for that.
pub(crate) fn control_points(curve: &ParametricCurve) -> Vec<CurvePoint> {
    let s1 = f64::from(curve.shadow_split) / 100.0;
    let s2 = f64::from(curve.midtone_split) / 100.0;
    let s3 = f64::from(curve.highlight_split) / 100.0;
    // Strictly increasing, which `Settings::validate` guarantees by refusing
    // equal or reversed splits (ADR 0137 §1) — the condition the interpolant
    // requires of its `x`.
    let centres = [s1 / 2.0, (s1 + s2) / 2.0, (s2 + s3) / 2.0, (s3 + 1.0) / 2.0];
    let lifts = [
        f64::from(curve.shadows),
        f64::from(curve.darks),
        f64::from(curve.lights),
        f64::from(curve.highlights),
    ];

    let mut points = Vec::with_capacity(6);
    points.push(CurvePoint { x: 0.0, y: 0.0 });
    for (x, lift) in centres.into_iter().zip(lifts) {
        points.push(CurvePoint {
            x,
            y: (x + lift / 100.0 * REACH).clamp(0.0, 1.0),
        });
    }
    points.push(CurvePoint { x: 1.0, y: 1.0 });

    let mut floor = 0.0;
    for point in &mut points {
        point.y = point.y.max(floor);
        floor = point.y;
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The neutral curve is the identity, point for point: a stage that runs
    /// at neutral and changes nothing is what makes `active` safe to trust.
    #[test]
    fn a_neutral_parametric_curve_is_the_identity() {
        let points = control_points(&ParametricCurve::default());
        assert_eq!(points.len(), 6);
        for point in &points {
            assert!(
                (point.x - point.y).abs() < 1e-12,
                "{point:?} is off the diagonal"
            );
        }
    }

    /// ADR 0137 §3: monotone whatever the sliders say. The pair that would
    /// break an additive scheme — highlights pulled down under lights pushed
    /// up — is the one to try.
    #[test]
    fn the_curve_never_goes_back_on_itself() {
        for (shadows, darks, lights, highlights) in [
            (100, -100, 100, -100),
            (-100, 100, -100, 100),
            (100, 100, 100, -100),
            (-100, -100, -100, 100),
        ] {
            let curve = ParametricCurve {
                shadows,
                darks,
                lights,
                highlights,
                ..ParametricCurve::default()
            };
            let points = control_points(&curve);
            let table = build_curve_lut(&points);
            let mut previous = f32::NEG_INFINITY;
            for (index, value) in table.iter().enumerate() {
                assert!(
                    *value >= previous - 1e-6,
                    "sample {index} of {curve:?} goes back on itself: {value} after {previous}"
                );
                previous = *value;
            }
        }
    }

    /// Both ends stay pinned, whatever the regions do: lifting the shadows
    /// is not a way to raise the black point.
    #[test]
    fn black_stays_black_and_white_stays_white() {
        let curve = ParametricCurve {
            shadows: 100,
            highlights: -100,
            ..ParametricCurve::default()
        };
        let points = control_points(&curve);
        assert_eq!(points.first().map(|p| (p.x, p.y)), Some((0.0, 0.0)));
        assert_eq!(points.last().map(|p| (p.x, p.y)), Some((1.0, 1.0)));
    }

    /// Moving a split moves the centres either side of it — which is what
    /// makes the splits a second control rather than decoration.
    #[test]
    fn a_split_moves_the_regions_around_it() {
        let narrow = control_points(&ParametricCurve {
            shadow_split: 10,
            ..ParametricCurve::default()
        });
        let wide = control_points(&ParametricCurve {
            shadow_split: 40,
            ..ParametricCurve::default()
        });
        // The shadows centre follows its own split...
        assert!(wide[1].x > narrow[1].x);
        // ...and so does the darks centre, which sits between two splits.
        assert!(wide[2].x > narrow[2].x);
        // The two above it are untouched.
        assert!((wide[3].x - narrow[3].x).abs() < 1e-12);
        assert!((wide[4].x - narrow[4].x).abs() < 1e-12);
    }
}
