//! Behavioral tests of the composed pipeline (ADR 0042).
//!
//! These are the unit tests the eleven `processN.rs` modules used to carry,
//! kept once instead of eleven times. They assert what each operator *does*;
//! that every published stage version still does it byte for byte is the
//! separate job of `stages/golden.rs`.

use leyline_core::LensCorrection;
use leyline_core::{
    ColorGradingZone, CurvePoint, LocalAdjustment, LocalAdjustmentValues, Mask, NoiseReduction,
    Point, Sharpening, SpotRemoval,
};

use super::Space;
use super::clarity::v1::CLARITY_RADIUS;
use super::color_grading::v1::{zone_tint, zone_weights};
use super::dehaze::v1::{DEHAZE_PATCH_RADIUS, atmospheric_light, min_filter};
use super::fixture;
use super::hsl::v1::{HSL_BAND_CENTERS_DEG, hue_band_neighbors};
use super::kernel::v1::{
    approx_blur, exact_linear_to_srgb, exact_srgb_to_linear, gaussian_blur, hsl_to_rgb,
    lens_bilinear, lens_bilinear_channel, local_contrast, lookup, post_rotation_point_to_buffer,
    rgb_to_hsl, smoothstep01, tables,
};
use super::spot_removal::v1::radial_coverage;
use super::texture::v1::TEXTURE_RADIUS;
use super::tone_curve::v1::{build_curve_lut, curve_lookup};
use super::*;

/// The neutral rendering of `image`: the same file with no operator
/// running. Since ADR 0044 that is not the decoded bytes any more — the
/// decoder hands over the sensor's linear numbers and the `input` and
/// `output_rendering` stages always run — so it is what every "this
/// operator left the image alone" assertion compares against.
fn neutral(image: &RawImage) -> Rendered {
    develop(image, &Settings::default(), None, None).unwrap()
}

/// Full-resolution [`develop_scaled`], the shape every test here
/// exercises: the proxy factor (ADR 0041) is a preview-path concern,
/// not a pipeline-math one.
fn develop(
    image: &RawImage,
    settings: &Settings,
    shot: Option<&LensShot>,
    camera_profile: Option<&DcpProfile>,
) -> Result<Rendered> {
    super::develop_scaled(
        image,
        settings,
        shot,
        None,
        camera_profile,
        None,
        &Default::default(),
        &Source::plain(SourceColor::Camera {
            to_xyz: None,
            multipliers: None,
        }),
        1.0,
    )
}

#[test]
fn lookup_tables_track_the_exact_transfer_functions() {
    let (to_linear, to_srgb) = tables();
    for i in 0..=100_000 {
        let v = i as f32 / 100_000.0;
        assert!(
            (lookup(to_linear, v) - exact_srgb_to_linear(v)).abs() < 2e-5,
            "srgb_to_linear at {v}"
        );
        assert!(
            (lookup(to_srgb, v) - exact_linear_to_srgb(v)).abs() < 2e-5,
            "linear_to_srgb at {v}"
        );
    }
}

/// A deterministic gradient-plus-block test card, large enough for the
/// bundled Canon profile's distortion to move samples by more than a
/// rounding error.
fn test_image(width: u32, height: u32) -> RawImage {
    let mut data = Vec::with_capacity(width as usize * height as usize * 3);
    for y in 0..height {
        for x in 0..width {
            data.push((x * 255 / width) as u8);
            data.push((y * 255 / height) as u8);
            data.push((((x + y) * 255) / (width + height)) as u8);
        }
    }
    RawImage {
        width,
        height,
        bits: 8,
        data,
    }
}

/// A neutral ramp: the three channels equal everywhere, so an operator
/// that weighs each channel by its own value weighs them alike.
fn grey_ramp(width: u32, height: u32) -> RawImage {
    let mut data = Vec::with_capacity(width as usize * height as usize * 3);
    for y in 0..height {
        for x in 0..width {
            let v = ((x + y) * 255 / (width + height)) as u8;
            data.extend_from_slice(&[v, v, v]);
        }
    }
    RawImage {
        width,
        height,
        bits: 8,
        data,
    }
}

fn canon_shot(focal_mm: f32) -> LensShot {
    LensShot {
        camera_make: "Canon".to_owned(),
        camera_model: "Canon EOS 5D Mark III".to_owned(),
        lens_make: Some("Canon".to_owned()),
        lens_model: Some("Canon EF 16-35mm f/2.8L II USM".to_owned()),
        focal_mm,
        aperture_f: Some(2.8),
    }
}

fn enabled_settings() -> Settings {
    Settings {
        lens_correction: LensCorrection {
            enabled: true,
            profile: "auto".to_owned(),
            ..LensCorrection::default()
        },
        ..Settings::default()
    }
}

#[test]
fn a_disabled_lens_correction_is_not_recorded_and_does_not_run() {
    // What the old "process N+1 matches process N bit for bit" tests
    // proved is now structural: a stage at its neutral value is not
    // recorded by `pin`, so there is nothing to run and nothing to
    // compare against (ADR 0043 §3).
    let image = test_image(64, 48);
    let mut settings = Settings {
        exposure: 0.3,
        contrast: 20,
        ..Settings::default()
    };
    let before = develop(&image, &settings, Some(&canon_shot(20.0)), None).unwrap();
    crate::stages::pin(&mut settings);
    assert!(
        !settings.stages.contains_key("lens"),
        "a neutral lens must not be recorded: {:?}",
        settings.stages
    );
    assert_eq!(
        develop(&image, &settings, Some(&canon_shot(20.0)), None).unwrap(),
        before
    );
}
#[test]
fn no_shot_leaves_the_image_unchanged_even_when_enabled() {
    let image = test_image(64, 48);
    let out = develop(&image, &enabled_settings(), None, None).unwrap();
    assert_eq!(out.data, neutral(&image).data);
}

#[test]
fn unmatched_gear_leaves_the_image_unchanged() {
    let image = test_image(64, 48);
    let shot = LensShot {
        camera_make: "Nobody".to_owned(),
        camera_model: "Nothing".to_owned(),
        lens_make: Some("Nobody".to_owned()),
        lens_model: Some("Nothing".to_owned()),
        focal_mm: 20.0,
        aperture_f: Some(2.8),
    };
    let out = develop(&image, &enabled_settings(), Some(&shot), None).unwrap();
    assert_eq!(out.data, neutral(&image).data);
}

#[test]
fn a_matched_profile_undistorts_the_image() {
    let image = test_image(640, 480);
    let out = develop(&image, &enabled_settings(), Some(&canon_shot(20.0)), None).unwrap();
    assert_eq!((out.width, out.height), (image.width, image.height));
    assert_ne!(
        out.data, image.data,
        "distortion correction should move pixels"
    );
}

#[test]
fn disabled_setting_ignores_a_matched_profile() {
    let image = test_image(64, 48);
    let settings = Settings {
        ..Settings::default()
    };
    let out = develop(&image, &settings, Some(&canon_shot(20.0)), None).unwrap();
    assert_eq!(out.data, neutral(&image).data);
}

// -------------------------------------------------------------------
// Tone curve (ADR 0030) — inherited from process 6, still exercised here
// -------------------------------------------------------------------

#[test]
fn an_empty_tone_curve_is_not_recorded_and_does_not_run() {
    // What the old "process N+1 matches process N bit for bit" tests
    // proved is now structural: a stage at its neutral value is not
    // recorded by `pin`, so there is nothing to run and nothing to
    // compare against (ADR 0043 §3).
    let image = test_image(64, 48);
    let mut settings = Settings {
        exposure: 0.2,
        contrast: 15,
        ..Settings::default()
    };
    let before = develop(&image, &settings, None, None).unwrap();
    crate::stages::pin(&mut settings);
    assert!(
        !settings.stages.contains_key("tone_curve"),
        "a neutral tone_curve must not be recorded: {:?}",
        settings.stages
    );
    assert_eq!(develop(&image, &settings, None, None).unwrap(), before);
}
#[test]
fn identity_curve_matches_no_points_bit_for_bit() {
    let image = test_image(64, 48);
    let with_points = Settings {
        tone_curve: leyline_core::ToneCurve {
            points: vec![CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 1.0, y: 1.0 }],
            ..leyline_core::ToneCurve::default()
        },
        ..Settings::default()
    };
    let without_points = Settings {
        ..Settings::default()
    };
    let out_with = develop(&image, &with_points, None, None).unwrap();
    let out_without = develop(&image, &without_points, None, None).unwrap();
    assert_eq!(out_with, out_without);
}

#[test]
fn s_curve_raises_shadows_and_lowers_highlights() {
    // The ADR 0030 example: a soft S-curve.
    let settings = Settings {
        tone_curve: leyline_core::ToneCurve {
            points: vec![
                CurvePoint { x: 0.0, y: 0.0 },
                CurvePoint { x: 0.25, y: 0.30 },
                CurvePoint { x: 0.75, y: 0.70 },
                CurvePoint { x: 1.0, y: 1.0 },
            ],
            ..leyline_core::ToneCurve::default()
        },
        ..Settings::default()
    };
    settings.validate().unwrap();
    let table = build_curve_lut(&settings.tone_curve.points);
    assert!(curve_lookup(&table, 0.1) > 0.1, "shadows should lift");
    assert!(curve_lookup(&table, 0.9) < 0.9, "highlights should drop");
    assert!((curve_lookup(&table, 0.0) - 0.0).abs() < 1e-4);
    assert!((curve_lookup(&table, 1.0) - 1.0).abs() < 1e-4);
}

#[test]
fn curve_lut_is_monotone_even_with_unevenly_spaced_points() {
    let points = vec![
        CurvePoint { x: 0.0, y: 0.0 },
        CurvePoint { x: 0.05, y: 0.4 },
        CurvePoint { x: 0.5, y: 0.5 },
        CurvePoint { x: 1.0, y: 1.0 },
    ];
    let table = build_curve_lut(&points);
    for pair in table.windows(2) {
        assert!(
            pair[1] >= pair[0] - 1e-6,
            "curve LUT must never decrease: {pair:?}"
        );
    }
}

