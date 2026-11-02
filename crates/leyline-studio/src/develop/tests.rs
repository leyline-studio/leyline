use leyline_sdk::{CameraProfile, Perspective, WhiteBalance};

use super::*;

/// ADR 0119: a keystone guide comes back twice — once for the solver,
/// in the frame the stage sees, and once for the overlay, in the
/// displayed frame's own units.
#[test]
fn a_keystone_guide_is_kept_in_both_frames() {
    // A square viewport showing a 2:1 image: the letterbox puts the
    // photograph in the middle half.
    let (view, image) = ((200.0, 200.0), (200.0, 100.0));
    let (line, drawn) =
        keystone_line((50.0, 50.0), (150.0, 150.0), view, image, None).expect("a drag");
    assert!((drawn[0] - 0.25).abs() < 1e-9, "{drawn:?}");
    assert!((drawn[1] - 0.0).abs() < 1e-9, "{drawn:?}");
    assert!((drawn[2] - 0.75).abs() < 1e-9, "{drawn:?}");
    assert!((drawn[3] - 1.0).abs() < 1e-9, "{drawn:?}");
    // Nothing cropped, so the two frames coincide.
    assert!((line.a.x - 0.25).abs() < 1e-9 && (line.b.y - 1.0).abs() < 1e-9);
}

/// The crop is undone on the way in — the guides answer a question about
/// the *uncropped* buffer, and getting that wrong would tilt every
/// answer on a cropped photograph.
#[test]
fn a_keystone_guide_is_read_through_the_crop() {
    let crop = Crop {
        x: 0.25,
        y: 0.5,
        width: 0.5,
        height: 0.25,
    };
    let (line, drawn) = keystone_line(
        (0.0, 0.0),
        (100.0, 100.0),
        (100.0, 100.0),
        (100.0, 100.0),
        Some(&crop),
    )
    .expect("a drag");
    // The overlay still speaks of the picture on screen.
    assert_eq!(drawn, [0.0, 0.0, 1.0, 1.0]);
    // The solver is handed the same drag inside the cropped rectangle.
    assert!(
        (line.a.x - 0.25).abs() < 1e-9 && (line.a.y - 0.5).abs() < 1e-9,
        "{line:?}"
    );
    assert!(
        (line.b.x - 0.75).abs() < 1e-9 && (line.b.y - 0.75).abs() < 1e-9,
        "{line:?}"
    );
}

/// A click is not a guide.
#[test]
fn a_click_is_not_a_keystone_guide() {
    assert!(
        keystone_line(
            (40.0, 40.0),
            (40.0, 40.0),
            (100.0, 100.0),
            (100.0, 100.0),
            None
        )
        .is_none()
    );
}

/// Neutral settings, with the given white balance / crop overrides.
fn settings(wb: Option<WhiteBalance>, crop: Option<Crop>) -> Settings {
    Settings {
        white_balance: wb,
        crop,
        ..Settings::default()
    }
}

#[test]
fn exposure_and_rotation_stay_floats() {
    let neutral = settings(None, None);
    assert_eq!(
        action("exposure", 1.25, &neutral),
        Some((Param::Exposure, Value::Float(1.25)))
    );
    assert_eq!(
        action("rotation", -2.5, &neutral),
        Some((Param::Rotation, Value::Float(-2.5)))
    );
}

#[test]
fn sliders_round_to_integers() {
    let neutral = settings(None, None);
    assert_eq!(
        action("contrast", 24.6, &neutral),
        Some((Param::Contrast, Value::Int(25)))
    );
    assert_eq!(
        action("saturation", -99.7, &neutral),
        Some((Param::Saturation, Value::Int(-100)))
    );
}

#[test]
fn white_balance_sliders_merge_into_the_override() {
    let current = settings(
        Some(WhiteBalance {
            temperature: 5200,
            tint: 12,
        }),
        None,
    );
    assert_eq!(
        action("wb-temp", 7000.0, &current),
        Some((
            Param::WhiteBalance,
            Value::WhiteBalance(Some(WhiteBalance {
                temperature: 7000,
                tint: 12,
            }))
        ))
    );
    assert_eq!(
        action("wb-tint", -20.0, &current),
        Some((
            Param::WhiteBalance,
            Value::WhiteBalance(Some(WhiteBalance {
                temperature: 5200,
                tint: -20,
            }))
        ))
    );
}

