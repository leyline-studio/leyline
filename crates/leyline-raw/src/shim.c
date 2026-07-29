/* Field accessors for LibRaw structs, compiled against the installed headers.
 *
 * The LibRaw C API exposes setters for most processing parameters but not for
 * every field leyline-raw needs. Reading struct fields directly from Rust
 * would hard-code one version's layout; going through this shim keeps the
 * offsets correct for whatever LibRaw version is installed. */

#include <libraw/libraw.h>

/* --- processing parameters ------------------------------------------- */

void leyline_shim_set_options(libraw_data_t *d, int use_camera_wb,
                              int half_size, int output_bps,
                              int no_auto_bright, int camera_native,
                              int highlight) {
    d->params.use_camera_wb = use_camera_wb;
    d->params.half_size = half_size;
    d->params.output_bps = output_bps;
    d->params.no_auto_bright = no_auto_bright;
    /* Clipped highlight handling, before demosaic (ADR 0050): 0 clips at
     * white, 2 blends the clipped and unclipped channels, 5 rebuilds the
     * saturated channel from the others. The caller decides; LibRaw's own
     * default is 0, which is what Leyline asked for implicitly until
     * ADR 0050 gave it a name. */
    d->params.highlight = highlight;
    if (camera_native) {
        /* Raw camera color space (ADR 0035): no color matrix applied, so a
         * camera profile (DCP) can operate on genuinely camera-native
         * data instead of LibRaw's own built-in sRGB conversion. Gamma
         * 1/1 = linear output — DNG color matrices are defined on linear
         * camera data, never a gamma-curved one. */
        d->params.output_color = 0;
        d->params.gamm[0] = 1.0;
        d->params.gamm[1] = 1.0;
    } else {
        d->params.output_color = 1; /* sRGB */
    }
}

/* --- colorimetry ------------------------------------------------------ */

/* The camera's XYZ->camera matrix, as LibRaw fills it during identify from
 * its per-model table. Rows are camera channels, columns X, Y, Z; the 4th
 * row (a 4-color sensor's emerald) is not copied — the develop pipeline is
 * three-channel throughout. Returns 0 when LibRaw knows no matrix for this
 * body, which is the caller's cue to fall back rather than to invert zeros. */
int leyline_shim_cam_xyz(const libraw_data_t *d, double out[9]) {
    int nonzero = 0;
    for (int i = 0; i < 3; i++) {
        for (int j = 0; j < 3; j++) {
            double v = (double)d->color.cam_xyz[i][j];
            out[i * 3 + j] = v;
            if (v != 0.0) nonzero = 1;
        }
    }
    return nonzero;
}

/* The camera's as-shot channel multipliers (white balance as the body
 * recorded it), as LibRaw fills them during identify. Four channels: a
 * three-color sensor leaves the fourth at 0 or repeats the second green.
 *
 * They are what `scale_colors` normalizes the image by, and the normalization
 * differs between highlight modes (dcraw divides by the smallest multiplier
 * when clipping, by the largest otherwise), which is why a caller asking for
 * highlight reconstruction needs them to undo the resulting global gain
 * change (ADR 0050 §3). Returns 0 when LibRaw has no camera white balance for
 * this file. */
int leyline_shim_cam_mul(const libraw_data_t *d, double out[4]) {
    if (d->color.cam_mul[0] <= 0.0f) return 0;
    for (int c = 0; c < 4; c++) out[c] = (double)d->color.cam_mul[c];
    return 1;
}

/* --- identification metadata ----------------------------------------- */

const char *leyline_shim_make(const libraw_data_t *d) { return d->idata.make; }
const char *leyline_shim_model(const libraw_data_t *d) { return d->idata.model; }
const char *leyline_shim_lens_make(const libraw_data_t *d) {
    return d->lens.LensMake;
}
const char *leyline_shim_lens_model(const libraw_data_t *d) {
    return d->lens.Lens;
}
float leyline_shim_iso(const libraw_data_t *d) { return d->other.iso_speed; }
float leyline_shim_shutter(const libraw_data_t *d) { return d->other.shutter; }
float leyline_shim_aperture(const libraw_data_t *d) { return d->other.aperture; }
float leyline_shim_focal_len(const libraw_data_t *d) { return d->other.focal_len; }
long long leyline_shim_timestamp(const libraw_data_t *d) {
    return (long long)d->other.timestamp;
}
int leyline_shim_flip(const libraw_data_t *d) { return d->sizes.flip; }
int leyline_shim_raw_width(const libraw_data_t *d) { return d->sizes.width; }
int leyline_shim_raw_height(const libraw_data_t *d) { return d->sizes.height; }

/* --- GPS (ADR 0040) ---------------------------------------------------- */

int leyline_shim_gps_parsed(const libraw_data_t *d) {
    return (int)d->other.parsed_gps.gpsparsed;
}
float leyline_shim_gps_lat_deg(const libraw_data_t *d) {
    return d->other.parsed_gps.latitude[0];
}
float leyline_shim_gps_lat_min(const libraw_data_t *d) {
    return d->other.parsed_gps.latitude[1];
}
float leyline_shim_gps_lat_sec(const libraw_data_t *d) {
    return d->other.parsed_gps.latitude[2];
}
float leyline_shim_gps_lon_deg(const libraw_data_t *d) {
    return d->other.parsed_gps.longitude[0];
}
float leyline_shim_gps_lon_min(const libraw_data_t *d) {
    return d->other.parsed_gps.longitude[1];
}
float leyline_shim_gps_lon_sec(const libraw_data_t *d) {
    return d->other.parsed_gps.longitude[2];
}
float leyline_shim_gps_altitude(const libraw_data_t *d) {
    return d->other.parsed_gps.altitude;
}
/* EXIF GPSAltitudeRef is a byte (0 = above sea level, 1 = below), not an
 * ASCII character like lat/longref — `altref` carries that raw byte. */
int leyline_shim_gps_altitude_below_sea_level(const libraw_data_t *d) {
    return d->other.parsed_gps.altref != 0;
}
int leyline_shim_gps_lat_south(const libraw_data_t *d) {
    return d->other.parsed_gps.latref == 'S';
}
int leyline_shim_gps_lon_west(const libraw_data_t *d) {
    return d->other.parsed_gps.longref == 'W';
}

/* --- processed image accessors ---------------------------------------- */

int leyline_shim_image_type(const libraw_processed_image_t *img) {
    return (int)img->type;
}
int leyline_shim_image_width(const libraw_processed_image_t *img) {
    return img->width;
}
int leyline_shim_image_height(const libraw_processed_image_t *img) {
    return img->height;
}
int leyline_shim_image_colors(const libraw_processed_image_t *img) {
    return img->colors;
}
int leyline_shim_image_bits(const libraw_processed_image_t *img) {
    return img->bits;
}
unsigned leyline_shim_image_size(const libraw_processed_image_t *img) {
    return img->data_size;
}
const unsigned char *leyline_shim_image_data(const libraw_processed_image_t *img) {
    return img->data;
}