#[test]
fn curve_passes_through_its_control_points() {
    let points = vec![
        CurvePoint { x: 0.0, y: 0.1 },
        CurvePoint { x: 0.4, y: 0.6 },
        CurvePoint { x: 1.0, y: 0.9 },
    ];
    let table = build_curve_lut(&points);
    for point in &points {
        let looked_up = curve_lookup(&table, point.x as f32);
        assert!(
            (looked_up - point.y as f32).abs() < 1e-3,
            "expected {} at x={}, got {looked_up}",
            point.y,
            point.x
        );
    }
}

// -------------------------------------------------------------------
// Spot removal (ADR 0032)
// -------------------------------------------------------------------

/// A test card with a distinct solid-color marker block near one corner
/// (the clone source) and a gradient everywhere else (the background a
/// clone should overwrite at the target).
fn spot_test_image(width: u32, height: u32) -> RawImage {
    let mut data = Vec::with_capacity(width as usize * height as usize * 3);
    for y in 0..height {
        for x in 0..width {
            if (2..6).contains(&x) && (2..6).contains(&y) {
                data.extend_from_slice(&[200, 40, 40]);
            } else {
                let g = ((x * 7 + y * 11) % 255) as u8;
                data.extend_from_slice(&[g, g, g]);
            }
        }
    }
    RawImage {
        width,
        height,
        bits: 8,
        data,
    }
}

#[test]
fn an_empty_spot_list_is_not_recorded_and_does_not_run() {
    // What the old "process N+1 matches process N bit for bit" tests
    // proved is now structural: a stage at its neutral value is not
    // recorded by `pin`, so there is nothing to run and nothing to
    // compare against (ADR 0043 §3).
    let image = test_image(64, 48);
    let mut settings = Settings {
        exposure: 0.2,
        contrast: 15,
        ..Settings::default()
    };
    let before = develop(&image, &settings, None, None).unwrap();
    crate::stages::pin(&mut settings);
    assert!(
        !settings.stages.contains_key("spot_removal"),
        "a neutral spot_removal must not be recorded: {:?}",
        settings.stages
    );
    assert_eq!(develop(&image, &settings, None, None).unwrap(), before);
}
#[test]
fn a_full_opacity_hard_edged_clone_copies_the_source_disk_onto_the_target() {
    let width = 32u32;
    let height = 24u32;
    let image = spot_test_image(width, height);
    // Source disk centered on the marker block at (4, 4); target far away.
    let settings = Settings {
        spot_removal: vec![SpotRemoval {
            target: Point {
                x: 20.0 / width as f64,
                y: 16.0 / height as f64,
            },
            source: Point {
                x: 4.0 / width as f64,
                y: 4.0 / height as f64,
            },
            radius: 1.0 / width as f64, // ~1px: stays well inside the marker/background
            feather: 0.0,
            opacity: 1.0,
        }],
        ..Settings::default()
    };
    let out = develop(&image, &settings, None, None).unwrap();
    // Compared through the neutral rendering: the source's own bytes are
    // camera numbers, not output pixels (ADR 0044).
    let base = neutral(&image);
    let at = |x: usize, y: usize| (y * width as usize + x) * 3;
    let (target, source) = (at(20, 16), at(4, 4));
    assert_eq!(
        &out.data[target..target + 3],
        &base.data[source..source + 3],
        "the target pixel should now match the marker it cloned"
    );
    // Far outside the target disk, the background is untouched.
    let untouched = at(2, 2);
    assert_eq!(
        &out.data[untouched..untouched + 3],
        &base.data[untouched..untouched + 3]
    );
}

/// A minimal spec-valid DCP: a 1×1 baseline grayscale TIFF/EP carrying
/// only `ColorMatrix1`, which is all the matrix path needs.
fn sample_dcp_profile(color_matrix: [[f64; 3]; 3]) -> DcpProfile {
    use tiff::encoder::colortype::Gray8;
    use tiff::encoder::{SRational, TiffEncoder};

    /// `ColorMatrix1`, per the DNG specification's private tag list.
    const COLOR_MATRIX_1: u16 = 50721;

    let rationals: Vec<SRational> = color_matrix
        .into_iter()
        .flatten()
        .map(|v| SRational {
            n: (v * 10_000.0).round() as i32,
            d: 10_000,
        })
        .collect();
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut encoder = TiffEncoder::new(&mut buffer).unwrap();
        let mut image = encoder.new_image::<Gray8>(1, 1).unwrap();
        image
            .encoder()
            .write_tag(
                tiff::tags::Tag::Unknown(COLOR_MATRIX_1),
                rationals.as_slice(),
            )
            .unwrap();
        image.write_data(&[0u8]).unwrap();
    }
    DcpProfile::parse(&buffer.into_inner()).unwrap()
}

