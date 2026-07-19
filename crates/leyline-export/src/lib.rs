//! Image export: encode rendered pixels to files (`docs/engine-api.md` §12).
//!
//! This crate only encodes: it receives finished 8-bit RGB pixels and
//! writes them in the requested format. Rendering, scaling and catalog
//! bookkeeping belong to the engine. V1 covers JPEG, PNG, TIFF and
//! (lossless) WebP; AVIF is a planned addition to [`ExportFormat`].
//!
//! [`ExportSettings`] doubles as the `settings_json` of export presets
//! (`docs/catalog.md` §27), so a preset is exactly a named, stored instance
//! of what this crate consumes.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Errors produced while encoding an export file.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    /// The output file could not be written.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    /// The pixel buffer does not describe a usable image.
    #[error("invalid image: {0}")]
    InvalidImage(String),
    /// The settings document is malformed or from a newer engine.
    #[error("invalid export settings: {0}")]
    InvalidSettings(String),
    /// The encoder itself failed.
    #[error("encoding failed: {0}")]
    Encode(String),
}

/// Output file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    /// JPEG, quality-controlled, no alpha.
    Jpeg,
    /// PNG, lossless.
    Png,
    /// TIFF, lossless, deflate-compressed.
    Tiff,
    /// WebP, lossless.
    Webp,
}

impl ExportFormat {
    /// The conventional file extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Jpeg => "jpg",
            ExportFormat::Png => "png",
            ExportFormat::Tiff => "tif",
            ExportFormat::Webp => "webp",
        }
    }
}

/// One export recipe — and the `settings_json` of a §27 preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExportSettings {
    /// Output format.
    pub format: ExportFormat,
    /// JPEG quality in [1, 100]; ignored by lossless formats.
    pub quality: u8,
    /// Scale so the longest edge fits this, never upscaling; `None` = full
    /// resolution. The engine applies it before encoding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_edge: Option<u32>,
}

impl Default for ExportSettings {
    /// Full-resolution JPEG at quality 90.
    fn default() -> Self {
        ExportSettings {
            format: ExportFormat::Jpeg,
            quality: 90,
            max_edge: None,
        }
    }
}

impl ExportSettings {
    /// Parses a preset's `settings_json`.
    ///
    /// Unknown fields are refused: a preset written by a newer engine must
    /// not be applied partially (the philosophy of `docs/pipeline.md` §3.4).
    pub fn parse(json: &str) -> Result<ExportSettings, ExportError> {
        let settings: ExportSettings =
            serde_json::from_str(json).map_err(|e| ExportError::InvalidSettings(e.to_string()))?;
        settings.validate()?;
        Ok(settings)
    }

