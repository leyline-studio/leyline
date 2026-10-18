//! Image export: encode rendered pixels to files (`docs/engine-api.md` §12).
//!
//! This crate only encodes: it receives finished 8-bit RGB pixels and
//! writes them in the requested format. Rendering, scaling and catalog
//! bookkeeping belong to the engine. V1 covers the `specification.md`
//! formats — JPEG, TIFF, (lossless) WebP, AVIF — plus lossless PNG.
//!
//! [`ExportSettings`] doubles as the `settings_json` of export presets
//! (`docs/catalog.md` §27), so a preset is exactly a named, stored instance
//! of what this crate consumes.
//!
//! The pixels arrive already in sRGB (`docs/adr/0015-color-management-srgb.md`).
//! JPEG, PNG, TIFF and WebP get an explicit sRGB ICC profile embedded via
//! [`leyline_color::srgb_icc_profile`] so color-managed viewers render them
//! correctly instead of relying on convention; AVIF is written without one
//! because its encoder offers no way to add it.
//!
//! What the file says about the photograph — capture, authorship, keywords,
//! location — is ADR 0157's: the recipe's [`ExportMetadata`] decides how much
//! of the engine's [`PhotoMetadata`] each format carries.

// ADR 0152: no panic on a production path.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]
#![forbid(unsafe_code)]

use std::path::Path;

use serde::{Deserialize, Serialize};

mod contact_sheet;
mod metadata;
mod print;
mod watermark;
pub use contact_sheet::{
    CaptionSource, ContactSheetSettings, SheetCell, compose_page, encode_contact_sheet,
};
pub use metadata::{CaptureTime, ExportMetadata, GpsPosition, PhotoMetadata, Provenance, Ratio};
pub use print::{Margins, Orientation, PaperSize, PrintSettings, encode_print};
pub use watermark::{Watermark, WatermarkAnchor, WatermarkFont};

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
    /// The metadata does not fit the one segment JPEG gives it — thousands
    /// of keywords. Refused rather than truncated: losing keywords without a
    /// word is worse than an export that says why it stopped (ADR 0157 §5).
    #[error("metadata too large for a JPEG segment: {bytes} bytes, at most {limit}")]
    MetadataTooLarge {
        /// The segment's size.
        bytes: usize,
        /// The most a segment holds.
        limit: usize,
    },
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
    /// AVIF, quality-controlled.
    Avif,
}

impl ExportFormat {
    /// The conventional file extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Jpeg => "jpg",
            ExportFormat::Png => "png",
            ExportFormat::Tiff => "tif",
            ExportFormat::Webp => "webp",
            ExportFormat::Avif => "avif",
        }
    }

    /// Whether files of this format carry the metadata of ADR 0157. AVIF
    /// does not, because its encoder offers no way to; the clients say so
    /// rather than let a policy look applied (ADR 0157 §5).
    pub fn carries_metadata(self) -> bool {
        !matches!(self, ExportFormat::Avif)
    }
}

/// One export recipe — and the `settings_json` of a §27 preset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExportSettings {
    /// Output format.
    pub format: ExportFormat,
    /// JPEG/AVIF quality in [1, 100]; ignored by lossless formats.
    pub quality: u8,
    /// AVIF encoder effort in [1, 10], low being slow and thorough; ignored
    /// by every other format (ADR 0067).
    ///
    /// It trades encoding time against compression efficiency, never against
    /// the image: the quality target stays `quality`. Always serialized, so a
    /// preset written today keeps producing the same file when the default
    /// below moves again.
    pub avif_speed: u8,
    /// Scale so the longest edge fits this, never upscaling; `None` = full
    /// resolution. The engine applies it before encoding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_edge: Option<u32>,
    /// Text watermark composited last, immediately before encoding; `None` =
    /// none (ADR 0034, ADR 0051).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watermark: Option<Watermark>,
    /// What the file says about the photograph (ADR 0157 §1). Always
    /// serialized; a recipe written before the field existed reads as
    /// [`ExportMetadata::None`], so it keeps producing the same bytes.
    #[serde(default = "ExportMetadata::before_adr_0157")]
    pub metadata: ExportMetadata,
}

/// The AVIF effort the encoder uses unless a recipe says otherwise.
///
/// 9 rather than the 6 this crate hard-coded until ADR 0067: over a whole
/// export, 9 gave back 28 % of the time and 53 % of the CPU for 1.3 % of file
/// size. 10 is faster still but costs 0 to 14 % of size depending on the
/// image — a bet the caller takes explicitly, not one the default takes for
/// them.
pub const DEFAULT_AVIF_SPEED: u8 = 9;