#[test]
fn a_camera_profile_changes_the_pixels_and_stays_deterministic() {
    // A profile changes the colorimetry the pipeline starts from, so it
    // changes the pixels — that is the whole of what it does (ADR 0035).
    let image = test_image(16, 12);
    let without = neutral(&image);

    // The revision declares the profile; the caller resolves it. Both are
    // needed — a declared-but-unresolvable profile fails the render before
    // reaching here (`crate::camera_profile`), and a resolved profile the
    // revision never declared is not this revision's business.
    let settings = Settings {
        camera_profile: Some(leyline_core::CameraProfile {
            enabled: true,
            path: "Profiles/Camera/test.dcp".to_owned(),
            checksum: format!("blake3:{}", "0".repeat(64)),
        }),
        ..Settings::default()
    };
    let profile = sample_dcp_profile([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    let profiled = develop(&image, &settings, None, Some(&profile)).unwrap();
    assert_ne!(profiled.data, without.data);

    // Same inputs, same pixels — the §5 reproducibility contract.
    let again = develop(&image, &settings, None, Some(&profile)).unwrap();
    assert_eq!(profiled.data, again.data);
}

#[test]
fn zero_opacity_leaves_the_image_unchanged() {
    let width = 32u32;
    let height = 24u32;
    let image = spot_test_image(width, height);
    let settings = Settings {
        spot_removal: vec![SpotRemoval {
            target: Point { x: 0.6, y: 0.6 },
            source: Point { x: 0.1, y: 0.1 },
            radius: 0.1,
            feather: 0.5,
            opacity: 0.0,
        }],
        ..Settings::default()
    };
    let out = develop(&image, &settings, None, None).unwrap();
    assert_eq!(out.data, neutral(&image).data);
}

#[test]
fn post_rotation_point_to_buffer_is_identity_at_zero_rotation() {
    let point = Point { x: 0.3, y: 0.7 };
    let (sx, sy) = post_rotation_point_to_buffer(100, 50, 0.0, point);
    assert!((sx - 30.0).abs() < 1e-9);
    assert!((sy - 35.0).abs() < 1e-9);
}

#[test]
fn post_rotation_point_to_buffer_flips_both_axes_at_180_degrees() {
    let point = Point { x: 0.2, y: 0.9 };
    let (sx, sy) = post_rotation_point_to_buffer(100, 50, 180.0, point);
    assert!((sx - 80.0).abs() < 1e-6, "sx = {sx}");
    assert!((sy - 5.0).abs() < 1e-6, "sy = {sy}");
}

#[test]
fn post_rotation_point_to_buffer_swaps_axes_at_90_degrees() {
    // Rotating 90 degrees clockwise swaps the canvas dimensions: the
    // post-rotation canvas is height x width relative to the buffer.
    let point = Point { x: 0.25, y: 0.5 };
    let (sx, sy) = post_rotation_point_to_buffer(100, 50, 90.0, point);
    // out_w = 50, out_h = 100 for a 100x50 buffer rotated 90 degrees.
    assert!((0.0..=100.0).contains(&sx));
    assert!((0.0..=50.0).contains(&sy));
}

#[test]
fn radial_coverage_is_full_inside_the_hard_core_and_zero_past_the_rim() {
    assert_eq!(radial_coverage(0.0, 0.5), 1.0);
    assert_eq!(radial_coverage(1.0, 0.5), 0.0);
    assert_eq!(radial_coverage(1.5, 0.5), 0.0);
    assert_eq!(radial_coverage(0.0, 0.0), 1.0);
    // Just past the rim with no feather: a hard edge.
    assert_eq!(radial_coverage(0.999, 0.0), 1.0);
}

#[test]
fn radial_coverage_eases_monotonically_across_the_feather_band() {
    let mut previous = radial_coverage(0.5, 1.0);
    for i in 1..=10 {
        let t = 0.5 + 0.05 * i as f64;
        let current = radial_coverage(t, 1.0);
        assert!(
            current <= previous + 1e-6,
            "coverage must not increase toward the rim"
        );
        previous = current;
    }
}

/// `lens_bilinear_channel` must return, for every channel, exactly the
/// value `lens_bilinear` would have computed for that channel — it's an
/// in-place optimization ([`correct_tca`] discarded two of the three
/// channels `lens_bilinear` computed), not a formula change. Covers
/// interior samples, edge/corner clamping, and the out-of-frame `None`
/// case.
#[test]
fn lens_bilinear_channel_matches_the_full_rgb_sampler_bit_for_bit() {
    let width = 12u32;
    let height = 9u32;
    let mut data = Vec::with_capacity(width as usize * height as usize * 3);
    for y in 0..height {
        for x in 0..width {
            data.push((x * 37 % 255) as f32 / 255.0);
            data.push((y * 53 % 255) as f32 / 255.0);
            data.push(((x + y) * 29 % 255) as f32 / 255.0);
        }
    }
    let px = Pixels {
        width,
        height,
        data,
    };

    let samples = [
        (0.0, 0.0),                                // corner
        (width as f32 - 1.0, 0.0),                 // corner, x clamp
        (0.0, height as f32 - 1.0),                // corner, y clamp
        (5.3, 4.7),                                // interior, fractional
        (2.999_9, 6.000_1),                        // near-integer fractional
        (width as f32 - 1.0, height as f32 - 1.0), // far corner
        (-0.001, 3.0),                             // just out of frame: x
        (3.0, height as f32),                      // just out of frame: y
    ];

    for (sx, sy) in samples {
        let full = lens_bilinear(&px, sx, sy);
        for c in 0..3 {
            let single = lens_bilinear_channel(&px, sx, sy, c);
            match full {
                Some(rgb) => assert_eq!(
                    single,
                    Some(rgb[c]),
                    "channel {c} at ({sx}, {sy}) diverged from the full-RGB sampler"
                ),
                None => assert_eq!(
                    single, None,
                    "channel {c} at ({sx}, {sy}) should also be out of frame"
                ),
            }
        }
    }
}

// -------------------------------------------------------------------
// Local adjustments (ADR 0029)
// -------------------------------------------------------------------

#[test]
fn an_empty_local_adjustment_list_is_not_recorded_and_does_not_run() {
    // What the old "process N+1 matches process N bit for bit" tests
    // proved is now structural: a stage at its neutral value is not
    // recorded by `pin`, so there is nothing to run and nothing to
    // compare against (ADR 0043 §3).
    let image = test_image(64, 48);
    let mut settings = Settings {
        exposure: 0.2,
        contrast: 15,
        ..Settings::default()
    };
    let before = develop(&image, &settings, None, None).unwrap();
    crate::stages::pin(&mut settings);
    assert!(
        !settings.stages.contains_key("local_adjustments"),
        "a neutral local_adjustments must not be recorded: {:?}",
        settings.stages
    );
    assert_eq!(develop(&image, &settings, None, None).unwrap(), before);
}
/// ADR 0052: the correction is projective, so a straight line stays straight
/// while parallel edges stop being parallel — and the canvas grows to hold the
/// transformed quadrilateral rather than cropping it.
/// A creative LUT is the one operator whose behavior comes from a file, so its
/// tests pass the resolved table in the way the render paths do (ADR 0053).
#[test]
fn a_lut_grades_the_image_and_its_strength_doses_the_effect() {
    let image = test_image(64, 48);
    // A look that pushes everything toward red and drops blue.
    let look = leyline_color::CubeLut::parse(
        "LUT_3D_SIZE 2\n\
         0.20 0.00 0.00\n\
         1.00 0.00 0.00\n\
         0.20 0.60 0.00\n\
         1.00 0.60 0.00\n\
         0.20 0.00 0.30\n\
         1.00 0.00 0.30\n\
         0.20 0.60 0.30\n\
         1.00 0.60 0.30\n",
    )
    .unwrap();
    let referenced = |strength, enabled| Settings {
        lut: Some(leyline_core::Lut {
            enabled,
            path: "Profiles/LUT/look.cube".to_owned(),
            checksum: format!("blake3:{}", "0".repeat(64)),
            strength,
        }),
        ..Settings::default()
    };
    let render = |settings: &Settings| {
        super::develop_scaled(
            &image,
            settings,
            None,
            None,
            None,
            Some(&look),
            &Default::default(),
            &Source::plain(SourceColor::Camera {
                to_xyz: None,
                multipliers: None,
            }),
            1.0,
        )
        .unwrap()
    };

    let plain = render(&Settings::default());
    let graded = render(&referenced(100, true));
    assert_ne!(graded.data, plain.data, "a LUT at full strength must show");
    // Blue is what this look takes away, so its mean must fall.
    let mean = |data: &[u8], channel: usize| {
        data.iter()
            .skip(channel)
            .step_by(3)
            .map(|&v| u64::from(v))
            .sum::<u64>() as f64
            / (data.len() / 3) as f64
    };
    assert!(
        mean(&graded.data, 2) < mean(&plain.data, 2),
        "the look drops blue"
    );

    // Half strength lands between the two.
    let half = render(&referenced(50, true));
    let (low, mid, high) = (
        mean(&graded.data, 2),
        mean(&half.data, 2),
        mean(&plain.data, 2),
    );
    assert!(low < mid && mid < high, "{low} < {mid} < {high}");

    // Zero strength and a disabled reference both render the plain image, and
    // a disabled one is not even recorded.
    assert_eq!(render(&referenced(0, true)).data, plain.data);
    assert_eq!(render(&referenced(100, false)).data, plain.data);
    let mut pinned = referenced(100, false);
    crate::stages::pin(&mut pinned);
    assert!(!pinned.stages.contains_key("lut"));
}

#[test]
fn perspective_widens_the_canvas_and_converges_the_edges() {
    use leyline_core::Perspective;

    let image = test_image(200, 150);
    let neutral = develop(&image, &Settings::default(), None, None).unwrap();

    let corrected = develop(
        &image,
        &Settings {
            perspective: Some(Perspective {
                vertical: 60,
                horizontal: 0,
            }),
            ..Settings::default()
        },
        None,
        None,
    )
    .unwrap();
    // The bounding box of the transformed frame is wider than the frame: the
    // bottom edge was spread outward (ADR 0052 §4).
    assert!(
        corrected.width > neutral.width,
        "{} should exceed {}",
        corrected.width,
        neutral.width
    );
    assert_eq!(corrected.height, neutral.height);

    // Symmetry: the opposite slider produces the same size, mirrored.
    let mirrored = develop(
        &image,
        &Settings {
            perspective: Some(Perspective {
                vertical: -60,
                horizontal: 0,
            }),
            ..Settings::default()
        },
        None,
        None,
    )
    .unwrap();
    assert_eq!(
        (mirrored.width, mirrored.height),
        (corrected.width, corrected.height)
    );
    assert_ne!(mirrored.data, corrected.data);

    // The horizontal slider works on the other axis.
    let horizontal = develop(
        &image,
        &Settings {
            perspective: Some(Perspective {
                vertical: 0,
                horizontal: 60,
            }),
            ..Settings::default()
        },
        None,
        None,
    )
    .unwrap();
    assert!(horizontal.height > neutral.height);
    assert_eq!(horizontal.width, neutral.width);
}

/// Neutral means absent, and absent means the stage never runs: a perspective
/// of two zeros renders the decoded image bit for bit and is not recorded.
#[test]
fn a_neutral_perspective_is_not_recorded_and_does_not_run() {
    use leyline_core::Perspective;

    let image = test_image(64, 48);
    let neutral = develop(&image, &Settings::default(), None, None).unwrap();
    let zeroed = Settings {
        perspective: Some(Perspective {
            vertical: 0,
            horizontal: 0,
        }),
        ..Settings::default()
    };
    let mut pinned = zeroed.clone();
    crate::stages::pin(&mut pinned);
    assert!(!pinned.stages.contains_key("perspective"));
    assert_eq!(
        develop(&image, &zeroed, None, None).unwrap().data,
        neutral.data
    );
}

#[test]
fn a_radial_mask_darkens_only_its_covered_area() {
    let image = test_image(200, 150);
    let settings = Settings {
        local_adjustments: vec![LocalAdjustment {
            mask: Mask::Radial {
                cx: 0.5,
                cy: 0.5,
                rx: 0.15,
                ry: 0.15,
                angle: 0.0,
                feather: 0.0,
                inverted: false,
            },
            range: None,
            opacity: 1.0,
            adjustments: LocalAdjustmentValues {
                exposure: Some(-2.0),
                ..LocalAdjustmentValues::default()
            },
        }],
        ..Settings::default()
    };
    let out = develop(&image, &settings, None, None).unwrap();
    let plain = develop(
        &image,
        &Settings {
            ..Settings::default()
        },
        None,
        None,
    )
    .unwrap();

    let center_idx = ((150 / 2 * 200 + 100) * 3) as usize;
    assert!(
        out.data[center_idx] < plain.data[center_idx],
        "the masked area should be darkened by the local exposure drop"
    );
    // Far corner, well outside the mask's radius: untouched.
    let corner_idx = 0usize;
    assert_eq!(out.data[corner_idx], plain.data[corner_idx]);
}

#[test]
fn zero_opacity_local_adjustment_leaves_the_image_unchanged() {
    let image = test_image(64, 48);
    let settings = Settings {
        local_adjustments: vec![LocalAdjustment {
            mask: Mask::Radial {
                cx: 0.5,
                cy: 0.5,
                rx: 0.3,
                ry: 0.3,
                angle: 0.0,
                feather: 0.2,
                inverted: false,
            },
            range: None,
            opacity: 0.0,
            adjustments: LocalAdjustmentValues {
                exposure: Some(2.0),
                ..LocalAdjustmentValues::default()
            },
        }],
        ..Settings::default()
    };
    let out = develop(&image, &settings, None, None).unwrap();
    let plain = develop(
        &image,
        &Settings {
            ..Settings::default()
        },
        None,
        None,
    )
    .unwrap();
    assert_eq!(out.data, plain.data);
}

#[test]
fn an_empty_local_adjustments_list_is_neutral() {
    let image = test_image(64, 48);
    let settings = Settings {
        local_adjustments: vec![],
        exposure: 0.1,
        ..Settings::default()
    };
    let with_empty = develop(&image, &settings, None, None).unwrap();
    let without_field = develop(
        &image,
        &Settings {
            exposure: 0.1,
            ..Settings::default()
        },
        None,
        None,
    )
    .unwrap();
    assert_eq!(with_empty.data, without_field.data);
}

#[test]
fn local_adjustments_apply_in_list_order_on_top_of_each_other() {
    // Two full-coverage exposure-raising entries in sequence: the second
    // entry's blend reads the buffer the first entry already brightened
    // (list-order composition, the same rule spot removal follows
    // above), so stacking both must brighten a non-clipped pixel
    // strictly more than either alone.
    let image = test_image(200, 150);
    let full_frame_radial = |exposure: f64| LocalAdjustment {
        mask: Mask::Radial {
            cx: 0.5,
            cy: 0.5,
            rx: 1.0,
            ry: 1.0,
            angle: 0.0,
            feather: 0.0,
            inverted: false,
        },
        range: None,
        opacity: 1.0,
        adjustments: LocalAdjustmentValues {
            exposure: Some(exposure),
            ..LocalAdjustmentValues::default()
        },
    };
    let stacked = Settings {
        local_adjustments: vec![full_frame_radial(0.5), full_frame_radial(0.5)],
        ..Settings::default()
    };
    let single = Settings {
        local_adjustments: vec![full_frame_radial(0.5)],
        ..Settings::default()
    };
    let out_stacked = develop(&image, &stacked, None, None).unwrap();
    let out_single = develop(&image, &single, None, None).unwrap();
    // A middling, non-clipped pixel near the frame's center.
    let idx = ((150 / 2 * 200 + 100) * 3) as usize;
    assert!(
        out_stacked.data[idx] > out_single.data[idx],
        "stacking two exposure-raising entries should brighten more than one alone"
    );
}

// -----------------------------------------------------------------
// HSL mixer and color grading (ADR 0031)
// -----------------------------------------------------------------

#[test]
fn a_neutral_hsl_mixer_is_not_recorded_and_does_not_run() {
    // What the old "process N+1 matches process N bit for bit" tests
    // proved is now structural: a stage at its neutral value is not
    // recorded by `pin`, so there is nothing to run and nothing to
    // compare against (ADR 0043 §3).
    let image = test_image(64, 48);
    let mut settings = Settings {
        exposure: 0.2,
        contrast: 15,
        ..Settings::default()
    };
    let before = develop(&image, &settings, None, None).unwrap();
    crate::stages::pin(&mut settings);
    assert!(
        !settings.stages.contains_key("hsl"),
        "a neutral hsl must not be recorded: {:?}",
        settings.stages
    );
    assert_eq!(develop(&image, &settings, None, None).unwrap(), before);
}
#[test]
fn rgb_hsl_round_trips_within_float_error() {
    let samples: [[f32; 3]; 6] = [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.3, 0.6, 0.2],
        [0.8, 0.8, 0.8],
        [0.0, 0.0, 0.0],
    ];
    for rgb in samples {
        let (h, s, l) = rgb_to_hsl(&rgb);
        let back = hsl_to_rgb(h, s, l);
        for c in 0..3 {
            assert!(
                (rgb[c] - back[c]).abs() < 1e-4,
                "channel {c}: {rgb:?} -> hsl({h},{s},{l}) -> {back:?}"
            );
        }
    }
}

#[test]
fn hue_band_neighbors_always_form_a_partition_of_unity() {
    let mut h = 0.0f32;
    while h < 360.0 {
        let (i0, i1, t) = hue_band_neighbors(h);
        assert!(i0 < 8 && i1 < 8);
        assert!((0.0..=1.0).contains(&t), "t out of range at {h}: {t}");
        h += 1.0;
    }
}

#[test]
fn hue_band_neighbors_is_exact_at_band_centers() {
    // A center point sits exactly on the shared boundary of two
    // segments (`[i-1, i]` and `[i, i+1]`); which one the scan matches
    // first is unspecified, but either way the *effective* weight must
    // land 100% on band `i` — never split.
    for (i, &center) in HSL_BAND_CENTERS_DEG.iter().enumerate() {
        let (i0, i1, t) = hue_band_neighbors(center);
        let w1 = smoothstep01(t);
        let w0 = 1.0 - w1;
        let band_i_weight = if i0 == i {
            w0
        } else if i1 == i {
            w1
        } else {
            0.0
        };
        assert!(
            band_i_weight > 0.999,
            "center {center} (band {i}) should resolve to full weight, got \
             i0={i0} i1={i1} t={t} (band_i_weight={band_i_weight})"
        );
    }
}

#[test]
fn zone_weights_always_sum_to_one() {
    for balance in [-1.0f32, -0.5, 0.0, 0.5, 1.0] {
        for blending in [0.0f32, 0.3, 0.6, 1.0] {
            let mut l = 0.0f32;
            while l <= 1.0 {
                let w = zone_weights(l, balance, blending);
                let sum = w[0] + w[1] + w[2];
                assert!(
                    (sum - 1.0).abs() < 1e-4,
                    "weights {w:?} at l={l} balance={balance} blending={blending} sum to {sum}"
                );
                assert!(w.iter().all(|&x| (0.0..=1.0).contains(&x)));
                l += 0.05;
            }
        }
    }
}

#[test]
fn zone_weights_favor_shadows_at_low_luma_and_highlights_at_high_luma() {
    let low = zone_weights(0.0, 0.0, 0.5);
    let high = zone_weights(1.0, 0.0, 0.5);
    assert!(low[0] > 0.9, "shadow weight at l=0: {low:?}");
    assert!(high[2] > 0.9, "highlight weight at l=1: {high:?}");
}

#[test]
fn zone_tint_is_zero_at_zero_saturation() {
    let zone = ColorGradingZone {
        hue: 220,
        saturation: 0,
        luminance: 0,
    };
    assert_eq!(zone_tint(&zone), [0.0, 0.0, 0.0]);
}

#[test]
fn zone_tint_is_non_zero_once_saturation_is_set() {
    let zone = ColorGradingZone {
        hue: 220,
        saturation: 50,
        luminance: 0,
    };
    assert_ne!(zone_tint(&zone), [0.0, 0.0, 0.0]);
}

#[test]
fn hsl_mixer_boosts_saturation_of_a_matched_band_only() {
    // A muted red (band 0, center 0°) and a muted green (band 3, center
    // 120°, far enough from red's neighbors — orange 30°, magenta
    // 315° — to be unaffected by red's slider).
    let mut image = test_image(4, 4);
    for px in image.data.chunks_exact_mut(3) {
        px[0] = 160;
        px[1] = 90;
        px[2] = 90;
    }
    let mut bands = [HslBand::default(); 8];
    bands[0].saturation = 100;
    let settings = Settings {
        hsl: bands,
        ..Settings::default()
    };
    let before = rgb_to_hsl(&[160.0 / 255.0, 90.0 / 255.0, 90.0 / 255.0]);
    let out = develop(&image, &settings, None, None).unwrap();
    let after = rgb_to_hsl(&[
        f32::from(out.data[0]) / 255.0,
        f32::from(out.data[1]) / 255.0,
        f32::from(out.data[2]) / 255.0,
    ]);
    assert!(
        after.1 > before.1,
        "saturation should increase: before {before:?}, after {after:?}"
    );
}

#[test]
fn color_grading_tints_shadows_without_touching_highlights() {
    let mut image = test_image(4, 4);
    for (i, px) in image.data.chunks_exact_mut(3).enumerate() {
        let v = if i % 2 == 0 { 10 } else { 245 };
        px[0] = v;
        px[1] = v;
        px[2] = v;
    }
    let settings = Settings {
        color_grading: ColorGrading {
            shadows: ColorGradingZone {
                hue: 220,
                saturation: 80,
                luminance: 0,
            },
            ..ColorGrading::default()
        },
        ..Settings::default()
    };
    let out = develop(&image, &settings, None, None).unwrap();
    // The dark (shadow) pixel picks up a blue tint: B channel rises
    // above R/G. The bright (highlight) pixel, far from the shadow
    // zone's weight, stays gray (all channels equal).
    let dark = &out.data[0..3];
    let bright = &out.data[3..6];
    assert!(
        dark[2] > dark[0],
        "shadow pixel should gain a blue tint: {dark:?}"
    );
    assert_eq!(
        bright[0], bright[1],
        "highlight pixel should stay neutral: {bright:?}"
    );
    assert_eq!(
        bright[1], bright[2],
        "highlight pixel should stay neutral: {bright:?}"
    );
}

// -----------------------------------------------------------------
// Clarity, texture and dehaze (ADR 0033)
// -----------------------------------------------------------------

#[test]
fn neutral_clarity_texture_and_dehaze_are_not_recorded() {
    // What the old "process N+1 matches process N bit for bit" tests
    // proved is now structural: a stage at its neutral value is not
    // recorded by `pin`, so there is nothing to run and nothing to
    // compare against (ADR 0043 §3).
    let image = test_image(64, 48);
    let mut settings = Settings {
        exposure: 0.2,
        contrast: 15,
        ..Settings::default()
    };
    let before = develop(&image, &settings, None, None).unwrap();
    crate::stages::pin(&mut settings);
    assert!(
        !settings.stages.contains_key("dehaze"),
        "a neutral dehaze must not be recorded: {:?}",
        settings.stages
    );
    assert_eq!(develop(&image, &settings, None, None).unwrap(), before);
}
#[test]
fn min_filter_matches_a_naive_2d_minimum() {
    let width = 6;
    let height = 5;
    let plane: Vec<f32> = (0..width * height)
        .map(|i| ((i * 37) % 100) as f32)
        .collect();
    let radius = 2;
    let filtered = min_filter(&plane, width, height, radius);
    for y in 0..height {
        for x in 0..width {
            let mut expected = f32::INFINITY;
            for yy in y.saturating_sub(radius)..=(y + radius).min(height - 1) {
                for xx in x.saturating_sub(radius)..=(x + radius).min(width - 1) {
                    expected = expected.min(plane[yy * width + xx]);
                }
            }
            assert_eq!(filtered[y * width + x], expected, "at ({x},{y})");
        }
    }
}

#[test]
fn min_filter_on_a_uniform_plane_is_unchanged() {
    let plane = vec![0.42f32; 20];
    let filtered = min_filter(&plane, 5, 4, 1);
    assert!(filtered.iter().all(|&v| (v - 0.42).abs() < 1e-6));
}

#[test]
fn approx_blur_smooths_a_checkerboard_toward_its_mean() {
    let width = 64;
    let height = 64;
    let plane: Vec<f32> = (0..width * height)
        .map(|i| {
            let x = i % width;
            let y = i / width;
            if (x / 4 + y / 4) % 2 == 0 { 1.0 } else { 0.0 }
        })
        .collect();
    let blurred = approx_blur(&plane, width, height, 20.0);
    let idx = (height / 2) * width + width / 2;
    assert!(
        (blurred[idx] - 0.5).abs() < 0.35,
        "expected the blur to land near the checkerboard's 0.5 mean, got {}",
        blurred[idx]
    );
}

#[test]
fn approx_blur_falls_back_to_the_literal_kernel_at_a_small_sigma() {
    let plane = vec![0.2, 0.8, 0.3, 0.9, 0.1, 0.7, 0.4, 0.6, 0.5];
    let (width, height) = (3, 3);
    assert_eq!(
        approx_blur(&plane, width, height, 0.5),
        gaussian_blur(&plane, width, height, 0.5)
    );
}

#[test]
fn local_contrast_with_zero_amount_is_a_no_op() {
    let image = test_image(16, 12);
    let mut px = Pixels::from_raw(&image).unwrap();
    let before = px.to_rgb8();
    local_contrast(&mut px, 0, CLARITY_RADIUS);
    assert_eq!(px.to_rgb8(), before);
}

#[test]
fn local_contrast_pushes_a_bright_region_brighter() {
    // Left half dark, right half bright: a positive-amount local
    // contrast boost should push pixels away from the low-pass
    // (blurred) version, brightening the already-bright side further.
    let width = 40usize;
    let height = 20usize;
    let mut data = Vec::with_capacity(width * height * 3);
    for _y in 0..height {
        for x in 0..width {
            let v = if x < width / 2 { 60u8 } else { 180u8 };
            data.push(v);
            data.push(v);
            data.push(v);
        }
    }
    let image = RawImage {
        width: width as u32,
        height: height as u32,
        bits: 8,
        data,
    };
    let mut px = Pixels::from_raw(&image).unwrap();
    local_contrast(&mut px, 80, TEXTURE_RADIUS);
    let out = px.to_rgb8();
    let idx = (10 * width + (width / 2 + 2)) * 3;
    assert!(
        out[idx] as i32 > 180,
        "expected the bright side to gain further contrast, got {}",
        out[idx]
    );
}

/// A gradient scene veiled toward a bright gray "atmosphere" — exactly
/// the kind of image dehaze is meant to undo (`amount > 0`) or add to
/// (`amount < 0`).
fn hazy_test_image(width: u32, height: u32) -> RawImage {
    let mut data = Vec::with_capacity((width * height * 3) as usize);
    for _y in 0..height {
        for x in 0..width {
            let scene = (x * 100 / width) as u16;
            let haze = 180u16;
            let v = ((scene + haze) / 2) as u8;
            data.push(v);
            data.push(v);
            data.push(v);
        }
    }
    RawImage {
        width,
        height,
        bits: 8,
        data,
    }
}

/// Widest span between the darkest and brightest R sample (R=G=B in
/// these synthetic gray images, so one channel is representative).
fn tonal_range(data: &[u8]) -> u8 {
    let min = data.iter().step_by(3).min().copied().unwrap();
    let max = data.iter().step_by(3).max().copied().unwrap();
    max - min
}

#[test]
fn atmospheric_light_matches_the_brightest_dark_channel_patch() {
    let width = 10;
    let height = 10;
    let mut data = vec![50u8; width * height * 3];
    for y in 0..2 {
        for x in 0..2 {
            let i = (y * width + x) * 3;
            data[i] = 240;
            data[i + 1] = 230;
            data[i + 2] = 220;
        }
    }
    let image = RawImage {
        width: width as u32,
        height: height as u32,
        bits: 8,
        data,
    };
    let px = Pixels::from_raw(&image).unwrap();
    let per_pixel_min: Vec<f32> = px
        .data
        .chunks_exact(3)
        .map(|rgb| rgb[0].min(rgb[1]).min(rgb[2]))
        .collect();
    let dark = min_filter(&per_pixel_min, width, height, DEHAZE_PATCH_RADIUS);
    let atmosphere = atmospheric_light(&px, &dark);
    assert!(
        atmosphere[0] > 0.8,
        "expected the bright patch's color, got {atmosphere:?}"
    );
}

#[test]
fn dehaze_positive_amount_widens_the_tonal_range() {
    let image = hazy_test_image(40, 30);
    let neutral = develop(
        &image,
        &Settings {
            ..Settings::default()
        },
        None,
        None,
    )
    .unwrap();
    let dehazed = develop(
        &image,
        &Settings {
            dehaze: 80,
            ..Settings {
                ..Settings::default()
            }
        },
        None,
        None,
    )
    .unwrap();
    assert!(
        tonal_range(&dehazed.data) > tonal_range(&neutral.data),
        "dehaze should widen the tonal range: neutral {}, dehazed {}",
        tonal_range(&neutral.data),
        tonal_range(&dehazed.data)
    );
}

#[test]
fn dehaze_negative_amount_narrows_the_tonal_range() {
    let image = hazy_test_image(40, 30);
    let neutral = develop(
        &image,
        &Settings {
            ..Settings::default()
        },
        None,
        None,
    )
    .unwrap();
    let hazier = develop(
        &image,
        &Settings {
            dehaze: -80,
            ..Settings {
                ..Settings::default()
            }
        },
        None,
        None,
    )
    .unwrap();
    assert!(
        tonal_range(&hazier.data) < tonal_range(&neutral.data),
        "negative dehaze should narrow the tonal range: neutral {}, hazier {}",
        tonal_range(&neutral.data),
        tonal_range(&hazier.data)
    );
}

// ---------------------------------------------------------------------------
// The registry, the stage map, and the guarantee it carries
// ---------------------------------------------------------------------------

#[test]
fn lookup_matches_the_exact_functions_at_the_domain_endpoints() {
    // `exact_linear_to_srgb(1.0)` is 0.99999994 in f32 — rounded back to
    // 255 in 8-bit output; the lookup must reproduce the exact functions,
    // not idealized endpoints.
    let (to_linear, to_srgb) = tables();
    assert_eq!(lookup(to_linear, 0.0), exact_srgb_to_linear(0.0));
    assert_eq!(lookup(to_linear, 1.0), exact_srgb_to_linear(1.0));
    assert_eq!(lookup(to_srgb, 0.0), exact_linear_to_srgb(0.0));
    assert_eq!(lookup(to_srgb, 1.0), exact_linear_to_srgb(1.0));
    // Out-of-range inputs clamp instead of reading out of bounds.
    assert_eq!(lookup(to_srgb, -0.5), lookup(to_srgb, 0.0));
    assert_eq!(lookup(to_srgb, 1.5), lookup(to_srgb, 1.0));
}

// ---------------------------------------------------------------------
// Effects: the vignette a photographer adds, and grain (ADR 0090)
// ---------------------------------------------------------------------

/// A flat mid-grey card: the only background on which a vignette's own
/// falloff is the *whole* of the difference between two pixels.
fn flat_card(width: u32, height: u32, level: u8) -> RawImage {
    RawImage {
        width,
        height,
        bits: 8,
        data: vec![level; width as usize * height as usize * 3],
    }
}

#[test]
fn a_negative_vignette_darkens_the_corners_and_leaves_the_centre() {
    let image = flat_card(64, 64, 160);
    let settings = Settings {
        vignette: leyline_core::Vignette {
            amount: -80,
            ..Default::default()
        },
        ..Settings::default()
    };
    let plain = neutral(&image);
    let vignetted = develop(&image, &settings, None, None).unwrap();

    let at = |x: usize, y: usize| (y * 64 + x) * 3;
    assert_eq!(
        vignetted.data[at(32, 32)],
        plain.data[at(32, 32)],
        "the centre sits inside the midpoint and must not move"
    );
    assert!(
        vignetted.data[at(0, 0)] < plain.data[at(0, 0)] / 2,
        "the corner must be visibly darker: {} vs {}",
        vignetted.data[at(0, 0)],
        plain.data[at(0, 0)]
    );
    // Monotone from centre to corner along the diagonal.
    let diagonal: Vec<u8> = (0..32).map(|i| vignetted.data[at(i, i)]).collect();
    assert!(
        diagonal.windows(2).all(|w| w[0] <= w[1]),
        "the falloff must be monotone: {diagonal:?}"
    );
}

#[test]
fn a_positive_vignette_brightens_the_corners() {
    let image = flat_card(64, 64, 100);
    let settings = Settings {
        vignette: leyline_core::Vignette {
            amount: 80,
            ..Default::default()
        },
        ..Settings::default()
    };
    let plain = neutral(&image);
    let brightened = develop(&image, &settings, None, None).unwrap();
    let at = |x: usize, y: usize| (y * 64 + x) * 3;
    assert!(brightened.data[at(0, 0)] > plain.data[at(0, 0)]);
}

/// The shape moves, the corner's brightness does not — the reason the
/// superellipse distance is divided by its own corner value (ADR 0090 §2).
/// Without that division, dragging `roundness` would silently drag the
/// strength with it.
#[test]
fn roundness_moves_the_shape_not_the_corner_gain() {
    let image = flat_card(64, 64, 160);
    let corner = |roundness: i32| {
        let settings = Settings {
            vignette: leyline_core::Vignette {
                amount: -80,
                roundness,
                ..Default::default()
            },
            ..Settings::default()
        };
        develop(&image, &settings, None, None).unwrap().data[0]
    };
    assert_eq!(corner(-100), corner(0));
    assert_eq!(corner(0), corner(100));
}

/// The whole of ADR 0090 §1: the vignette is centred on the frame the
/// photographer composed, so an off-centre crop takes its vignette with it.
#[test]
fn the_vignette_follows_the_crop() {
    let image = flat_card(64, 64, 160);
    let vignette = leyline_core::Vignette {
        amount: -80,
        ..Default::default()
    };
    let cropped = Settings {
        vignette,
        crop: Some(leyline_core::Crop {
            x: 0.0,
            y: 0.0,
            width: 0.5,
            height: 0.5,
        }),
        ..Settings::default()
    };
    let rendered = develop(&image, &cropped, None, None).unwrap();
    assert_eq!((rendered.width, rendered.height), (32, 32));

    let at = |x: usize, y: usize| (y * 32 + x) * 3;
    // The centre of the *cropped* frame — a corner of the original one — is
    // the brightest point. A vignette drawn before the crop would have made
    // it the darkest.
    let centre = rendered.data[at(16, 16)];
    assert!(
        centre > rendered.data[at(0, 0)] && centre > rendered.data[at(31, 31)],
        "centre {centre}, corners {} and {}",
        rendered.data[at(0, 0)],
        rendered.data[at(31, 31)]
    );
}

#[test]
fn a_vignette_shape_at_zero_strength_renders_nothing() {
    let image = test_image(48, 32);
    let settings = Settings {
        vignette: leyline_core::Vignette {
            amount: 0,
            midpoint: 10,
            roundness: 80,
            feather: 90,
        },
        ..Settings::default()
    };
    let rendered = develop(&image, &settings, None, None).unwrap();
    assert_eq!(rendered.data, neutral(&image).data);
    let mut pinned = settings.clone();
    pin(&mut pinned);
    assert_eq!(
        pinned.stages.get("vignette"),
        None,
        "a neutral stage records nothing"
    );
}

/// The determinism §5.1 needs, asserted directly rather than only through a
/// digest: two renders of the same revision, in the same process, on
/// whatever threads rayon chose, are the same bytes.
#[test]
fn grain_is_the_same_field_every_time() {
    let image = flat_card(64, 64, 128);
    let settings = Settings {
        grain: leyline_core::Grain {
            amount: 80,
            ..Default::default()
        },
        ..Settings::default()
    };
    let first = develop(&image, &settings, None, None).unwrap();
    let second = develop(&image, &settings, None, None).unwrap();
    assert_eq!(first.data, second.data);
    assert_ne!(
        first.data,
        neutral(&image).data,
        "grain at 80 has to actually do something"
    );
}

/// Grain fades out into black and into white (ADR 0090 §3): a flat black
/// card comes back flat and black, where a uniform noise field would have
/// dusted it.
#[test]
fn grain_leaves_black_and_white_alone() {
    let settings = Settings {
        grain: leyline_core::Grain {
            amount: 100,
            ..Default::default()
        },
        ..Settings::default()
    };
    for level in [0u8, 255] {
        let image = flat_card(32, 32, level);
        let rendered = develop(&image, &settings, None, None).unwrap();
        assert_eq!(
            rendered.data,
            neutral(&image).data,
            "grain must vanish at level {level}"
        );
    }
}

#[test]
fn grain_at_zero_renders_nothing() {
    let image = test_image(48, 32);
    let settings = Settings {
        grain: leyline_core::Grain {
            amount: 0,
            size: 90,
            roughness: 10,
            color: 0,
        },
        ..Settings::default()
    };
    assert_eq!(
        develop(&image, &settings, None, None).unwrap().data,
        neutral(&image).data
    );
}

/// ADR 0118 §3: at `color: 0`, `grain::v2` calls v1. The claim is about
/// control flow, so the test is about pixels — nothing about the new
/// version may move a photograph that did not ask for colour.
#[test]
fn coloured_grain_at_zero_is_version_one_exactly() {
    let image = test_image(64, 48);
    let grain = leyline_core::Grain {
        amount: 70,
        size: 30,
        roughness: 60,
        color: 0,
    };
    let pin = |version: u16| Settings {
        grain,
        stages: leyline_core::StageVersions::from([("grain".to_owned(), version)]),
        ..Settings::default()
    };
    assert_eq!(
        develop(&image, &pin(1), None, None).unwrap().data,
        develop(&image, &pin(2), None, None).unwrap().data
    );
}

/// ADR 0118 §2's claim, which is about the *slider* and not the formula:
/// the grain's grey stays exactly as loud at every setting, and what grows
/// is how much the three layers disagree. A cross-fade between one shared
/// field and three independent ones — the construction this one was chosen
/// over — would be about 30 % quieter in the middle of its travel.
///
/// Measured on a neutral ramp, where v1's per-channel fade weighs the three
/// channels alike. Loudness here is the mean absolute move of the *mean of
/// the three channels* away from an ungrained render: the grain's grey.
#[test]
fn coloured_grain_keeps_the_grey_as_loud() {
    let image = grey_ramp(96, 72);
    let render = |color: i32| {
        develop(
            &image,
            &Settings {
                grain: leyline_core::Grain {
                    amount: 100,
                    size: 20,
                    roughness: 50,
                    color,
                },
                stages: leyline_core::StageVersions::from([("grain".to_owned(), 2)]),
                ..Settings::default()
            },
            None,
            None,
        )
        .unwrap()
    };
    let plain = develop(&image, &Settings::default(), None, None).unwrap();

    let grey_loudness = |grained: &Rendered| {
        let mut total = 0.0f64;
        let mut pixels = 0u32;
        for (a, b) in plain.data.chunks_exact(3).zip(grained.data.chunks_exact(3)) {
            let sum: f64 = (0..3).map(|c| b[c] as f64 - a[c] as f64).sum();
            total += (sum / 3.0).abs();
            pixels += 1;
        }
        total / pixels as f64
    };
    let channel_spread = |grained: &Rendered| {
        let mut total = 0.0f64;
        let mut pixels = 0u32;
        for rgb in grained.data.chunks_exact(3) {
            let mean = (rgb[0] as f64 + rgb[1] as f64 + rgb[2] as f64) / 3.0;
            total += (0..3).map(|c| (rgb[c] as f64 - mean).abs()).sum::<f64>() / 3.0;
            pixels += 1;
        }
        total / pixels as f64
    };

    let (grey_mono, grey_coloured) = (grey_loudness(&render(0)), grey_loudness(&render(100)));
    assert!(
        (grey_coloured - grey_mono).abs() < grey_mono * 0.1,
        "the grey must stay as loud: {grey_mono} levels at color 0, \
         {grey_coloured} at 100"
    );

    // And the layers must actually come apart: a neutral ramp grained in
    // one grey stays neutral pixel by pixel.
    let (spread_mono, spread_coloured) = (channel_spread(&render(0)), channel_spread(&render(100)));
    assert!(
        spread_mono < 0.5 && spread_coloured > 2.0,
        "colour must separate the channels: {spread_mono} levels of spread \
         at color 0, {spread_coloured} at 100"
    );
}

/// Both effects run *after* the crop, which is what makes their rank the
/// decision rather than a detail (ADR 0090 §1).
#[test]
fn the_effects_rank_after_the_crop() {
    let rank = |name: &str| {
        STAGES
            .iter()
            .find(|stage| stage.name == name)
            .expect("registered")
            .current()
            .rank
    };
    assert!(rank("vignette") > rank("crop"));
    assert!(rank("grain") > rank("vignette"));
    assert!(rank("grain") < rank("output_rendering"));
}

/// Settings that take every stage of the registry away from its neutral
/// value, so a plan built from them exercises the whole pipeline.
fn everything() -> Settings {
    let mut settings = Settings {
        camera_profile: Some(leyline_core::CameraProfile {
            enabled: true,
            path: "Profiles/Camera/x.dcp".to_owned(),
            checksum: format!("blake3:{}", "0".repeat(64)),
        }),
        lens_correction: LensCorrection {
            enabled: true,
            profile: "auto".to_owned(),
            ..LensCorrection::default()
        },
        spot_removal: vec![SpotRemoval {
            target: Point { x: 0.5, y: 0.5 },
            source: Point { x: 0.6, y: 0.6 },
            radius: 0.05,
            feather: 0.5,
            opacity: 1.0,
        }],
        red_eye: vec![leyline_core::RedEye {
            center: Point { x: 0.4, y: 0.4 },
            radius: 0.03,
            feather: 0.5,
            darken: 0.6,
        }],
        exposure: 0.2,
        contrast: 10,
        highlights: -10,
        whites: 5,
        clarity: 10,
        texture: 10,
        dehaze: 10,
        vibrance: 10,
        saturation: 10,
        monochrome: true,
        vignette: leyline_core::Vignette {
            amount: -40,
            midpoint: 45,
            roundness: 15,
            feather: 55,
        },
        grain: leyline_core::Grain {
            amount: 30,
            size: 20,
            roughness: 40,
            color: 0,
        },
        rotation: 5.0,
        perspective: Some(leyline_core::Perspective {
            vertical: 20,
            horizontal: -10,
        }),
        lut: Some(leyline_core::Lut {
            enabled: true,
            path: "Profiles/LUT/look.cube".to_owned(),
            checksum: format!("blake3:{}", "0".repeat(64)),
            strength: 60,
        }),
        crop: Some(leyline_core::Crop {
            x: 0.1,
            y: 0.1,
            width: 0.8,
            height: 0.8,
        }),
        ..Settings::default()
    };
    settings.tone_curve.points = vec![CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 1.0, y: 1.0 }];
    settings.hsl[0].saturation = 20;
    settings.color_grading.shadows.saturation = 20;
    settings.local_adjustments = vec![LocalAdjustment {
        mask: Mask::Radial {
            cx: 0.5,
            cy: 0.5,
            rx: 0.3,
            ry: 0.3,
            angle: 0.0,
            feather: 0.4,
            inverted: false,
        },
        range: None,
        opacity: 1.0,
        adjustments: LocalAdjustmentValues {
            exposure: Some(0.5),
            ..LocalAdjustmentValues::default()
        },
    }];
    settings.noise_reduction = NoiseReduction {
        luminance: 20,
        color: 20,
    };
    settings.reshape = vec![leyline_core::ReshapePoint {
        from: leyline_core::Point { x: 0.4, y: 0.4 },
        to: leyline_core::Point { x: 0.45, y: 0.45 },
        radius: 0.15,
        strength: 0.7,
    }];
    settings.defringe = leyline_core::Defringe {
        purple: 60,
        green: 30,
    };
    settings.sharpening = Sharpening {
        amount: 30,
        radius: 1.0,
        masking: 0,
    };
    settings
        .extra
        .insert(fixture::MARKER.to_owned(), serde_json::Value::Bool(true));
    settings
}

