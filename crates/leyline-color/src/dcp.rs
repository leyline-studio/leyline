//! Camera profile (DCP) parsing and application (ADR 0035, ADR 0037).
//!
//! DCP is Adobe's per-camera color calibration format: a handful of
//! private TIFF/EP tags (documented in Adobe's public DNG Specification)
//! carrying color matrices, calibration illuminants, and (optionally) a
//! tone curve and hue/saturation/look tables. It is **not** ICC — `lcms2`
//! cannot read it — which is why this module exists instead of routing
//! through the ICC path this crate already has (`OutputTransform`).
//!
//! Per ADR 0037: the container is read with a home-grown tag reader rather
//! than a new DCP-specific dependency — DCP's tag set is small and fully
//! documented, not a reverse-engineered format. That reader is [`Ifd`], and
//! it is genuinely home-grown: the `tiff` crate was tried first and cannot
//! serve, because it decodes *images* and refuses any file without
//! `ImageWidth`, which a camera profile never has. This module is
//! read-only: Leyline never authors `.dcp` files.
//!
//! **Scope of the color math applied here, stated plainly:** this reads
//! and applies [`ColorMatrix1`]/[`ForwardMatrix1`] (falling back to
//! `ColorMatrix1`'s inverse when no forward matrix is present) to convert
//! linear camera-native RGB to linear sRGB via the CIE XYZ (D50) connection
//! space, using the standard published D50→sRGB matrix. When **both**
//! calibration illuminants are present, this averages their two matrices
//! rather than interpolating by the estimated scene color temperature the
//! full DNG spec describes — a deliberate simplification, not a
//! misreading of the spec. `ProfileHueSatMapData`/`ProfileLookTableData`
//! (the 3D hue/saturation/value correction tables) and `ProfileToneCurve`
//! are **neither parsed nor applied**: ADR 0037 scopes the eventual parser
//! to include them, but [`DcpProfile`] currently carries only the profile
//! name and the resolved camera→XYZ matrix, and [`tag_id`] lists only the
//! tags that matrix is built from. A profile whose look is largely carried
//! by those tables will therefore render differently here than in Adobe's
//! own converter — a known gap, not a silently-dropped requirement.
//! Since ADR 0062 the two calibration illuminants are **interpolated for the
//! scene's light** rather than averaged — the simplification this module used
//! to carry, measured at up to 0.044 on a saturated red. The averaged path is
//! still here, unchanged, because `camera_profile::v1` is frozen on it.
//!
//! **What is verified, as of 2026-08:** real third-party `.dcp` files parse
//! (two Canon linear profiles), and the matrix path keeps a neutral sensor
//! triple neutral through camera→XYZ(D50)→sRGB — a check that needs no
//! reference renderer, and that a transposed matrix, a swapped
//! numerator/denominator or a mis-read byte order would all fail. Run it
//! with `LEYLINE_TEST_DCP` pointing at a directory of profiles.
//!
//! **What is still not verified:** agreement with Adobe's *rendered output*.
//! That needs Lightroom or ACR to produce the reference, and no amount of
//! reading `.dcp` files replaces it. Until then the feature stays marked
//! experimental — which now names a narrower gap than it did: the container
//! and the neutral axis are checked, the look is not.
//!
//! [`ColorMatrix1`]: https://helpx.adobe.com/camera-raw/digital-negative.html
//! [`ForwardMatrix1`]: https://helpx.adobe.com/camera-raw/digital-negative.html

/// Adobe DNG Specification private tag IDs this module reads. Not a
/// complete DNG/DCP tag set — only what a matrix-based render needs.
mod tag_id {
    pub const CALIBRATION_ILLUMINANT_1: u16 = 50778;
    pub const CALIBRATION_ILLUMINANT_2: u16 = 50779;
    pub const COLOR_MATRIX_1: u16 = 50721;
    pub const COLOR_MATRIX_2: u16 = 50722;
    pub const FORWARD_MATRIX_1: u16 = 50964;
    pub const FORWARD_MATRIX_2: u16 = 50965;
    pub const PROFILE_NAME: u16 = 50936;
    // The table tags (ADR 0063). Their numbering is easy to get wrong from
    // memory — 50937 is the *dims*, not the first data block — so it was
    // taken from the DNG specification rather than reconstructed.
    pub const HUE_SAT_MAP_DIMS: u16 = 50937;
    pub const HUE_SAT_MAP_DATA_1: u16 = 50938;
    pub const HUE_SAT_MAP_DATA_2: u16 = 50939;
    pub const TONE_CURVE: u16 = 50940;
    pub const LOOK_TABLE_DIMS: u16 = 50981;
    pub const LOOK_TABLE_DATA: u16 = 50982;
}

/// What can go wrong reading a `.dcp` file.
#[derive(Debug, thiserror::Error)]
pub enum DcpError {
    /// The file isn't a valid TIFF/EP container, or a required tag is
    /// missing/malformed.
    #[error("invalid DCP profile: {0}")]
    Invalid(String),
}

type Result<T> = std::result::Result<T, DcpError>;

/// A 3×3 matrix, row-major, as DCP color matrices are stored.
pub type Matrix3 = [[f64; 3]; 3];

/// A parsed camera profile (ADR 0035), ready to convert linear
/// camera-native RGB samples to linear sRGB.
#[derive(Debug, Clone, PartialEq)]
pub struct DcpProfile {
    /// Human-readable profile name, when the file declares one.
    pub name: Option<String>,
    /// The matrix this profile's camera→XYZ(D50) conversion is built from
    /// — already resolved from whichever combination of `ColorMatrix`/
    /// `ForwardMatrix` tags the file declared (see the module doc for the
    /// exact resolution rule).
    camera_to_xyz_d50: Matrix3,
    /// The per-illuminant calibrations, sorted by temperature ascending.
    /// One entry when the file declares a single illuminant, two when it
    /// declares both — the common case (ADR 0062).
    calibrations: Vec<Calibration>,
    /// The look table (ADR 0063), applied late and shared by both
    /// illuminants — the DNG format declares only one.
    look_table: Option<HsvTable>,
    /// The hue/saturation maps, one per calibration illuminant.
    hue_sat_map_1: Option<HsvTable>,
    hue_sat_map_2: Option<HsvTable>,
    /// The profile's tone curve as `(x, y)` control points, ascending in
    /// `x`. A "linear" profile carries exactly two — `(0,0)` and `(1,1)`,
    /// the identity, written out literally.
    tone_curve: Vec<(f32, f32)>,
    /// `ColorMatrix`, XYZ→camera, per illuminant. Needed on its own to
    /// find the scene white point from the camera's as-shot neutral, which
    /// is a chicken-and-egg the DNG spec resolves by iteration.
    xyz_to_camera: Vec<Calibration>,
}

/// XYZ (D50) → linear ProPhoto RGB (ROMM), the space DNG tables are defined
/// against. ProPhoto's white point *is* D50, so no chromatic adaptation
/// belongs here — adding one is a natural mistake that tilts every colour.
const XYZ_D50_TO_PROPHOTO: Matrix3 = [
    [1.3459433, -0.2556075, -0.0511118],
    [-0.5445989, 1.5081673, 0.0205351],
    [0.0000000, 0.0000000, 1.2118128],
];

/// The inverse of [`XYZ_D50_TO_PROPHOTO`].
const PROPHOTO_TO_XYZ_D50: Matrix3 = [
    [0.7976749, 0.1351917, 0.0313534],
    [0.2880402, 0.7118741, 0.0000857],
    [0.0000000, 0.0000000, 0.8252100],
];

/// A profile resolved for one scene light, ready to convert pixels
/// (ADR 0063).
///
/// Everything that depends on the light — the matrix, the blended
/// hue/saturation map — is settled once here rather than per pixel: neither
/// changes within an image, and doing it per sample would dominate the cost
/// of the conversion itself.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedProfile {
    camera_to_xyz_d50: Matrix3,
    hue_sat_map: Option<HsvTable>,
    look_table: Option<HsvTable>,
    /// Control points, or empty for the identity.
    tone_curve: Vec<(f32, f32)>,
}

