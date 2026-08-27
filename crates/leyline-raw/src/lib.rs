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
    ///
    /// A hint, not a guarantee: LibRaw honors it for standard Bayer sensor
    /// data, but some cameras write already-reduced RAW variants (e.g.
    /// Canon sRAW/mRAW, produced in-camera at a fixed lower resolution)
    /// that LibRaw cannot halve further — the decode then comes back at
    /// that variant's native size instead. Never larger than the full
    /// decode either way.
    pub half_size: bool,
    /// Output 16 bits per channel instead of 8.
    pub sixteen_bit: bool,
    /// Apply LibRaw's histogram-based auto-brightening. Off by default:
    /// the neutral rendering must not depend on image content.
    pub auto_brighten: bool,
    /// What to do with channels that saturated at the sensor (ADR 0050).
    /// `Default` is [`HighlightMode::Clip`], LibRaw's own default and what
    /// this crate asked for implicitly before that decision.
    pub highlight: HighlightMode,
    /// Decode to raw camera color space, linear (no color matrix, no gamma
    /// curve), instead of LibRaw's own built-in sRGB conversion (ADR 0035).
    /// Needed only when a camera profile (DCP) will replace that
    /// conversion with its own matrices — the reference rendering
    /// (`Default`) leaves this off and gets LibRaw's ordinary gamma-
    /// encoded sRGB.
    pub camera_native: bool,
    /// Which interpolation reconstructs the two missing channels of every
    /// sensor site (ADR 0061). `Default` is [`Demosaic::Ahd`], LibRaw's own
    /// default and what this crate asked for implicitly before that
    /// decision.
    ///
    /// Ignored when [`DecodeParams::half_size`] is set: half-size decoding
    /// takes one pixel per 2x2 Bayer group and skips interpolation
    /// altogether.
    pub demosaic: Demosaic,
    /// Which raw level the decoder treats as white (ADR 0066). `Default` is
    /// [`WhiteLevel::FormatCeiling`], what this crate asked for implicitly
    /// before that decision.
    pub white_level: WhiteLevel,
}

/// What the decoder normalizes the sensor's numbers by — the level it calls
/// white (ADR 0066).
///
/// A raw file's samples mean nothing until something says which value is
/// "fully exposed". Choosing wrong does not shift a colour, it shifts
/// *everything*: pick a level above the sensor's real saturation and a
/// blown highlight comes out grey, with the whole image proportionally dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WhiteLevel {
    /// The raw format's theoretical ceiling — 16383 for a 14-bit file,
    /// whatever the body. What LibRaw leaves in `maximum` after unpacking,
    /// and what this crate used before ADR 0066.
    #[default]
    FormatCeiling,
    /// The linearity margin the camera itself recorded, when it recorded one
    /// — the level above which its own sensor stops responding
    /// proportionally. Falls back to [`WhiteLevel::FormatCeiling`] for a body
    /// that writes none, so no file is left without an answer.
    ///
    /// This mode also switches off LibRaw's `adjust_maximum_thr`, whose
    /// default (0.75) picks the white level from the brightest sample **of
    /// the frame being decoded**. Either way, the level then depends on the
    /// camera and its sensitivity — never on what the photograph contains.
    CameraLinearityMargin,
}

/// Which interpolation reconstructs the missing channels (ADR 0061).
///
/// Four of LibRaw's values, named for what they do. AMaZE and LMMSE are
/// absent because they live in the GPL2/GPL3 demosaic packs, dropped from
/// LibRaw's own distribution at 0.19 and not present in the library linked
/// here — offering them would offer a choice that silently falls back to
/// AHD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Demosaic {
    /// Adaptive Homogeneity-Directed: good everywhere, best nowhere.
    /// LibRaw quality 3, and its default.
    #[default]
    Ahd,
    /// Variable Number of Gradients: gentler on gradients, less maze
    /// artifacting on flat areas. LibRaw quality 1.
    Vng,
    /// DCB: cleaner rendering of hard edges — the one to reach for when
    /// moire is the problem. LibRaw quality 4.
    Dcb,
    /// DHT: the finest on high-frequency detail, and the slowest. LibRaw
    /// quality 11.
    Dht,
}

