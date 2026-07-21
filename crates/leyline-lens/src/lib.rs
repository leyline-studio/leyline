//! Lens distortion correction for the Leyline engine, backed by Lensfun's
//! bundled camera/lens profile database (`docs/specification.md`).
//!
//! This crate only matches EXIF strings to a profile and turns it into a
//! per-pixel backward coordinate map; it does not resample images itself —
//! that stays with the engine's pixel buffer (mirrors how `leyline-raw`
//! decodes without knowing about `Pixels`).

use std::sync::OnceLock;

use lensfun::{Camera, Database, Lens, Modifier};

/// The bundled profile database, parsed once and reused for every lookup.
fn database() -> &'static Database {
    static DB: OnceLock<Database> = OnceLock::new();
    // The bundled XML ships with the crate: parsing it can only fail if the
    // dependency itself is broken, which is a build-time concern, not a
    // runtime one.
    DB.get_or_init(|| Database::load_bundled().expect("bundled lensfun database"))
}

/// A camera + lens pair matched against the bundled database, ready to build
/// a [`Correction`] for a specific shot.
pub struct Profile {
    camera: &'static Camera,
    lens: &'static Lens,
}

/// Matches EXIF camera and lens strings against the bundled database.
///
/// Returns `None` when either the camera body or the lens has no
/// corresponding profile — correction is then simply skipped, never
/// approximated from an unrelated lens. An empty or missing lens model never
/// matches: a body-only guess is not good enough for geometric correction.
pub fn find_profile(
    camera_make: &str,
    camera_model: &str,
    lens_make: Option<&str>,
    lens_model: &str,
) -> Option<Profile> {
    if lens_model.trim().is_empty() {
        return None;
    }
    let db = database();
    let camera = *db.find_cameras(Some(camera_make), camera_model).first()?;
    // Some cameras (Canon in particular) already write the maker as part of
    // `LensModel`; prepending it again would turn "Canon EF 50mm" into
    // "Canon Canon EF 50mm" and break the fuzzy match. Only prepend when the
    // model doesn't already mention the maker.
    let query = match lens_make {
        Some(make) if !make.trim().is_empty() && !contains_ci(lens_model, make) => {
            format!("{make} {lens_model}")
        }
        _ => lens_model.to_owned(),
    };
    let lens = *db.find_lenses(Some(camera), &query).first()?;
    Some(Profile { camera, lens })
}

/// Case-insensitive substring test (ASCII EXIF strings only).
fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

/// A geometric distortion correction, configured for one shot (focal length
/// and pixel dimensions).
pub struct Correction {
    modifier: Modifier,
    /// Whether a distortion calibration was found; [`Correction::source_row`]
    /// always returns an identity map otherwise, so callers can skip the
    /// per-pixel resample up front.
    distortion_matched: bool,
    /// Whether a TCA calibration was found; [`Correction::tca_row`] always
    /// returns an identity map (same coordinate for all three channels)
    /// otherwise, so callers can skip the per-channel resample up front.
    tca_matched: bool,
}

impl Correction {
    /// Builds the correction. `width`/`height` are the pixel dimensions of
    /// the image to correct — the same image the row coordinates in
    /// [`Correction::source_row`] are expressed against.
    pub fn new(profile: &Profile, focal_mm: f32, width: u32, height: u32) -> Correction {
        // `reverse = true`: the image already carries the lens's distortion
        // and must be corrected back to a rectilinear rendering.
        let mut modifier = Modifier::new(
            profile.lens,
            focal_mm,
            profile.camera.crop_factor,
            width,
            height,
            true,
        );
        let distortion_matched = modifier.enable_distortion_correction(profile.lens);
        let tca_matched = modifier.enable_tca_correction(profile.lens);
        Correction {
            modifier,
            distortion_matched,
            tca_matched,
        }
    }

    /// Whether a distortion calibration was found for this shot.
    pub fn distortion_matched(&self) -> bool {
        self.distortion_matched
    }