impl PreparedProfile {
    /// Converts one camera-native linear RGB sample into the linear
    /// Rec. 2020 working space, through every table the profile carries.
    ///
    /// The order is the specification's, and it is not the one the names
    /// suggest: the look table runs **before** the tone curve (ADR 0063 §1).
    pub fn camera_to_working(&self, camera_rgb: [f32; 3]) -> [f32; 3] {
        let mut rgb = camera_rgb;

        // 1. Hue/saturation map, early, on the camera's own values.
        if let Some(map) = &self.hue_sat_map {
            rgb = through_hsv(rgb, |h, s, v| map.apply(h, s, v));
        }

        // 2. Camera → XYZ(D50) → ProPhoto, where the remaining tables live.
        let xyz = apply_f32(self.camera_to_xyz_d50, rgb);
        let mut pro = apply_f32(XYZ_D50_TO_PROPHOTO, xyz);

        // 3. Look table, then tone curve. Both are defined on [0, 1] while
        //    the working buffer deliberately is not (ADR 0044): a sample
        //    above white passes through untouched rather than being clipped
        //    to receive a look (ADR 0063 §2).
        if let Some(table) = &self.look_table {
            let bounded = pro.iter().all(|c| (0.0..=1.0).contains(c));
            if bounded {
                pro = through_hsv(pro, |h, s, v| table.apply(h, s, v));
            }
        }
        if !self.tone_curve.is_empty() {
            for c in &mut pro {
                if (0.0..=1.0).contains(c) {
                    *c = tone_curve_at(&self.tone_curve, *c);
                }
            }
        }

        // 4. Back out to the working space.
        let xyz = apply_f32(PROPHOTO_TO_XYZ_D50, pro);
        let working =
            xyz_d50_to_linear_rec2020([f64::from(xyz[0]), f64::from(xyz[1]), f64::from(xyz[2])]);
        [working[0] as f32, working[1] as f32, working[2] as f32]
    }
}

/// Runs `f` on the sample's hue/saturation/value and returns to RGB.
fn through_hsv(rgb: [f32; 3], f: impl FnOnce(f32, f32, f32) -> (f32, f32, f32)) -> [f32; 3] {
    let (h, s, v) = rgb_to_hsv(rgb);
    let (h, s, v) = f(h, s, v);
    hsv_to_rgb(h, s, v)
}

/// RGB to hue (degrees), saturation and value, all on `[0, 1]` inputs.
fn rgb_to_hsv(rgb: [f32; 3]) -> (f32, f32, f32) {
    let max = rgb[0].max(rgb[1]).max(rgb[2]);
    let min = rgb[0].min(rgb[1]).min(rgb[2]);
    let span = max - min;
    let hue = if span <= 0.0 {
        0.0
    } else if max == rgb[0] {
        60.0 * (((rgb[1] - rgb[2]) / span) % 6.0)
    } else if max == rgb[1] {
        60.0 * ((rgb[2] - rgb[0]) / span + 2.0)
    } else {
        60.0 * ((rgb[0] - rgb[1]) / span + 4.0)
    };
    let saturation = if max <= 0.0 { 0.0 } else { span / max };
    (hue.rem_euclid(360.0), saturation, max)
}

/// The inverse of [`rgb_to_hsv`].
fn hsv_to_rgb(hue: f32, saturation: f32, value: f32) -> [f32; 3] {
    let h = hue.rem_euclid(360.0) / 60.0;
    let sector = h.floor();
    let f = h - sector;
    let (p, q, t) = (
        value * (1.0 - saturation),
        value * (1.0 - saturation * f),
        value * (1.0 - saturation * (1.0 - f)),
    );
    match sector as i32 % 6 {
        0 => [value, t, p],
        1 => [q, value, p],
        2 => [p, value, t],
        3 => [p, q, value],
        4 => [t, p, value],
        _ => [value, p, q],
    }
}

/// The tone curve's value at `x`, by linear interpolation between control
/// points. Outside the declared range the nearest end holds.
fn tone_curve_at(points: &[(f32, f32)], x: f32) -> f32 {
    match points.binary_search_by(|(px, _)| px.total_cmp(&x)) {
        Ok(i) => points[i].1,
        Err(0) => points[0].1,
        Err(i) if i >= points.len() => points[points.len() - 1].1,
        Err(i) => {
            let (x0, y0) = points[i - 1];
            let (x1, y1) = points[i];
            if (x1 - x0).abs() < f32::EPSILON {
                y0
            } else {
                y0 + (y1 - y0) * (x - x0) / (x1 - x0)
            }
        }
    }
}

/// `m · v` in `f32`.
fn apply_f32(m: Matrix3, v: [f32; 3]) -> [f32; 3] {
    [
        (m[0][0] * f64::from(v[0]) + m[0][1] * f64::from(v[1]) + m[0][2] * f64::from(v[2])) as f32,
        (m[1][0] * f64::from(v[0]) + m[1][1] * f64::from(v[1]) + m[1][2] * f64::from(v[2])) as f32,
        (m[2][0] * f64::from(v[0]) + m[2][1] * f64::from(v[1]) + m[2][2] * f64::from(v[2])) as f32,
    ]
}

/// A profile's hue/saturation/value correction table (ADR 0063).
///
/// A sampled cube — hue × saturation × value — of three deltas per entry: a
/// hue *shift* in degrees, a saturation *scale*, a value *scale*. It is what
/// gives a profile its look, where the matrix only gives it correctness.
///
/// Dimensions vary widely between profiles (4 608 to 81 000 entries among the
/// three real ones inventoried), so nothing about the shape is assumed.
#[derive(Debug, Clone, PartialEq)]
pub struct HsvTable {
    hue_divisions: usize,
    sat_divisions: usize,
    val_divisions: usize,
    /// `hue_divisions * sat_divisions * val_divisions` triples.
    data: Vec<[f32; 3]>,
}

impl HsvTable {
    /// Builds a table from its declared dimensions and flat data, refusing a
    /// pair that does not describe a whole cube.
    fn new(dims: &[u32], data: &[f32]) -> Option<HsvTable> {
        let [hue, sat, val] = dims else {
            return None;
        };
        let (hue, sat, val) = (*hue as usize, *sat as usize, *val as usize);
        if hue == 0 || sat == 0 || val == 0 || data.len() != hue * sat * val * 3 {
            return None;
        }
        Some(HsvTable {
            hue_divisions: hue,
            sat_divisions: sat,
            val_divisions: val,
            data: data.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
        })
    }

    /// The entry at a lattice point, hue wrapping around.
    fn at(&self, h: usize, s: usize, v: usize) -> [f32; 3] {
        let h = h % self.hue_divisions;
        let s = s.min(self.sat_divisions - 1);
        let v = v.min(self.val_divisions - 1);
        self.data[(v * self.sat_divisions + s) * self.hue_divisions + h]
    }

    /// Applies the table to one HSV sample: `hue` in degrees `[0, 360)`,
    /// `sat` and `val` in `[0, 1]`.
    ///
    /// Trilinear, with **hue cyclic**: the last hue division is adjacent to
    /// the first. Treating hue as an open axis instead leaves a visible seam
    /// on reds, which is exactly the kind of defect that survives a test on
    /// a synthetic gradient and shows up on a face.
    pub fn apply(&self, hue: f32, sat: f32, val: f32) -> (f32, f32, f32) {
        let hue_scale = self.hue_divisions as f32 / 360.0;
        let h = (hue.rem_euclid(360.0)) * hue_scale;
        let h0 = h.floor();
        let hf = h - h0;
        let h0 = h0 as usize;

        let sat_scale = (self.sat_divisions - 1) as f32;
        let s = (sat.clamp(0.0, 1.0) * sat_scale).min(sat_scale);
        let s0 = s.floor();
        let sf = s - s0;
        let s0 = s0 as usize;

        // A single value division is a legitimate shape — two of the three
        // real profiles use it — and degenerates to no interpolation here.
        let (v0, vf) = if self.val_divisions == 1 {
            (0usize, 0.0)
        } else {
            let val_scale = (self.val_divisions - 1) as f32;
            let v = (val.clamp(0.0, 1.0) * val_scale).min(val_scale);
            let floor = v.floor();
            (floor as usize, v - floor)
        };

        let mut out = [0.0f32; 3];
        for (dv, wv) in [(0usize, 1.0 - vf), (1, vf)] {
            if wv == 0.0 {
                continue;
            }
            for (ds, ws) in [(0usize, 1.0 - sf), (1, sf)] {
                if ws == 0.0 {
                    continue;
                }
                for (dh, wh) in [(0usize, 1.0 - hf), (1, hf)] {
                    if wh == 0.0 {
                        continue;
                    }
                    let entry = self.at(h0 + dh, s0 + ds, v0 + dv);
                    let weight = wv * ws * wh;
                    for i in 0..3 {
                        out[i] += entry[i] * weight;
                    }
                }
            }
        }

        // Hue is a shift in degrees; saturation and value are scales.
        (hue + out[0], (sat * out[1]).clamp(0.0, 1.0), val * out[2])
    }

