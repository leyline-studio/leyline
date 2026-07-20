//! RAW file decoding for the Leyline engine.
//!
//! Backed by LibRaw, used exclusively through its LGPL-2.1 branch and linked
//! dynamically (ADR 0004). LibRaw is an implementation detail: no type of this
//! crate's API exposes it, so the decoder can be swapped (e.g. for `rawler`)
//! without touching any other crate.
//!
//! Decoding is deterministic: identical file, identical [`DecodeParams`] —
//! identical pixels (`docs/pipeline.md` §5).

mod ffi;

use std::ffi::{CStr, CString, c_int};
use std::path::Path;

/// Errors produced while identifying or decoding a RAW file.
///
/// This crate is internal to the engine, which maps these errors onto
/// `leyline_core::LeylineError` once it knows the asset involved.
#[derive(Debug, thiserror::Error)]
pub enum RawError {
    /// The file could not be read.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    /// The file is not a RAW format LibRaw understands.
    #[error("unsupported file format")]
    Unsupported,
    /// LibRaw failed while decoding.
    #[error("decode failed: {0}")]
    Decode(String),
}

/// Decoding options. `Default` is the reference rendering: camera white
/// balance, full size, 8-bit sRGB, no auto-brightening.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DecodeParams {
    /// Decode at half resolution — much faster, meant for previews.
    pub half_size: bool,
    /// Output 16 bits per channel instead of 8.
    pub sixteen_bit: bool,
    /// Apply LibRaw's histogram-based auto-brightening. Off by default:
    /// the neutral rendering must not depend on image content.
    pub auto_brighten: bool,
}

/// Identification metadata read from a RAW file's header, without decoding.
#[derive(Debug, Clone, PartialEq)]
pub struct RawMetadata {
    /// Camera manufacturer, as written by the camera.
    pub make: String,
    /// Camera model.
    pub model: String,
    /// Lens manufacturer, when recorded.
    pub lens_make: Option<String>,
    /// Lens model, when recorded.
    pub lens_model: Option<String>,
    /// Image width in pixels, after camera crop, before orientation.
    pub width: u32,
    /// Image height in pixels, after camera crop, before orientation.
    pub height: u32,
    /// ISO speed, when recorded.
    pub iso: Option<f32>,
    /// Shutter time in seconds, when recorded.
    pub shutter_s: Option<f32>,
    /// Aperture as f-number, when recorded.
    pub aperture_f: Option<f32>,
    /// Focal length in millimetres, when recorded.
    pub focal_mm: Option<f32>,
    /// Capture instant, Unix epoch milliseconds, when recorded. Cameras write
    /// wall-clock time; the offset convention of `docs/catalog.md` §9 applies.
    pub capture_ms: Option<i64>,
    /// dcraw flip code (0 none, 3 = 180°, 5 = 90° CCW, 6 = 90° CW).
    pub flip: i32,
}

/// A decoded image: interleaved RGB, tightly packed, orientation applied.
#[derive(Debug, Clone, PartialEq)]
pub struct RawImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bits per channel: 8 or 16 (16-bit samples are native-endian).
    pub bits: u8,
    /// Pixel data, `width * height * 3` samples.
    pub data: Vec<u8>,
}

/// Result of [`decode`]: pixels plus the identification metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    /// The rendered image.
    pub image: RawImage,
    /// Metadata read from the file header.
    pub metadata: RawMetadata,
}

/// RAII wrapper around a `libraw_data_t` handle.
struct Handle(*mut ffi::LibrawData);