impl Default for ExportSettings {
    /// Full-resolution JPEG at quality 90, without the location.
    fn default() -> Self {
        ExportSettings {
            format: ExportFormat::Jpeg,
            quality: 90,
            avif_speed: DEFAULT_AVIF_SPEED,
            max_edge: None,
            watermark: None,
            metadata: ExportMetadata::WithoutLocation,
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
    #[allow(
        clippy::expect_used,
        reason = "serde_json only fails on a failing Serialize impl or a non-string map key, and the recipe has neither"
    )]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("settings serialization cannot fail")
    }

    /// Validates value ranges.
    pub fn validate(&self) -> Result<(), ExportError> {
        if let Some(watermark) = &self.watermark {
            watermark.validate()?;
        }
        if !(1..=100).contains(&self.quality) {
            return Err(ExportError::InvalidSettings(format!(
                "quality must be in [1, 100], got {}",
                self.quality
            )));
        }
        if !(1..=10).contains(&self.avif_speed) {
            return Err(ExportError::InvalidSettings(format!(
                "avif_speed must be in [1, 10], got {}",
                self.avif_speed
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

/// Encodes tightly packed 8-bit interleaved RGB pixels to `path`, with what
/// `settings.metadata` lets `photo` say about them.
///
/// The pixels are written as-is: any `max_edge` scaling already happened.
/// The parent directory must exist; an existing file is replaced.
pub fn encode(
    path: &Path,
    width: u32,
    height: u32,
    rgb8: &[u8],
    settings: &ExportSettings,
    photo: &PhotoMetadata,
) -> Result<(), ExportError> {
    settings.validate()?;
    let expected = width as usize * height as usize * 3;
    if width == 0 || height == 0 || rgb8.len() != expected {
        return Err(ExportError::InvalidImage(format!(
            "{width}x{height} RGB needs {expected} samples, got {}",
            rgb8.len()
        )));
    }

    // The watermark is the last thing that touches the pixels (ADR 0034), and
    // it touches a copy: `rgb8` is the rendering of a revision, and a
    // decoration must not be burned into the caller's buffer (ADR 0051 §3).
    let watermarked;
    let rgb8 = match &settings.watermark {
        Some(mark) => {
            let mut pixels = rgb8.to_vec();
            watermark::draw(width, height, &mut pixels, mark)?;
            watermarked = pixels;
            watermarked.as_slice()
        }
        None => rgb8,
    };

    // Both blocks are built before the file is created, so an oversized one
    // refuses the export without leaving half a file behind.
    let written = photo.under(settings.metadata);
    let exif = written.as_ref().map(metadata::exif_block::exif_block);
    let xmp = written.as_ref().map(metadata::xmp_packet::xmp_packet);

    match settings.format {
        ExportFormat::Jpeg => {
            let (Ok(w), Ok(h)) = (u16::try_from(width), u16::try_from(height)) else {
                return Err(ExportError::InvalidImage(format!(
                    "JPEG cannot exceed 65535 pixels per edge, got {width}x{height}"
                )));
            };
            // EXIF first: readers look for it right after the start marker.
            let segments: Vec<Vec<u8>> = [
                exif.map(|block| [metadata::exif_block::JPEG_PREFIX, &block].concat()),
                xmp.map(|packet| {
                    [metadata::xmp_packet::JPEG_NAMESPACE, packet.as_bytes()].concat()
                }),
            ]
            .into_iter()
            .flatten()
            .collect();
            for segment in &segments {
                if segment.len() > JPEG_SEGMENT_LIMIT {
                    return Err(ExportError::MetadataTooLarge {
                        bytes: segment.len(),
                        limit: JPEG_SEGMENT_LIMIT,
                    });
                }
            }
            let mut encoder = jpeg_encoder::Encoder::new_file(path, settings.quality)
                .map_err(|e| ExportError::Encode(e.to_string()))?;
            for segment in &segments {
                encoder
                    .add_app_segment(1, segment)
                    .map_err(|e| ExportError::Encode(e.to_string()))?;
            }
            encoder
                .add_icc_profile(leyline_color::srgb_icc_profile())
                .map_err(|e| ExportError::Encode(e.to_string()))?;
            encoder
                .encode(rgb8, w, h, jpeg_encoder::ColorType::Rgb)
                .map_err(|e| ExportError::Encode(e.to_string()))?;
        }
        ExportFormat::Png => {
            let file = std::fs::File::create(path)?;
            let mut info = png::Info::with_size(width, height);
            info.color_type = png::ColorType::Rgb;
            info.bit_depth = png::BitDepth::Eight;
            info.icc_profile = Some(leyline_color::srgb_icc_profile().into());
            info.exif_metadata = exif.map(Into::into);
            let mut encoder = png::Encoder::with_info(std::io::BufWriter::new(file), info)
                .map_err(|e| ExportError::Encode(e.to_string()))?;
            if let Some(packet) = xmp {
                encoder
                    .add_itxt_chunk(metadata::xmp_packet::PNG_KEYWORD.to_owned(), packet)
                    .map_err(|e| ExportError::Encode(e.to_string()))?;
            }
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
            let mut image = encoder
                .new_image::<tiff::encoder::colortype::RGB8>(width, height)
                .map_err(|e| ExportError::Encode(e.to_string()))?;
            image
                .encoder()
                .write_tag(
                    tiff::tags::Tag::IccProfile,
                    leyline_color::srgb_icc_profile(),
                )
                .map_err(|e| ExportError::Encode(e.to_string()))?;
            if let Some(photo) = &written {
                write_tiff_tags(image.encoder(), photo, xmp.as_deref())?;
            }
            image
                .write_data(rgb8)
                .map_err(|e| ExportError::Encode(e.to_string()))?;
        }
        ExportFormat::Webp => {
            let file = std::fs::File::create(path)?;
            let mut encoder = image_webp::WebPEncoder::new(std::io::BufWriter::new(file));
            encoder.set_icc_profile(leyline_color::srgb_icc_profile().to_vec());
            if let Some(block) = exif {
                encoder.set_exif_metadata(block);
            }
            if let Some(packet) = xmp {
                encoder.set_xmp_metadata(packet.into_bytes());
            }
            encoder
                .encode(rgb8, width, height, image_webp::ColorType::Rgb8)
                .map_err(|e| ExportError::Encode(e.to_string()))?;
        }
        ExportFormat::Avif => {
            use rgb::FromSlice;
            let image = ravif::Img::new(rgb8.as_rgb(), width as usize, height as usize);
            let encoded = ravif::Encoder::new()
                .with_quality(f32::from(settings.quality))
                .with_speed(settings.avif_speed)
                .encode_rgb(image)
                .map_err(|e| ExportError::Encode(e.to_string()))?;
            std::fs::write(path, encoded.avif_file)?;
        }
    }
    Ok(())
}

/// The most one JPEG APP1 segment holds, its two length bytes aside.
const JPEG_SEGMENT_LIMIT: usize = 65_533;

/// TIFF carries its metadata in IFD 0 itself: the baseline tags, and the
/// whole XMP packet as tag 700 (ADR 0157 §5).
fn write_tiff_tags<W: std::io::Write + std::io::Seek, K: tiff::encoder::TiffKind>(
    ifd: &mut tiff::encoder::DirectoryEncoder<'_, W, K>,
    photo: &PhotoMetadata,
    xmp: Option<&str>,
) -> Result<(), ExportError> {
    use tiff::tags::Tag;
    let failed = |e: tiff::TiffError| ExportError::Encode(e.to_string());
    for (tag, value) in [
        (Tag::Make, &photo.make),
        (Tag::Model, &photo.model),
        (Tag::Artist, &photo.creator),
        (Tag::Copyright, &photo.copyright),
    ] {
        // The encoder refuses anything but ASCII in an ASCII tag. A name with
        // an accent is then left to the XMP packet below, which carries every
        // field in UTF-8 — absent from one carrier, never mangled in it.
        if let Some(value) = value
            .as_deref()
            .filter(|v| v.is_ascii() && !v.contains('\0'))
        {
            ifd.write_tag(tag, value).map_err(failed)?;
        }
    }
    // Baseline TIFF has no capture date of its own; `DateTime` is the one
    // date it has, and writing the export's clock there is what ADR 0157 §3
    // refuses — so it holds the capture.
    if let Some(time) = &photo.captured {
        ifd.write_tag(
            Tag::DateTime,
            metadata::exif_block::exif_date(time).as_str(),
        )
        .map_err(failed)?;
    }
    ifd.write_tag(Tag::Orientation, 1u16).map_err(failed)?;
    ifd.write_tag(Tag::Software, metadata::SOFTWARE)
        .map_err(failed)?;
    if let Some(packet) = xmp {
        ifd.write_tag(Tag::Unknown(700), packet.as_bytes())
            .map_err(failed)?;
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
    fn every_format_writes_its_file_signature() {
        let dir = tempfile::tempdir().unwrap();
        let pixels = gradient(8, 6);

        let jpg = dir.path().join("out.jpg");
        encode(
            &jpg,
            8,
            6,
            &pixels,
            &ExportSettings::default(),
            &PhotoMetadata::default(),
        )
        .unwrap();
        let bytes = std::fs::read(&jpg).unwrap();
        assert_eq!(&bytes[..3], &[0xFF, 0xD8, 0xFF], "JPEG SOI marker");

        let png_path = dir.path().join("out.png");
        let png_settings = ExportSettings {
            format: ExportFormat::Png,
            ..ExportSettings::default()
        };
        encode(
            &png_path,
            8,
            6,
            &pixels,
            &png_settings,
            &PhotoMetadata::default(),
        )
        .unwrap();
        let bytes = std::fs::read(&png_path).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "PNG signature");

        let tif = dir.path().join("out.tif");
        let tiff_settings = ExportSettings {
            format: ExportFormat::Tiff,
            ..ExportSettings::default()
        };
        encode(
            &tif,
            8,
            6,
            &pixels,
            &tiff_settings,
            &PhotoMetadata::default(),
        )
        .unwrap();
        let bytes = std::fs::read(&tif).unwrap();
        assert_eq!(&bytes[..4], b"II*\0", "little-endian TIFF header");

        let webp = dir.path().join("out.webp");
        let webp_settings = ExportSettings {
            format: ExportFormat::Webp,
            ..ExportSettings::default()
        };
        encode(
            &webp,
            8,
            6,
            &pixels,
            &webp_settings,
            &PhotoMetadata::default(),
        )
        .unwrap();
        let bytes = std::fs::read(&webp).unwrap();
        assert_eq!(&bytes[..4], b"RIFF", "WebP RIFF container");
        assert_eq!(&bytes[8..12], b"WEBP", "WebP fourcc");

        let avif = dir.path().join("out.avif");
        let avif_settings = ExportSettings {
            format: ExportFormat::Avif,
            ..ExportSettings::default()
        };
        encode(
            &avif,
            8,
            6,
            &pixels,
            &avif_settings,
            &PhotoMetadata::default(),
        )
        .unwrap();
        let bytes = std::fs::read(&avif).unwrap();
        assert_eq!(&bytes[4..8], b"ftyp", "ISOBMFF ftyp box");
        assert_eq!(&bytes[8..12], b"avif", "AVIF brand");
    }

    #[test]
    fn jpeg_png_and_tiff_embed_the_srgb_icc_profile() {
        let dir = tempfile::tempdir().unwrap();
        let pixels = gradient(8, 6);
        let icc = leyline_color::srgb_icc_profile();

        let jpg = dir.path().join("out.jpg");
        encode(
            &jpg,
            8,
            6,
            &pixels,
            &ExportSettings::default(),
            &PhotoMetadata::default(),
        )
        .unwrap();
        let bytes = std::fs::read(&jpg).unwrap();
        assert!(
            bytes.windows(icc.len()).any(|w| w == icc),
            "JPEG file should contain the ICC_PROFILE APP2 segment payload"
        );

        let png_path = dir.path().join("out.png");
        let png_settings = ExportSettings {
            format: ExportFormat::Png,
            ..ExportSettings::default()
        };
        encode(
            &png_path,
            8,
            6,
            &pixels,
            &png_settings,
            &PhotoMetadata::default(),
        )
        .unwrap();
        let file = std::io::BufReader::new(std::fs::File::open(&png_path).unwrap());
        let reader = png::Decoder::new(file).read_info().unwrap();
        assert_eq!(
            reader.info().icc_profile.as_deref(),
            Some(icc),
            "PNG iCCP chunk should round-trip the sRGB profile"
        );

        let tif = dir.path().join("out.tif");
        let tiff_settings = ExportSettings {
            format: ExportFormat::Tiff,
            ..ExportSettings::default()
        };
        encode(
            &tif,
            8,
            6,
            &pixels,
            &tiff_settings,
            &PhotoMetadata::default(),
        )
        .unwrap();
        let bytes = std::fs::read(&tif).unwrap();
        assert!(
            bytes.windows(icc.len()).any(|w| w == icc),
            "TIFF file should contain the ICCProfile tag payload"
        );
    }

    #[test]
    fn rejects_malformed_buffers_and_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.jpg");

        assert!(matches!(
            encode(
                &path,
                8,
                6,
                &[0u8; 10],
                &ExportSettings::default(),
                &PhotoMetadata::default()
            ),
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
                },
                &PhotoMetadata::default(),
            ),
            Err(ExportError::InvalidSettings(_))
        ));
    }

    #[test]
    fn settings_round_trip_and_refuse_newer_fields() {
        let settings = ExportSettings {
            format: ExportFormat::Png,
            quality: 80,
            avif_speed: 4,
            max_edge: Some(2048),
            watermark: Some(Watermark {
                text: "© 2026".to_owned(),
                ..Watermark::default()
            }),
            metadata: ExportMetadata::Full,
        };
        assert_eq!(
            ExportSettings::parse(&settings.to_json()).unwrap(),
            settings
        );
        // A recipe from before ADR 0157 keeps writing no metadata, whatever
        // a new export starts at.
        assert_eq!(
            ExportSettings::parse(r#"{"format":"jpeg"}"#).unwrap(),
            ExportSettings {
                metadata: ExportMetadata::None,
                ..ExportSettings::default()
            }
        );
        assert!(
            ExportSettings::default()
                .to_json()
                .contains(r#""metadata":"without-location""#)
        );

        // A watermark is a known field now (ADR 0051), and an object: the
        // string form ADR 0034 used as its example of an unknown field is
        // still refused, as is a field this engine has never heard of.
        assert!(matches!(
            ExportSettings::parse(r#"{"format":"jpeg","watermark":"logo.png"}"#),
            Err(ExportError::InvalidSettings(_))
        ));
        assert!(matches!(
            ExportSettings::parse(r#"{"format":"jpeg","watermark":{"text":"a","glow":true}}"#),
            Err(ExportError::InvalidSettings(_))
        ));
        // A watermark naming only its text is complete: every other field of
        // the decoration has a default.
        let only_text =
            ExportSettings::parse(r#"{"format":"jpeg","watermark":{"text":"© 2026"}}"#).unwrap();
        assert_eq!(
            only_text.watermark,
            Some(Watermark {
                text: "© 2026".to_owned(),
                ..Watermark::default()
            })
        );
        // And an empty one is refused rather than silently drawing nothing.
        assert!(matches!(
            ExportSettings::parse(r#"{"format":"jpeg","watermark":{"text":""}}"#),
            Err(ExportError::InvalidSettings(_))
        ));
        assert!(matches!(
            ExportSettings::parse(r#"{"format":"jxl"}"#),
            Err(ExportError::InvalidSettings(_))
        ));
    }

    /// ADR 0067: the effort dial is a recipe field, written unconditionally so
    /// a preset keeps producing the same file when the default moves again,
    /// and read back as the default by a preset written before it existed.
    #[test]
    fn the_avif_speed_is_pinned_by_the_preset_and_defaulted_by_an_older_one() {
        let json = ExportSettings::default().to_json();
        assert!(
            json.contains(r#""avif_speed":9"#),
            "the speed is always written: {json}"
        );

        let older = ExportSettings::parse(r#"{"format":"avif","quality":90}"#).unwrap();
        assert_eq!(older.avif_speed, DEFAULT_AVIF_SPEED);

        let pinned = ExportSettings::parse(r#"{"format":"avif","avif_speed":6}"#).unwrap();
        assert_eq!(pinned.avif_speed, 6);

        for refused in [0, 11, 255] {
            assert!(
                matches!(
                    ExportSettings {
                        avif_speed: refused,
                        ..ExportSettings::default()
                    }
                    .validate(),
                    Err(ExportError::InvalidSettings(_))
                ),
                "speed {refused} is outside [1, 10]"
            );
        }
    }

    /// Every speed encodes the same picture at the same quality target: what
    /// changes is the encoder's search effort, so the file size moves and the
    /// image does not (ADR 0067 §Contexte).
    #[test]
    fn every_avif_speed_produces_a_readable_file() {
        let dir = tempfile::tempdir().unwrap();
        let pixels = gradient(64, 48);
        for speed in [1, 9, 10] {
            let path = dir.path().join(format!("speed{speed}.avif"));
            encode(
                &path,
                64,
                48,
                &pixels,
                &ExportSettings {
                    format: ExportFormat::Avif,
                    avif_speed: speed,
                    ..ExportSettings::default()
                },
                &PhotoMetadata::default(),
            )
            .unwrap();
            let bytes = std::fs::read(&path).unwrap();
            assert_eq!(&bytes[8..12], b"avif", "AVIF brand at speed {speed}");
        }
    }
}