#[test]
fn as_shot_starts_from_the_default_override() {
    let default = WhiteBalance::default();
    assert_eq!(
        action("wb-tint", 15.0, &settings(None, None)),
        Some((
            Param::WhiteBalance,
            Value::WhiteBalance(Some(WhiteBalance {
                temperature: default.temperature,
                tint: 15,
            }))
        ))
    );
}

#[test]
fn detail_sliders_keep_their_sibling_field() {
    let mut current = settings(None, None);
    current.noise_reduction.color = 40;
    current.sharpening.radius = 2.0;
    assert_eq!(
        action("nr-luminance", 30.0, &current),
        Some((
            Param::NoiseReduction,
            Value::NoiseReduction(NoiseReduction {
                luminance: 30,
                color: 40,
            })
        ))
    );
    assert_eq!(
        action("sharpen-amount", 55.0, &current),
        Some((
            Param::Sharpening,
            Value::Sharpening(Sharpening {
                amount: 55,
                radius: 2.0,
                masking: 0,
            })
        ))
    );
    assert_eq!(
        action("sharpen-radius", 0.0, &current),
        Some((
            Param::Sharpening,
            Value::Sharpening(Sharpening {
                amount: 0,
                radius: 0.1,
                masking: 0,
            })
        ))
    );
}

#[test]
fn camera_profile_toggles_the_referenced_profile_and_never_invents_one() {
    let neutral = settings(None, None);
    assert_eq!(action("camera-profile", 1.0, &neutral), None);

    let referenced = Settings {
        camera_profile: Some(CameraProfile {
            enabled: true,
            path: "Profiles/Camera/mine.dcp".to_owned(),
            checksum: format!("blake3:{}", "a".repeat(64)),
        }),
        ..neutral
    };
    let Some((Param::CameraProfile, Value::CameraProfile(Some(off)))) =
        action("camera-profile", 0.0, &referenced)
    else {
        panic!("expected a camera profile update");
    };
    assert!(!off.enabled);
    // The path and checksum are carried through untouched.
    assert_eq!(off.path, "Profiles/Camera/mine.dcp");
    assert_eq!(off.checksum, referenced.camera_profile.unwrap().checksum);
}

#[test]
fn lens_correction_toggles_on_and_off() {
    let neutral = settings(None, None);
    assert_eq!(
        action("lens-correction", 1.0, &neutral),
        Some((
            Param::LensCorrection,
            Value::LensCorrection(LensCorrection {
                enabled: true,
                profile: "auto".to_owned(),
                ..LensCorrection::default()
            })
        ))
    );
    assert_eq!(
        action("lens-correction", 0.0, &neutral),
        Some((
            Param::LensCorrection,
            Value::LensCorrection(LensCorrection {
                enabled: false,
                profile: "auto".to_owned(),
                ..LensCorrection::default()
            })
        ))
    );
}

#[test]
fn the_demosaic_picker_names_its_four_algorithms() {
    for (name, expected) in [
        ("ahd", Demosaic::Ahd),
        ("vng", Demosaic::Vng),
        ("dcb", Demosaic::Dcb),
        ("dht", Demosaic::Dht),
    ] {
        assert_eq!(
            demosaic_action(name),
            Some((Param::Demosaic, Value::Demosaic(expected))),
            "{name}"
        );
    }
    // AMaZE and LMMSE live in LibRaw's GPL demosaic packs, absent from
    // the linked build (ADR 0061): naming one must be no update, never
    // a silent fall back to AHD.
    assert_eq!(demosaic_action("amaze"), None);
    assert_eq!(demosaic_action("lmmse"), None);
}