    /// Whether a TCA calibration was found for this shot.
    pub fn tca_matched(&self) -> bool {
        self.tca_matched
    }

    /// The source coordinate to sample for each pixel of output row `y`,
    /// `width` pixels wide starting at `x = 0`.
    ///
    /// When the profile has no distortion calibration for this focal length,
    /// every entry maps to itself: the caller can resample unconditionally,
    /// the identity map is a correct no-op.
    pub fn source_row(&self, y: u32, width: u32) -> Vec<(f32, f32)> {
        let mut coords: Vec<f32> = (0..width).flat_map(|x| [x as f32, y as f32]).collect();
        self.modifier
            .apply_geometry_distortion(0.0, y as f32, width as usize, 1, &mut coords);
        coords.chunks_exact(2).map(|c| (c[0], c[1])).collect()
    }

    /// Same result as [`Correction::source_row`], written into caller-owned
    /// buffers instead of allocating fresh ones. `scratch` and `out` are both
    /// cleared and refilled — callers that reuse them across many rows (e.g.
    /// one buffer pair per render thread) turn per-row allocation into a
    /// one-time capacity grow, with no change to the coordinates produced.
    pub fn source_row_into(
        &self,
        y: u32,
        width: u32,
        scratch: &mut Vec<f32>,
        out: &mut Vec<(f32, f32)>,
    ) {
        scratch.clear();
        scratch.extend((0..width).flat_map(|x| [x as f32, y as f32]));
        self.modifier
            .apply_geometry_distortion(0.0, y as f32, width as usize, 1, scratch);
        out.clear();
        out.extend(scratch.chunks_exact(2).map(|c| (c[0], c[1])));
    }

    /// The per-channel source coordinate `[red, green, blue]` to sample for
    /// each pixel of output row `y` — transverse chromatic aberration shifts
    /// each channel's true position slightly differently, unlike distortion
    /// which shifts a pixel's channels together.
    ///
    /// When the profile has no TCA calibration for this focal length, every
    /// channel maps to the same identity coordinate, the same safe no-op
    /// convention as [`Correction::source_row`]. Intentionally independent of
    /// distortion: apply after resampling with `source_row` (or after
    /// [`Correction::source_row`]'s no-op), not fused into one pass — see
    /// ADR 0018.
    pub fn tca_row(&self, y: u32, width: u32) -> Vec<[(f32, f32); 3]> {
        let mut coords: Vec<f32> = (0..width)
            .flat_map(|x| [x as f32, y as f32, x as f32, y as f32, x as f32, y as f32])
            .collect();
        self.modifier
            .apply_subpixel_distortion(0.0, y as f32, width as usize, 1, &mut coords);
        coords
            .chunks_exact(6)
            .map(|c| [(c[0], c[1]), (c[2], c[3]), (c[4], c[5])])
            .collect()
    }

    /// Same result as [`Correction::tca_row`], written into caller-owned
    /// buffers instead of allocating fresh ones — see
    /// [`Correction::source_row_into`] for why this is worth having.
    pub fn tca_row_into(
        &self,
        y: u32,
        width: u32,
        scratch: &mut Vec<f32>,
        out: &mut Vec<[(f32, f32); 3]>,
    ) {
        scratch.clear();
        scratch.extend(
            (0..width).flat_map(|x| [x as f32, y as f32, x as f32, y as f32, x as f32, y as f32]),
        );
        self.modifier
            .apply_subpixel_distortion(0.0, y as f32, width as usize, 1, scratch);
        out.clear();
        out.extend(
            scratch
                .chunks_exact(6)
                .map(|c| [(c[0], c[1]), (c[2], c[3]), (c[4], c[5])]),
        );
    }
}

/// Subject distance assumed when de-vignetting, in meters: EXIF rarely
/// records the real focus distance, and Lensfun's own convention uses 1000
/// as its "effectively infinity" calibration bucket — the least wrong
/// default for typical (non-macro) photography.
const ASSUMED_DISTANCE_M: f32 = 1000.0;

