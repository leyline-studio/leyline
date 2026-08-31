//! Tone curve v2 (ADR 0098) — rank 80. The master curve of v1, plus one
//! curve per channel.
//!
//! **With no channel curve set, this calls v1.** Not "computes the same
//! thing" — calls it, so bit-identity is a property of the control flow
//! rather than an argument about floating point (ADR 0098 §1). The golden
//! manifest holds the two together: a case whose channel curves are empty
//! carries the same digest under v1 and under v2.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use leyline_core::ToneCurve;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{in_display, par_rows};
use crate::stages::tone_curve::v1::{CURVE_LUT_SIZE, build_curve_lut, curve_lookup};

/// Applies the master curve, then each channel's own curve.
///
/// The master curve's table is built by v1's own `build_curve_lut`, reused
/// rather than restated: the two versions must agree on the spline exactly,
/// and the surest way to agree with code is to run it.
pub(crate) fn tone_curve(px: &mut Pixels, curve: &ToneCurve) {
    if !curve.has_channel_curves() {
        // The identity of ADR 0098 §1, made structural.
        //
        // The emptiness guard is not redundant with the stage's `active`
        // predicate: v1's table builder needs at least two points, and a
        // direct caller (a test, a future one) handing this an all-empty
        // curve must get the identity rather than a panic.
        if !curve.points.is_empty() {
            super::v1::tone_curve(px, &curve.points);
        }
        return;
    }

    let master = (!curve.points.is_empty()).then(|| build_curve_lut(&curve.points));
    let channels = [&curve.red, &curve.green, &curve.blue]
        .map(|points| (!points.is_empty()).then(|| build_curve_lut(points)));

    // What each channel does to display white, so a sample above it keeps
    // its headroom instead of being crushed onto the curve's endpoint —
    // v1's `display_curve` rule, applied per channel (ADR 0098 §2).
    let at_white = std::array::from_fn::<f32, 3, _>(|c| {
        let after_master = master.as_ref().map_or(1.0, |lut| curve_lookup(lut, 1.0));
        channels[c]
            .as_ref()
            .map_or(after_master, |lut| curve_lookup(lut, after_master))
    });

    in_display(px, |px| {
        par_rows(px, |row| {
            for rgb in row.chunks_exact_mut(3) {
                for (c, sample) in rgb.iter_mut().enumerate() {
                    *sample = if *sample <= 1.0 {
                        let v = master
                            .as_ref()
                            .map_or(*sample, |lut| curve_lookup(lut, *sample));
                        channels[c].as_ref().map_or(v, |lut| curve_lookup(lut, v))
                    } else {
                        *sample * at_white[c]
                    };
                }
            }
        });
    });
}

/// Kept so the table size this version depends on is named here too: a
/// change to it is a change of pixels, i.e. a new version module.
const _: () = assert!(CURVE_LUT_SIZE == 4096);

#[cfg(test)]
mod tests {
    use super::*;
    use leyline_core::CurvePoint;

    fn image(width: u32, height: u32) -> Pixels {
        let mut data = vec![0.0f32; (width * height * 3) as usize];
        for (i, sample) in data.iter_mut().enumerate() {
            // A gradient per channel, offset so the three differ.
            *sample = ((i % 97) as f32 / 96.0) * 0.9 + 0.05;
        }
        Pixels {
            width,
            height,
            data,
        }
    }

    fn lift() -> Vec<CurvePoint> {
        vec![
            CurvePoint { x: 0.0, y: 0.12 },
            CurvePoint { x: 0.5, y: 0.55 },
            CurvePoint { x: 1.0, y: 1.0 },
        ]
    }

    /// ADR 0098 §1: no channel curve, and v2 *is* v1.
    #[test]
    fn without_channel_curves_v2_matches_v1() {
        let points = lift();
        let mut old = image(16, 8);
        let mut new = image(16, 8);
        super::super::v1::tone_curve(&mut old, &points);
        tone_curve(
            &mut new,
            &ToneCurve {
                points: points.clone(),
                ..ToneCurve::default()
            },
        );
        assert_eq!(old.data, new.data, "v2 must delegate, not reimplement");

        // An entirely empty curve is the identity and must not reach v1's
        // table builder, which needs two points (the stage's `active`
        // predicate spares it in the pipeline; a direct caller does not).
        let source = image(16, 8);
        let mut untouched = source.clone();
        tone_curve(&mut untouched, &ToneCurve::default());
        assert_eq!(source.data, untouched.data);
    }

    /// A curve on one channel moves that channel and leaves the others.
    #[test]
    fn a_channel_curve_moves_only_its_channel() {
        let source = image(16, 8);
        let mut lifted = source.clone();
        tone_curve(
            &mut lifted,
            &ToneCurve {
                blue: lift(),
                ..ToneCurve::default()
            },
        );
        let mut moved = [false; 3];
        for (i, (before, after)) in source.data.iter().zip(lifted.data.iter()).enumerate() {
            if (before - after).abs() > 1e-6 {
                moved[i % 3] = true;
            }
        }
        assert_eq!(
            moved,
            [false, false, true],
            "only the blue channel may move"
        );
    }

    /// ADR 0098 §2: the master runs first, so a channel curve reads what
    /// the master produced — composing the two by hand gives the same
    /// pixels as asking for both at once.
    #[test]
    fn the_master_runs_before_the_channel() {
        let master = vec![
            CurvePoint { x: 0.0, y: 0.0 },
            CurvePoint { x: 0.5, y: 0.35 },
            CurvePoint { x: 1.0, y: 1.0 },
        ];
        let both = ToneCurve {
            points: master.clone(),
            red: lift(),
            ..ToneCurve::default()
        };
        let mut at_once = image(16, 8);
        tone_curve(&mut at_once, &both);

        // By hand: the master alone (v1), then the red curve alone.
        let mut by_hand = image(16, 8);
        super::super::v1::tone_curve(&mut by_hand, &master);
        tone_curve(
            &mut by_hand,
            &ToneCurve {
                red: lift(),
                ..ToneCurve::default()
            },
        );
        for (a, b) in at_once.data.iter().zip(by_hand.data.iter()) {
            assert!(
                (a - b).abs() < 1e-5,
                "composition order differs: {a} vs {b}"
            );
        }
    }

    /// A highlight above display white survives, scaled rather than
    /// clamped onto the curve's endpoint (ADR 0044 §2).
    #[test]
    fn a_highlight_above_white_keeps_its_headroom() {
        let mut px = Pixels {
            width: 1,
            height: 1,
            // Well above display white in the working space.
            data: vec![8.0, 8.0, 8.0],
        };
        tone_curve(
            &mut px,
            &ToneCurve {
                blue: lift(),
                ..ToneCurve::default()
            },
        );
        assert!(
            px.data.iter().all(|&v| v > 1.0),
            "the highlight was crushed: {:?}",
            px.data
        );
    }
}