    /// `a` and `b` blended by `weight` on `a` — the same mireds weight the
    /// matrices use, so a profile never interpolates its matrices under one
    /// light and its tables under another (ADR 0063 §4).
    fn blend(a: &HsvTable, b: &HsvTable, weight: f32) -> Option<HsvTable> {
        if a.hue_divisions != b.hue_divisions
            || a.sat_divisions != b.sat_divisions
            || a.val_divisions != b.val_divisions
        {
            return None;
        }
        Some(HsvTable {
            hue_divisions: a.hue_divisions,
            sat_divisions: a.sat_divisions,
            val_divisions: a.val_divisions,
            data: a
                .data
                .iter()
                .zip(&b.data)
                .map(|(x, y)| {
                    [
                        x[0] * weight + y[0] * (1.0 - weight),
                        x[1] * weight + y[1] * (1.0 - weight),
                        x[2] * weight + y[2] * (1.0 - weight),
                    ]
                })
                .collect(),
        })
    }
}

/// One illuminant's calibration: the matrix, and the light it was measured
/// under.
#[derive(Debug, Clone, PartialEq)]
struct Calibration {
    matrix: Matrix3,
    /// Colour temperature in kelvin.
    temperature: f64,
}

/// Colour temperature of a DNG `CalibrationIlluminant` code, in kelvin.
///
/// These are the DNG SDK's own values, not the physically exact ones —
/// illuminant A is 2856 K in the standard and 2850 K here. The goal is to
/// reproduce the reference implementation's blend, so its numbers are the
/// right ones (ADR 0062 §1).
fn illuminant_temperature(code: u32) -> Option<f64> {
    Some(match code {
        1 => 6500.0,      // Daylight
        2 => 4200.0,      // Fluorescent
        3 | 17 => 2850.0, // Tungsten, Standard Light A
        4 => 5500.0,      // Flash
        10 => 5500.0,     // Fine weather
        11 => 6500.0,     // Cloudy weather
        12 => 7500.0,     // Shade
        13 => 5700.0,     // Daylight fluorescent
        14 => 4600.0,     // Day white fluorescent
        15 => 3800.0,     // Cool white fluorescent
        16 => 2900.0,     // White fluorescent
        18 => 4874.0,     // Standard light B
        19 => 6774.0,     // Standard light C
        20 => 5500.0,     // D55
        21 => 6500.0,     // D65
        22 => 7500.0,     // D75
        23 => 5000.0,     // D50
        24 => 3200.0,     // ISO studio tungsten
        _ => return None,
    })
}

/// Robertson's isotemperature lines, as the DNG SDK tabulates them:
/// `(reciprocal megakelvin, u, v, slope)`.
///
/// Standard published colorimetry (Wyszecki & Stiles), and the same table
/// every DNG-conformant implementation uses — which is what makes two
/// implementations agree on a temperature rather than merely agree in
/// spirit.
const ROBERTSON: [[f64; 4]; 31] = [
    [0.0, 0.18006, 0.26352, -0.24341],
    [10.0, 0.18066, 0.26589, -0.25479],
    [20.0, 0.18133, 0.26846, -0.26876],
    [30.0, 0.18208, 0.27119, -0.28539],
    [40.0, 0.18293, 0.27407, -0.30470],
    [50.0, 0.18388, 0.27709, -0.32675],
    [60.0, 0.18494, 0.28021, -0.35156],
    [70.0, 0.18611, 0.28342, -0.37915],
    [80.0, 0.18740, 0.28668, -0.40955],
    [90.0, 0.18880, 0.28997, -0.44278],
    [100.0, 0.19032, 0.29326, -0.47888],
    [125.0, 0.19462, 0.30141, -0.58204],
    [150.0, 0.19962, 0.30921, -0.70471],
    [175.0, 0.20525, 0.31647, -0.84901],
    [200.0, 0.21142, 0.32312, -1.0182],
    [225.0, 0.21807, 0.32909, -1.2168],
    [250.0, 0.22511, 0.33439, -1.4512],
    [275.0, 0.23247, 0.33904, -1.7298],
    [300.0, 0.24010, 0.34308, -2.0637],
    [325.0, 0.24702, 0.34655, -2.4681],
    [350.0, 0.25591, 0.34951, -2.9641],
    [375.0, 0.26400, 0.35200, -3.5814],
    [400.0, 0.27218, 0.35407, -4.3633],
    [425.0, 0.28039, 0.35577, -5.3762],
    [450.0, 0.28863, 0.35714, -6.7262],
    [475.0, 0.29685, 0.35823, -8.5955],
    [500.0, 0.30505, 0.35907, -11.324],
    [525.0, 0.31320, 0.35968, -15.628],
    [550.0, 0.32129, 0.36011, -23.325],
    [575.0, 0.32931, 0.36038, -40.770],
    [600.0, 0.33724, 0.36051, -116.45],
];

/// The blend weight of the cooler calibration for a scene temperature,
/// linear in mireds and clamped to the calibrated interval (ADR 0062 §1).
fn mireds_mix(temperature_k: f64, cool_k: f64, warm_k: f64) -> f64 {
    if temperature_k <= cool_k {
        return 1.0;
    }
    if temperature_k >= warm_k {
        return 0.0;
    }
    let (inv, inv_cool, inv_warm) = (1.0 / temperature_k, 1.0 / cool_k, 1.0 / warm_k);
    ((inv - inv_warm) / (inv_cool - inv_warm)).clamp(0.0, 1.0)
}

/// `a·wa + b·wb`, elementwise.
fn mix3(a: Matrix3, wa: f64, b: Matrix3, wb: f64) -> Matrix3 {
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = a[i][j] * wa + b[i][j] * wb;
        }
    }
    out
}

/// D65, the fallback when nothing says what the light was.
pub const FALLBACK_TEMPERATURE_K: f64 = 6500.0;

/// The colour temperature of a chromaticity, by Robertson's method.
fn xy_to_temperature(x: f64, y: f64) -> f64 {
    let denom = 1.5 - x + 6.0 * y;
    if denom.abs() < 1e-12 {
        return FALLBACK_TEMPERATURE_K;
    }
    let (u, v) = (2.0 * x / denom, 3.0 * y / denom);
    let mut last_dt = 0.0;
    for index in 1..=30 {
        let slope = ROBERTSON[index][3];
        let len = (1.0 + slope * slope).sqrt();
        let (du, dv) = (1.0 / len, slope / len);
        let uu = u - ROBERTSON[index][1];
        let vv = v - ROBERTSON[index][2];
        let mut dt = -uu * dv + vv * du;
        if dt <= 0.0 || index == 30 {
            dt = (-dt.min(0.0)).abs();
            let f = if index == 1 { 0.0 } else { dt / (last_dt + dt) };
            let r = ROBERTSON[index - 1][0] * f + ROBERTSON[index][0] * (1.0 - f);
            return if r.abs() < 1e-12 {
                FALLBACK_TEMPERATURE_K
            } else {
                1.0e6 / r
            };
        }
        last_dt = dt;
    }
    FALLBACK_TEMPERATURE_K
}

/// The version number a real `.dcp` writes where TIFF writes 42.
///
/// A camera profile is a TIFF *structure* — byte-order mark, version, IFD
/// offset, tag table — carrying a version of its own. Adobe's tools write
/// `0x4352` there, and every `.dcp` in the wild begins `49 49 52 43`
/// (`II` then `0x4352`, little-endian).
const DCP_VERSION: u16 = 0x4352;

/// TIFF's own version number, which a fixture may legitimately carry.
const TIFF_VERSION: u16 = 42;

/// The tag directory of a camera profile, read directly.
///
/// ADR 0037 described "a home-grown tag reader built on the `tiff` crate's
/// generic IFD decoder". That crate turned out to have no such thing:
/// `tiff::Decoder` decodes *images*, and refuses any file without
/// `ImageWidth`. A camera profile has no image at all — it is a bare
/// directory of metadata — so a real `.dcp` could never get past it. The
/// fixtures that passed were self-built TIFF images carrying DCP tags,
/// which is why nothing caught it.
///
/// So the reader is genuinely home-grown, as the ADR intended. A TIFF
/// directory is a small, fully documented structure: an 8-byte header,
/// then entries of `(tag, type, count, value-or-offset)`, each 12 bytes.
/// Nothing here interprets pixels, because there are none.
struct Ifd<'a> {
    bytes: &'a [u8],
    little_endian: bool,
    /// `(type, count, value-or-offset field)` per tag.
    entries: std::collections::HashMap<u16, (u16, u32, [u8; 4])>,
}

