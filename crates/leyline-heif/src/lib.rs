//! HEIF/HEIC decoding for the Leyline engine (ADR 0114).
//!
//! Backed by **the libheif the platform provides**, linked dynamically and
//! never vendored — the arrangement LibRaw has had since
//! [ADR 0004](../../../docs/adr/0004-libraw.md), for a stronger reason here:
//! a HEIC is usually HEVC-coded, HEVC is patented, and a decoder we do not
//! ship is a decoder we do not distribute. Our own packages are therefore
//! built with this crate's feature **off**; a source build or a
//! distribution's package has it on.
//!
//! libheif is an implementation detail: nothing in this API exposes it, so a
//! platform-native backend (Image I/O, WIC) can replace it later without
//! touching another crate.

use std::path::Path;

use libheif_rs::{ColorSpace, HeifContext, RgbChroma};

/// Errors produced while decoding a HEIF file.
///
/// This crate is internal to the engine, which maps these onto
/// `leyline_core::LeylineError` once it knows the asset involved.
#[derive(Debug, thiserror::Error)]
pub enum HeifError {
    /// The path is not valid UTF-8, which libheif's C API cannot take.
    #[error("path is not valid UTF-8: {0}")]
    Path(String),
    /// libheif refused the file. Its message is carried through as it is: it
    /// is the one place that can say *no HEVC decoder plugin is installed*,
    /// which is the failure a person on a fresh machine actually hits.
    #[error("libheif: {0}")]
    Heif(String),
    /// The decode produced no interleaved plane, which should not happen for
    /// an RGB request and is not worth a panic.
    #[error("decoded image carries no interleaved RGB plane")]
    NoPlane,
}

/// One decoded HEIF image, in the shape the engine's RAW path already
/// produces: interleaved RGB, 8 or 16 bits per channel, native-endian.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bits per channel: 8 or 16.
    pub bits: u8,
    /// `width * height * 3` samples, 16-bit ones native-endian.
    pub data: Vec<u8>,
}

/// Decodes the primary image of a HEIF file.
///
/// `native_depth` is `input: 5`'s rule (ADR 0107): when the file carries more
/// than eight bits per channel, keep them. A HEIC from a phone is commonly
/// ten, and truncating it on the way in would throw away the headroom the
/// pipeline exists to use.
///
/// The container's own rotation and mirroring (`irot`/`imir`) are applied by
/// libheif while decoding. The EXIF orientation tag is therefore **not**
/// applied again by the caller — unlike the JPEG path, where the codec
/// applies nothing (ADR 0114 §3).
pub fn decode(path: &Path, native_depth: bool) -> Result<Decoded, HeifError> {
    let name = path
        .to_str()
        .ok_or_else(|| HeifError::Path(path.display().to_string()))?;
    let context = HeifContext::read_from_file(name).map_err(heif_error)?;
    let handle = context.primary_image_handle().map_err(heif_error)?;

    let wide = native_depth && handle.luma_bits_per_pixel() > 8;
    let chroma = if wide {
        // Native-endian, because that is how `Pixels::from_raw` reads a
        // 16-bit buffer — the same convention LibRaw's output follows.
        if cfg!(target_endian = "little") {
            RgbChroma::HdrRgbLe
        } else {
            RgbChroma::HdrRgbBe
        }
    } else {
        RgbChroma::Rgb
    };
    let image = handle
        .decode(ColorSpace::Rgb(chroma), None)
        .map_err(heif_error)?;

    let planes = image.planes();
    let plane = planes.interleaved.ok_or(HeifError::NoPlane)?;
    let (width, height) = (plane.width, plane.height);
    let bytes_per_pixel = if wide { 6 } else { 3 };
    let row_bytes = width as usize * bytes_per_pixel;

    // Row by row: libheif's stride is the allocation's, not the image's, and
    // copying the whole buffer would carry its padding into the pipeline.
    let mut data = Vec::with_capacity(row_bytes * height as usize);
    for y in 0..height as usize {
        let from = y * plane.stride;
        data.extend_from_slice(&plane.data[from..from + row_bytes]);
    }

    Ok(Decoded {
        width,
        height,
        bits: if wide { 16 } else { 8 },
        data,
    })
}

/// Carries libheif's message through, and adds the one sentence it never
/// says: *which package is missing*.
///
/// A distribution that ships libheif does not necessarily ship an **HEVC**
/// decoder plugin for it — Ubuntu 24.04 puts `libheif-plugin-libde265` in its
/// own package and installs `libheif-dev` without it, so a machine that
/// builds Leyline perfectly can still refuse every HEIC a phone made. The
/// bare message for that case is "Unsupported codec", which tells a person
/// nothing they can act on.
fn heif_error(error: libheif_rs::HeifError) -> HeifError {
    let message = error.to_string();
    if message.contains("Unsupported codec") {
        return HeifError::Heif(format!(
            "{message} — this system's libheif has no decoder plugin for this \
             file's codec. A HEIC from a phone is HEVC-coded: install the \
             plugin your distribution packages separately (Debian and Ubuntu: \
             libheif-plugin-libde265)"
        ));
    }
    HeifError::Heif(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixture is an **AV1-coded** HEIF, not HEVC: the build machine has
    /// no HEVC encoder, and reading one needs none. Every line below the
    /// choice of codec plugin is the same either way — which is exactly the
    /// limit ADR 0114 records rather than hides.
    fn fixture() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixture.heic")
    }

    #[test]
    fn decodes_the_primary_image_to_interleaved_rgb() {
        let decoded = decode(&fixture(), false).unwrap();
        assert_eq!((decoded.width, decoded.height), (96, 64));
        assert_eq!(decoded.bits, 8);
        assert_eq!(
            decoded.data.len(),
            decoded.width as usize * decoded.height as usize * 3,
            "no stride padding survives the copy"
        );
        // The fixture is a purple-to-white gradient, top to bottom: the
        // last row is brighter than the first, and the first is purple.
        let brightness = |row: usize| -> u32 {
            decoded.data[row * 96 * 3..row * 96 * 3 + 3]
                .iter()
                .map(|v| u32::from(*v))
                .sum()
        };
        let top = &decoded.data[0..3];
        assert!(
            top[0] > top[1] && top[2] > top[1],
            "purple at the top: {top:?}"
        );
        assert!(
            brightness(63) > brightness(0),
            "{} then {}",
            brightness(0),
            brightness(63)
        );
    }

    /// The file is 12-bit, so `native_depth` changes what comes out — the
    /// rule `input: 5` asks for.
    #[test]
    fn a_file_deeper_than_eight_bits_keeps_its_depth_when_asked() {
        let deep = decode(&fixture(), true).unwrap();
        assert_eq!(deep.bits, 16);
        assert_eq!(
            deep.data.len(),
            deep.width as usize * deep.height as usize * 6
        );
    }

    #[test]
    fn a_file_that_is_not_heif_is_refused_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not.heic");
        std::fs::write(&path, b"certainly not a heif file").unwrap();
        assert!(matches!(decode(&path, false), Err(HeifError::Heif(_))));
    }
}
