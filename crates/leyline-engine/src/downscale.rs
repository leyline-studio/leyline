//! Reduction of a decoded image before it enters the develop pipeline
//! (ADR 0041).
//!
//! The preview path used to develop the whole decoded buffer and only then
//! shrink the result to the requested size class — for a 30 Mpx body that
//! is ~7.6 Mpx of pipeline work to display 0.7 Mpx. Shrinking *first* is
//! the same picture for ~10× less work, provided every pixel-denominated
//! radius downstream is scaled by the same factor (ADR 0041 §2, which the
//! caller is responsible for passing on).
//!
//! Export and print never come through here: they render at full
//! resolution, unchanged and bit-identical to before.

use leyline_raw::RawImage;

/// Box-filter reduction of `image` so its longest edge is at most
/// `max_edge`, plus the scale factor that was applied.
///
/// Returns the image untouched with a scale of `1.0` when it already fits —
/// callers can therefore run this unconditionally.
///
/// A box filter (the plain average of the source pixels each destination
/// pixel covers) is the right tool for pure reduction: it is the cheapest
/// filter that still integrates *every* source pixel, so it neither aliases
/// like nearest-neighbour nor invents detail. It is also exactly
/// reversible in intent — the average of a region is what a smaller sensor
/// photosite would have recorded.
pub(crate) fn downscale_to_fit(image: &RawImage, max_edge: u32) -> (RawImage, f32) {
    let longest = image.width.max(image.height);
    if longest <= max_edge || max_edge == 0 || image.width == 0 || image.height == 0 {
        return (image.clone(), 1.0);
    }
    // Round to nearest, and never to zero: a 1-pixel edge is degenerate but
    // still renderable, a 0-pixel one is not.
    let scale = f64::from(max_edge) / f64::from(longest);
    let width = ((f64::from(image.width) * scale).round() as u32).max(1);
    let height = ((f64::from(image.height) * scale).round() as u32).max(1);

    let data = match image.bits {
        16 => box_filter::<2>(image, width, height),
        _ => box_filter::<1>(image, width, height),
    };
    let scaled = RawImage {
        width,
        height,
        bits: image.bits,
        data,
    };
    // The *actual* factor, derived from the rounded dimensions rather than
    // the requested ratio: a downstream radius must match the buffer that
    // exists, not the one that was asked for.
    let applied = f64::from(width) / f64::from(image.width);
    (scaled, applied as f32)
}