/// A vignetting (corner darkening) correction, configured for one shot.
///
/// A separate [`Modifier`] from [`Correction`]: Lensfun's `reverse` flag
/// means opposite things for the two passes (`true` corrects distortion but
/// *simulates* vignetting), so the two corrections can't share one
/// `Modifier` instance.
pub struct Vignetting {
    modifier: Modifier,
    /// Whether a calibration was found; [`Vignetting::gain_row`] always
    /// returns identity gains otherwise, so callers can skip the
    /// per-pixel work up front.
    matched: bool,
}

impl Vignetting {
    /// Builds the correction for one shot: focal length, aperture (f-number)
    /// and the image's pixel dimensions.
    pub fn new(
        profile: &Profile,
        focal_mm: f32,
        aperture_f: f32,
        width: u32,
        height: u32,
    ) -> Vignetting {
        // `reverse = false`: opposite of `Correction::new` — see struct docs.
        let mut modifier = Modifier::new(
            profile.lens,
            focal_mm,
            profile.camera.crop_factor,
            width,
            height,
            false,
        );
        let matched =
            modifier.enable_vignetting_correction(profile.lens, aperture_f, ASSUMED_DISTANCE_M);
        Vignetting { modifier, matched }
    }

    /// Whether a vignetting calibration was found for this shot.
    pub fn matched(&self) -> bool {
        self.matched
    }