impl<'a> Ifd<'a> {
    /// Reads the header and the first directory.
    fn parse(bytes: &'a [u8]) -> Result<Ifd<'a>> {
        let header: [u8; 8] = bytes
            .get(..8)
            .and_then(|h| h.try_into().ok())
            .ok_or_else(|| DcpError::Invalid("too short to be a camera profile".to_owned()))?;

        let little_endian = match &header[..2] {
            b"II" => true,
            b"MM" => false,
            _ => {
                return Err(DcpError::Invalid(
                    "not a camera profile: no TIFF byte-order mark".to_owned(),
                ));
            }
        };
        let u16_at = |b: [u8; 2]| {
            if little_endian {
                u16::from_le_bytes(b)
            } else {
                u16::from_be_bytes(b)
            }
        };
        let u32_at = |b: [u8; 4]| {
            if little_endian {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            }
        };

        let version = u16_at([header[2], header[3]]);
        if version != DCP_VERSION && version != TIFF_VERSION {
            return Err(DcpError::Invalid(format!(
                "not a camera profile: version {version} is neither DCP's {DCP_VERSION} \
                 nor TIFF's {TIFF_VERSION}"
            )));
        }

        let offset = u32_at([header[4], header[5], header[6], header[7]]) as usize;
        let count_bytes: [u8; 2] = bytes
            .get(offset..offset + 2)
            .and_then(|b| b.try_into().ok())
            .ok_or_else(|| DcpError::Invalid("directory offset is past the file".to_owned()))?;
        let count = u16_at(count_bytes) as usize;

        let mut entries = std::collections::HashMap::with_capacity(count);
        for i in 0..count {
            let at = offset + 2 + i * 12;
            let entry: [u8; 12] = bytes
                .get(at..at + 12)
                .and_then(|b| b.try_into().ok())
                .ok_or_else(|| DcpError::Invalid("directory runs past the file".to_owned()))?;
            let tag = u16_at([entry[0], entry[1]]);
            let kind = u16_at([entry[2], entry[3]]);
            let n = u32_at([entry[4], entry[5], entry[6], entry[7]]);
            entries.insert(tag, (kind, n, [entry[8], entry[9], entry[10], entry[11]]));
        }

        Ok(Ifd {
            bytes,
            little_endian,
            entries,
        })
    }

    fn u16_at(&self, b: [u8; 2]) -> u16 {
        if self.little_endian {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        }
    }

    fn u32_at(&self, b: [u8; 4]) -> u32 {
        if self.little_endian {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        }
    }

    fn i32_at(&self, b: [u8; 4]) -> i32 {
        if self.little_endian {
            i32::from_le_bytes(b)
        } else {
            i32::from_be_bytes(b)
        }
    }

    /// The raw bytes of a tag's value, inline or followed to its offset.
    ///
    /// TIFF stores a value inside the entry when it fits in four bytes and
    /// at an offset otherwise — the one subtlety of the format, and the
    /// reason a matrix (nine rationals, 72 bytes) is never inline while a
    /// short illuminant code always is.
    fn value_bytes(&self, tag: u16) -> Option<&'a [u8]> {
        let (kind, count, field) = *self.entries.get(&tag)?;
        let unit = match kind {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 | 11 => 4,
            5 | 10 | 12 => 8,
            _ => return None,
        };
        let len = unit * count as usize;
        if len <= 4 {
            // Borrowing from the entry itself is impossible — it was copied
            // into the map — so the inline case is served from the file at
            // the entry's own position instead.
            let at = self.inline_position(tag)?;
            return self.bytes.get(at..at + len);
        }
        let at = self.u32_at(field) as usize;
        self.bytes.get(at..at + len)
    }

    /// Where a tag's inline value sits in the file: the last four bytes of
    /// its directory entry.
    fn inline_position(&self, tag: u16) -> Option<usize> {
        let header: [u8; 4] = self.bytes.get(4..8)?.try_into().ok()?;
        let offset = self.u32_at(header) as usize;
        let count = self.u16_at(self.bytes.get(offset..offset + 2)?.try_into().ok()?) as usize;
        (0..count).find_map(|i| {
            let at = offset + 2 + i * 12;
            let entry: [u8; 2] = self.bytes.get(at..at + 2)?.try_into().ok()?;
            (self.u16_at(entry) == tag).then_some(at + 8)
        })
    }

    /// A tag read as a list of decimals. DCP matrices are SRATIONAL —
    /// signed numerator over denominator — which is why this cannot be a
    /// plain integer read.
    fn rationals(&self, tag: u16) -> Option<Vec<f64>> {
        let (kind, _, _) = *self.entries.get(&tag)?;
        let bytes = self.value_bytes(tag)?;
        match kind {
            // SRATIONAL and RATIONAL: two 4-byte halves each.
            5 | 10 => Some(
                bytes
                    .chunks_exact(8)
                    .map(|c| {
                        let (n, d) = (
                            self.i32_at(c[..4].try_into().unwrap()),
                            self.i32_at(c[4..].try_into().unwrap()),
                        );
                        if d == 0 {
                            0.0
                        } else {
                            f64::from(n) / f64::from(d)
                        }
                    })
                    .collect(),
            ),
            // DOUBLE, which a hand-written profile may use instead.
            12 => Some(
                bytes
                    .chunks_exact(8)
                    .map(|c| {
                        let raw = c.try_into().unwrap();
                        if self.little_endian {
                            f64::from_le_bytes(raw)
                        } else {
                            f64::from_be_bytes(raw)
                        }
                    })
                    .collect(),
            ),
            _ => None,
        }
    }

    /// A SHORT or LONG tag, as the calibration illuminants are stored.
    fn integer(&self, tag: u16) -> Option<u32> {
        let (kind, _, _) = *self.entries.get(&tag)?;
        let bytes = self.value_bytes(tag)?;
        match kind {
            3 => Some(u32::from(self.u16_at(bytes.get(..2)?.try_into().ok()?))),
            4 => Some(self.u32_at(bytes.get(..4)?.try_into().ok()?)),
            _ => None,
        }
    }

    /// A tag read as 32-bit floats — how every DCP table is stored.
    fn floats(&self, tag: u16) -> Option<Vec<f32>> {
        let (kind, _, _) = *self.entries.get(&tag)?;
        if kind != 11 {
            return None;
        }
        let bytes = self.value_bytes(tag)?;
        Some(
            bytes
                .chunks_exact(4)
                .map(|c| {
                    let raw = c.try_into().unwrap();
                    if self.little_endian {
                        f32::from_le_bytes(raw)
                    } else {
                        f32::from_be_bytes(raw)
                    }
                })
                .collect(),
        )
    }

    /// A tag read as unsigned 32-bit integers — the table dimensions.
    fn longs(&self, tag: u16) -> Option<Vec<u32>> {
        let (kind, _, _) = *self.entries.get(&tag)?;
        if kind != 4 {
            return None;
        }
        let bytes = self.value_bytes(tag)?;
        Some(
            bytes
                .chunks_exact(4)
                .map(|c| self.u32_at(c.try_into().unwrap()))
                .collect(),
        )
    }

    /// An ASCII tag, trailing NUL removed.
    fn ascii(&self, tag: u16) -> Option<String> {
        let bytes = self.value_bytes(tag)?;
        let text = String::from_utf8_lossy(bytes);
        Some(text.trim_end_matches('\0').to_owned())
    }
}