#[test]
fn the_highlight_reconstruction_picker_names_its_three_modes() {
    assert_eq!(
        highlight_reconstruction_action("rebuild"),
        Some((
            Param::HighlightReconstruction,
            Value::HighlightReconstruction(HighlightReconstruction::Rebuild)
        ))
    );
    assert_eq!(
        highlight_reconstruction_action("clip"),
        Some((
            Param::HighlightReconstruction,
            Value::HighlightReconstruction(HighlightReconstruction::Clip)
        ))
    );
    assert_eq!(highlight_reconstruction_action("guess"), None);
}

#[test]
fn the_perspective_sliders_merge_and_clear_together() {
    let neutral = settings(None, None);
    assert_eq!(
        action("perspective-vertical", 40.0, &neutral),
        Some((
            Param::Perspective,
            Value::Perspective(Some(Perspective {
                vertical: 40,
                horizontal: 0
            }))
        ))
    );
    let tilted = Settings {
        perspective: Some(Perspective {
            vertical: 40,
            horizontal: 0,
        }),
        ..settings(None, None)
    };
    // Moving one slider keeps the other.
    assert_eq!(
        action("perspective-horizontal", -20.0, &tilted),
        Some((
            Param::Perspective,
            Value::Perspective(Some(Perspective {
                vertical: 40,
                horizontal: -20
            }))
        ))
    );
    // Back to zero on the only non-zero term clears the override.
    assert_eq!(
        action("perspective-vertical", 0.0, &tilted),
        Some((Param::Perspective, Value::Perspective(None)))
    );
}

/// A look is a referenced file: nothing here invents one, exactly like the
/// camera profile above.
#[test]
fn the_lut_controls_only_move_a_look_that_was_already_chosen() {
    let neutral = settings(None, None);
    assert_eq!(action("lut", 1.0, &neutral), None);
    assert_eq!(action("lut-strength", 50.0, &neutral), None);

    let referenced = Settings {
        lut: Some(leyline_sdk::Lut {
            enabled: true,
            path: "Profiles/LUT/look.cube".to_owned(),
            checksum: format!("blake3:{}", "a".repeat(64)),
            strength: 100,
        }),
        ..neutral
    };
    let Some((Param::Lut, Value::Lut(Some(off)))) = action("lut", 0.0, &referenced) else {
        panic!("expected a LUT update");
    };
    assert!(!off.enabled);
    assert_eq!(off.path, "Profiles/LUT/look.cube");
    let Some((Param::Lut, Value::Lut(Some(dosed)))) = action("lut-strength", 40.4, &referenced)
    else {
        panic!("expected a LUT update");
    };
    assert_eq!((dosed.strength, dosed.enabled), (40, true));
}

#[test]
fn unknown_sliders_do_nothing() {
    let neutral = settings(None, None);
    assert_eq!(action("gamma", 1.0, &neutral), None);
    assert_eq!(action("crop-diagonal", 10.0, &neutral), None);
}

#[test]
fn crop_sliders_merge_and_stay_inside_the_frame() {
    let Some((Param::Crop, Value::Crop(Some(crop)))) =
        action("crop-left", 25.0, &settings(None, None))
    else {
        panic!("expected a crop update");
    };
    assert_eq!((crop.x, crop.width), (0.25, 0.75));

    let Some((_, Value::Crop(Some(crop)))) =
        action("crop-width", 90.0, &settings(None, Some(crop)))
    else {
        panic!("expected a crop update");
    };
    assert_eq!((crop.x, crop.width), (0.25, 0.75));

    let Some((_, Value::Crop(Some(crop)))) =
        action("crop-height", 50.0, &settings(None, Some(crop)))
    else {
        panic!("expected a crop update");
    };
    assert_eq!((crop.y, crop.height), (0.0, 0.5));
}

/// Unwraps the crop rectangle produced by [`drag_crop`].
fn dragged(update: Option<(Param, Value)>) -> Crop {
    match update {
        Some((Param::Crop, Value::Crop(Some(crop)))) => crop,
        other => panic!("expected a crop update, got {other:?}"),
    }
}

