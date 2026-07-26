//! The engine's working pixel buffer and its conversions.
//!
//! Since ADR 0044 the develop pipeline operates on **linear Rec. 2020
//! samples, `f32`, floored at 0 and unbounded above**. Three properties of
//! that sentence each answer a defect of the buffer it replaced:
//!
//! * *Rec. 2020* — wide enough to hold what a sensor sees, so no color is
//!   thrown away before the first slider runs. The narrowing to the output
//!   space happens once, at the very end (`stages::output_rendering::v1`);
//! * *linear* — the operators that describe light (white balance, exposure,
//!   vignetting) and every geometric resampling are multiplications and
//!   weighted sums of actual light, which is the only way they compose
//!   correctly. Tone controls, which are statements about *perceived*
//!   lightness, declare the display axis explicitly
//!   (`stages::kernel::v1::in_display`);
//! * *unbounded above* — a highlight brighter than white survives the
//!   pipeline instead of being flattened at each step. It is what makes
//!   +1 EV followed by −1 EV give back the image it started from, and what
//!   the highlight roll-off has to work with at the end.

use leyline_core::{LeylineError, Result};
use leyline_raw::RawImage;
use rayon::prelude::*;

/// The working buffer of the develop pipeline: interleaved RGB, `f32`
/// samples, linear Rec. 2020, `>= 0` and unbounded above.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Pixels {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 3` samples.
    pub data: Vec<f32>,
}

impl Pixels {
    /// Normalizes a decoded image (8 or 16 bits per channel) to `f32` with
    /// white at 1.
    ///
    /// This is a scaling, not a conversion: what the samples *mean* — the
    /// decoder's own space and transfer function — is the `input` stage's
    /// business, and depends on the source (ADR 0044 §3).
    pub fn from_raw(image: &RawImage) -> Result<Pixels> {
        let samples = image.width as usize * image.height as usize * 3;
        if image.width == 0 || image.height == 0 {
            return Err(LeylineError::InvalidImage(format!(
                "zero dimension: {}x{}",
                image.width, image.height
            )));
        }
        let data = match image.bits {
            8 => {
                if image.data.len() != samples {
                    return Err(bad_length(image, samples));
                }
                image
                    .data
                    .par_iter()
                    .map(|&v| f32::from(v) / 255.0)
                    .collect()
            }
            16 => {
                if image.data.len() != samples * 2 {
                    return Err(bad_length(image, samples * 2));
                }
                image
                    .data
                    .par_chunks_exact(2)
                    .map(|pair| {
                        let v = u16::from_ne_bytes([pair[0], pair[1]]);
                        f32::from(v) / 65535.0
                    })
                    .collect()
            }
            bits => {
                return Err(LeylineError::InvalidImage(format!(
                    "unsupported bit depth: {bits}"
                )));
            }
        };
        Ok(Pixels {
            width: image.width,
            height: image.height,
            data,
        })
    }

    /// Quantizes to tightly packed 8-bit RGB.
    ///
    /// Only a quantization: `output_rendering` has already brought the
    /// buffer into the output space and encoded it (ADR 0044 §3), so the
    /// clamp here catches rounding, not headroom.
    pub fn to_rgb8(&self) -> Vec<u8> {
        self.data
            .par_iter()
            .map(|&v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect()
    }
}

fn bad_length(image: &RawImage, expected: usize) -> LeylineError {
    LeylineError::InvalidImage(format!(
        "buffer holds {} bytes, {}x{} RGB at {} bits needs {expected}",
        image.data.len(),
        image.width,
        image.height,
        image.bits
    ))
}

/// Luma of an RGB triple in the working space, with Rec. 2020's own
/// coefficients — the working space's primaries, so the weights match the
/// green the buffer actually holds rather than sRGB's.
pub(crate) fn luma(rgb: &[f32]) -> f32 {
    0.2627 * rgb[0] + 0.6780 * rgb[1] + 0.0593 * rgb[2]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw8(width: u32, height: u32, data: Vec<u8>) -> RawImage {
        RawImage {
            width,
            height,
            bits: 8,
            data,
        }
    }

    #[test]
    fn eight_bit_round_trips_exactly() {
        let bytes: Vec<u8> = (0..=255).cycle().take(2 * 3 * 3).collect();
        let px = Pixels::from_raw(&raw8(2, 3, bytes.clone())).unwrap();
        assert_eq!(px.to_rgb8(), bytes);
    }

    #[test]
    fn sixteen_bit_maps_onto_the_full_range() {
        let mut data = Vec::new();
        for v in [0u16, 32768, 65535] {
            for _ in 0..3 {
                data.extend_from_slice(&v.to_ne_bytes());
            }
        }
        let px = Pixels::from_raw(&RawImage {
            width: 3,
            height: 1,
            bits: 16,
            data,
        })
        .unwrap();
        assert_eq!(px.to_rgb8(), [0, 0, 0, 128, 128, 128, 255, 255, 255]);
    }

    #[test]
    fn rejects_malformed_buffers() {
        for image in [
            raw8(2, 2, vec![0; 11]),
            raw8(0, 4, vec![]),
            RawImage {
                width: 1,
                height: 1,
                bits: 12,
                data: vec![0; 6],
            },
        ] {
            assert!(matches!(
                Pixels::from_raw(&image),
                Err(LeylineError::InvalidImage(_))
            ));
        }
    }
}