impl DcpProfile {
    /// Parses a `.dcp` file's bytes.
    pub fn parse(bytes: &[u8]) -> Result<DcpProfile> {
        let ifd = Ifd::parse(bytes)?;

        let color_matrix_1 = read_matrix(&ifd, tag_id::COLOR_MATRIX_1)?
            .ok_or_else(|| DcpError::Invalid("missing required ColorMatrix1 tag".to_owned()))?;
        let color_matrix_2 = read_matrix(&ifd, tag_id::COLOR_MATRIX_2)?;
        let forward_matrix_1 = read_matrix(&ifd, tag_id::FORWARD_MATRIX_1)?;
        let forward_matrix_2 = read_matrix(&ifd, tag_id::FORWARD_MATRIX_2)?;
        let illuminant_1 = ifd
            .integer(tag_id::CALIBRATION_ILLUMINANT_1)
            .and_then(illuminant_temperature);
        let illuminant_2 = ifd
            .integer(tag_id::CALIBRATION_ILLUMINANT_2)
            .and_then(illuminant_temperature);
        let name = ifd.ascii(tag_id::PROFILE_NAME).filter(|n| !n.is_empty());

        // Prefer the forward matrix/matrices (the DNG spec's documented
        // preference when present); average both illuminants' matrices
        // when both are declared, matrix-invert ColorMatrix1 as the last
        // resort. All three branches are legitimate per-file shapes, not
        // error cases.
        let camera_to_xyz_d50 = match (forward_matrix_1, forward_matrix_2) {
            (Some(a), Some(b)) => average(a, b),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => {
                let color_matrix = match color_matrix_2 {
                    Some(cm2) => average(color_matrix_1, cm2),
                    None => color_matrix_1,
                };
                invert(color_matrix)
                    .ok_or_else(|| DcpError::Invalid("ColorMatrix1 is not invertible".to_owned()))?
            }
        };

        // The per-illuminant calibrations `camera_profile::v2` interpolates
        // between (ADR 0062). A file declaring one illuminant, or matrices
        // for only one, yields a single entry — and a single entry means no
        // interpolation to do, whatever the scene temperature.
        let pair = |a: Option<Matrix3>, b: Option<Matrix3>| -> Vec<Calibration> {
            let mut out = Vec::new();
            if let (Some(m), Some(t)) = (a, illuminant_1) {
                out.push(Calibration {
                    matrix: m,
                    temperature: t,
                });
            }
            if let (Some(m), Some(t)) = (b, illuminant_2) {
                out.push(Calibration {
                    matrix: m,
                    temperature: t,
                });
            }
            out.sort_by(|x, y| x.temperature.total_cmp(&y.temperature));
            out
        };

        let mut calibrations = pair(forward_matrix_1, forward_matrix_2);
        if calibrations.is_empty() {
            // No forward matrices: invert the colour matrices, exactly as
            // the averaged path does, one illuminant at a time.
            calibrations = pair(invert(color_matrix_1), color_matrix_2.and_then(invert));
        }
        let xyz_to_camera = pair(Some(color_matrix_1), color_matrix_2);

        // The tables (ADR 0063). A profile declaring dimensions that do not
        // match its data is treated as having no table rather than refused:
        // the matrices are still usable, and a look is not worth losing a
        // profile over.
        let hsm_dims = ifd.longs(tag_id::HUE_SAT_MAP_DIMS);
        let table = |dims: &Option<Vec<u32>>, tag: u16| -> Option<HsvTable> {
            HsvTable::new(dims.as_ref()?, &ifd.floats(tag)?)
        };
        let hue_sat_map_1 = table(&hsm_dims, tag_id::HUE_SAT_MAP_DATA_1);
        let hue_sat_map_2 = table(&hsm_dims, tag_id::HUE_SAT_MAP_DATA_2);
        let look_table = table(&ifd.longs(tag_id::LOOK_TABLE_DIMS), tag_id::LOOK_TABLE_DATA);
        let tone_curve = ifd
            .floats(tag_id::TONE_CURVE)
            .map(|v| v.chunks_exact(2).map(|c| (c[0], c[1])).collect())
            .unwrap_or_default();

        Ok(DcpProfile {
            name,
            camera_to_xyz_d50,
            calibrations,
            look_table,
            hue_sat_map_1,
            hue_sat_map_2,
            tone_curve,
            xyz_to_camera,
        })
    }

    /// Camera→XYZ(D50) for a scene of `temperature_k`, interpolated between
    /// the profile's calibrations (ADR 0062 §1).
    ///
    /// The blend is linear in **reciprocal** temperature — mireds — which is
    /// where a colour difference is perceptually even. Blending on kelvin
    /// instead is the natural mistake, and it is wrong in the middle of the
    /// interval, exactly where interpolation is supposed to help.
    pub fn camera_to_xyz_at(&self, temperature_k: f64) -> Matrix3 {
        match self.calibrations.as_slice() {
            [] => self.camera_to_xyz_d50,
            [only] => only.matrix,
            [cool, warm, ..] => {
                let mix = mireds_mix(temperature_k, cool.temperature, warm.temperature);
                mix3(cool.matrix, mix, warm.matrix, 1.0 - mix)
            }
        }
    }

    /// Settles everything that depends on the scene's light, once
    /// (ADR 0063 §4).
    ///
    /// The two hue/saturation maps are blended by the **same** mireds weight
    /// the matrices use: a profile must never interpolate its matrices under
    /// one light and its tables under another.
    pub fn prepare(&self, temperature_k: f64) -> PreparedProfile {
        let hue_sat_map = match (&self.hue_sat_map_1, &self.hue_sat_map_2) {
            (Some(a), Some(b)) => {
                let weight = match self.calibrations.as_slice() {
                    [cool, warm, ..] => {
                        mireds_mix(temperature_k, cool.temperature, warm.temperature) as f32
                    }
                    _ => 1.0,
                };
                HsvTable::blend(a, b, weight).or_else(|| Some(a.clone()))
            }
            (Some(only), None) | (None, Some(only)) => Some(only.clone()),
            (None, None) => None,
        };
        PreparedProfile {
            camera_to_xyz_d50: self.camera_to_xyz_at(temperature_k),
            hue_sat_map,
            look_table: self.look_table.clone(),
            // Two points is the identity a "linear" profile writes out
            // literally; carrying it would cost a lookup per sample to
            // change nothing.
            tone_curve: if self.tone_curve.len() > 2 {
                self.tone_curve.clone()
            } else {
                Vec::new()
            },
        }
    }

    /// A one-line description of which tables this profile carries — for
    /// diagnostics and for the tests that check a real file was read whole.
    pub fn tables_summary(&self) -> String {
        let shape = |t: &Option<HsvTable>| match t {
            Some(t) => format!(
                "{}x{}x{}",
                t.hue_divisions, t.sat_divisions, t.val_divisions
            ),
            None => "aucune".to_owned(),
        };
        format!(
            "hue_sat_map_1 {}, hue_sat_map_2 {}, look_table {}, tone_curve {} point(s)",
            shape(&self.hue_sat_map_1),
            shape(&self.hue_sat_map_2),
            shape(&self.look_table),
            self.tone_curve.len()
        )
    }

    /// The scene temperature implied by the camera's as-shot neutral, in
    /// kelvin (ADR 0062 §2).
    ///
    /// Chicken and egg: the matrix that turns the neutral into a chromaticity
    /// depends on the temperature, which is what we are trying to find. The
    /// DNG spec resolves it by iterating from D50 until the chromaticity
    /// settles, and caps the passes so a pathological profile cannot spin.
    pub fn temperature_from_neutral(&self, neutral: [f64; 3]) -> f64 {
        const MAX_PASSES: usize = 30;
        const SETTLED: f64 = 1e-7;

        let (mut x, mut y) = (0.3457, 0.3585); // D50
        for _ in 0..MAX_PASSES {
            let temperature = xy_to_temperature(x, y);
            let xyz_to_camera = match self.xyz_to_camera.as_slice() {
                [] => return FALLBACK_TEMPERATURE_K,
                [only] => only.matrix,
                [cool, warm, ..] => {
                    let mix = mireds_mix(temperature, cool.temperature, warm.temperature);
                    mix3(cool.matrix, mix, warm.matrix, 1.0 - mix)
                }
            };
            let Some(camera_to_xyz) = invert(xyz_to_camera) else {
                return FALLBACK_TEMPERATURE_K;
            };
            let xyz = [
                camera_to_xyz[0][0] * neutral[0]
                    + camera_to_xyz[0][1] * neutral[1]
                    + camera_to_xyz[0][2] * neutral[2],
                camera_to_xyz[1][0] * neutral[0]
                    + camera_to_xyz[1][1] * neutral[1]
                    + camera_to_xyz[1][2] * neutral[2],
                camera_to_xyz[2][0] * neutral[0]
                    + camera_to_xyz[2][1] * neutral[1]
                    + camera_to_xyz[2][2] * neutral[2],
            ];
            let sum = xyz[0] + xyz[1] + xyz[2];
            if sum.abs() < 1e-12 {
                return FALLBACK_TEMPERATURE_K;
            }
            let (nx, ny) = (xyz[0] / sum, xyz[1] / sum);
            if (nx - x).abs() + (ny - y).abs() < SETTLED {
                return xy_to_temperature(nx, ny);
            }
            (x, y) = (nx, ny);
        }
        xy_to_temperature(x, y)
    }

    /// Converts one linear camera-native RGB sample (in `[0, 1]`, as
    /// [`leyline_raw::DecodeParams::camera_native`] decodes) to linear
    /// sRGB via this profile's camera→XYZ(D50) matrix and the standard
    /// XYZ(D50)→sRGB matrix. The caller is responsible for the sRGB gamma
    /// encode this doesn't do — matrix work stays in linear light.
    pub fn camera_to_linear_srgb(&self, camera_rgb: [f64; 3]) -> [f64; 3] {
        let xyz = apply(self.camera_to_xyz_d50, camera_rgb);
        apply(XYZ_D50_TO_LINEAR_SRGB, xyz)
    }

