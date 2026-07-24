//! Raw FFI surface: the handful of LibRaw C API functions leyline-raw uses,
//! plus the field accessors provided by `shim.c`. Nothing here leaks out of
//! the crate (ADR 0004).

use std::ffi::{c_char, c_int, c_longlong, c_uint};

/// Opaque LibRaw handle (`libraw_data_t`).
#[repr(C)]
pub(crate) struct LibrawData {
    _private: [u8; 0],
}

/// Opaque processed image (`libraw_processed_image_t`).
#[repr(C)]
pub(crate) struct LibrawProcessedImage {
    _private: [u8; 0],
}

/// `LibRaw_errors::LIBRAW_FILE_UNSUPPORTED`.
pub(crate) const LIBRAW_FILE_UNSUPPORTED: c_int = -2;
/// `LibRaw_errors::LIBRAW_DATA_ERROR`.
pub(crate) const LIBRAW_DATA_ERROR: c_int = -100_008;
/// `LibRaw_errors::LIBRAW_IO_ERROR`.
pub(crate) const LIBRAW_IO_ERROR: c_int = -100_009;
/// `LibRaw_image_formats::LIBRAW_IMAGE_BITMAP`.
pub(crate) const LIBRAW_IMAGE_BITMAP: c_int = 2;

unsafe extern "C" {
    pub(crate) fn libraw_init(flags: c_uint) -> *mut LibrawData;
    pub(crate) fn libraw_open_file(data: *mut LibrawData, path: *const c_char) -> c_int;
    pub(crate) fn libraw_unpack(data: *mut LibrawData) -> c_int;
    pub(crate) fn libraw_dcraw_process(data: *mut LibrawData) -> c_int;
    pub(crate) fn libraw_dcraw_make_mem_image(
        data: *mut LibrawData,
        errc: *mut c_int,
    ) -> *mut LibrawProcessedImage;
    pub(crate) fn libraw_dcraw_clear_mem(image: *mut LibrawProcessedImage);
    pub(crate) fn libraw_close(data: *mut LibrawData);
    pub(crate) fn libraw_strerror(code: c_int) -> *const c_char;

    // shim.c
    pub(crate) fn leyline_shim_set_options(
        data: *mut LibrawData,
        use_camera_wb: c_int,
        half_size: c_int,
        output_bps: c_int,
        no_auto_bright: c_int,
    );
    pub(crate) fn leyline_shim_make(data: *const LibrawData) -> *const c_char;
    pub(crate) fn leyline_shim_model(data: *const LibrawData) -> *const c_char;
    pub(crate) fn leyline_shim_lens_make(data: *const LibrawData) -> *const c_char;
    pub(crate) fn leyline_shim_lens_model(data: *const LibrawData) -> *const c_char;
    pub(crate) fn leyline_shim_iso(data: *const LibrawData) -> f32;
    pub(crate) fn leyline_shim_shutter(data: *const LibrawData) -> f32;
    pub(crate) fn leyline_shim_aperture(data: *const LibrawData) -> f32;
    pub(crate) fn leyline_shim_focal_len(data: *const LibrawData) -> f32;
    pub(crate) fn leyline_shim_timestamp(data: *const LibrawData) -> c_longlong;
    pub(crate) fn leyline_shim_flip(data: *const LibrawData) -> c_int;
    pub(crate) fn leyline_shim_raw_width(data: *const LibrawData) -> c_int;
    pub(crate) fn leyline_shim_raw_height(data: *const LibrawData) -> c_int;

    pub(crate) fn leyline_shim_gps_parsed(data: *const LibrawData) -> c_int;
    pub(crate) fn leyline_shim_gps_lat_deg(data: *const LibrawData) -> f32;
    pub(crate) fn leyline_shim_gps_lat_min(data: *const LibrawData) -> f32;
    pub(crate) fn leyline_shim_gps_lat_sec(data: *const LibrawData) -> f32;
    pub(crate) fn leyline_shim_gps_lon_deg(data: *const LibrawData) -> f32;
    pub(crate) fn leyline_shim_gps_lon_min(data: *const LibrawData) -> f32;
    pub(crate) fn leyline_shim_gps_lon_sec(data: *const LibrawData) -> f32;
    pub(crate) fn leyline_shim_gps_altitude(data: *const LibrawData) -> f32;
    pub(crate) fn leyline_shim_gps_altitude_below_sea_level(data: *const LibrawData) -> c_int;
    pub(crate) fn leyline_shim_gps_lat_south(data: *const LibrawData) -> c_int;
    pub(crate) fn leyline_shim_gps_lon_west(data: *const LibrawData) -> c_int;

    pub(crate) fn leyline_shim_image_type(image: *const LibrawProcessedImage) -> c_int;
    pub(crate) fn leyline_shim_image_width(image: *const LibrawProcessedImage) -> c_int;
    pub(crate) fn leyline_shim_image_height(image: *const LibrawProcessedImage) -> c_int;
    pub(crate) fn leyline_shim_image_colors(image: *const LibrawProcessedImage) -> c_int;
    pub(crate) fn leyline_shim_image_bits(image: *const LibrawProcessedImage) -> c_int;
    pub(crate) fn leyline_shim_image_size(image: *const LibrawProcessedImage) -> c_uint;
    pub(crate) fn leyline_shim_image_data(image: *const LibrawProcessedImage) -> *const u8;
}