    /// Serializes the recipe to its `settings_json` form.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("settings serialization cannot fail")
    }

    /// Validates value ranges.
    pub fn validate(&self) -> Result<(), ExportError> {
        if !(1..=100).contains(&self.quality) {
            return Err(ExportError::InvalidSettings(format!(
                "quality must be in [1, 100], got {}",
                self.quality
            )));
        }
        if self.max_edge == Some(0) {
            return Err(ExportError::InvalidSettings(
                "max_edge must be strictly positive".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Encodes tightly packed 8-bit interleaved RGB pixels to `path`.
///
/// The pixels are written as-is: any `max_edge` scaling already happened.
/// The parent directory must exist; an existing file is replaced.
pub fn encode(
    path: &Path,
    width: u32,
    height: u32,
    rgb8: &[u8],
    settings: &ExportSettings,
) -> Result<(), ExportError> {
    settings.validate()?;
    let expected = width as usize * height as usize * 3;
    if width == 0 || height == 0 || rgb8.len() != expected {
        return Err(ExportError::InvalidImage(format!(
            "{width}x{height} RGB needs {expected} samples, got {}",
            rgb8.len()
        )));
    }

    match settings.format {
        ExportFormat::Jpeg => {
            let (Ok(w), Ok(h)) = (u16::try_from(width), u16::try_from(height)) else {
                return Err(ExportError::InvalidImage(format!(
                    "JPEG cannot exceed 65535 pixels per edge, got {width}x{height}"
                )));
            };
            let encoder = jpeg_encoder::Encoder::new_file(path, settings.quality)
                .map_err(|e| ExportError::Encode(e.to_string()))?;
            encoder
                .encode(rgb8, w, h, jpeg_encoder::ColorType::Rgb)
                .map_err(|e| ExportError::Encode(e.to_string()))?;
        }
        ExportFormat::Png => {
            let file = std::fs::File::create(path)?;
            let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder
                .write_header()
                .map_err(|e| ExportError::Encode(e.to_string()))?;
            writer
                .write_image_data(rgb8)
                .map_err(|e| ExportError::Encode(e.to_string()))?;
        }
        ExportFormat::Tiff => {
            let file = std::fs::File::create(path)?;
            let mut encoder = tiff::encoder::TiffEncoder::new(std::io::BufWriter::new(file))
                .map_err(|e| ExportError::Encode(e.to_string()))?
                .with_compression(tiff::encoder::Compression::Deflate(
                    tiff::encoder::DeflateLevel::default(),
                ));
            encoder
                .write_image::<tiff::encoder::colortype::RGB8>(width, height, rgb8)
                .map_err(|e| ExportError::Encode(e.to_string()))?;
        }
        ExportFormat::Webp => {
            let file = std::fs::File::create(path)?;
            image_webp::WebPEncoder::new(std::io::BufWriter::new(file))
                .encode(rgb8, width, height, image_webp::ColorType::Rgb8)
                .map_err(|e| ExportError::Encode(e.to_string()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(width: u32, height: u32) -> Vec<u8> {
        let mut data = Vec::new();
        for y in 0..height {
            for x in 0..width {
                data.extend_from_slice(&[(x * 20) as u8, (y * 20) as u8, 128]);
            }
        }
        data
    }

    #[test]
    fn encodes_jpeg_and_png_signatures() {
        let dir = tempfile::tempdir().unwrap();
        let pixels = gradient(8, 6);

        let jpg = dir.path().join("out.jpg");
        encode(&jpg, 8, 6, &pixels, &ExportSettings::default()).unwrap();
        let bytes = std::fs::read(&jpg).unwrap();
        assert_eq!(&bytes[..3], &[0xFF, 0xD8, 0xFF], "JPEG SOI marker");

        let png_path = dir.path().join("out.png");
        let png_settings = ExportSettings {
            format: ExportFormat::Png,
            ..ExportSettings::default()
        };
        encode(&png_path, 8, 6, &pixels, &png_settings).unwrap();
        let bytes = std::fs::read(&png_path).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "PNG signature");

        let tif = dir.path().join("out.tif");
        let tiff_settings = ExportSettings {
            format: ExportFormat::Tiff,
            ..ExportSettings::default()
        };
        encode(&tif, 8, 6, &pixels, &tiff_settings).unwrap();
        let bytes = std::fs::read(&tif).unwrap();
        assert_eq!(&bytes[..4], b"II*\0", "little-endian TIFF header");

        let webp = dir.path().join("out.webp");
        let webp_settings = ExportSettings {
            format: ExportFormat::Webp,
            ..ExportSettings::default()
        };
        encode(&webp, 8, 6, &pixels, &webp_settings).unwrap();
        let bytes = std::fs::read(&webp).unwrap();
        assert_eq!(&bytes[..4], b"RIFF", "WebP RIFF container");
        assert_eq!(&bytes[8..12], b"WEBP", "WebP fourcc");
    }

    #[test]
    fn rejects_malformed_buffers_and_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.jpg");

        assert!(matches!(
            encode(&path, 8, 6, &[0u8; 10], &ExportSettings::default()),
            Err(ExportError::InvalidImage(_))
        ));
        assert!(matches!(
            encode(
                &path,
                8,
                6,
                &gradient(8, 6),
                &ExportSettings {
                    quality: 0,
                    ..ExportSettings::default()
                }
            ),
            Err(ExportError::InvalidSettings(_))
        ));
    }

    #[test]
    fn settings_round_trip_and_refuse_newer_fields() {
        let settings = ExportSettings {
            format: ExportFormat::Png,
            quality: 80,
            max_edge: Some(2048),
        };
        assert_eq!(
            ExportSettings::parse(&settings.to_json()).unwrap(),
            settings
        );
        assert_eq!(
            ExportSettings::parse(r#"{"format":"jpeg"}"#).unwrap(),
            ExportSettings::default()
        );

        // §3.4 philosophy: a newer preset is refused, never applied partially.
        assert!(matches!(
            ExportSettings::parse(r#"{"format":"jpeg","watermark":"logo.png"}"#),
            Err(ExportError::InvalidSettings(_))
        ));
        assert!(matches!(
            ExportSettings::parse(r#"{"format":"avif"}"#),
            Err(ExportError::InvalidSettings(_))
        ));
    }
}