impl Handle {
    fn open(path: &Path) -> Result<Handle, RawError> {
        // Report unreadable files as precise I/O errors ourselves: LibRaw
        // collapses "missing" and "not a RAW" into the same code.
        std::fs::metadata(path)?;

        let c_path = path_to_cstring(path)?;
        // SAFETY: libraw_init allocates a fresh handle; 0 = default flags.
        let raw = unsafe { ffi::libraw_init(0) };
        if raw.is_null() {
            return Err(RawError::Decode("libraw_init returned NULL".to_owned()));
        }
        let handle = Handle(raw);
        // SAFETY: handle is valid, path is a NUL-terminated string.
        match unsafe { ffi::libraw_open_file(handle.0, c_path.as_ptr()) } {
            0 => Ok(handle),
            // The file exists and is readable, but LibRaw cannot make sense
            // of it: every open-stage failure means "not a supported RAW".
            ffi::LIBRAW_FILE_UNSUPPORTED | ffi::LIBRAW_DATA_ERROR | ffi::LIBRAW_IO_ERROR => {
                Err(RawError::Unsupported)
            }
            code => Err(check(code).unwrap_err()),
        }
    }

    fn metadata(&self) -> RawMetadata {
        // SAFETY: the handle stays valid for every accessor; the shim strings
        // point into fixed-size, NUL-terminated buffers inside the handle and
        // are copied before the borrow ends.
        unsafe {
            let seconds = ffi::leyline_shim_timestamp(self.0);
            RawMetadata {
                make: shim_string(ffi::leyline_shim_make(self.0)),
                model: shim_string(ffi::leyline_shim_model(self.0)),
                lens_make: non_empty(shim_string(ffi::leyline_shim_lens_make(self.0))),
                lens_model: non_empty(shim_string(ffi::leyline_shim_lens_model(self.0))),
                width: ffi::leyline_shim_raw_width(self.0).max(0) as u32,
                height: ffi::leyline_shim_raw_height(self.0).max(0) as u32,
                iso: positive(ffi::leyline_shim_iso(self.0)),
                shutter_s: positive(ffi::leyline_shim_shutter(self.0)),
                aperture_f: positive(ffi::leyline_shim_aperture(self.0)),
                focal_mm: positive(ffi::leyline_shim_focal_len(self.0)),
                capture_ms: (seconds > 0).then(|| seconds * 1000),
                flip: ffi::leyline_shim_flip(self.0),
            }
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the pointer came from libraw_init and is closed only here.
        unsafe { ffi::libraw_close(self.0) };
    }
}

/// RAII wrapper around a `libraw_processed_image_t`.
struct ProcessedImage(*mut ffi::LibrawProcessedImage);

impl Drop for ProcessedImage {
    fn drop(&mut self) {
        // SAFETY: the pointer came from libraw_dcraw_make_mem_image.
        unsafe { ffi::libraw_dcraw_clear_mem(self.0) };
    }
}

/// Reads a RAW file's identification metadata without decoding the sensor
/// data. Cheap: header parsing only.
pub fn identify(path: &Path) -> Result<RawMetadata, RawError> {
    Ok(Handle::open(path)?.metadata())
}

/// Decodes a RAW file to an RGB image.
pub fn decode(path: &Path, params: &DecodeParams) -> Result<Decoded, RawError> {
    let handle = Handle::open(path)?;
    let metadata = handle.metadata();

    // SAFETY: the handle is valid; the calls follow LibRaw's mandated order
    // (open → set params → unpack → process → make_mem_image).
    unsafe {
        ffi::leyline_shim_set_options(
            handle.0,
            1,
            c_int::from(params.half_size),
            if params.sixteen_bit { 16 } else { 8 },
            c_int::from(!params.auto_brighten),
        );
        check(ffi::libraw_unpack(handle.0))?;
        check(ffi::libraw_dcraw_process(handle.0))?;

        let mut errc: c_int = 0;
        let image = ffi::libraw_dcraw_make_mem_image(handle.0, &mut errc);
        if image.is_null() {
            return Err(check(errc).err().unwrap_or_else(|| {
                RawError::Decode("libraw_dcraw_make_mem_image returned NULL".to_owned())
            }));
        }
        let image = ProcessedImage(image);

        if ffi::leyline_shim_image_type(image.0) != ffi::LIBRAW_IMAGE_BITMAP
            || ffi::leyline_shim_image_colors(image.0) != 3
        {
            return Err(RawError::Decode(
                "unexpected processed image layout".to_owned(),
            ));
        }
        let data_len = ffi::leyline_shim_image_size(image.0) as usize;
        let data = std::slice::from_raw_parts(ffi::leyline_shim_image_data(image.0), data_len);

        Ok(Decoded {
            image: RawImage {
                width: ffi::leyline_shim_image_width(image.0).max(0) as u32,
                height: ffi::leyline_shim_image_height(image.0).max(0) as u32,
                bits: ffi::leyline_shim_image_bits(image.0) as u8,
                data: data.to_vec(),
            },
            metadata,
        })
    }
}

/// Maps a LibRaw return code: positive codes are system errnos, negative ones
/// LibRaw errors.
fn check(code: c_int) -> Result<(), RawError> {
    match code {
        0 => Ok(()),
        ffi::LIBRAW_FILE_UNSUPPORTED => Err(RawError::Unsupported),
        errno if errno > 0 => Err(RawError::Io(std::io::Error::from_raw_os_error(errno))),
        other => {
            // SAFETY: libraw_strerror returns a static NUL-terminated string.
            let message = unsafe { shim_string(ffi::libraw_strerror(other)) };
            Err(RawError::Decode(message))
        }
    }
}

/// Copies a NUL-terminated C string owned by LibRaw.
unsafe fn shim_string(ptr: *const std::ffi::c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: caller guarantees a valid NUL-terminated string.
    unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

/// A metadata float is "recorded" when strictly positive and finite.
fn positive(value: f32) -> Option<f32> {
    (value.is_finite() && value > 0.0).then_some(value)
}

/// LibRaw returns empty strings, not NULL, for absent lens fields.
fn non_empty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

/// Converts a path for `libraw_open_file`.
fn path_to_cstring(path: &Path) -> Result<CString, RawError> {
    #[cfg(unix)]
    let bytes = std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str()).to_vec();
    #[cfg(not(unix))]
    let bytes = path.to_string_lossy().into_owned().into_bytes();
    CString::new(bytes).map_err(|_| {
        RawError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path contains a NUL byte",
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn missing_file_is_an_io_error() {
        let err = identify(Path::new("/nonexistent/IMG_0001.CR3")).unwrap_err();
        assert!(matches!(err, RawError::Io(_)), "got {err:?}");
    }

    #[test]
    fn non_raw_file_is_unsupported() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(b"this is definitely not a raw file")
            .unwrap();
        let err = identify(file.path()).unwrap_err();
        assert!(matches!(err, RawError::Unsupported), "got {err:?}");
        let err = decode(file.path(), &DecodeParams::default()).unwrap_err();
        assert!(matches!(err, RawError::Unsupported), "got {err:?}");
    }

    /// End-to-end decode of a real RAW file. Needs a sample: run with
    /// `LEYLINE_TEST_RAW=/path/to/file.ext cargo test -p leyline-raw -- --ignored`.
    #[test]
    #[ignore = "needs a real RAW file via LEYLINE_TEST_RAW"]
    fn decodes_a_real_raw_file() {
        let path = std::env::var("LEYLINE_TEST_RAW").expect("set LEYLINE_TEST_RAW");
        let decoded = decode(Path::new(&path), &DecodeParams::default()).unwrap();

        assert!(decoded.image.width > 0 && decoded.image.height > 0);
        assert_eq!(decoded.image.bits, 8);
        assert_eq!(
            decoded.image.data.len(),
            decoded.image.width as usize * decoded.image.height as usize * 3
        );
        assert!(!decoded.metadata.make.is_empty());

        // Determinism: decoding twice yields identical pixels (pipeline.md §5).
        let again = decode(Path::new(&path), &DecodeParams::default()).unwrap();
        assert_eq!(decoded.image.data, again.image.data);

        // Half-size previews really are smaller.
        let half = decode(
            Path::new(&path),
            &DecodeParams {
                half_size: true,
                ..DecodeParams::default()
            },
        )
        .unwrap();
        assert!(half.image.width <= decoded.image.width / 2 + 1);
    }
}