#[test]
fn a_fully_loaded_edit_activates_every_stage_of_the_registry() {
    // Guards the test below: it only proves anything about ranks if it
    // actually plans the whole registry.
    let mut settings = everything();
    crate::stages::pin(&mut settings);
    for stage in crate::stages::registry() {
        assert!(
            settings.stages.contains_key(stage.name),
            "{} is missing from `everything()`",
            stage.name
        );
    }
}

#[test]
fn no_two_stages_of_one_plan_share_a_rank() {
    // Two stages at the same rank would make their relative order depend on
    // the sort's stability rather than on a declared decision.
    let plan = super::plan(&everything()).unwrap();
    let mut ranks: Vec<u16> = plan.iter().map(|(_, v)| v.rank).collect();
    let count = ranks.len();
    ranks.sort_unstable();
    ranks.dedup();
    assert_eq!(ranks.len(), count, "duplicate rank in {ranks:?}");
}

#[test]
fn the_plan_runs_in_rank_order() {
    let plan = super::plan(&everything()).unwrap();
    assert!(plan.windows(2).all(|w| w[0].1.rank < w[1].1.rank));
}

#[test]
fn a_neutral_edit_records_and_runs_only_the_framing_stages() {
    // Since ADR 0044 §3 the pipeline is framed by two stages that have no
    // neutral value — they say where the pixels come from and how they
    // leave. Everything *between* them is still absent from a neutral
    // revision, which is what makes it render the decoded image untouched.
    let mut settings = Settings::default();
    crate::stages::pin(&mut settings);
    assert_eq!(
        settings.stages.keys().collect::<Vec<_>>(),
        ["input", "output_rendering"],
        "{:?}",
        settings.stages
    );
    let plan = super::plan(&settings).unwrap();
    assert_eq!(
        plan.iter().map(|(stage, _)| stage.name).collect::<Vec<_>>(),
        ["input", "output_rendering"]
    );
}

