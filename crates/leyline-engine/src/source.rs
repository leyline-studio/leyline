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
    /// libheif refused the file, or this build has no HEIF backend at all
    /// (ADR 0114 §2): the two are told apart by the message, because they
    /// are told apart by what the reader has to do about them.
    Heif(String),
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceError::Raw(e) => e.fmt(f),
            SourceError::Image(e) => e.fmt(f),
            SourceError::Undecodable(kind) => {
                write!(f, "{kind:?} files cannot be developed in V1")
            }
            SourceError::Heif(message) => write!(f, "{message}"),
        }
    }
}

/// Decodes a source file to an interleaved RGB image.
///
/// `params` drives LibRaw for RAW and DNG files. Non-RAW sources read only
/// `native_depth` of it: they always come back full size, with their EXIF
/// orientation applied — decoding them is cheap enough that size classes
/// are the scaler's business, not the decoder's — at eight bits per
/// channel, or at their own depth when `native_depth` is set.
///
/// `native_depth` is not a caller's preference: it is what the revision's
/// `input` version says (`stages::native_bit_depth`, ADR 0107 §6). Passing
/// `false` is what every revision written before that version renders
/// through, and passing it by hand belongs only to the paths that render no
/// revision at all — the import thumbnailer's, which has none to read.
pub(crate) fn decode(
    path: &Path,
    params: &DecodeParams,
    native_depth: bool,
) -> Result<RawImage, SourceError> {
    let media_type = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .and_then(|e| crate::import::media_type(&e));
    match media_type {
        Some(MediaType::Jpeg | MediaType::Png | MediaType::Tiff) => {
            decode_native(path, native_depth)
        }
        // HEIF goes to the platform's libheif when this build has the
        // backend, and says which of the two things went wrong when it does
        // not (ADR 0114). The container's own rotation is applied by the
        // decoder, so unlike `decode_native` nothing re-applies EXIF's.
        Some(MediaType::Heif) => decode_heif(path, native_depth),
        Some(kind @ (MediaType::Psd | MediaType::Other)) => Err(SourceError::Undecodable(kind)),
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
pub(crate) fn color(path: &Path) -> crate::stages::Source {
    if !is_camera_native(path) {
        // What the file says about itself, when it says anything this
        // engine can reduce to primaries and a curve (ADR 0115).
        return crate::stages::Source {
            color: crate::stages::SourceColor::Srgb,
            profile: tagged_profile(path),
        };
    }
    let metadata = leyline_raw::identify(path).ok();
    crate::stages::Source::plain(crate::stages::SourceColor::Camera {
        to_xyz: metadata.as_ref().and_then(|m| m.camera_to_xyz),
        multipliers: metadata.and_then(|m| m.camera_multipliers),
    })
}

/// The colour space a non-RAW file declares (ADR 0115), or `None` when it
/// declares nothing, declares something unreadable, or declares an HDR
/// transfer function this engine deliberately refuses (§4).
fn tagged_profile(path: &Path) -> Option<leyline_color::TaggedSource> {
    let media_type = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .and_then(|e| crate::import::media_type(&e));
    match media_type {
        Some(MediaType::Heif) => heif_profile(path),
        Some(MediaType::Jpeg | MediaType::Png | MediaType::Tiff) => {
            let reader = image::ImageReader::open(path)
                .and_then(|reader| reader.with_guessed_format())
                .ok()?;
            let mut decoder = reader.into_decoder().ok()?;
            let icc = decoder.icc_profile().ok()??;
            leyline_color::read_rgb_profile(&icc)
        }
        _ => None,
    }
}

/// The HEIF half: an embedded ICC, or the container's `nclx` box.
#[cfg(feature = "heif")]
fn heif_profile(path: &Path) -> Option<leyline_color::TaggedSource> {
    match leyline_heif::colour(path)? {
        leyline_heif::Colour::Icc(icc) => leyline_color::read_rgb_profile(&icc),
        leyline_heif::Colour::Nclx {
            primaries,
            white,
            srgb_transfer,
        } => {
            // An HDR or log curve is refused rather than approximated
            // (ADR 0115 §4): rendering it through an SDR curve would be
            // wrong in a way that reads as a bug rather than as a missing
            // feature.
            if !srgb_transfer {
                return None;
            }
            leyline_color::from_chromaticities(primaries, white, leyline_color::Transfer::Srgb)
        }
    }
}

#[cfg(not(feature = "heif"))]
fn heif_profile(_path: &Path) -> Option<leyline_color::TaggedSource> {
    None
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
/// orientation applied, samples normalized to 8-bit RGB — or to 16-bit RGB
/// when `native_depth` is set and the file carries more than eight bits
/// (ADR 0107 §6).
///
/// A file that holds eight bits comes back as eight bits either way: there
/// is nothing to keep, and promoting it would make the two paths differ for
/// no gain. What `native_depth` decides is whether a 16-bit TIFF or PNG is
/// truncated on the way in.
fn decode_native(path: &Path, native_depth: bool) -> Result<RawImage, SourceError> {
    let reader = image::ImageReader::open(path)
        .and_then(|reader| reader.with_guessed_format())
        .map_err(|e| SourceError::Image(image::ImageError::IoError(e)))?;
    let mut decoder = reader.into_decoder().map_err(SourceError::Image)?;
    let orientation = decoder.orientation().map_err(SourceError::Image)?;
    let mut decoded = image::DynamicImage::from_decoder(decoder).map_err(SourceError::Image)?;
    decoded.apply_orientation(orientation);
    if native_depth && wider_than_eight_bits(&decoded) {
        let rgb = decoded.into_rgb16();
        let (width, height) = (rgb.width(), rgb.height());
        // Native-endian, because that is how `Pixels::from_raw` reads a
        // 16-bit buffer — the same convention LibRaw's own output follows.
        let data = rgb
            .into_raw()
            .into_iter()
            .flat_map(u16::to_ne_bytes)
            .collect();
        return Ok(RawImage {
            width,
            height,
            bits: 16,
            data,
        });
    }
    let rgb = decoded.into_rgb8();
    Ok(RawImage {
        width: rgb.width(),
        height: rgb.height(),
        bits: 8,
        data: rgb.into_raw(),
    })
}

/// Decodes a HEIF file through the system libheif (ADR 0114).
#[cfg(feature = "heif")]
fn decode_heif(path: &Path, native_depth: bool) -> Result<RawImage, SourceError> {
    let decoded =
        leyline_heif::decode(path, native_depth).map_err(|e| SourceError::Heif(e.to_string()))?;
    Ok(RawImage {
        width: decoded.width,
        height: decoded.height,
        bits: decoded.bits,
        data: decoded.data,
    })
}

/// The same, in a build without the backend: a named refusal rather than a
/// mystery (ADR 0114 §2). The distinction matters to the person reading it —
/// this one is fixed by installing a different build, not by installing a
/// codec.
#[cfg(not(feature = "heif"))]
fn decode_heif(_path: &Path, _native_depth: bool) -> Result<RawImage, SourceError> {
    Err(SourceError::Heif(
        "this build has no HEIF decoder: it was compiled without the `heif` \
         feature, which links the libheif your system provides"
            .to_owned(),
    ))
}

/// Whether a decoded image holds more than eight bits per channel — the only
/// case where keeping the native depth changes anything.
fn wider_than_eight_bits(image: &image::DynamicImage) -> bool {
    use image::DynamicImage::*;
    matches!(
        image,
        ImageLuma16(_)
            | ImageLumaA16(_)
            | ImageRgb16(_)
            | ImageRgba16(_)
            | ImageRgb32F(_)
            | ImageRgba32F(_)
    )
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
            let image = decode(&decoded, &DecodeParams::default(), false).unwrap();
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
        let err = decode(&path, &DecodeParams::default(), false).unwrap_err();
        assert!(matches!(err, SourceError::Image(_)), "got {err:?}");
    }

    #[test]
    fn psd_is_cleanly_undecodable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.psd");
        std::fs::write(&path, b"whatever").unwrap();
        let err = decode(&path, &DecodeParams::default(), false).unwrap_err();
        assert!(err.to_string().contains("cannot be developed"), "{err}");
    }

    /// HEIF stopped being undecodable with ADR 0114 — what it gives now is
    /// either libheif's own refusal of a file that is not one, or, in a build
    /// without the backend, a sentence saying exactly that. Both are
    /// `SourceError::Heif`, and neither is the V1 "cannot be developed".
    #[test]
    fn heif_is_refused_by_the_decoder_or_by_the_build_never_as_a_media_type() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.heic");
        std::fs::write(&path, b"whatever").unwrap();
        let err = decode(&path, &DecodeParams::default(), false).unwrap_err();
        assert!(matches!(err, SourceError::Heif(_)), "got {err:?}");
        assert!(!err.to_string().contains("cannot be developed"), "{err}");
        #[cfg(not(feature = "heif"))]
        assert!(err.to_string().contains("no HEIF decoder"), "{err}");
    }
}
