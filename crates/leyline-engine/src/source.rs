//! Source image decoding — the single entry point turning a catalogued
//! asset into pixels, whatever its media type.
//!
//! RAW and DNG files go through LibRaw (`leyline-raw`). JPEG, PNG and TIFF
//! files — which the catalog accepts at import (`docs/catalog.md` §10) —
//! are decoded by native codecs and enter the very same develop pipeline
//! (`docs/pipeline.md` §3.1). HEIF and PSD are catalogued but have no
//! decoder in V1: asking for their pixels is a clear error, not a LibRaw
//! refusal.

use std::fmt;
use std::path::Path;

use image::ImageDecoder as _;
use leyline_core::MediaType;
use leyline_raw::{DecodeParams, RawError, RawImage};

/// Errors produced while decoding a source image.
#[derive(Debug)]
pub(crate) enum SourceError {
    /// LibRaw failed on a RAW or DNG file (or on an unknown extension).
    Raw(RawError),
    /// A native codec failed on a JPEG, PNG or TIFF file.
    Image(image::ImageError),
    /// The media type has no decoder in V1.
    Undecodable(MediaType),
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceError::Raw(e) => e.fmt(f),
            SourceError::Image(e) => e.fmt(f),
            SourceError::Undecodable(kind) => {
                write!(f, "{kind:?} files cannot be developed in V1")
            }
        }
    }
}

/// Decodes a source file to an interleaved RGB image.
///
/// `params` drives LibRaw for RAW and DNG files. Non-RAW sources ignore
/// it: they always come back full size, 8 bits per channel, with their
/// EXIF orientation applied — decoding them is cheap enough that size
/// classes are the scaler's business, not the decoder's.
pub(crate) fn decode(path: &Path, params: &DecodeParams) -> Result<RawImage, SourceError> {
    let media_type = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .and_then(|e| crate::import::media_type(&e));
    match media_type {
        Some(MediaType::Jpeg | MediaType::Png | MediaType::Tiff) => decode_native(path),
        Some(kind @ (MediaType::Heif | MediaType::Psd | MediaType::Other)) => {
            Err(SourceError::Undecodable(kind))
        }
        // RAW, DNG — and unknown extensions, which only LibRaw can judge:
        // it recognizes content, not names.
        _ => leyline_raw::decode(path, params)
            .map(|decoded| decoded.image)
            .map_err(SourceError::Raw),
    }
}

/// What colorimetry [`decode`] leaves `path`'s pixels in — the `input`
/// stage's other input (ADR 0044 §3).
///
/// Reads the RAW header only (no unpack, no develop) to pick up the body's
/// color matrix; a file LibRaw cannot identify still renders, from its
/// sensor's own numbers, which is all anyone has for it.
pub(crate) fn color(path: &Path) -> crate::stages::SourceColor {
    if !is_camera_native(path) {
        return crate::stages::SourceColor::Srgb;
    }
    let metadata = leyline_raw::identify(path).ok();
    crate::stages::SourceColor::Camera {
        to_xyz: metadata.as_ref().and_then(|m| m.camera_to_xyz),
        multipliers: metadata.and_then(|m| m.camera_multipliers),
    }
}

/// Whether `path` is a source [`decode`] hands to LibRaw — the only kind
/// whose samples are camera-native, and therefore the only kind a DCP
/// camera profile (ADR 0035) may be applied to. Mirrors [`decode`]'s own
/// dispatch: everything LibRaw judges by content (unknown extensions
/// included) counts, everything the `image` crate decodes to ready-made
/// sRGB does not.
pub(crate) fn is_camera_native(path: &Path) -> bool {
    let media_type = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .and_then(|e| crate::import::media_type(&e));
    !matches!(
        media_type,
        Some(
            MediaType::Jpeg
                | MediaType::Png
                | MediaType::Tiff
                | MediaType::Heif
                | MediaType::Psd
                | MediaType::Other
        )
    )
}

/// Reads a non-RAW image's pixel dimensions from its header, without a
/// full decode. The import pipeline records them when available.
pub(crate) fn probe_dimensions(path: &Path) -> Result<(u32, u32), SourceError> {
    image::image_dimensions(path).map_err(SourceError::Image)
}

/// Decodes through the `image` crate: format sniffed from content, EXIF
/// orientation applied, samples normalized to 8-bit RGB.
fn decode_native(path: &Path) -> Result<RawImage, SourceError> {
    let reader = image::ImageReader::open(path)
        .and_then(|reader| reader.with_guessed_format())
        .map_err(|e| SourceError::Image(image::ImageError::IoError(e)))?;
    let mut decoder = reader.into_decoder().map_err(SourceError::Image)?;
    let orientation = decoder.orientation().map_err(SourceError::Image)?;
    let mut decoded = image::DynamicImage::from_decoder(decoder).map_err(SourceError::Image)?;
    decoded.apply_orientation(orientation);
    let rgb = decoded.into_rgb8();
    Ok(RawImage {
        width: rgb.width(),
        height: rgb.height(),
        bits: 8,
        data: rgb.into_raw(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a 2×1 image (one red pixel, one green) in the given format.
    fn sample(dir: &Path, name: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        image::save_buffer(
            &path,
            &[255, 0, 0, 0, 255, 0],
            2,
            1,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
        path
    }

    #[test]
    fn decodes_png_jpeg_and_tiff_to_rgb8() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["a.png", "a.jpg", "a.tif"] {
            let decoded = sample(dir.path(), name);
            let image = decode(&decoded, &DecodeParams::default()).unwrap();
            assert_eq!((image.width, image.height, image.bits), (2, 1, 8), "{name}");
            assert_eq!(image.data.len(), 6, "{name}");
        }
    }

    #[test]
    fn probes_dimensions_without_decoding() {
        let dir = tempfile::tempdir().unwrap();
        let png = sample(dir.path(), "b.png");
        assert_eq!(probe_dimensions(&png).unwrap(), (2, 1));
    }

    #[test]
    fn a_corrupt_image_is_a_codec_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.png");
        std::fs::write(&path, b"not an image at all").unwrap();
        let err = decode(&path, &DecodeParams::default()).unwrap_err();
        assert!(matches!(err, SourceError::Image(_)), "got {err:?}");
    }

    #[test]
    fn heif_and_psd_are_cleanly_undecodable() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["c.heic", "c.psd"] {
            let path = dir.path().join(name);
            std::fs::write(&path, b"whatever").unwrap();
            let err = decode(&path, &DecodeParams::default()).unwrap_err();
            assert!(
                err.to_string().contains("cannot be developed"),
                "{name}: {err}"
            );
        }
    }
}