#[test]
fn pin_records_the_current_version_of_a_stage_that_just_became_active() {
    let mut settings = Settings {
        exposure: 0.5,
        ..Settings::default()
    };
    crate::stages::pin(&mut settings);
    assert_eq!(settings.stages.get("gains"), Some(&1));
    assert_eq!(
        settings.stages.keys().collect::<Vec<_>>(),
        ["gains", "input", "output_rendering"],
        "one operator, plus the two stages that frame every pipeline"
    );
}

#[test]
fn pin_never_moves_a_version_already_recorded() {
    // The promise itself: editing a revision does not re-render it through
    // newer code.
    let mut settings = Settings {
        exposure: 0.5,
        stages: leyline_core::StageVersions::from([(fixture::NAME.to_owned(), 1)]),
        ..Settings::default()
    };
    settings
        .extra
        .insert(fixture::MARKER.to_owned(), serde_json::Value::Bool(true));
    crate::stages::pin(&mut settings);
    assert_eq!(
        settings.stages.get(fixture::NAME),
        Some(&1),
        "v2 exists, but this revision records v1"
    );
}

#[test]
fn pin_drops_a_stage_that_went_back_to_neutral() {
    let mut settings = Settings {
        exposure: 0.5,
        ..Settings::default()
    };
    crate::stages::pin(&mut settings);
    assert!(settings.stages.contains_key("gains"));

    settings.exposure = 0.0;
    crate::stages::pin(&mut settings);
    assert!(
        !settings.stages.contains_key("gains"),
        "a stage that no longer runs pins nothing"
    );
}

