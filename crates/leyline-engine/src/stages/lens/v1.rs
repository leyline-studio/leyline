//! Lens correction v1 — rank 20. Distortion, TCA, vignetting.
//!
//! The three Lensfun-backed corrections of ADR 0016 (distortion), ADR 0017
//! (vignetting) and ADR 0018 (transverse chromatic aberration). TCA runs as
//! its own resampling pass, on the buffer `undistort` produced; both
//! geometric passes share one built [`leyline_lens::Correction`].
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use std::cell::RefCell;

use rayon::prelude::*;

use crate::pixels::Pixels;
use crate::stages::kernel::v1::{lens_bilinear, lens_bilinear_channel, lookup, tables};

/// Undistorts the image geometrically using an already-built [`leyline_lens::Correction`].
/// No distortion calibration at this focal length leaves `px` unchanged:
/// correction is only ever applied from real calibration data, never
/// approximated. `correction` is shared with [`correct_tca`] — one profile
/// match, one `Correction` built, both geometric passes reuse it.
pub(crate) fn undistort(px: &Pixels, correction: &leyline_lens::Correction) -> Pixels {
    if !correction.distortion_matched() {
        // No distortion calibration at this focal length: `source_row`
        // would return the identity map, and resampling at identity
        // coordinates is bit-identical to the original pixel (integer
        // coordinates make every bilinear weight exactly 0.0 or 1.0) — so
        // skip the whole per-pixel pass rather than pay for a no-op.
        return px.clone();
    }
    // Reused per render thread rather than allocated fresh per row: `source_row`
    // computes the same coordinates either way, this only spares the two Vec
    // allocations `source_row` would otherwise make on every one of the
    // image's rows.
    type SourceRowScratch = RefCell<(Vec<f32>, Vec<(f32, f32)>)>;
    thread_local! {
        static SCRATCH: SourceRowScratch = const { RefCell::new((Vec::new(), Vec::new())) };
    }
    let mut data = vec![0.0f32; px.data.len()];
    data.par_chunks_mut(px.width as usize * 3)
        .enumerate()
        .for_each(|(y, row)| {
            SCRATCH.with(|cell| {
                let (scratch, sources) = &mut *cell.borrow_mut();
                correction.source_row_into(y as u32, px.width, scratch, sources);
                for (x, rgb_out) in row.chunks_exact_mut(3).enumerate() {
                    let (sx, sy) = sources[x];
                    if let Some(rgb) = lens_bilinear(px, sx, sy) {
                        rgb_out.copy_from_slice(&rgb);
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

/// Corrects transverse chromatic aberration: each channel of each output
/// pixel is resampled independently from its own Lensfun-reported source
/// coordinate, on top of the buffer [`undistort`] already produced (see the
/// module docs for why this runs as a second independent pass rather than a
/// combined distortion+TCA remap). No TCA calibration at this focal length
/// leaves every channel's coordinate at the identity, so `px` comes back
/// unchanged.
pub(crate) fn correct_tca(px: &Pixels, correction: &leyline_lens::Correction) -> Pixels {
    if !correction.tca_matched() {
        // No TCA calibration at this focal length: `tca_row` would map
        // every channel to the same identity coordinate, and resampling at
        // an identity coordinate is bit-identical to the original pixel
        // (integer coordinates make every bilinear weight exactly 0.0 or
        // 1.0) — so skip the whole per-channel pass rather than pay for a
        // no-op that discards nothing new but still costs three independent
        // resamples per pixel.
        return px.clone();
    }
    // Same reuse-per-thread rationale as `undistort`'s scratch buffers above.
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
                correction.tca_row_into(y as u32, px.width, scratch, channels);
                for (x, rgb_out) in row.chunks_exact_mut(3).enumerate() {
                    for (c, &(sx, sy)) in channels[x].iter().enumerate() {
                        if let Some(value) = lens_bilinear_channel(px, sx, sy, c) {
                            rgb_out[c] = value;
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

/// Corrects corner darkening (vignetting) using an already-matched Lensfun
/// profile, `aperture_f` and [`leyline_lens::Vignetting`]'s assumed subject
/// distance. No vignetting calibration for this focal/aperture pair leaves
/// `px` unchanged. The gain is a radial multiplier defined in linear light
/// (a physical falloff of incoming light), so each sample round-trips
/// through the same transfer tables as [`crate::stages::gains::v1::linear_gains`] rather than being
/// multiplied directly in gamma space.
pub(crate) fn devignette(
    px: &mut Pixels,
    profile: &leyline_lens::Profile,
    focal_mm: f32,
    aperture_f: f32,
) {
    let vignetting =
        leyline_lens::Vignetting::new(profile, focal_mm, aperture_f, px.width, px.height);
    if !vignetting.matched() {
        return;
    }
    let (to_linear, to_srgb) = tables();
    let width = px.width;
    px.data
        .par_chunks_mut(width as usize * 3)
        .enumerate()
        .for_each(|(y, row)| {
            let gains = vignetting.gain_row(y as u32, width);
            for (x, rgb) in row.chunks_exact_mut(3).enumerate() {
                let gain = gains[x];
                for sample in rgb {
                    *sample = lookup(to_srgb, (lookup(to_linear, *sample) * gain).clamp(0.0, 1.0));
                }
            }
        });
}