#[test]
fn drag_maps_through_the_letterbox() {
    // A 100×100 preview centered in a 200×100 viewport: 50 px bands.
    let crop = dragged(drag_crop(
        (75.0, 25.0),
        (125.0, 75.0),
        (200.0, 100.0),
        (100.0, 100.0),
        &None,
    ));
    assert_eq!(
        (crop.x, crop.y, crop.width, crop.height),
        (0.25, 0.25, 0.5, 0.5)
    );
}

#[test]
fn drag_direction_and_overshoot_are_normalized() {
    // Dragged up-left from outside the frame: clamps then reorders.
    let crop = dragged(drag_crop(
        (120.0, 80.0),
        (-10.0, 25.0),
        (100.0, 100.0),
        (100.0, 100.0),
        &None,
    ));
    assert_eq!(
        (crop.x, crop.y, crop.width, crop.height),
        (0.0, 0.25, 1.0, 0.55)
    );
}

#[test]
fn drag_composes_with_the_current_crop() {
    let current = Some(Crop {
        x: 0.5,
        y: 0.0,
        width: 0.5,
        height: 0.5,
    });
    let crop = dragged(drag_crop(
        (25.0, 25.0),
        (75.0, 75.0),
        (100.0, 100.0),
        (100.0, 100.0),
        &current,
    ));
    assert_eq!(
        (crop.x, crop.y, crop.width, crop.height),
        (0.625, 0.125, 0.25, 0.25)
    );
}

#[test]
fn tiny_and_degenerate_drags_are_ignored() {
    let view = (100.0, 100.0);
    assert_eq!(
        drag_crop((50.0, 50.0), (50.5, 90.0), view, view, &None),
        None
    );
    assert_eq!(
        drag_crop((50.0, 50.0), (90.0, 50.5), view, view, &None),
        None
    );
    assert_eq!(
        drag_crop((10.0, 10.0), (90.0, 90.0), view, (0.0, 100.0), &None),
        None
    );
    assert_eq!(
        drag_crop((10.0, 10.0), (90.0, 90.0), (0.0, 0.0), view, &None),
        None
    );
}

#[test]
fn full_frame_drag_changes_nothing() {
    let view = (100.0, 100.0);
    assert_eq!(
        drag_crop((-5.0, -5.0), (110.0, 110.0), view, view, &None),
        None
    );
}

#[test]
fn full_frame_and_reset_clear_the_crop() {
    let current = Crop {
        x: 0.2,
        y: 0.0,
        width: 0.5,
        height: 1.0,
    };
    assert_eq!(
        action("crop-reset", 0.0, &settings(None, Some(current.clone()))),
        Some((Param::Crop, Value::Crop(None)))
    );
    let Some((_, Value::Crop(Some(widened)))) =
        action("crop-left", 0.0, &settings(None, Some(current)))
    else {
        panic!("expected a crop update");
    };
    assert_eq!(
        action("crop-width", 100.0, &settings(None, Some(widened))),
        Some((Param::Crop, Value::Crop(None)))
    );
}

/// Unwraps the tone-curve points produced by [`curve_point`].
fn curved(update: Option<(Param, Value)>) -> Vec<CurvePoint> {
    match update {
        Some((Param::ToneCurve, Value::ToneCurve(curve))) => curve.points,
        other => panic!("expected a tone-curve update, got {other:?}"),
    }
}

#[test]
fn curve_click_seeds_the_identity_endpoints_on_an_empty_curve() {
    // ToneCurve requires >= 2 points, or 0 (`leyline-core::settings`):
    // the first point ever placed brings the fixed (0,0)/(1,1)
    // endpoints along with it.
    let points = curved(curve_point((0.5, 0.75), "master", &ToneCurve::default()));
    assert_eq!(
        points,
        vec![
            CurvePoint { x: 0.0, y: 0.0 },
            CurvePoint { x: 0.5, y: 0.75 },
            CurvePoint { x: 1.0, y: 1.0 },
        ]
    );
}