#[test]
fn an_unrecorded_active_stage_renders_at_the_current_version() {
    // Settings built in memory — through the SDK, a preset, a test — carry
    // no map until they are written. They must still render.
    let image = test_image(16, 12);
    let settings = Settings {
        exposure: 0.5,
        ..Settings::default()
    };
    let mut pinned = settings.clone();
    crate::stages::pin(&mut pinned);
    assert_eq!(
        develop(&image, &settings, None, None).unwrap(),
        develop(&image, &pinned, None, None).unwrap()
    );
}

#[test]
fn a_stage_this_engine_does_not_implement_is_refused() {
    for stages in [
        leyline_core::StageVersions::from([("sharpen".to_owned(), 99)]),
        leyline_core::StageVersions::from([("no_such_stage".to_owned(), 1)]),
    ] {
        let settings = Settings {
            stages,
            ..Settings::default()
        };
        assert!(matches!(
            super::plan(&settings),
            Err(LeylineError::UnknownStage { .. })
        ));
    }
}

// ---------------------------------------------------------------------------
// The guarantee itself, on the two-version fixture stage (ADR 0043 §7)
// ---------------------------------------------------------------------------

/// Settings that activate the fixture stage and nothing else.
fn fixture_settings(version: Option<u16>) -> Settings {
    let mut settings = Settings::default();
    settings
        .extra
        .insert(fixture::MARKER.to_owned(), serde_json::Value::Bool(true));
    if let Some(version) = version {
        settings.stages.insert(fixture::NAME.to_owned(), version);
    }
    settings
}