    /// Converts one linear camera-native RGB sample into the pipeline's
    /// working space, linear Rec. 2020 (ADR 0044 §1), through this profile's
    /// camera→XYZ(D50) matrix, the Bradford adaptation to D65 and the
    /// standard XYZ→Rec. 2020 matrix.
    ///
    /// Nothing is clipped on the way: a color the working space cannot hold
    /// would come out negative, and it is the *output* stage's business to
    /// decide what happens to it, not this one's.
    pub fn camera_to_linear_rec2020(&self, camera_rgb: [f64; 3]) -> [f64; 3] {
        let xyz_d50 = apply(self.camera_to_xyz_d50, camera_rgb);
        let xyz_d65 = apply(BRADFORD_D50_TO_D65, xyz_d50);
        crate::working_space::apply_matrix(crate::working_space::XYZ_D65_TO_REC2020, xyz_d65)
    }

    /// The same conversion, through the calibration interpolated for a scene
    /// of `temperature_k` rather than the average of the two (ADR 0062).
    ///
    /// Callers converting a whole image should resolve the matrix once with
    /// [`DcpProfile::camera_to_xyz_at`] and reuse it, rather than calling
    /// this per pixel: the blend does not change within one render.
    pub fn camera_to_linear_rec2020_at(
        &self,
        temperature_k: f64,
        camera_rgb: [f64; 3],
    ) -> [f64; 3] {
        let xyz_d50 = apply(self.camera_to_xyz_at(temperature_k), camera_rgb);
        let xyz_d65 = apply(BRADFORD_D50_TO_D65, xyz_d50);
        crate::working_space::apply_matrix(crate::working_space::XYZ_D65_TO_REC2020, xyz_d65)
    }
}

/// XYZ (D50) to the linear Rec. 2020 working space (ADR 0044).
///
/// Exposed so a caller resolving a profile's matrix once per image — which
/// is what [`DcpProfile::camera_to_xyz_at`] invites — can finish the
/// conversion itself instead of paying a matrix blend per pixel.
pub fn xyz_d50_to_linear_rec2020(xyz_d50: [f64; 3]) -> [f64; 3] {
    let xyz_d65 = apply(BRADFORD_D50_TO_D65, xyz_d50);
    crate::working_space::apply_matrix(crate::working_space::XYZ_D65_TO_REC2020, xyz_d65)
}

/// The standard XYZ (D50) to linear sRGB matrix (Bruce Lindbloom's
/// widely-published Bradford-adapted conversion) — the same connection
/// space DNG color matrices are defined against.
const XYZ_D50_TO_LINEAR_SRGB: Matrix3 = [
    [3.1338561, -1.6168667, -0.4906146],
    [-0.9787684, 1.9161415, 0.0334540],
    [0.0719453, -0.2289914, 1.4052427],
];

/// Bradford chromatic adaptation from D50 — the illuminant DNG color
/// matrices are defined against — to D65, the working space's white point.
const BRADFORD_D50_TO_D65: Matrix3 = [
    [0.955_576_6, -0.023_039_3, 0.063_163_6],
    [-0.028_289_5, 1.009_941_6, 0.021_007_7],
    [0.012_298_2, -0.020_483_0, 1.329_909_8],
];

/// Applies a 3×3 matrix to a 3-vector.
fn apply(m: Matrix3, v: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

/// Element-wise average of two matrices — the simplified stand-in for
/// CCT-weighted dual-illuminant interpolation (module doc).
fn average(a: Matrix3, b: Matrix3) -> Matrix3 {
    let mut out = [[0.0; 3]; 3];
    for r in 0..3 {
        for c in 0..3 {
            out[r][c] = (a[r][c] + b[r][c]) / 2.0;
        }
    }
    out
}

/// 3×3 matrix inverse via the adjugate/determinant, `None` when singular.
pub(crate) fn invert(m: Matrix3) -> Option<Matrix3> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    Some([
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv_det,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_det,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_det,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv_det,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_det,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv_det,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv_det,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv_det,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv_det,
        ],
    ])
}

