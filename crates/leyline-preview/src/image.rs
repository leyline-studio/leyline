//! The pixel buffer previews are built from, and its downscaling.
//!
//! Previews are always 8-bit RGB: the engine renders or converts to this
//! format before caching. Scaling is a plain box filter — every source pixel
//! contributes exactly once per destination pixel — so it is deterministic
//! and free of sampling artefacts when shrinking, which is the only
//! direction previews ever go.

use crate::PreviewError;

/// An owned, tightly packed, interleaved 8-bit RGB image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgb8 {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl Rgb8 {
    /// Wraps a pixel buffer; `data` must hold exactly
    /// `width * height * 3` samples and both dimensions must be non-zero.
    pub fn new(width: u32, height: u32, data: Vec<u8>) -> Result<Rgb8, PreviewError> {
        if width == 0 || height == 0 {
            return Err(PreviewError::InvalidImage(format!(
                "zero dimension: {width}x{height}"
            )));
        }
        let expected = width as usize * height as usize * 3;
        if data.len() != expected {
            return Err(PreviewError::InvalidImage(format!(
                "buffer holds {} bytes, {width}x{height} RGB needs {expected}",
                data.len()
            )));
        }
        Ok(Rgb8 {
            width,
            height,
            data,
        })
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Pixel data, `width * height * 3` samples.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Returns the image scaled so its longest edge is at most `max_edge`,
    /// preserving aspect ratio. Never upscales: an image already small
    /// enough is returned unchanged.
    pub fn scaled_to_fit(&self, max_edge: u32) -> Rgb8 {
        let longest = self.width.max(self.height);
        if longest <= max_edge {
            return self.clone();
        }
        let scale = |edge: u32| -> u32 {
            let rounded = (u64::from(edge) * u64::from(max_edge) + u64::from(longest) / 2)
                / u64::from(longest);
            (rounded as u32).max(1)
        };
        self.box_scaled(scale(self.width), scale(self.height))
    }

    /// Box-filter downscale to exactly `out_width` x `out_height`: each
    /// destination pixel is the average of its source rectangle.
    fn box_scaled(&self, out_width: u32, out_height: u32) -> Rgb8 {
        let (w, h) = (self.width as usize, self.height as usize);
        let (ow, oh) = (out_width as usize, out_height as usize);
        let mut data = Vec::with_capacity(ow * oh * 3);

        for y in 0..oh {
            let sy0 = y * h / oh;
            let sy1 = ((y + 1) * h).div_ceil(oh).max(sy0 + 1);
            for x in 0..ow {
                let sx0 = x * w / ow;
                let sx1 = ((x + 1) * w).div_ceil(ow).max(sx0 + 1);

                let mut sum = [0u64; 3];
                for sy in sy0..sy1 {
                    for sx in sx0..sx1 {
                        let offset = (sy * w + sx) * 3;
                        for (channel, total) in sum.iter_mut().enumerate() {
                            *total += u64::from(self.data[offset + channel]);
                        }
                    }
                }
                let count = ((sy1 - sy0) * (sx1 - sx0)) as u64;
                for total in sum {
                    data.push(((total + count / 2) / count) as u8);
                }
            }
        }

        Rgb8 {
            width: out_width,
            height: out_height,
            data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uniform(width: u32, height: u32, rgb: [u8; 3]) -> Rgb8 {
        let data = rgb.repeat(width as usize * height as usize).to_vec();
        Rgb8::new(width, height, data).unwrap()
    }

    #[test]
    fn new_validates_the_buffer() {
        assert!(matches!(
            Rgb8::new(0, 4, vec![]),
            Err(PreviewError::InvalidImage(_))
        ));
        assert!(matches!(
            Rgb8::new(2, 2, vec![0; 11]),
            Err(PreviewError::InvalidImage(_))
        ));
        assert!(Rgb8::new(2, 2, vec![0; 12]).is_ok());
    }

    #[test]
    fn scaling_preserves_aspect_ratio() {
        let scaled = uniform(6000, 4000, [10, 20, 30]).scaled_to_fit(256);
        assert_eq!((scaled.width(), scaled.height()), (256, 171));

        let portrait = uniform(4000, 6000, [0, 0, 0]).scaled_to_fit(256);
        assert_eq!((portrait.width(), portrait.height()), (171, 256));
    }

    #[test]
    fn scaling_never_upscales() {
        let image = uniform(200, 100, [1, 2, 3]);
        assert_eq!(image.scaled_to_fit(256), image);
        assert_eq!(image.scaled_to_fit(200), image);
    }

    #[test]
    fn extreme_ratios_keep_at_least_one_pixel() {
        let scaled = uniform(10_000, 2, [7, 7, 7]).scaled_to_fit(100);
        assert_eq!((scaled.width(), scaled.height()), (100, 1));
    }

    #[test]
    fn a_uniform_image_stays_uniform() {
        let scaled = uniform(999, 501, [10, 200, 45]).scaled_to_fit(64);
        assert!(scaled.data().chunks(3).all(|px| px == [10, 200, 45]));
    }

    #[test]
    fn averages_the_source_rectangle() {
        // Two pixels collapse into one: the result is their rounded mean.
        let image = Rgb8::new(2, 1, vec![0, 10, 255, 100, 20, 0]).unwrap();
        let scaled = image.scaled_to_fit(1);
        assert_eq!(scaled.data(), [50, 15, 128]);
    }
}