/// Box-filters `image` down to `width`x`height`. `BYTES` is the sample
/// width: 1 for 8-bit, 2 for native-endian 16-bit
/// ([`RawImage::bits`]) — the averaging is done in `u32` either way, so
/// neither depth can overflow or lose a bit it would have kept.
fn box_filter<const BYTES: usize>(image: &RawImage, width: u32, height: u32) -> Vec<u8> {
    let (src_w, src_h) = (image.width as usize, image.height as usize);
    let (dst_w, dst_h) = (width as usize, height as usize);
    let mut out = vec![0u8; dst_w * dst_h * 3 * BYTES];

    let sample = |index: usize| -> u32 {
        match BYTES {
            2 => u32::from(u16::from_ne_bytes([
                image.data[index * 2],
                image.data[index * 2 + 1],
            ])),
            _ => u32::from(image.data[index]),
        }
    };

    for dy in 0..dst_h {
        // Source rows this destination row covers, as a half-open range.
        let y0 = dy * src_h / dst_h;
        let y1 = (((dy + 1) * src_h).div_ceil(dst_h)).min(src_h).max(y0 + 1);
        for dx in 0..dst_w {
            let x0 = dx * src_w / dst_w;
            let x1 = (((dx + 1) * src_w).div_ceil(dst_w)).min(src_w).max(x0 + 1);
            let count = ((y1 - y0) * (x1 - x0)) as u32;
            for channel in 0..3 {
                let mut total = 0u32;
                for sy in y0..y1 {
                    let row = sy * src_w;
                    for sx in x0..x1 {
                        total += sample((row + sx) * 3 + channel);
                    }
                }
                let value = total / count;
                let at = (dy * dst_w + dx) * 3 + channel;
                match BYTES {
                    2 => {
                        let bytes = (value as u16).to_ne_bytes();
                        out[at * 2] = bytes[0];
                        out[at * 2 + 1] = bytes[1];
                    }
                    _ => out[at] = value as u8,
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: u32, height: u32, fill: u8) -> RawImage {
        RawImage {
            width,
            height,
            bits: 8,
            data: vec![fill; (width * height * 3) as usize],
        }
    }

    #[test]
    fn an_image_already_within_the_limit_is_returned_untouched() {
        let source = image(100, 50, 7);
        let (out, scale) = downscale_to_fit(&source, 200);
        assert_eq!(out, source);
        assert_eq!(scale, 1.0);
    }

    #[test]
    fn the_longest_edge_lands_on_the_limit_and_the_aspect_ratio_holds() {
        let source = image(6744, 4502, 0);
        let (out, scale) = downscale_to_fit(&source, 1024);
        assert_eq!(out.width, 1024);
        // 4502 * 1024 / 6744 = 683.6 -> 684, the same rounding the preview
        // cache produced before this stage existed.
        assert_eq!(out.height, 684);
        assert!((scale - 1024.0 / 6744.0).abs() < 1e-6, "got {scale}");
        assert_eq!(out.data.len(), 1024 * 684 * 3);
    }

    #[test]
    fn a_flat_image_stays_flat_at_every_depth() {
        for bits in [8u8, 16] {
            let source = RawImage {
                width: 64,
                height: 32,
                bits,
                data: match bits {
                    16 => vec![0x77; 64 * 32 * 3 * 2],
                    _ => vec![0x77; 64 * 32 * 3],
                },
            };
            let (out, _) = downscale_to_fit(&source, 16);
            assert!(
                out.data.iter().all(|&v| v == 0x77),
                "{bits}-bit flat image should stay flat"
            );
        }
    }

    #[test]
    fn every_source_pixel_contributes_so_a_half_black_image_averages_to_grey() {
        // Two source pixels per destination pixel, one 0 and one 255: a
        // filter that dropped either would give 0 or 255, not 127.
        let mut data = Vec::new();
        for _y in 0..4 {
            for x in 0..4 {
                let v = if x % 2 == 0 { 0 } else { 254 };
                data.extend_from_slice(&[v, v, v]);
            }
        }
        let source = RawImage {
            width: 4,
            height: 4,
            bits: 8,
            data,
        };
        let (out, _) = downscale_to_fit(&source, 2);
        assert_eq!((out.width, out.height), (2, 2));
        assert!(
            out.data.iter().all(|&v| v == 127),
            "got {:?}",
            &out.data[..6]
        );
    }

    #[test]
    fn sixteen_bit_samples_keep_their_precision() {
        // 8-bit averaging would quantize these to the same byte; the
        // 16-bit path must keep them apart.
        let mut data = Vec::new();
        for i in 0..4u16 {
            let v = 30_000 + i * 100;
            for _ in 0..3 {
                data.extend_from_slice(&v.to_ne_bytes());
            }
        }
        let source = RawImage {
            width: 4,
            height: 1,
            bits: 16,
            data,
        };
        let (out, _) = downscale_to_fit(&source, 2);
        let first = u16::from_ne_bytes([out.data[0], out.data[1]]);
        let second = u16::from_ne_bytes([out.data[6], out.data[7]]);
        assert_eq!(first, 30_050);
        assert_eq!(second, 30_250);
    }

    /// The point of the whole exercise: developing a reduced image with
    /// scaled radii must land close to developing at full size and
    /// reducing afterwards. Close, not equal — reducing then blurring at
    /// σ·s is not the same operation as blurring at σ then reducing
    /// (ADR 0041 §2 says so outright). This pins "close" to a number so a
    /// future change to the scaling rule cannot quietly drift.
    #[test]
    fn a_proxy_render_tracks_a_full_render_reduced_afterwards() {
        use leyline_core::{NoiseReduction, Settings, Sharpening};

        // Structured content: flat fields would pass any filter.
        let (w, h) = (600u32, 400u32);
        let mut data = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                let checker = if ((x / 40) + (y / 40)) % 2 == 0 {
                    40
                } else {
                    200
                };
                let ramp = (x * 255 / w) as u8;
                data.extend_from_slice(&[checker, ramp, checker.wrapping_add(ramp)]);
            }
        }
        let image = leyline_raw::RawImage {
            width: w,
            height: h,
            bits: 8,
            data,
        };
        let settings = Settings {
            exposure: 0.3,
            contrast: 20,
            clarity: 30,
            texture: 25,
            dehaze: 20,
            noise_reduction: NoiseReduction {
                luminance: 30,
                color: 20,
            },
            sharpening: Sharpening {
                amount: 50,
                radius: 1.5,
            },
            ..Settings::default()
        };

        let edge = 150;
        let (proxy_image, scale) = downscale_to_fit(&image, edge);
        let proxy = crate::render::render_scaled(&proxy_image, &settings, None, None, scale)
            .expect("proxy render");
        let full = crate::render::render(&image, &settings, None, None).expect("full render");
        let full_reduced = leyline_preview::Rgb8::new(full.width, full.height, full.data)
            .unwrap()
            .scaled_to_fit(edge);

        assert_eq!(
            (proxy.width, proxy.height),
            (full_reduced.width(), full_reduced.height())
        );
        let proxy_px = proxy.data;
        let full_px = full_reduced.data();
        let mean = proxy_px
            .iter()
            .zip(full_px.iter())
            .map(|(a, b)| i32::from(*a).abs_diff(i32::from(*b)))
            .sum::<u32>() as f64
            / proxy_px.len() as f64;
        assert!(
            mean < 12.0,
            "mean absolute difference {mean:.2}/255 is too large"
        );
    }

    #[test]
    fn a_scale_of_one_is_reported_when_nothing_is_resized() {
        let (_, scale) = downscale_to_fit(&image(10, 10, 0), 0);
        assert_eq!(scale, 1.0, "a zero limit must not divide by zero");
    }
}