#[test]
fn curve_click_adds_a_sorted_point_to_an_existing_curve() {
    let current = vec![
        CurvePoint { x: 0.0, y: 0.0 },
        CurvePoint { x: 0.5, y: 0.75 },
        CurvePoint { x: 1.0, y: 1.0 },
    ];
    let points = curved(curve_point((0.2, 0.1), "master", &master(&current)));
    assert_eq!(
        points,
        vec![
            CurvePoint { x: 0.0, y: 0.0 },
            CurvePoint { x: 0.2, y: 0.1 },
            CurvePoint { x: 0.5, y: 0.75 },
            CurvePoint { x: 1.0, y: 1.0 },
        ]
    );
}

#[test]
fn clicking_near_a_point_removes_it() {
    let points = vec![
        CurvePoint { x: 0.0, y: 0.0 },
        CurvePoint { x: 0.5, y: 0.5 },
        CurvePoint { x: 1.0, y: 1.0 },
    ];
    assert_eq!(
        curved(curve_point((0.51, 0.49), "master", &master(&points))),
        vec![CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 1.0, y: 1.0 }]
    );
}

#[test]
fn removing_a_point_down_to_one_clears_the_whole_curve() {
    let points = vec![CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 1.0, y: 1.0 }];
    assert_eq!(
        curved(curve_point((0.01, 0.01), "master", &master(&points))),
        vec![]
    );
}

#[test]
fn clicking_near_another_points_x_is_ignored() {
    let points = vec![
        CurvePoint { x: 0.0, y: 0.0 },
        CurvePoint { x: 0.5, y: 0.5 },
        CurvePoint { x: 1.0, y: 1.0 },
    ];
    assert_eq!(curve_point((0.52, 0.9), "master", &master(&points)), None);
}

#[test]
fn a_first_click_too_close_to_an_endpoint_is_ignored() {
    assert_eq!(
        curve_point((0.02, 0.5), "master", &ToneCurve::default()),
        None
    );
    assert_eq!(
        curve_point((0.98, 0.5), "master", &ToneCurve::default()),
        None
    );
}

#[test]
fn reset_curve_clears_every_point() {
    assert_eq!(
        reset_curve("master", &ToneCurve::default()),
        (Param::ToneCurve, Value::ToneCurve(ToneCurve::default()))
    );
}

/// A `ToneCurve` carrying `points` as its master curve.
fn master(points: &[CurvePoint]) -> ToneCurve {
    ToneCurve {
        points: points.to_vec(),
        ..ToneCurve::default()
    }
}

/// ADR 0098 §4: editing one channel leaves the other three alone —
/// the reason the panel can offer four curves over one canvas.
#[test]
fn editing_one_curve_leaves_the_other_three_alone() {
    let start = ToneCurve {
        points: vec![CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 1.0, y: 1.0 }],
        blue: vec![CurvePoint { x: 0.0, y: 0.1 }, CurvePoint { x: 1.0, y: 0.9 }],
        ..ToneCurve::default()
    };
    let Some((_, Value::ToneCurve(after))) = curve_point((0.4, 0.6), "red", &start) else {
        panic!("a click on an empty channel curve must seed it");
    };
    assert_eq!(after.red.len(), 3, "the red curve gained its point");
    assert_eq!(after.points, start.points, "the master is untouched");
    assert_eq!(after.blue, start.blue, "the blue curve is untouched");
    assert!(after.green.is_empty());

    // Resetting one channel clears only that one.
    let (_, Value::ToneCurve(cleared)) = reset_curve("blue", &after) else {
        panic!()
    };
    assert!(cleared.blue.is_empty());
    assert_eq!(cleared.red, after.red, "the red curve survives");
    assert_eq!(cleared.points, start.points, "so does the master");
}

#[test]
fn curve_layout_maps_unit_points_into_canvas_pixels_with_y_flipped() {
    let points = vec![CurvePoint { x: 0.0, y: 1.0 }, CurvePoint { x: 1.0, y: 0.0 }];
    let (path, markers) = curve_layout(&points, 200.0);
    assert_eq!(markers, vec![(0.0, 0.0), (200.0, 200.0)]);
    assert_eq!(path, "M0 0 L200 200");
}

#[test]
fn histogram_layout_starts_and_ends_on_the_baseline() {
    let mut bins = [0u32; 256];
    bins[128] = 100;
    let path = histogram_layout(&bins, 100, 256.0, 90.0);
    assert!(path.starts_with("M0 90"));
    assert!(path.ends_with("L256 90 Z"));
}