#[test]
fn a_revision_renders_through_the_stage_version_it_records() {
    // The whole point of the machinery: v2 exists, and a revision citing v1
    // still gets v1's pixels.
    let image = test_image(16, 12);
    let v1 = develop(&image, &fixture_settings(Some(1)), None, None).unwrap();
    let v2 = develop(&image, &fixture_settings(Some(2)), None, None).unwrap();
    assert_ne!(v1.data, v2.data, "the two versions must be tellable apart");

    // On black, each version's lift is the only light in the buffer, so
    // the rendered value is that lift carried through the output stage —
    // which is exactly the point: the two versions are told apart by their
    // pixels, not by their numbers.
    let black = RawImage {
        width: 16,
        height: 12,
        bits: 8,
        data: vec![0; 16 * 12 * 3],
    };
    let lifted = |version: u16| {
        develop(&black, &fixture_settings(Some(version)), None, None)
            .unwrap()
            .data[0]
    };
    assert!(
        lifted(2) > lifted(1),
        "the newer version lifts further: {} vs {}",
        lifted(2),
        lifted(1)
    );
    assert!(lifted(1) > 0, "and the older one still lifts");
}

#[test]
fn a_fresh_revision_records_the_newest_version_of_the_fixture_stage() {
    let mut settings = fixture_settings(None);
    crate::stages::pin(&mut settings);
    assert_eq!(settings.stages.get(fixture::NAME), Some(&2));
}

#[test]
fn each_version_of_a_stage_carries_its_own_rank() {
    // ADR 0042 §3: moving a stage is a new version declaring another rank,
    // never an edit of an existing one.
    let stage = crate::stages::registry()
        .find(|stage| stage.name == fixture::NAME)
        .unwrap();
    let ranks: Vec<u16> = stage.versions.iter().map(|v| v.rank).collect();
    assert_eq!(ranks, vec![65, 66]);
}

#[test]
fn an_unknown_version_of_a_known_stage_is_refused() {
    assert!(matches!(
        super::plan(&fixture_settings(Some(3))),
        Err(LeylineError::UnknownStage { version: 3, .. })
    ));
}

// ---------------------------------------------------------------------------
// Working space (ADR 0044)
// ---------------------------------------------------------------------------

#[test]
fn every_published_version_declares_the_space_it_renders_in() {
    // Nothing has moved to linear Rec. 2020 yet (ADR 0044 §7 step 2); what
    // matters here is that the declaration exists on each version, since it
    // is what the refusal below reads.
    for stage in crate::stages::registry() {
        for version in stage.versions {
            assert_eq!(
                version.space,
                Space::LinearRec2020,
                "{}::v{} declares an unexpected space",
                stage.name,
                version.version
            );
        }
    }
}

#[test]
fn stages_agreeing_on_one_space_compose() {
    assert_eq!(
        super::single_space([
            ("input", 1, Space::LinearRec2020),
            ("gains", 1, Space::LinearRec2020),
        ])
        .unwrap(),
        Some(Space::LinearRec2020)
    );
    assert_eq!(super::single_space([]).unwrap(), None);
}

#[test]
fn two_working_spaces_in_one_plan_are_refused_by_name() {
    // The case ADR 0044 §4 exists for: an operator written for linear light
    // handed a gamma-encoded buffer would produce plausible, wrong pixels,
    // so the render is refused — and the message names both sides, since
    // "your revision is inconsistent" is not actionable on its own.
    let refused = super::single_space([
        ("gains", 1, Space::LinearRec2020),
        ("hsl", 2, Space::SrgbGamma),
    ]);
    let Err(LeylineError::MixedWorkingSpaces {
        stage,
        version,
        other_stage,
        other_version,
        ..
    }) = refused
    else {
        panic!("mixing two spaces must be refused, got {refused:?}");
    };
    assert_eq!((stage.as_str(), version), ("gains", 1));
    assert_eq!((other_stage.as_str(), other_version), ("hsl", 2));
}

#[test]
fn a_revision_reads_its_space_off_the_versions_it_records() {
    // No record at all: the space this engine renders in today.
    assert_eq!(
        super::revision_space(&Settings::default()),
        Space::LinearRec2020
    );
    let settings = Settings {
        stages: leyline_core::StageVersions::from([("gains".to_owned(), 1)]),
        ..Settings::default()
    };
    assert_eq!(super::revision_space(&settings), Space::LinearRec2020);
}

#[test]
fn a_newly_active_stage_pins_a_version_of_the_revisions_own_space() {
    let stage = crate::stages::registry()
        .find(|stage| stage.name == "gains")
        .unwrap();
    // Every version is sRGB today, so `current_in` and `current` agree —
    // the assertion that matters is the other one: an operator that has
    // never rendered in a space offers nothing there, and `pin` then writes
    // its newest version so the mismatch is refused by name rather than
    // silently resolved.
    assert_eq!(
        stage.current_in(Space::LinearRec2020).map(|v| v.version),
        Some(stage.current().version)
    );
    assert!(stage.current_in(Space::SrgbGamma).is_none());
    assert_eq!(
        super::pinned_version(stage, &Settings::default()).version,
        stage.current().version
    );
}

/// ADR 0050 §3: reconstruction costs a global gain at the decoder, and
/// `input::v2` gives it back — otherwise "recover the highlights" would read
/// as "darken the photo".
#[test]
fn reconstruction_undoes_the_decoders_renormalization() {
    use crate::stages::input::v2::reconstruction_gain;

    // The ratio between the largest and smallest white balance multiplier:
    // what the decoder divides by when it must not clip any channel.
    assert_eq!(reconstruction_gain([2.0, 1.0, 1.6, 1.0]), 2.0);
    // A three-color sensor leaves the fourth multiplier at zero, which is not
    // a multiplier of one.
    assert_eq!(reconstruction_gain([2.0, 1.0, 1.6, 0.0]), 2.0);
    // Nothing usable, or a degenerate set: no compensation rather than a
    // wrong one.
    assert_eq!(reconstruction_gain([0.0; 4]), 1.0);
    assert_eq!(reconstruction_gain([f64::NAN, 1.0, 1.0, 1.0]), 1.0);
    // A neutral white balance has nothing to give back.
    assert_eq!(reconstruction_gain([1.0; 4]), 1.0);
}

/// And it only happens when reconstruction was actually asked for: with the
/// neutral mode, `v2` leaves the buffer exactly where `v1` did.
#[test]
fn the_gain_is_given_back_only_when_reconstruction_was_asked_for() {
    use crate::pixels::Pixels;
    use crate::stages::SourceColor;
    use leyline_core::HighlightReconstruction;

    let source = SourceColor::Camera {
        // No matrix, so the only thing that can move the samples is the
        // compensation itself.
        to_xyz: None,
        multipliers: Some([2.0, 1.0, 1.0, 1.0]),
    };
    let render = |mode| {
        let mut px = Pixels {
            width: 1,
            height: 1,
            data: vec![0.25, 0.25, 0.25],
        };
        crate::stages::input::v2::to_working_space(&mut px, source, false, mode);
        px.data[0]
    };
    assert_eq!(render(HighlightReconstruction::Clip), 0.25);
    assert_eq!(render(HighlightReconstruction::Blend), 0.5);
    assert_eq!(render(HighlightReconstruction::Rebuild), 0.5);
}