/// Reads a 9-entry SRATIONAL tag (a DCP color/forward matrix) as a
/// row-major 3×3, `None` when the tag is absent.
fn read_matrix(ifd: &Ifd<'_>, tag: u16) -> Result<Option<Matrix3>> {
    let Some(entries) = ifd.rationals(tag) else {
        return Ok(None);
    };
    if entries.len() != 9 {
        return Err(DcpError::Invalid(format!(
            "tag {tag} has {} entries, expected 9 (a 3x3 matrix)",
            entries.len()
        )));
    }
    Ok(Some([
        [entries[0], entries[1], entries[2]],
        [entries[3], entries[4], entries[5]],
        [entries[6], entries[7], entries[8]],
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tiff::encoder::TiffEncoder;
    use tiff::encoder::colortype::Gray8;
    use tiff::tags::Tag;

    /// Builds a minimal but spec-valid TIFF/EP container: a 1x1 baseline
    /// grayscale image (satisfying the decoder's mandatory-tag checks)
    /// plus whichever DCP tags the caller adds via `extra`.
    fn sample_dcp(
        extra: impl FnOnce(
            &mut tiff::encoder::DirectoryEncoder<
                '_,
                &mut Cursor<Vec<u8>>,
                tiff::encoder::TiffKindStandard,
            >,
        ),
    ) -> Vec<u8> {
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut encoder = TiffEncoder::new(&mut buffer).unwrap();
            let mut image = encoder.new_image::<Gray8>(1, 1).unwrap();
            extra(image.encoder());
            image.write_data(&[0u8]).unwrap();
        }
        buffer.into_inner()
    }

    fn srational_tag(matrix: Matrix3) -> Vec<tiff::encoder::SRational> {
        matrix
            .into_iter()
            .flatten()
            .map(|v| tiff::encoder::SRational {
                n: (v * 10000.0).round() as i32,
                d: 10000,
            })
            .collect()
    }

    const IDENTITY: Matrix3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    #[test]
    fn parses_a_minimal_profile_with_only_color_matrix_1() {
        let bytes = sample_dcp(|dir| {
            dir.write_tag(
                Tag::Unknown(tag_id::COLOR_MATRIX_1),
                srational_tag(IDENTITY).as_slice(),
            )
            .unwrap();
        });
        let profile = DcpProfile::parse(&bytes).unwrap();
        // Camera->XYZ is ColorMatrix1's inverse; the inverse of the
        // identity is the identity.
        let out = profile.camera_to_linear_srgb([1.0, 0.0, 0.0]);
        let expected = apply(XYZ_D50_TO_LINEAR_SRGB, [1.0, 0.0, 0.0]);
        for (a, b) in out.iter().zip(expected.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn missing_color_matrix_1_is_an_error() {
        let bytes = sample_dcp(|_dir| {});
        assert!(DcpProfile::parse(&bytes).is_err());
    }

    #[test]
    fn reads_the_profile_name_when_present() {
        let bytes = sample_dcp(|dir| {
            dir.write_tag(
                Tag::Unknown(tag_id::COLOR_MATRIX_1),
                srational_tag(IDENTITY).as_slice(),
            )
            .unwrap();
            dir.write_tag(Tag::Unknown(tag_id::PROFILE_NAME), "Test Camera Profile")
                .unwrap();
        });
        let profile = DcpProfile::parse(&bytes).unwrap();
        assert_eq!(profile.name.as_deref(), Some("Test Camera Profile"));
    }

    #[test]
    fn forward_matrix_takes_precedence_over_color_matrix() {
        let scale2 = [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 2.0]];
        let bytes = sample_dcp(|dir| {
            dir.write_tag(
                Tag::Unknown(tag_id::COLOR_MATRIX_1),
                srational_tag(IDENTITY).as_slice(),
            )
            .unwrap();
            dir.write_tag(
                Tag::Unknown(tag_id::FORWARD_MATRIX_1),
                srational_tag(scale2).as_slice(),
            )
            .unwrap();
        });
        let profile = DcpProfile::parse(&bytes).unwrap();
        let out = profile.camera_to_linear_srgb([1.0, 1.0, 1.0]);
        // ForwardMatrix (scale by 2) then XYZ->sRGB, not ColorMatrix1's
        // (identity) inverse.
        let expected = apply(XYZ_D50_TO_LINEAR_SRGB, [2.0, 2.0, 2.0]);
        for (a, b) in out.iter().zip(expected.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn invert_recovers_the_original_matrix() {
        let m = [[2.0, 0.0, 0.0], [0.0, 4.0, 0.0], [1.0, 0.0, 1.0]];
        let inv = invert(m).unwrap();
        // `inv` undoes `m`: applying both to each standard basis vector
        // must recover that same vector (m * inv = identity), checked via
        // `apply` (already tested on its own) instead of a hand-rolled
        // matrix-multiply loop.
        for basis in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
            let round_tripped = apply(m, apply(inv, basis));
            for (actual, expected) in round_tripped.iter().zip(basis.iter()) {
                assert!(
                    (actual - expected).abs() < 1e-9,
                    "m * inv * {basis:?} = {round_tripped:?}"
                );
            }
        }
    }

    #[test]
    fn invert_of_a_singular_matrix_is_none() {
        let singular = [[1.0, 2.0, 3.0], [2.0, 4.0, 6.0], [1.0, 1.0, 1.0]];
        assert!(invert(singular).is_none());
    }

    #[test]
    fn average_of_two_matrices_is_elementwise() {
        let a = [[2.0, 2.0, 2.0], [2.0, 2.0, 2.0], [2.0, 2.0, 2.0]];
        let b = [[4.0, 4.0, 4.0], [4.0, 4.0, 4.0], [4.0, 4.0, 4.0]];
        assert_eq!(average(a, b), [[3.0, 3.0, 3.0]; 3]);
    }

    #[test]
    fn not_a_tiff_file_is_rejected() {
        assert!(DcpProfile::parse(b"not a tiff file at all").is_err());
    }

    /// The blend is linear in mireds, not in kelvin (ADR 0062 §1) — the
    /// difference is invisible at the ends and largest exactly where
    /// interpolation is supposed to earn its keep.
    #[test]
    fn the_illuminant_blend_is_linear_in_mireds() {
        // At and beyond each calibration, the calibration itself.
        assert_eq!(mireds_mix(2850.0, 2850.0, 6500.0), 1.0);
        assert_eq!(mireds_mix(2000.0, 2850.0, 6500.0), 1.0);
        assert_eq!(mireds_mix(6500.0, 2850.0, 6500.0), 0.0);
        assert_eq!(mireds_mix(9000.0, 2850.0, 6500.0), 0.0);

        // Halfway in mireds is 3963 K, not the 4675 K a kelvin blend would
        // put there. Getting this backwards is the whole point of the test.
        let mid_mireds: f64 = 1.0 / ((1.0 / 2850.0 + 1.0 / 6500.0) / 2.0);
        assert!((mid_mireds - 3962.6).abs() < 1.0, "{mid_mireds}");
        assert!((mireds_mix(mid_mireds, 2850.0, 6500.0) - 0.5).abs() < 1e-9);

        // And the reverse reading, which is the one that surprises: at the
        // *kelvin* midpoint (4675 K) the blend is nowhere near even — the
        // daylight calibration already carries 70 % of it, because in mireds
        // 4675 K sits far closer to 6500 K than to 2850 K. A kelvin-linear
        // blend would say 0.5 here, and be wrong by that whole margin.
        let mix = mireds_mix(4675.0, 2850.0, 6500.0);
        assert!(
            (mix - 0.305).abs() < 0.01,
            "kelvin midpoint should lean daylight, not sit at 0.5: {mix}"
        );
    }

    /// The claim ADR 0062 rests on: with two calibrations, interpolating is
    /// *not* averaging — and the gap is where the light actually is.
    ///
    /// The golden renders cannot show this: their fixture profile declares a
    /// single illuminant, so `v1` and `v2` agree there by construction. This
    /// builds the two-illuminant case they lack.
    #[test]
    fn interpolating_two_calibrations_differs_from_averaging_them() {
        let tungsten: Matrix3 = [[0.8, 0.1, 0.1], [0.2, 0.9, -0.1], [0.0, -0.4, 1.3]];
        let daylight: Matrix3 = [[0.7, 0.2, 0.1], [0.3, 0.9, -0.2], [0.0, -0.2, 1.0]];
        let profile = DcpProfile {
            name: None,
            camera_to_xyz_d50: average(tungsten, daylight),
            calibrations: vec![
                Calibration {
                    matrix: tungsten,
                    temperature: 2850.0,
                },
                Calibration {
                    matrix: daylight,
                    temperature: 6500.0,
                },
            ],
            xyz_to_camera: Vec::new(),
            look_table: None,
            hue_sat_map_1: None,
            hue_sat_map_2: None,
            tone_curve: Vec::new(),
        };

        // Under tungsten the tungsten calibration is used outright, and the
        // average is measurably elsewhere.
        let at_tungsten = profile.camera_to_xyz_at(2850.0);
        assert_eq!(at_tungsten, tungsten);
        let averaged = profile.camera_to_xyz_d50;
        let gap = (0..3)
            .flat_map(|i| (0..3).map(move |j| (i, j)))
            .map(|(i, j)| (at_tungsten[i][j] - averaged[i][j]).abs())
            .fold(0.0_f64, f64::max);
        assert!(
            gap > 0.05,
            "averaging should be visibly off under tungsten: {gap}"
        );

        // Under daylight, symmetrically.
        assert_eq!(profile.camera_to_xyz_at(6500.0), daylight);

        // And a single-calibration profile has nothing to interpolate, so it
        // must return that one matrix whatever the light — the case the
        // golden fixture exercises.
        let single = DcpProfile {
            name: None,
            camera_to_xyz_d50: tungsten,
            calibrations: vec![Calibration {
                matrix: tungsten,
                temperature: 2850.0,
            }],
            xyz_to_camera: Vec::new(),
            look_table: None,
            hue_sat_map_1: None,
            hue_sat_map_2: None,
            tone_curve: Vec::new(),
        };
        assert_eq!(single.camera_to_xyz_at(9000.0), tungsten);
    }

    /// Hue is a *cyclic* axis: the last division is adjacent to the first.
    /// Treating it as open leaves a seam on reds — a defect that survives a
    /// synthetic gradient and shows up on a face (ADR 0063 §3).
    #[test]
    fn the_hue_axis_of_a_table_wraps_around() {
        // Two hue divisions, one saturation, one value: a table whose only
        // content is the wrap. Division 0 shifts +10°, division 1 shifts
        // −10°, both leaving saturation and value alone.
        let table = HsvTable::new(&[2, 1, 1], &[10.0, 1.0, 1.0, -10.0, 1.0, 1.0])
            .expect("a 2x1x1 table is well formed");

        // Just past the last division, interpolation must come back towards
        // division 0 rather than clamp on division 1.
        let (near_end, _, _) = table.apply(359.0, 0.5, 0.5);
        let (at_start, _, _) = table.apply(0.0, 0.5, 0.5);
        assert!(
            (near_end - 359.0 - 10.0).abs() < 1.0,
            "359 degrees should be almost entirely division 0 again: {near_end}"
        );
        assert!((at_start - 10.0).abs() < 1e-4, "{at_start}");

        // Halfway between the two divisions the shift averages out.
        let (mid, _, _) = table.apply(90.0, 0.5, 0.5);
        assert!((mid - 90.0).abs() < 1e-4, "midpoint should cancel: {mid}");
    }

    /// Saturation and value are *scales*, hue is a *shift* — mixing the two
    /// up is silent and wrong.
    #[test]
    fn a_table_shifts_hue_and_scales_the_rest() {
        let table = HsvTable::new(&[1, 1, 1], &[30.0, 2.0, 0.5]).unwrap();
        let (h, s, v) = table.apply(100.0, 0.4, 0.8);
        assert!((h - 130.0).abs() < 1e-4, "hue shifts: {h}");
        assert!((s - 0.8).abs() < 1e-4, "saturation scales: {s}");
        assert!((v - 0.4).abs() < 1e-4, "value scales: {v}");

        // Saturation is a ratio, so it saturates at 1 rather than running
        // past it.
        let (_, clamped, _) = table.apply(0.0, 0.9, 0.5);
        assert!((clamped - 1.0).abs() < 1e-6, "{clamped}");
    }

    /// A malformed pair of dimensions and data yields no table, never a
    /// panic and never a half-read cube.
    #[test]
    fn dimensions_that_do_not_match_the_data_yield_no_table() {
        assert!(HsvTable::new(&[2, 2, 2], &[0.0; 3]).is_none());
        assert!(HsvTable::new(&[0, 1, 1], &[]).is_none());
        assert!(HsvTable::new(&[1, 1], &[0.0; 3]).is_none());
    }

    /// A profile with no tables must convert exactly as the matrix path
    /// does — that equivalence is what makes reprocessing into `v3` safe,
    /// and the golden renders cannot show it because their fixture has no
    /// tables to begin with.
    #[test]
    fn a_profile_without_tables_converts_like_the_matrix_alone() {
        let matrix: Matrix3 = [[0.75, 0.06, 0.14], [0.21, 0.89, -0.10], [0.0, -0.43, 1.25]];
        let profile = DcpProfile {
            name: None,
            camera_to_xyz_d50: matrix,
            calibrations: vec![Calibration {
                matrix,
                temperature: 6500.0,
            }],
            xyz_to_camera: Vec::new(),
            look_table: None,
            hue_sat_map_1: None,
            hue_sat_map_2: None,
            tone_curve: Vec::new(),
        };

        let prepared = profile.prepare(6500.0);
        for sample in [[0.5, 0.5, 0.5], [0.8, 0.2, 0.1], [0.1, 0.4, 0.7]] {
            let through_tables = prepared.camera_to_working(sample);
            let matrix_only = profile.camera_to_linear_rec2020_at(
                6500.0,
                [
                    f64::from(sample[0]),
                    f64::from(sample[1]),
                    f64::from(sample[2]),
                ],
            );
            for i in 0..3 {
                let gap = (f64::from(through_tables[i]) - matrix_only[i]).abs();
                assert!(gap < 1e-5, "{sample:?} channel {i}: {gap}");
            }
        }
    }

    /// The highlight rule (ADR 0063 §2): a sample above white crosses the
    /// tables untouched rather than being clipped to receive a look. The
    /// working buffer is unbounded above white on purpose (ADR 0044), and
    /// that is worth more than a look on a blown highlight.
    #[test]
    fn a_highlight_above_white_crosses_the_tables_unchanged() {
        let matrix: Matrix3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        // A look table that would darken everything it touches by half.
        let crusher = HsvTable::new(&[1, 1, 1], &[0.0, 1.0, 0.5]).unwrap();
        let profile = DcpProfile {
            name: None,
            camera_to_xyz_d50: matrix,
            calibrations: vec![Calibration {
                matrix,
                temperature: 6500.0,
            }],
            xyz_to_camera: Vec::new(),
            look_table: Some(crusher),
            hue_sat_map_1: None,
            hue_sat_map_2: None,
            tone_curve: Vec::new(),
        };
        let prepared = profile.prepare(6500.0);

        // Inside the range, the table bites.
        let inside = prepared.camera_to_working([0.4, 0.4, 0.4]);
        let untouched = DcpProfile {
            look_table: None,
            ..profile.clone()
        }
        .prepare(6500.0)
        .camera_to_working([0.4, 0.4, 0.4]);
        assert!(
            inside[1] < untouched[1] * 0.9,
            "the table should darken inside the range: {inside:?} vs {untouched:?}"
        );

        // Above white it does not: the headroom survives.
        let above = prepared.camera_to_working([3.0, 3.0, 3.0]);
        let above_untouched = DcpProfile {
            look_table: None,
            ..profile.clone()
        }
        .prepare(6500.0)
        .camera_to_working([3.0, 3.0, 3.0]);
        for i in 0..3 {
            assert!(
                (above[i] - above_untouched[i]).abs() < 1e-4,
                "a highlight must cross untouched: {above:?} vs {above_untouched:?}"
            );
        }
    }

    /// Robertson's method against the illuminants the table is built for.
    #[test]
    fn known_chromaticities_recover_their_temperature() {
        // D65 and illuminant A, standard chromaticities.
        for (x, y, expected) in [(0.31271, 0.32902, 6504.0), (0.44757, 0.40745, 2856.0)] {
            let found = xy_to_temperature(x, y);
            let error = (found - expected).abs() / expected;
            assert!(
                error < 0.02,
                "xy ({x}, {y}) gave {found} K, expected ~{expected}"
            );
        }
    }

    /// A real `.dcp` is a *bare tag directory* carrying DCP's own version
    /// number — no image, no `ImageWidth`. Every fixture above is a TIFF
    /// image with DCP tags bolted on, which is why they all passed while
    /// no genuine profile could be read at all. This one is shaped like
    /// the real thing, by hand.
    #[test]
    fn a_bare_directory_with_the_dcp_version_parses() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"II");
        bytes.extend_from_slice(&DCP_VERSION.to_le_bytes());
        bytes.extend_from_slice(&8u32.to_le_bytes()); // directory at byte 8

        // One entry: ColorMatrix1, nine SRATIONALs living after the
        // directory. 2 + 12 + 4 = 18 bytes of directory, so the values
        // start at 8 + 18 = 26.
        let values_at = 26u32;
        bytes.extend_from_slice(&1u16.to_le_bytes()); // entry count
        bytes.extend_from_slice(&tag_id::COLOR_MATRIX_1.to_le_bytes());
        bytes.extend_from_slice(&10u16.to_le_bytes()); // SRATIONAL
        bytes.extend_from_slice(&9u32.to_le_bytes());
        bytes.extend_from_slice(&values_at.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes()); // next directory: none
        assert_eq!(bytes.len(), values_at as usize);

        // The identity matrix, as nine numerator/denominator pairs.
        for value in [1i32, 0, 0, 0, 1, 0, 0, 0, 1] {
            bytes.extend_from_slice(&value.to_le_bytes());
            bytes.extend_from_slice(&1i32.to_le_bytes());
        }

        let profile = DcpProfile::parse(&bytes).expect("a bare DCP directory must parse");
        // Identity camera->XYZ inverts to identity, so the round trip is
        // just the standard XYZ(D50)->sRGB matrix; a neutral triple stays
        // neutral through it.
        let grey = profile.camera_to_linear_srgb([0.5, 0.5, 0.5]);
        assert!(grey.iter().all(|c| c.is_finite()), "{grey:?}");
    }

    /// The same, in big-endian: the byte-order mark drives every read, and
    /// getting it wrong would silently mis-parse rather than fail.
    #[test]
    fn a_big_endian_directory_is_read_the_other_way_round() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"MM");
        bytes.extend_from_slice(&DCP_VERSION.to_be_bytes());
        bytes.extend_from_slice(&8u32.to_be_bytes());
        let values_at = 26u32;
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&tag_id::COLOR_MATRIX_1.to_be_bytes());
        bytes.extend_from_slice(&10u16.to_be_bytes());
        bytes.extend_from_slice(&9u32.to_be_bytes());
        bytes.extend_from_slice(&values_at.to_be_bytes());
        bytes.extend_from_slice(&0u32.to_be_bytes());
        for value in [2i32, 0, 0, 0, 2, 0, 0, 0, 2] {
            bytes.extend_from_slice(&value.to_be_bytes());
            bytes.extend_from_slice(&1i32.to_be_bytes());
        }
        assert!(DcpProfile::parse(&bytes).is_ok());
    }

    /// Real profiles, when the machine has them: `LEYLINE_TEST_DCP` names a
    /// directory of `.dcp` files, the same opt-in shape `LEYLINE_TEST_RAW`
    /// uses for real photographs.
    ///
    /// They are not committed — they are third-party work of unknown
    /// licence, and this repository is meant to go public. The fixtures
    /// above are what CI checks; this is what catches whatever a fixture
    /// cannot imagine, which on the day it was written was the entire
    /// container format.
    #[test]
    fn real_profiles_parse_and_keep_a_neutral_grey_neutral() {
        let Ok(dir) = std::env::var("LEYLINE_TEST_DCP") else {
            return;
        };
        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).expect("LEYLINE_TEST_DCP must name a directory") {
            let path = entry.expect("readable directory entry").path();
            if path
                .extension()
                .is_none_or(|e| !e.eq_ignore_ascii_case("dcp"))
            {
                continue;
            }
            let bytes = std::fs::read(&path).expect("readable profile");
            let profile = DcpProfile::parse(&bytes)
                .unwrap_or_else(|e| panic!("{} failed to parse: {e}", path.display()));

            // The check that needs no reference renderer: a neutral sensor
            // triple must come out neutral. A transposed matrix, a swapped
            // numerator/denominator or a botched endianness all break this
            // long before they become a subtle hue error.
            let grey = profile.camera_to_linear_srgb([0.5, 0.5, 0.5]);
            let spread = grey.iter().cloned().fold(f64::MIN, f64::max)
                - grey.iter().cloned().fold(f64::MAX, f64::min);
            assert!(
                spread < 0.01,
                "{} turns a neutral grey into {grey:?}",
                path.display()
            );
            checked += 1;
        }
        assert!(checked > 0, "LEYLINE_TEST_DCP held no .dcp file");
    }
}