#[test]
fn histogram_layout_the_tallest_bin_reaches_the_top_when_it_is_the_scale_max() {
    let mut bins = [0u32; 256];
    bins[0] = 400; // sqrt(400) = 20 = scale, so this bin fills the full height
    let path = histogram_layout(&bins, 400, 256.0, 90.0);
    assert!(
        path.contains("L0 0 "),
        "expected bin 0 to reach the top: {path}"
    );
}

#[test]
fn histogram_layout_handles_an_all_empty_scale_without_dividing_by_zero() {
    let bins = [0u32; 256];
    // scale_max of 0 (nothing rendered yet) must not divide by zero or
    // produce NaN/negative coordinates.
    let path = histogram_layout(&bins, 0, 256.0, 90.0);
    assert!(!path.contains("NaN"));
    assert!(path.starts_with("M0 90"));
    assert!(path.ends_with("L256 90 Z"));
}

#[test]
fn curve_layout_of_the_identity_curve_is_empty() {
    let (path, markers) = curve_layout(&[], 200.0);
    assert_eq!((path.as_str(), markers.as_slice()), ("", [].as_slice()));
}

/// Unwraps the spot list produced by [`place_spot`]/[`undo_last_spot`].
fn spots(update: Option<(Param, Value)>) -> Vec<SpotRemoval> {
    match update {
        Some((Param::SpotRemoval, Value::SpotRemoval(spots))) => spots,
        other => panic!("expected a spot-removal update, got {other:?}"),
    }
}

#[test]
fn place_spot_maps_both_clicks_through_the_letterbox() {
    // Same 100x100 preview centered in a 200x100 viewport as
    // `drag_maps_through_the_letterbox`: 50 px side bands.
    let placed = spots(place_spot(
        (75.0, 25.0),
        (125.0, 75.0),
        (200.0, 100.0),
        (100.0, 100.0),
        (0.05, 0.5, 1.0),
        &[],
    ));
    assert_eq!(
        placed,
        vec![SpotRemoval {
            source: Point { x: 0.25, y: 0.25 },
            target: Point { x: 0.75, y: 0.75 },
            radius: 0.05,
            feather: 0.5,
            opacity: 1.0,
        }]
    );
}

#[test]
fn place_spot_rejects_a_non_positive_radius() {
    let view = (100.0, 100.0);
    assert_eq!(
        place_spot((10.0, 10.0), (20.0, 20.0), view, view, (0.0, 0.5, 1.0), &[]),
        None
    );
}

#[test]
fn undo_last_spot_pops_the_most_recent_one() {
    let one = SpotRemoval {
        source: Point { x: 0.1, y: 0.1 },
        target: Point { x: 0.2, y: 0.2 },
        radius: 0.05,
        feather: 0.5,
        opacity: 1.0,
    };
    let two = SpotRemoval { radius: 0.1, ..one };
    assert_eq!(
        undo_last_spot(&[one, two]),
        Some((Param::SpotRemoval, Value::SpotRemoval(vec![one])))
    );
    assert_eq!(undo_last_spot(&[]), None);
}

#[test]
fn reset_spots_clears_every_spot() {
    assert_eq!(
        reset_spots(),
        (Param::SpotRemoval, Value::SpotRemoval(Vec::new()))
    );
}

#[test]
fn color_grading_balance_and_blending_merge_into_the_current_grading() {
    let current = Settings {
        color_grading: ColorGrading {
            blending: 40,
            ..ColorGrading::default()
        },
        ..Settings::default()
    };
    assert_eq!(
        action("color-grading-balance", 25.0, &current),
        Some((
            Param::ColorGrading,
            Value::ColorGrading(ColorGrading {
                balance: 25,
                blending: 40,
                ..ColorGrading::default()
            })
        ))
    );
}