impl Demosaic {
    /// The `params.user_qual` value LibRaw expects.
    fn libraw_quality(self) -> i32 {
        match self {
            Demosaic::Ahd => 3,
            Demosaic::Vng => 1,
            Demosaic::Dcb => 4,
            Demosaic::Dht => 11,
        }
    }
}

/// What the decoder does with a channel that saturated at the sensor
/// (ADR 0050), applied *before* demosaic — the only place where a clipped
/// pixel still has unclipped neighbors in the other channels.
///
/// Three of LibRaw's nine modes. `Unclip` (its mode 1) is deliberately not
/// among them: it leaves the highlights with the magenta cast of a channel
/// carried past the others, which looks like a defect rather than a choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HighlightMode {
    /// Clip at white — nothing is recovered, and nothing changes from what
    /// this crate did before ADR 0050. LibRaw mode 0.
    #[default]
    Clip,
    /// Blend the clipped and unclipped channels: recovers texture without
    /// drifting in color. LibRaw mode 2.
    Blend,
    /// Rebuild the saturated channel from the others: recovers the most, at
    /// the risk of a hue shift in deeply saturated areas. LibRaw mode 5, the
    /// median of its 3..9 rebuild family.
    Rebuild,
}

impl HighlightMode {
    /// The `params.highlight` value LibRaw expects.
    fn libraw_mode(self) -> i32 {
        match self {
            HighlightMode::Clip => 0,
            HighlightMode::Blend => 2,
            HighlightMode::Rebuild => 5,
        }
    }
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
    /// Capture instant, Unix epoch milliseconds, when recorded.
    ///
    /// Cameras write a **wall-clock** time with no zone, and a RAW never
    /// carries an offset, so `docs/catalog.md` §9's second case applies: the
    /// clock reading is stored as if it were UTC. The value is therefore the
    /// same on every machine, and displaying it without conversion gives back
    /// what the photographer saw on the body. `leyline_shim_timestamp` undoes
    /// LibRaw's local-zone interpretation to get there.
    pub capture_ms: Option<i64>,
    /// dcraw flip code (0 none, 3 = 180°, 5 = 90° CCW, 6 = 90° CW).
    pub flip: i32,
    /// GPS latitude in decimal degrees, `[-90, 90]`, when recorded
    /// (`docs/adr/0040-gps-map-view.md`).
    pub gps_latitude: Option<f64>,
    /// GPS longitude in decimal degrees, `[-180, 180]`, when recorded.
    pub gps_longitude: Option<f64>,
    /// GPS altitude in meters, when recorded.
    pub gps_altitude: Option<f64>,
    /// The body's XYZ→camera matrix (rows: camera channels; columns: X, Y,
    /// Z), from LibRaw's per-model table. `None` when LibRaw knows no matrix
    /// for this camera.
    ///
    /// This is what lets a caller convert camera-native pixels into a working
    /// space of its own choosing. Asking LibRaw to do the conversion instead
    /// would clip every color outside the space it converts to, which is
    /// exactly what a wide-gamut pipeline must not do.
    pub camera_to_xyz: Option<[[f64; 3]; 3]>,
    /// The camera's as-shot channel multipliers — the white balance the body
    /// recorded — in the decoder's channel order, `None` when the file
    /// carries none.
    ///
    /// A three-color sensor leaves the fourth entry at zero or repeats the
    /// second green, so a caller must skip non-positive entries.
    ///
    /// Exposed for one reason (ADR 0050 §3): the decoder normalizes the image
    /// by these multipliers, and it normalizes by the *smallest* of them when
    /// clipping highlights but by the *largest* when reconstructing them. The
    /// ratio between the two is a global gain difference a caller has to know
    /// about to undo — it is the same for every pixel, which is what makes it
    /// undoable at all.
    pub camera_multipliers: Option<[f64; 4]>,
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

/// The preview a camera wrote inside its own RAW file (ADR 0065 §2).
///
/// Extracting it costs a header read and a copy — no sensor data is unpacked
/// and no interpolation runs. That is the whole point: it is how a file can
/// be *looked at* before anyone decides to import it.
#[derive(Debug, Clone, PartialEq)]
pub struct Thumbnail {
    /// The preview, in whichever form the body stored it.
    pub kind: ThumbnailKind,
    /// The RAW's own orientation tag ([`RawMetadata::flip`]), for a preview
    /// that carries none of its own — LibRaw applies no rotation here, unlike
    /// [`decode`].
    pub flip: i32,
}

/// How a body stored its embedded preview.
#[derive(Debug, Clone, PartialEq)]
pub enum ThumbnailKind {
    /// A JPEG, byte for byte as the camera wrote it. The usual case.
    Jpeg(Vec<u8>),
    /// An uncompressed RGB bitmap: a few bodies (and some DNG writers)
    /// store one instead.
    Bitmap(RawImage),
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
            let (gps_latitude, gps_longitude, gps_altitude) =
                if ffi::leyline_shim_gps_parsed(self.0) != 0 {
                    (
                        Some(dms_to_decimal(
                            ffi::leyline_shim_gps_lat_deg(self.0),
                            ffi::leyline_shim_gps_lat_min(self.0),
                            ffi::leyline_shim_gps_lat_sec(self.0),
                            ffi::leyline_shim_gps_lat_south(self.0) != 0,
                        )),
                        Some(dms_to_decimal(
                            ffi::leyline_shim_gps_lon_deg(self.0),
                            ffi::leyline_shim_gps_lon_min(self.0),
                            ffi::leyline_shim_gps_lon_sec(self.0),
                            ffi::leyline_shim_gps_lon_west(self.0) != 0,
                        )),
                        Some(signed_altitude(
                            ffi::leyline_shim_gps_altitude(self.0),
                            ffi::leyline_shim_gps_altitude_below_sea_level(self.0) != 0,
                        )),
                    )
                } else {
                    (None, None, None)
                };
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
                gps_latitude,
                gps_longitude,
                gps_altitude,
                camera_to_xyz: cam_xyz(self.0),
                camera_multipliers: cam_mul(self.0),
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

/// Re-reads a LibRaw capture timestamp under the convention of
/// `docs/catalog.md` §9: the camera's wall clock, stored as if it were UTC.
///
/// LibRaw parses the naive `DateTimeOriginal` with `mktime`, so its
/// `timestamp` depends on the time zone of the machine doing the import —
/// the same file dates two hours apart in Paris and eleven in Tokyo. This
/// undoes that interpretation exactly, by breaking the value down in the same
/// zone that built it and reassembling those fields as UTC.
///
/// Exposed for the test that pins a zone and checks the arithmetic; the
/// decode path applies it on its own and callers of [`identify`] and
/// [`decode`] get a corrected value already.
#[doc(hidden)]
pub fn wall_clock_from_local(timestamp: i64) -> i64 {
    // SAFETY: a pure arithmetic helper over C time functions; it touches no
    // LibRaw state and takes no pointer.
    unsafe { ffi::leyline_shim_wall_clock(timestamp) }
}

/// Extracts the preview the camera embedded in a RAW file, without decoding/// Extracts the preview the camera embedded in a RAW file, without decoding
/// any sensor data (ADR 0065 §2).
///
/// `Ok(None)` when the file carries no preview, or one this LibRaw build
/// cannot extract: both are ordinary facts about a file — a scan showing
/// every other photo must not stop on them.
pub fn thumbnail(path: &Path) -> Result<Option<Thumbnail>, RawError> {
    let handle = Handle::open(path)?;
    // SAFETY: the handle is valid; unpack_thumb follows open, and
    // make_mem_thumb follows unpack_thumb — LibRaw's mandated order.
    unsafe {
        let flip = ffi::leyline_shim_flip(handle.0);
        match ffi::libraw_unpack_thumb(handle.0) {
            0 => {}
            ffi::LIBRAW_NO_THUMBNAIL | ffi::LIBRAW_UNSUPPORTED_THUMBNAIL => return Ok(None),
            code => return Err(check(code).unwrap_err()),
        }

        let mut errc: c_int = 0;
        let thumb = ffi::libraw_dcraw_make_mem_thumb(handle.0, &mut errc);
        if thumb.is_null() {
            return match errc {
                ffi::LIBRAW_NO_THUMBNAIL | ffi::LIBRAW_UNSUPPORTED_THUMBNAIL => Ok(None),
                code => Err(check(code).err().unwrap_or_else(|| {
                    RawError::Decode("libraw_dcraw_make_mem_thumb returned NULL".to_owned())
                })),
            };
        }
        let thumb = ProcessedImage(thumb);

        let len = ffi::leyline_shim_image_size(thumb.0) as usize;
        let bytes = std::slice::from_raw_parts(ffi::leyline_shim_image_data(thumb.0), len);
        let kind = match ffi::leyline_shim_image_type(thumb.0) {
            ffi::LIBRAW_IMAGE_JPEG => ThumbnailKind::Jpeg(bytes.to_vec()),
            ffi::LIBRAW_IMAGE_BITMAP if ffi::leyline_shim_image_colors(thumb.0) == 3 => {
                ThumbnailKind::Bitmap(RawImage {
                    width: ffi::leyline_shim_image_width(thumb.0).max(0) as u32,
                    height: ffi::leyline_shim_image_height(thumb.0).max(0) as u32,
                    bits: ffi::leyline_shim_image_bits(thumb.0) as u8,
                    data: bytes.to_vec(),
                })
            }
            // A layout no caller could read: treated as "no usable preview"
            // rather than an error, for the same reason as above.
            _ => return Ok(None),
        };
        Ok(Some(Thumbnail { kind, flip }))
    }
}

/// Decodes a RAW file to an RGB image.
pub fn decode(path: &Path, params: &DecodeParams) -> Result<Decoded, RawError> {
    let handle = Handle::open(path)?;
    let metadata = handle.metadata();

    // SAFETY: the handle is valid; the calls follow LibRaw's mandated order
    // (open → set params → unpack → process → make_mem_image).
    unsafe {
        // Before `unpack`, and only here: identify fills `maximum` from the
        // camera's own metadata, then unpack overwrites it with the format's
        // ceiling. Handing the metadata value back as `user_sat` is what
        // makes the decoder normalize by the sensor rather than by the word
        // size (ADR 0066).
        if params.white_level == WhiteLevel::CameraLinearityMargin {
            // Off first, unconditionally: LibRaw's default lowers the white
            // level to the brightest sample of *this frame*, so leaving it on
            // would keep the fallback path content-dependent for a body that
            // writes no margin.
            ffi::leyline_shim_set_adjust_maximum_thr(handle.0, 0.0);
            let margin = ffi::leyline_shim_linear_max(handle.0);
            if margin > 0 {
                ffi::leyline_shim_set_user_sat(handle.0, margin);
            }
        }
        ffi::leyline_shim_set_options(
            handle.0,
            1,
            c_int::from(params.half_size),
            if params.sixteen_bit { 16 } else { 8 },
            c_int::from(!params.auto_brighten),
            c_int::from(params.camera_native),
            params.highlight.libraw_mode(),
            params.demosaic.libraw_quality(),
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

/// Reads the body's XYZ→camera matrix, `None` when LibRaw has none.
///
/// # Safety
/// `handle` must be a live LibRaw handle whose file has been opened
/// (identify fills the matrix; no unpack is needed).
unsafe fn cam_xyz(handle: *const ffi::LibrawData) -> Option<[[f64; 3]; 3]> {
    let mut flat = [0.0f64; 9];
    // SAFETY: the shim writes exactly nine doubles into the buffer.
    let known = unsafe { ffi::leyline_shim_cam_xyz(handle, flat.as_mut_ptr()) };
    if known == 0 {
        return None;
    }
    let mut matrix = [[0.0f64; 3]; 3];
    for (row, chunk) in matrix.iter_mut().zip(flat.chunks_exact(3)) {
        row.copy_from_slice(chunk);
    }
    Some(matrix)
}

/// The camera's as-shot channel multipliers, `None` when the file has none.
///
/// # Safety
/// `handle` must be a live LibRaw handle whose file has been opened.
unsafe fn cam_mul(handle: *const ffi::LibrawData) -> Option<[f64; 4]> {
    let mut multipliers = [0.0f64; 4];
    // SAFETY: the shim writes exactly four doubles into the buffer.
    let known = unsafe { ffi::leyline_shim_cam_mul(handle, multipliers.as_mut_ptr()) };
    (known != 0 && multipliers.iter().all(|m| m.is_finite())).then_some(multipliers)
}

/// A metadata float is "recorded" when strictly positive and finite.
fn positive(value: f32) -> Option<f32> {
    (value.is_finite() && value > 0.0).then_some(value)
}

/// LibRaw returns empty strings, not NULL, for absent lens fields.
fn non_empty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

/// Degrees/minutes/seconds (LibRaw's `parsed_gps` convention) to a signed
/// decimal degree, negative for south latitudes / west longitudes.
fn dms_to_decimal(deg: f32, min: f32, sec: f32, negative: bool) -> f64 {
    let value = f64::from(deg) + f64::from(min) / 60.0 + f64::from(sec) / 3600.0;
    if negative { -value } else { value }
}

/// LibRaw (and the EXIF `GPSAltitudeRef` tag it reads) always reports
/// altitude as a positive magnitude plus a separate above/below-sea-level
/// reference — never a signed value on its own. Below sea level (a valid
/// EXIF case: Death Valley, below-grade locations) needs the sign applied
/// here, or every such photo would silently import above sea level.
fn signed_altitude(magnitude: f32, below_sea_level: bool) -> f64 {
    let value = f64::from(magnitude);
    if below_sea_level { -value } else { value }
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
    fn dms_to_decimal_matches_a_known_coordinate() {
        // Sydney Opera House, roughly: 33°51'54"S 151°12'32"E.
        let lat = dms_to_decimal(33.0, 51.0, 54.0, true);
        let lon = dms_to_decimal(151.0, 12.0, 32.0, false);
        assert!((lat - -33.865).abs() < 0.001, "got {lat}");
        assert!((lon - 151.209).abs() < 0.001, "got {lon}");
    }

    #[test]
    fn dms_to_decimal_north_and_east_stay_positive() {
        assert_eq!(dms_to_decimal(10.0, 0.0, 0.0, false), 10.0);
    }

    #[test]
    fn signed_altitude_above_sea_level_stays_positive() {
        assert_eq!(signed_altitude(42.0, false), 42.0);
    }

    #[test]
    fn signed_altitude_below_sea_level_is_negated() {
        // GPSAltitudeRef = 1 (below sea level) is a real EXIF case — Death
        // Valley, below-grade locations — not just a theoretical bit.
        assert_eq!(signed_altitude(42.0, true), -42.0);
    }

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
        let err = thumbnail(file.path()).unwrap_err();
        assert!(matches!(err, RawError::Unsupported), "got {err:?}");
    }

    /// The white level a decode normalizes by is the camera's, not the
    /// frame's (ADR 0066). Needs a sample:
    /// `LEYLINE_TEST_RAW=... cargo test -p leyline-raw -- --ignored`.
    #[test]
    #[ignore = "needs a real RAW file via LEYLINE_TEST_RAW"]
    fn the_camera_white_level_changes_the_decode_and_stays_put() {
        let path = std::env::var("LEYLINE_TEST_RAW").expect("set LEYLINE_TEST_RAW");
        let path = Path::new(&path);
        let decode = |white_level| {
            decode(
                path,
                &DecodeParams {
                    sixteen_bit: true,
                    white_level,
                    ..DecodeParams::default()
                },
            )
            .unwrap()
            .image
            .data
        };

        let ceiling = decode(WhiteLevel::FormatCeiling);
        let camera = decode(WhiteLevel::CameraLinearityMargin);
        let mean = |data: &[u8]| -> f64 {
            let sum: u64 = data
                .chunks_exact(2)
                .map(|c| u64::from(u16::from_ne_bytes([c[0], c[1]])))
                .sum();
            sum as f64 / (data.len() / 2) as f64
        };
        // A body that records a linearity margin below the format ceiling
        // renders brighter for it — that is the whole correction. A body
        // that records none falls back, and this assertion would be the
        // wrong one to make; every camera in the test corpus records one.
        assert!(
            mean(&camera) > mean(&ceiling),
            "camera white level {} vs format ceiling {}",
            mean(&camera),
            mean(&ceiling)
        );

        // And it is a property of the file, not of the run: same bytes twice
        // (`docs/pipeline.md` §5.1).
        assert_eq!(camera, decode(WhiteLevel::CameraLinearityMargin));
    }

    /// The embedded preview of a real RAW file (ADR 0065 §2). Same sample
    /// as the decode test: `LEYLINE_TEST_RAW=... cargo test -p leyline-raw
    /// -- --ignored`.
    #[test]
    #[ignore = "needs a real RAW file via LEYLINE_TEST_RAW"]
    fn extracts_the_embedded_preview_of_a_real_raw_file() {
        let path = std::env::var("LEYLINE_TEST_RAW").expect("set LEYLINE_TEST_RAW");
        let path = Path::new(&path);
        let thumb = thumbnail(path).unwrap().expect("this body writes one");
        assert_eq!(thumb.flip, identify(path).unwrap().flip);
        match &thumb.kind {
            ThumbnailKind::Jpeg(bytes) => {
                // Really a JPEG, not whatever LibRaw had lying around.
                assert!(bytes.len() > 1024);
                assert_eq!(&bytes[..2], &[0xFF, 0xD8]);
            }
            ThumbnailKind::Bitmap(image) => {
                assert!(image.width > 0 && image.height > 0);
                assert_eq!(
                    image.data.len(),
                    image.width as usize * image.height as usize * 3
                );
            }
        }

        // Extracting it costs no sensor decode: it must stay far cheaper
        // than the smallest full decode, which is the reason it exists.
        let start = std::time::Instant::now();
        thumbnail(path).unwrap();
        let embedded = start.elapsed();
        let start = std::time::Instant::now();
        decode(
            path,
            &DecodeParams {
                half_size: true,
                ..DecodeParams::default()
            },
        )
        .unwrap();
        assert!(
            embedded < start.elapsed(),
            "embedded preview took {embedded:?}, half-size decode {:?}",
            start.elapsed()
        );
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

        // `half_size` never grows the image, but on some RAW variants
        // (Canon sRAW/mRAW, already reduced in-camera) LibRaw can't shrink
        // it further, so an exact half is not guaranteed — see
        // `DecodeParams::half_size`.
        let half = decode(
            Path::new(&path),
            &DecodeParams {
                half_size: true,
                ..DecodeParams::default()
            },
        )
        .unwrap();
        assert!(half.image.width <= decoded.image.width);
        assert!(half.image.height <= decoded.image.height);
    }
}