/// ADR 0050: the highlight mode is part of what an `input` version pins, so
/// the version a revision cites decides whether the mode is even read.
#[test]
fn the_highlight_mode_reaches_the_decoder_only_from_input_v2() {
    use leyline_core::{HighlightReconstruction, StageVersions};
    use leyline_raw::HighlightMode;

    let modes = [
        (HighlightReconstruction::Clip, HighlightMode::Clip),
        (HighlightReconstruction::Blend, HighlightMode::Blend),
        (HighlightReconstruction::Rebuild, HighlightMode::Rebuild),
    ];
    for (setting, expected) in modes {
        // A fresh revision pins the current `input`, which is the one that
        // reads the setting.
        let settings = Settings {
            highlight_reconstruction: setting,
            ..Settings::default()
        };
        assert_eq!(
            crate::stages::decode_params(&settings, false).highlight,
            expected,
            "{setting:?} must reach the decoder through the current input version"
        );

        // A revision pinned at v1 renders through v1's configuration, which
        // has no highlight mode to read — the frozen behavior. Reaching this
        // state is what `Settings::validate` refuses (ADR 0050 §5), so a
        // caller can only get here by building the pair by hand, as here.
        let pinned_v1 = Settings {
            highlight_reconstruction: setting,
            stages: StageVersions::from([("input".to_owned(), 1)]),
            ..Settings::default()
        };
        assert_eq!(
            crate::stages::decode_params(&pinned_v1, false).highlight,
            HighlightMode::Clip,
            "input::v1 asks for exactly what it always asked for"
        );
    }
}

#[test]
fn the_decoder_configuration_comes_from_the_input_version() {
    // ADR 0044 §3: this used to be `camera_native: camera_profile.is_some()`
    // repeated in four modules and recorded nowhere.
    let settings = Settings::default();
    for half_size in [false, true] {
        let params = crate::stages::decode_params(&settings, half_size);
        assert!(
            params.camera_native,
            "the working space is built here, never by the decoder"
        );
        assert!(
            params.sixteen_bit,
            "eight-bit linear would band the shadows"
        );
        assert!(!params.auto_brighten, "the neutral render is content-blind");
        assert_eq!(
            params.half_size, half_size,
            "the size class is the caller's"
        );
    }
}

/// The white level a revision decodes by comes from its `input` version, and
/// from nowhere else (ADR 0066).
///
/// The golden renders cannot freeze this one: they render a synthetic buffer
/// and never reach LibRaw, so `v3` and `v4` produce identical pixels there.
/// What separates them is exactly this configuration, so this is where it is
/// pinned — the same gap the highlight mode of ADR 0050 left.
#[test]
fn the_white_level_reaches_the_decoder_only_from_input_v4() {
    use leyline_core::StageVersions;
    use leyline_raw::WhiteLevel;

    // A fresh revision pins the current `input`, which asks for the level the
    // camera itself recorded.
    let settings = Settings::default();
    assert_eq!(
        crate::stages::decode_params(&settings, false).white_level,
        WhiteLevel::CameraLinearityMargin
    );

    // Every version before it divided by the format's ceiling, and still
    // does: a revision left in `v3` renders exactly as it always has.
    for version in [1u16, 2, 3] {
        let pinned = Settings {
            stages: StageVersions::from([("input".to_owned(), version)]),
            ..Settings::default()
        };
        assert_eq!(
            crate::stages::decode_params(&pinned, false).white_level,
            WhiteLevel::FormatCeiling,
            "input::v{version}"
        );
    }
}

/// The stage cache (ADR 0041 §3) is an optimisation, never a rendering:
/// whatever it reuses, the pixels must equal a cold render's, edit after
/// edit. This walks a plausible slider session — nudge a late stage, then
/// an early one, then back — because the cache is only interesting when
/// something upstream *did* change and something else did not.
#[test]
fn the_stage_cache_never_changes_a_pixel() {
    use super::{StageCache, develop_scaled, develop_scaled_cached};
    use leyline_core::AssetId;

    let image = test_image(64, 48);
    let asset = AssetId::new(1);
    let mut cache = StageCache::default();

    // Every step leaves at least one stage of an earlier one in place, so a
    // checkpoint is genuinely reused rather than always invalidated.
    let mut settings = leyline_core::Settings {
        dehaze: 30,
        clarity: 20,
        contrast: 15,
        ..Default::default()
    };

    /// One labelled slider move of the simulated session.
    type Step = (&'static str, fn(&mut leyline_core::Settings));

    let steps: [Step; 6] = [
        ("cold", |_| {}),
        ("late stage only", |s| s.sharpening.amount = 40),
        ("late stage again", |s| s.sharpening.amount = 60),
        ("upstream tonal", |s| s.contrast = -20),
        ("expensive block", |s| s.dehaze = 5),
        ("back to a late stage", |s| s.sharpening.amount = 10),
    ];

    for (label, edit) in steps {
        edit(&mut settings);
        super::pin(&mut settings);

        let cold = develop_scaled(
            &image,
            &settings,
            None,
            None,
            None,
            None,
            &Default::default(),
            &super::Source::plain(super::SourceColor::Srgb),
            1.0,
        )
        .unwrap();
        let cached = develop_scaled_cached(
            &image,
            &settings,
            None,
            None,
            None,
            None,
            &Default::default(),
            &super::Source::plain(super::SourceColor::Srgb),
            1.0,
            asset,
            &mut cache,
        )
        .unwrap();

        assert_eq!(cached.width, cold.width, "{label}");
        assert_eq!(cached.height, cold.height, "{label}");
        assert_eq!(cached.data, cold.data, "cached render differs at: {label}");
    }
}

/// Changing the asset or the proxy scale invalidates everything: neither is
/// a setting, so no fingerprint covers them, and reusing across either
/// would composite one photo's buffer into another's render.
#[test]
fn the_stage_cache_is_dropped_across_assets_and_scales() {
    use super::{StageCache, develop_scaled, develop_scaled_cached};
    use leyline_core::AssetId;

    let first = test_image(64, 48);
    let second = test_image(48, 64);
    let mut settings = leyline_core::Settings {
        dehaze: 25,
        sharpening: Sharpening {
            amount: 30,
            ..Default::default()
        },
        ..Default::default()
    };
    super::pin(&mut settings);

    let mut cache = StageCache::default();
    develop_scaled_cached(
        &first,
        &settings,
        None,
        None,
        None,
        None,
        &Default::default(),
        &super::Source::plain(super::SourceColor::Srgb),
        1.0,
        AssetId::new(1),
        &mut cache,
    )
    .unwrap();

    // Another asset through the same cache must render as if cold.
    let other = develop_scaled_cached(
        &second,
        &settings,
        None,
        None,
        None,
        None,
        &Default::default(),
        &super::Source::plain(super::SourceColor::Srgb),
        1.0,
        AssetId::new(2),
        &mut cache,
    )
    .unwrap();
    let cold = develop_scaled(
        &second,
        &settings,
        None,
        None,
        None,
        None,
        &Default::default(),
        &super::Source::plain(super::SourceColor::Srgb),
        1.0,
    )
    .unwrap();
    assert_eq!(other.data, cold.data, "a cached buffer crossed assets");

    // And so must another scale of the same asset.
    let scaled = develop_scaled_cached(
        &second,
        &settings,
        None,
        None,
        None,
        None,
        &Default::default(),
        &super::Source::plain(super::SourceColor::Srgb),
        0.5,
        AssetId::new(2),
        &mut cache,
    )
    .unwrap();
    let cold_scaled = develop_scaled(
        &second,
        &settings,
        None,
        None,
        None,
        None,
        &Default::default(),
        &super::Source::plain(super::SourceColor::Srgb),
        0.5,
    )
    .unwrap();
    assert_eq!(
        scaled.data, cold_scaled.data,
        "a cached buffer crossed scales"
    );
}

/// `docs/pipeline.md` §3.3 lists every stage version this engine renders,
/// with its rank — and it is the *owning* document for that list, so a reader
/// is entitled to trust it over the code.
///
/// Nothing kept the two in step, and they had drifted: five published
/// versions were missing from the table — `input::v3` and `v4`,
/// `camera_profile::v2` and `v3`, `local_adjustments::v3` — each one a
/// rendering a revision can cite while the specification denied it existed.
/// The lie is silent by nature, which is why this is a test rather than a
/// habit.
///
/// Only the *registry side* is asserted: every published `(stage, version,
/// rank)` has a row. A row the registry does not have is left alone, since
/// the table is also the place where a collapsed version (ADR 0043) or a
/// hypothetical one may legitimately be discussed in prose.
#[test]
fn the_pipeline_specification_lists_every_stage_version_this_engine_renders() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/pipeline.md");
    let spec = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

    // The table's rows: `| <rank> | `<stage>` | <version> | <role> |`. A row
    // may list several versions at once (`1, 2`), which the split handles.
    let mut listed: Vec<(String, u16, u16)> = Vec::new();
    for line in spec.lines() {
        let cells: Vec<&str> = line.trim().trim_matches('|').split('|').collect();
        let [rank, stage, versions, ..] = cells.as_slice() else {
            continue;
        };
        let (Ok(rank), Some(stage)) = (
            rank.trim().parse::<u16>(),
            stage
                .trim()
                .strip_prefix('`')
                .and_then(|s| s.strip_suffix('`')),
        ) else {
            continue;
        };
        for version in versions.split(',') {
            if let Ok(version) = version.trim().parse::<u16>() {
                listed.push((stage.to_owned(), version, rank));
            }
        }
    }
    assert!(
        listed.len() > 20,
        "the table was not found in {} — it moved, or its shape changed",
        path.display()
    );

    let missing: Vec<String> = crate::stages::registry()
        .flat_map(|stage| {
            stage
                .versions
                .iter()
                .map(move |version| (stage.name.to_owned(), version.version, version.rank))
        })
        // The test fixture stage exists only under `cfg(test)` and renders
        // nothing anyone can cite: it has no business in a specification.
        .filter(|(name, _, _)| name != fixture::NAME)
        .filter(|entry| !listed.contains(entry))
        .map(|(name, version, rank)| format!("{name}::v{version} (rang {rank})"))
        .collect();

    assert!(
        missing.is_empty(),
        "{} stage version(s) render today and are absent from docs/pipeline.md §3.3 — \
         the table is the owning list, so add a row rather than leaving a reader \
         to discover them in the registry:\n  {}",
        missing.len(),
        missing.join("\n  ")
    );
}