#[test]
fn hsl_band_action_merges_one_field_and_leaves_its_siblings() {
    let mut current = Settings::default();
    current.hsl[2] = HslBand {
        hue: 10,
        saturation: 20,
        luminance: 30,
    };
    assert_eq!(
        hsl_band_action(2, "luminance", -15.0, &current),
        Some((
            Param::HslBand(2),
            Value::HslBand(HslBand {
                hue: 10,
                saturation: 20,
                luminance: -15,
            })
        ))
    );
}

#[test]
fn hsl_band_action_rejects_an_out_of_range_index_or_unknown_field() {
    let current = Settings::default();
    assert_eq!(hsl_band_action(8, "hue", 10.0, &current), None);
    assert_eq!(hsl_band_action(0, "bogus", 10.0, &current), None);
}

#[test]
fn color_grading_zone_action_merges_one_field_and_leaves_its_siblings() {
    let mut current = Settings::default();
    current.color_grading.midtones = leyline_sdk::ColorGradingZone {
        hue: 200,
        saturation: 50,
        luminance: 0,
    };
    assert_eq!(
        color_grading_zone_action("midtones", "hue", 90.0, &current),
        Some((
            Param::ColorGrading,
            Value::ColorGrading(ColorGrading {
                midtones: leyline_sdk::ColorGradingZone {
                    hue: 90,
                    saturation: 50,
                    luminance: 0,
                },
                ..ColorGrading::default()
            })
        ))
    );
}

#[test]
fn color_grading_zone_action_rejects_an_unknown_zone_or_field() {
    let current = Settings::default();
    assert_eq!(
        color_grading_zone_action("bogus", "hue", 10.0, &current),
        None
    );
    assert_eq!(
        color_grading_zone_action("shadows", "bogus", 10.0, &current),
        None
    );
}

/// ADR 0092: the scan reports both ends whether or not it paints, the
/// painting only touches the asked-for end, and a clean image reports
/// nothing and stays untouched.
#[test]
fn clipping_is_scanned_always_and_painted_on_request() {
    // One blown pixel, one crushed, one clean.
    let source = [255u8, 200, 100, 0, 0, 0, 128, 128, 128];

    let mut data = source;
    assert_eq!(paint_clipping(&mut data, false, false), (true, true));
    assert_eq!(data, source, "scan alone must not paint");

    let mut data = source;
    assert_eq!(paint_clipping(&mut data, true, false), (true, true));
    assert_eq!(&data[0..3], &[255, 59, 48], "blown pixel painted red");
    assert_eq!(&data[3..6], &[0, 0, 0], "shadows untouched when not asked");

    let mut data = source;
    paint_clipping(&mut data, false, true);
    assert_eq!(&data[3..6], &[10, 132, 255], "crushed pixel painted blue");
    assert_eq!(&data[0..3], &[255, 200, 100], "highlights untouched");

    // All-zero on one channel only is not a crushed shadow, and 254 is
    // not a blown highlight.
    let mut clean = [254u8, 0, 128];
    assert_eq!(paint_clipping(&mut clean, true, true), (false, false));
    assert_eq!(clean, [254, 0, 128]);
}

/// ADR 0103: one click, one disk, appended to what is there — a face
/// has two eyes, so the second click must not replace the first.
#[test]
fn a_red_eye_click_appends_a_disk() {
    let view = (100.0, 100.0);
    let image = (100.0, 100.0);
    let Some((Param::RedEye, Value::RedEye(first))) =
        place_red_eye((30.0, 40.0), view, image, (0.02, 0.5, 0.6), &[])
    else {
        panic!("a click must place a disk");
    };
    assert_eq!(first.len(), 1);
    assert!((first[0].center.x - 0.3).abs() < 1e-9);
    assert!((first[0].center.y - 0.4).abs() < 1e-9);

    let Some((_, Value::RedEye(both))) =
        place_red_eye((70.0, 40.0), view, image, (0.02, 0.5, 0.6), &first)
    else {
        panic!()
    };
    assert_eq!(both.len(), 2, "the first eye must survive the second");
    assert!((both[1].center.x - 0.7).abs() < 1e-9);

    // A degenerate radius proposes nothing, matching `validate`.
    assert!(place_red_eye((50.0, 50.0), view, image, (0.0, 0.5, 0.6), &[]).is_none());
}
