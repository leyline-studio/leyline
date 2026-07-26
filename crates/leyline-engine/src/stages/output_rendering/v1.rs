//! Output rendering v1 (ADR 0044 §3) — rank 900, after every other stage.
//!
//! The pipeline's exit: it turns the working buffer — linear Rec. 2020,
//! unbounded above — into a display signal, gamma-encoded sRGB in [0, 1].
//! Two things happen, in this order.
//!
//! **The highlight roll-off.** Everything above white is headroom the
//! pipeline has been carrying since the decoder: a window a stop and a half
//! brighter than the wall next to it. A display cannot show it, so it has
//! to come back into range, and *how* is a photographic decision rather
//! than a technical one — which is why it is the one setting this stage
//! exposes ([`leyline_core::OutputRendering::highlight_rolloff`]). At 0 the
//! headroom is cut off at white, the way every version of this pipeline
//! before ADR 0044 did it. Turned up, a shoulder starts lower and bends the
//! headroom into the last stretch below white, so a blown sky comes back as
//! a bright sky.
//!
//! **The conversion to the output space.** Rec. 2020 → sRGB is where the
//! gamut finally narrows, and doing it here rather than at the decoder is
//! the whole point of ADR 0044: every operator upstream got to work on the
//! colors the sensor actually recorded. A color sRGB cannot hold arrives
//! negative and is clipped — once, at the end, instead of before the first
//! slider.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use leyline_color::{REC2020_TO_LINEAR_SRGB, apply_matrix};

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{display, par_rows};

/// Where the shoulder starts at `highlight_rolloff = 100`. Below this the
/// image is untouched at every setting, so the roll-off can never flatten
/// the midtones — it is a highlight control, not a tone mapper.
pub(crate) const LOWEST_KNEE: f32 = 0.5;

/// The brightest value the shoulder still maps below white, for a shoulder
/// starting at `knee`.
///
/// Not a free choice: `2 - knee` is the one that makes the shoulder's slope
/// at the knee exactly 1, so the curve joins the untouched part of the
/// range without a visible break. It also states the trade the roll-off
/// makes — the further down the knee, the more headroom is recovered, and
/// the further white itself has to come down to make room for it.
pub(crate) fn white_point(knee: f32) -> f32 {
    2.0 - knee
}

/// Maps `[knee, white_point(knee)]` onto `[knee, 1]` with a quadratic
/// ease-out, and cuts off above.
///
/// A shoulder that reaches white at a finite input, rather than
/// approaching it asymptotically: an asymptote would mean nothing in the
/// image ever *is* white, which on a paper-white subject reads as a grey
/// cast rather than as recovered highlights.
///
/// `knee = 1` is the degenerate case and means "no shoulder": the value is
/// cut off at white, exactly as every version of this pipeline did before
/// ADR 0044.
pub(crate) fn rolloff(value: f32, knee: f32) -> f32 {
    if value <= knee || knee >= 1.0 {
        return value.min(1.0);
    }
    let white = white_point(knee);
    if value >= white {
        return 1.0;
    }
    let t = (value - knee) / (white - knee);
    let eased = 1.0 - (1.0 - t) * (1.0 - t);
    knee + (1.0 - knee) * eased
}

/// The knee a slider value asks for: `0` cuts off at white, `100` starts
/// the shoulder at [`LOWEST_KNEE`].
pub(crate) fn knee(highlight_rolloff: i32) -> f32 {
    let t = f32::from(highlight_rolloff as i16).clamp(0.0, 100.0) / 100.0;
    1.0 - (1.0 - LOWEST_KNEE) * t
}

/// Renders the working buffer to a display signal: highlight roll-off in
/// linear light, then Rec. 2020 → sRGB, then the sRGB transfer function.
///
/// The roll-off runs per channel and *before* the matrix. Per channel
/// because that is what keeps a single blown channel from dragging the
/// other two down with it; before the matrix because the headroom is a
/// property of the light the sensor measured, not of the space it is being
/// shown in.
pub(crate) fn render_output(px: &mut Pixels, highlight_rolloff: i32) {
    let knee = knee(highlight_rolloff);
    par_rows(px, |row| {
        for rgb in row.chunks_exact_mut(3) {
            for sample in rgb.iter_mut() {
                *sample = rolloff(*sample, knee);
            }
            let srgb = apply_matrix(
                REC2020_TO_LINEAR_SRGB,
                [f64::from(rgb[0]), f64::from(rgb[1]), f64::from(rgb[2])],
            );
            for (sample, value) in rgb.iter_mut().zip(srgb) {
                *sample = display((value as f32).clamp(0.0, 1.0));
            }
        }
    });
}
