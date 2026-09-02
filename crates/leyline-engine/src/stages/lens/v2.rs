//! Lens correction v2 — rank 20. Everything [`super::v1`] does, plus the
//! manual transverse chromatic aberration of ADR 0111.
//!
//! Distortion and vignetting are **v1's, called as they are**: this version
//! changes one thing, the per-channel resampling, and delegating the other
//! two is what proves it (the shape [ADR 0098](../../../../docs/adr/0098-per-channel-tone-curves.md)
//! used for `tone_curve::v2`).
//!
//! At `tca_red = tca_blue = 0` and with a Lensfun correction in hand, the
//! resampling below computes exactly v1's coordinates and therefore renders
//! v1's pixels, bit for bit. That is what makes the version bump safe rather
//! than merely legal (ADR 0096 §1's property, restated here).
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever.

use std::cell::RefCell;

use rayon::prelude::*;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::lens_bilinear_channel;

pub(crate) use super::v1::{devignette, undistort};

/// Corrects transverse chromatic aberration from a Lensfun calibration, from
/// two manually measured coefficients, or from both — in **one** resampling
/// pass whichever it is (ADR 0111 §4).
///
/// `correction` is the Lensfun map when a profile matched, `None` when the
/// lens is uncalibrated or the caller turned the Lensfun half off.
/// `red_pct`/`blue_pct` are percents of the radius (ADR 0111 §1): the output
/// pixel at radius `r` reads that channel at `r × (1 + pct / 100)`.
///
/// The manual magnification scales the coordinate Lensfun produced, not the
/// output coordinate, because it was measured in the source frame — the
/// composition order ADR 0111 §4 states.
pub(crate) fn correct_tca(
    px: &Pixels,
    correction: Option<&leyline_lens::Correction>,
    red_pct: f64,
    blue_pct: f64,
) -> Pixels {
    let lensfun = correction.filter(|c| c.tca_matched());
    let manual = red_pct != 0.0 || blue_pct != 0.0;
    if lensfun.is_none() && !manual {
        // Every channel would map to its own identity coordinate, and
        // resampling at an integer coordinate returns the pixel itself —
        // v1's reasoning, and v1's early return.
        return px.clone();
    }

    // The frame centre the radial model is written about: the midpoint of the
    // pixel grid, so a 2x2 image scales about (0.5, 0.5) and not about a
    // corner.
    let cx = (px.width as f32 - 1.0) / 2.0;
    let cy = (px.height as f32 - 1.0) / 2.0;
    let scales = [
        1.0 + red_pct as f32 / 100.0,
        1.0,
        1.0 + blue_pct as f32 / 100.0,
    ];

    // Same reuse-per-thread rationale as v1's scratch buffers.
    type TcaRowScratch = RefCell<(Vec<f32>, Vec<[(f32, f32); 3]>)>;
    thread_local! {
        static SCRATCH: TcaRowScratch = const { RefCell::new((Vec::new(), Vec::new())) };
    }
    let mut data = vec![0.0f32; px.data.len()];
    data.par_chunks_mut(px.width as usize * 3)
        .enumerate()
        .for_each(|(y, row)| {
            SCRATCH.with(|cell| {
                let (scratch, channels) = &mut *cell.borrow_mut();
                if let Some(correction) = lensfun {
                    correction.tca_row_into(y as u32, px.width, scratch, channels);
                }
                for (x, rgb_out) in row.chunks_exact_mut(3).enumerate() {
                    for (c, sample) in rgb_out.iter_mut().enumerate() {
                        // Lensfun's coordinate for this channel, or the
                        // identity when there is no calibration to ask.
                        let (sx, sy) = match lensfun {
                            Some(_) => channels[x][c],
                            None => (x as f32, y as f32),
                        };
                        let (sx, sy) = if scales[c] == 1.0 {
                            (sx, sy)
                        } else {
                            (cx + (sx - cx) * scales[c], cy + (sy - cy) * scales[c])
                        };
                        if let Some(value) = lens_bilinear_channel(px, sx, sy, c) {
                            *sample = value;
                        }
                    }
                }
            });
        });
    Pixels {
        width: px.width,
        height: px.height,
        data,
    }
}