    /// The gain to multiply each **linear-light** sample of output row `y`
    /// by, `width` pixels wide starting at `x = 0`. All 1.0 when no
    /// calibration matched — safe to apply unconditionally.
    pub fn gain_row(&self, y: u32, width: u32) -> Vec<f32> {
        let mut gains = vec![1.0f32; width as usize];
        self.modifier
            .apply_color_modification_f32(&mut gains, 0.0, y as f32, width as usize, 1, 1);
        gains
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real bundled entries (lensfun crate's data/db/slr-canon.xml): a body
    // with no calibrations of its own, paired with a lens calibrated at
    // focals 16/19/23/29/35mm — 20mm below exercises the interpolation.
    const CAMERA_MAKE: &str = "Canon";
    const CAMERA_MODEL: &str = "Canon EOS 5D Mark III";
    const LENS_MAKE: &str = "Canon";
    const LENS_MODEL: &str = "Canon EF 16-35mm f/2.8L II USM";

    #[test]
    fn unknown_gear_has_no_profile() {
        assert!(find_profile("Nobody", "Nothing", Some("Nobody"), "Nothing").is_none());
    }

    #[test]
    fn missing_lens_model_has_no_profile() {
        assert!(find_profile(CAMERA_MAKE, CAMERA_MODEL, Some(LENS_MAKE), "").is_none());
    }

    #[test]
    fn known_gear_matches_a_profile() {
        assert!(find_profile(CAMERA_MAKE, CAMERA_MODEL, Some(LENS_MAKE), LENS_MODEL).is_some());
    }

    #[test]
    fn distortion_shifts_the_corner_but_leaves_the_center_in_place() {
        let profile = find_profile(CAMERA_MAKE, CAMERA_MODEL, Some(LENS_MAKE), LENS_MODEL).unwrap();
        let (width, height) = (6720_u32, 4480_u32);
        let correction = Correction::new(&profile, 20.0, width, height);

        let center_row = correction.source_row(height / 2, width);
        let (cx, cy) = center_row[(width / 2) as usize];
        assert!((cx - (width / 2) as f32).abs() < 1.0);
        assert!((cy - (height / 2) as f32).abs() < 1.0);

        let top_row = correction.source_row(0, width);
        let (tx, _ty) = top_row[0];
        // Barrel/pincushion distortion at 20mm moves the top-left corner's
        // source noticeably off its destination — this lens is not neutral.
        assert!((tx - 0.0).abs() > 1.0, "corner unexpectedly unmoved: {tx}");
    }

    #[test]
    fn source_row_covers_every_output_pixel() {
        let profile = find_profile(CAMERA_MAKE, CAMERA_MODEL, Some(LENS_MAKE), LENS_MODEL).unwrap();
        let correction = Correction::new(&profile, 20.0, 100, 100);
        assert_eq!(correction.source_row(50, 100).len(), 100);
    }

    #[test]
    fn tca_separates_channel_positions_toward_the_frame_edge() {
        let profile = find_profile(CAMERA_MAKE, CAMERA_MODEL, Some(LENS_MAKE), LENS_MODEL).unwrap();
        let (width, height) = (6720_u32, 4480_u32);
        let correction = Correction::new(&profile, 20.0, width, height);

        let corner = correction.tca_row(0, width)[0];
        let [red, _green, blue] = corner;
        assert_ne!(
            red, blue,
            "TCA should separate the red and blue channel positions at the corner"
        );
    }

    #[test]
    fn distortion_matched_and_tca_matched_reflect_the_calibration() {
        // The main fixture lens has both distortion and TCA data at 20mm.
        let profile = find_profile(CAMERA_MAKE, CAMERA_MODEL, Some(LENS_MAKE), LENS_MODEL).unwrap();
        let correction = Correction::new(&profile, 20.0, 100, 100);
        assert!(correction.distortion_matched());
        assert!(correction.tca_matched());

        // This bundled lens has a distortion calibration but no TCA entries
        // at all: distortion_matched should be true, tca_matched false, and
        // tca_row should therefore be the identity map (see doc comment).
        let no_tca_profile = find_profile(
            CAMERA_MAKE,
            CAMERA_MODEL,
            Some(LENS_MAKE),
            "Canon EF 17-35mm f/2.8L USM",
        )
        .unwrap();
        let no_tca_correction = Correction::new(&no_tca_profile, 20.0, 100, 100);
        assert!(no_tca_correction.distortion_matched());
        assert!(!no_tca_correction.tca_matched());
        let identity = no_tca_correction.tca_row(50, 100)[42];
        assert_eq!(identity, [(42.0, 50.0), (42.0, 50.0), (42.0, 50.0)]);
    }

    #[test]
    fn tca_row_covers_every_output_pixel() {
        let profile = find_profile(CAMERA_MAKE, CAMERA_MODEL, Some(LENS_MAKE), LENS_MODEL).unwrap();
        let correction = Correction::new(&profile, 20.0, 100, 100);
        assert_eq!(correction.tca_row(50, 100).len(), 100);
    }

    #[test]
    fn vignetting_brightens_corners_and_leaves_the_center_alone() {
        let profile = find_profile(CAMERA_MAKE, CAMERA_MODEL, Some(LENS_MAKE), LENS_MODEL).unwrap();
        let (width, height) = (6720_u32, 4480_u32);
        let vignetting = Vignetting::new(&profile, 20.0, 2.8, width, height);
        assert!(vignetting.matched());

        let center = vignetting.gain_row(height / 2, width)[(width / 2) as usize];
        assert!(
            (center - 1.0).abs() < 0.01,
            "center should be ~neutral: {center}"
        );

        let corner = vignetting.gain_row(0, width)[0];
        assert!(
            corner > 1.5,
            "corner should brighten well above 1.0: {corner}"
        );
    }

    #[test]
    fn unmatched_gear_reports_no_match_and_identity_gains() {
        let profile = find_profile(CAMERA_MAKE, CAMERA_MODEL, Some(LENS_MAKE), LENS_MODEL).unwrap();
        // A focal length far outside any calibrated range: no usable data.
        let vignetting = Vignetting::new(&profile, 9999.0, 2.8, 100, 100);
        assert!(!vignetting.matched());
        assert_eq!(vignetting.gain_row(50, 100), vec![1.0; 100]);
    }
}
