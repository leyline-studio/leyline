//! Levels v1 (ADR 0164) — rank 78, between the parametric curve and the
//! point curve.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use leyline_core::Levels;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{display_curve, in_display};

/// The scale the five numbers are written on (ADR 0164 §3) — a scale only:
/// the pipeline stays floating point.
const SCALE: f64 = 255.0;

/// Applies levels identically to every channel, on the display axis.
///
/// Evaluated per sample rather than through the curves' lookup table: a
/// gamma near 10 is steep enough at black that a table's first interval
/// would visibly flatten the deepest shadows, and this stage only runs when
/// someone set it.
pub(crate) fn levels(px: &mut Pixels, levels: &Levels) {
    let map = mapping(levels);
    in_display(px, |px| display_curve(px, map));
}

/// The levels transfer on `[0, 1]` (ADR 0164 §2).
fn mapping(levels: &Levels) -> impl Fn(f32) -> f32 + Send + Sync + use<> {
    let in_black = levels.input_black as f64 / SCALE;
    let in_range = (levels.input_white - levels.input_black) as f64 / SCALE;
    let out_black = levels.output_black as f64 / SCALE;
    let out_range = (levels.output_white - levels.output_black) as f64 / SCALE;
    let exponent = 1.0 / levels.gamma;
    move |x| {
        let t = ((f64::from(x) - in_black) / in_range).clamp(0.0, 1.0);
        let shaped = if exponent == 1.0 { t } else { t.powf(exponent) };
        (out_black + out_range * shaped) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn levels(input_black: i32, input_white: i32, gamma: f64, out: (i32, i32)) -> Levels {
        Levels {
            input_black,
            input_white,
            gamma,
            output_black: out.0,
            output_white: out.1,
        }
    }

    #[test]
    fn neutral_is_the_identity() {
        let map = mapping(&Levels::default());
        for x in [0.0, 0.1, 0.5, 0.9, 1.0] {
            assert!((map(x) - x).abs() < 1e-6, "{x}");
        }
    }

    #[test]
    fn the_points_land_where_they_are_named() {
        let map = mapping(&levels(51, 204, 1.0, (0, 255)));
        assert_eq!(map(0.1), 0.0); // below the black point
        assert_eq!(map(0.9), 1.0); // above the white point
        assert!((map(0.5) - 0.5).abs() < 1e-6);

        let map = mapping(&levels(0, 255, 1.0, (20, 235)));
        assert!((map(0.0) - 20.0 / 255.0).abs() < 1e-6);
        assert!((map(1.0) - 235.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn gamma_above_one_brightens_the_midtones() {
        let brighter = mapping(&levels(0, 255, 2.0, (0, 255)));
        let darker = mapping(&levels(0, 255, 0.5, (0, 255)));
        assert!(brighter(0.25) > 0.25);
        assert!(darker(0.25) < 0.25);
        assert!((brighter(0.25) - 0.5).abs() < 1e-6);
    }
}
