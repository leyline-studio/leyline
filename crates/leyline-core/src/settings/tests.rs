use super::*;

/// The exact example of `docs/pipeline.md` §3.2.
const SPEC_EXAMPLE: &str = r#"{
        "schema": 1,
        "stages": { "gains": 1, "contrast": 1, "crop": 1 },

        "white_balance": { "temperature": 5400, "tint": 4 },
        "exposure": 0.35,
        "contrast": 12,
        "highlights": -40,
        "shadows": 25,
        "whites": 0,
        "blacks": -5,
        "vibrance": 18,
        "saturation": 0,

        "lens_correction": { "enabled": true, "profile": "auto" },
        "noise_reduction": { "luminance": 15, "color": 25 },
        "sharpening": { "amount": 40, "radius": 1.0 },

        "rotation": 0.0,
        "crop": { "x": 0.1, "y": 0.2, "width": 0.8, "height": 0.7 }
    }"#;

#[test]
fn parses_the_spec_example() {
    let s = Settings::parse(SPEC_EXAMPLE).unwrap();
    assert_eq!(s.schema, 1);
    assert_eq!(s.stages["gains"], 1);
    assert_eq!(s.stages.len(), 3);
    assert_eq!(
        s.white_balance,
        Some(WhiteBalance {
            temperature: 5400,
            tint: 4
        })
    );
    assert_eq!(s.exposure, 0.35);
    assert_eq!(s.contrast, 12);
    assert_eq!(s.highlights, -40);
    assert_eq!(s.noise_reduction.color, 25);
    assert_eq!(s.sharpening.amount, 40);
    assert_eq!(
        s.crop,
        Some(Crop {
            x: 0.1,
            y: 0.2,
            width: 0.8,
            height: 0.7
        })
    );
    assert!(s.extra.is_empty());
    s.validate().unwrap();
}

#[test]
fn round_trips_losslessly() {
    let parsed = Settings::parse(SPEC_EXAMPLE).unwrap();
    let reparsed = Settings::parse(&parsed.to_json()).unwrap();
    assert_eq!(parsed, reparsed);
}

#[test]
fn omitted_values_are_neutral() {
    let s = Settings::parse(r#"{ "schema": 1, "stages": { "gains": 1 } }"#).unwrap();
    assert_eq!(
        s,
        Settings {
            schema: 1,
            stages: StageVersions::from([("gains".to_owned(), 1)]),
            ..Settings::default()
        }
    );
    assert_eq!(Settings::parse("{}").unwrap(), Settings::default());
}

#[test]
fn a_document_still_carrying_process_is_refused() {
    // Not the same case as an unknown field from a newer schema: this
    // one is a *removed* field, so the document predates ADR 0043 and
    // rendering it would silently give it another engine's pixels.
    let s = Settings::parse(r#"{ "schema": 1, "process": 8, "exposure": 0.5 }"#).unwrap();
    assert!(matches!(
        s.validate(),
        Err(LeylineError::InvalidSettings(_))
    ));
}

#[test]
fn preserves_unknown_fields_verbatim() {
    // A placeholder name for a field this engine doesn't know yet —
    // picked to stay clear of any real `Settings` field, past or
    // future (`clarity` used to fill this role until ADR 0033 made it
    // a real one).
    let json = r#"{ "schema": 2, "vignette_style": 30, "exposure": 1.5 }"#;
    let s = Settings::parse(json).unwrap();
    assert_eq!(s.schema, 2);
    assert_eq!(s.extra.get("vignette_style"), Some(&serde_json::json!(30)));

    let round_tripped: serde_json::Value = serde_json::from_str(&s.to_json()).unwrap();
    assert_eq!(round_tripped["vignette_style"], serde_json::json!(30));
    assert_eq!(round_tripped["exposure"], serde_json::json!(1.5));
}

#[test]
fn rejects_out_of_range_values() {
    let mut s = Settings {
        contrast: 150,
        ..Settings::default()
    };
    assert!(matches!(
        s.validate(),
        Err(LeylineError::InvalidSettings(_))
    ));

    s.contrast = 0;
    s.crop = Some(Crop {
        x: 0.5,
        y: 0.0,
        width: 0.8,
        height: 1.0,
    });
    assert!(matches!(
        s.validate(),
        Err(LeylineError::InvalidSettings(_))
    ));
}

#[test]
fn absent_camera_profile_validates() {
    Settings::default().validate().unwrap();
}

#[test]
fn camera_profile_rejects_an_empty_path() {
    let s = Settings {
        camera_profile: Some(CameraProfile {
            enabled: true,
            path: "  ".to_owned(),
            checksum: format!("blake3:{}", "a".repeat(64)),
        }),
        ..Settings::default()
    };
    assert!(matches!(
        s.validate(),
        Err(LeylineError::InvalidSettings(_))
    ));
}

#[test]
fn camera_profile_rejects_a_path_that_escapes_the_library() {
    // `settings_json` is user-editable and the path is joined onto the
    // library root: traversal and absolute paths would turn a revision
    // into an arbitrary-file read (`validate_library_relative_path`).
    for path in [
        "../outside.dcp",
        "Profiles/../../outside.dcp",
        "/tmp/x.dcp",
        "/etc/passwd",
        "C:\\x.dcp",
        "C:/x.dcp",
        "Profiles\\Camera\\mine.dcp",
        "./mine.dcp",
    ] {
        let s = Settings {
            camera_profile: Some(CameraProfile {
                enabled: true,
                path: path.to_owned(),
                checksum: format!("blake3:{}", "a".repeat(64)),
            }),
            ..Settings::default()
        };
        assert!(
            matches!(s.validate(), Err(LeylineError::InvalidSettings(_))),
            "{path:?} should be rejected"
        );
    }
}

#[test]
fn camera_profile_rejects_a_malformed_checksum() {
    for checksum in ["", "not-blake3:abcd", "blake3:tooshort", "blake3:zz"] {
        let s = Settings {
            camera_profile: Some(CameraProfile {
                enabled: true,
                path: "Profiles/Camera/mine.dcp".to_owned(),
                checksum: checksum.to_owned(),
            }),
            ..Settings::default()
        };
        assert!(
            matches!(s.validate(), Err(LeylineError::InvalidSettings(_))),
            "{checksum:?} should be rejected"
        );
    }
}

#[test]
fn camera_profile_accepts_a_well_formed_reference() {
    let s = Settings {
        camera_profile: Some(CameraProfile {
            enabled: true,
            path: "Profiles/Camera/mine.dcp".to_owned(),
            checksum: format!("blake3:{}", "a".repeat(64)),
        }),
        ..Settings::default()
    };
    s.validate().unwrap();
}

#[test]
fn clarity_texture_dehaze_out_of_range_are_rejected() {
    for field in ["clarity", "texture", "dehaze"] {
        let mut s = Settings::default();
        match field {
            "clarity" => s.clarity = 101,
            "texture" => s.texture = -101,
            _ => s.dehaze = 200,
        }
        assert!(
            matches!(s.validate(), Err(LeylineError::InvalidSettings(_))),
            "{field} should be rejected"
        );
    }
}

#[test]
fn neutral_hsl_and_color_grading_validate() {
    Settings::default().validate().unwrap();
}

#[test]
fn hsl_band_out_of_range_is_rejected() {
    let mut s = Settings::default();
    s.hsl[0].hue = 150;
    assert!(matches!(
        s.validate(),
        Err(LeylineError::InvalidSettings(_))
    ));
}

#[test]
fn color_grading_hue_must_be_a_full_degree_circle() {
    let mut s = Settings::default();
    s.color_grading.shadows.hue = 360;
    assert!(matches!(
        s.validate(),
        Err(LeylineError::InvalidSettings(_))
    ));
    s.color_grading.shadows.hue = 359;
    s.validate().unwrap();
}

#[test]
fn color_grading_balance_and_blending_are_range_checked() {
    let mut s = Settings::default();
    s.color_grading.blending = -1;
    assert!(matches!(
        s.validate(),
        Err(LeylineError::InvalidSettings(_))
    ));
    s.color_grading.blending = 0;
    s.color_grading.balance = 200;
    assert!(matches!(
        s.validate(),
        Err(LeylineError::InvalidSettings(_))
    ));
}

#[test]
fn tone_curve_accepts_empty_or_well_formed_points() {
    let mut s = Settings::default();
    s.validate().unwrap();

    s.tone_curve = ToneCurve {
        points: vec![
            CurvePoint { x: 0.0, y: 0.0 },
            CurvePoint { x: 0.5, y: 0.6 },
            CurvePoint { x: 1.0, y: 1.0 },
        ],
        ..ToneCurve::default()
    };
    s.validate().unwrap();
}

#[test]
fn tone_curve_rejects_a_single_point() {
    let s = Settings {
        tone_curve: ToneCurve {
            points: vec![CurvePoint { x: 0.5, y: 0.5 }],
            ..ToneCurve::default()
        },
        ..Settings::default()
    };
    assert!(matches!(
        s.validate(),
        Err(LeylineError::InvalidSettings(_))
    ));
}

#[test]
fn tone_curve_rejects_out_of_range_coordinates() {
    let s = Settings {
        tone_curve: ToneCurve {
            points: vec![CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 1.5, y: 1.0 }],
            ..ToneCurve::default()
        },
        ..Settings::default()
    };
    assert!(matches!(
        s.validate(),
        Err(LeylineError::InvalidSettings(_))
    ));
}

#[test]
fn tone_curve_rejects_non_increasing_x() {
    let s = Settings {
        tone_curve: ToneCurve {
            points: vec![CurvePoint { x: 0.5, y: 0.0 }, CurvePoint { x: 0.5, y: 1.0 }],
            ..ToneCurve::default()
        },
        ..Settings::default()
    };
    assert!(matches!(
        s.validate(),
        Err(LeylineError::InvalidSettings(_))
    ));
}

#[test]
fn spot_removal_accepts_a_well_formed_spot() {
    let s = Settings {
        spot_removal: vec![SpotRemoval {
            target: Point { x: 0.62, y: 0.31 },
            source: Point { x: 0.55, y: 0.29 },
            radius: 0.03,
            feather: 0.4,
            opacity: 1.0,
        }],
        ..Settings::default()
    };
    s.validate().unwrap();
}

#[test]
fn spot_removal_rejects_out_of_range_points() {
    let s = Settings {
        spot_removal: vec![SpotRemoval {
            target: Point { x: 1.5, y: 0.31 },
            source: Point { x: 0.55, y: 0.29 },
            radius: 0.03,
            feather: 0.4,
            opacity: 1.0,
        }],
        ..Settings::default()
    };
    assert!(matches!(
        s.validate(),
        Err(LeylineError::InvalidSettings(_))
    ));
}

#[test]
fn spot_removal_rejects_non_positive_radius() {
    let s = Settings {
        spot_removal: vec![SpotRemoval {
            target: Point { x: 0.5, y: 0.5 },
            source: Point { x: 0.4, y: 0.4 },
            radius: 0.0,
            feather: 0.4,
            opacity: 1.0,
        }],
        ..Settings::default()
    };
    assert!(matches!(
        s.validate(),
        Err(LeylineError::InvalidSettings(_))
    ));
}

#[test]
fn spot_removal_rejects_out_of_range_feather_and_opacity() {
    let base = SpotRemoval {
        target: Point { x: 0.5, y: 0.5 },
        source: Point { x: 0.4, y: 0.4 },
        radius: 0.03,
        feather: 0.4,
        opacity: 1.0,
    };
    let mut s = Settings {
        spot_removal: vec![SpotRemoval {
            feather: 1.5,
            ..base
        }],
        ..Settings::default()
    };
    assert!(matches!(
        s.validate(),
        Err(LeylineError::InvalidSettings(_))
    ));

    s.spot_removal = vec![SpotRemoval {
        opacity: -0.1,
        ..base
    }];
    assert!(matches!(
        s.validate(),
        Err(LeylineError::InvalidSettings(_))
    ));
}

#[test]
fn spot_removal_round_trips_and_is_omitted_when_empty() {
    let s = Settings {
        spot_removal: vec![SpotRemoval {
            target: Point { x: 0.62, y: 0.31 },
            source: Point { x: 0.55, y: 0.29 },
            radius: 0.03,
            feather: 0.4,
            opacity: 1.0,
        }],
        ..Settings::default()
    };
    let reparsed = Settings::parse(&s.to_json()).unwrap();
    assert_eq!(s, reparsed);

    let neutral = Settings::default();
    let value: serde_json::Value = serde_json::from_str(&neutral.to_json()).unwrap();
    assert!(value.get("spot_removal").is_none());
}

// -------------------------------------------------------------------
// Local adjustments (ADR 0029)
// -------------------------------------------------------------------

fn radial_adjustment() -> LocalAdjustment {
    LocalAdjustment {
        mask: Mask::Radial {
            cx: 0.5,
            cy: 0.42,
            rx: 0.30,
            ry: 0.22,
            angle: 0.0,
            feather: 0.40,
            inverted: false,
        },
        range: None,
        opacity: 1.0,
        adjustments: LocalAdjustmentValues {
            exposure: Some(0.6),
            contrast: Some(15),
            highlights: Some(-20),
            ..LocalAdjustmentValues::default()
        },
    }
}

#[test]
fn local_adjustments_round_trip_and_are_omitted_when_empty() {
    let s = Settings {
        local_adjustments: vec![radial_adjustment()],
        ..Settings::default()
    };
    s.validate().unwrap();
    let reparsed = Settings::parse(&s.to_json()).unwrap();
    assert_eq!(s, reparsed);

    let neutral = Settings::default();
    let value: serde_json::Value = serde_json::from_str(&neutral.to_json()).unwrap();
    assert!(value.get("local_adjustments").is_none());
}

#[test]
fn parses_the_adr_0029_example_json() {
    let json = r#"{
            "schema": 1,
            "stages": { "gains": 1, "vibrance": 1, "local_adjustments": 1 },
            "exposure": 0.35,
            "vibrance": 18,
            "local_adjustments": [
                {
                    "mask": {
                        "type": "radial",
                        "cx": 0.5, "cy": 0.42,
                        "rx": 0.30, "ry": 0.22,
                        "angle": 0.0,
                        "feather": 0.40,
                        "inverted": false
                    },
                    "opacity": 1.0,
                    "adjustments": { "exposure": 0.6, "contrast": 15, "highlights": -20 }
                },
                {
                    "mask": {
                        "type": "gradient",
                        "x0": 0.5, "y0": 0.0,
                        "x1": 0.5, "y1": 0.35
                    },
                    "opacity": 0.8,
                    "adjustments": { "exposure": -0.8, "whites": -10, "temperature": 5200, "tint": 6 }
                },
                {
                    "mask": {
                        "type": "brush",
                        "strokes": [
                            { "x": 0.20, "y": 0.60, "radius": 0.04, "flow": 1.0, "hardness": 0.5 },
                            { "x": 0.23, "y": 0.61, "radius": 0.04, "flow": 1.0, "hardness": 0.5 },
                            { "x": 0.26, "y": 0.62, "radius": 0.04, "flow": 1.0, "hardness": 0.5 }
                        ]
                    },
                    "opacity": 1.0,
                    "adjustments": { "saturation": -30, "shadows": 20 }
                }
            ]
        }"#;
    let s = Settings::parse(json).unwrap();
    assert_eq!(s.local_adjustments.len(), 3);
    s.validate().unwrap();
    match &s.local_adjustments[1].mask {
        Mask::Gradient { x0, y0, x1, y1 } => {
            assert_eq!((*x0, *y0, *x1, *y1), (0.5, 0.0, 0.5, 0.35));
        }
        other => panic!("expected a gradient mask, got {other:?}"),
    }
    assert_eq!(s.local_adjustments[1].adjustments.temperature, Some(5200));
    match &s.local_adjustments[2].mask {
        Mask::Brush { strokes } => assert_eq!(strokes.len(), 3),
        other => panic!("expected a brush mask, got {other:?}"),
    }
}

#[test]
fn local_adjustments_rejects_out_of_range_opacity() {
    let mut adjustment = radial_adjustment();
    adjustment.opacity = 1.5;
    let s = Settings {
        local_adjustments: vec![adjustment],
        ..Settings::default()
    };
    assert!(matches!(
        s.validate(),
        Err(LeylineError::InvalidSettings(_))
    ));
}

// -------------------------------------------------------------------
// Range masks (ADR 0048)
// -------------------------------------------------------------------

fn ranged_adjustment() -> LocalAdjustment {
    LocalAdjustment {
        range: Some(RangeMask {
            luminance: Some(LuminanceRange::default()),
            color: Some(ColorRange::default()),
        }),
        ..radial_adjustment()
    }
}

/// ADR 0050 §5, the same capability rule one layer lower: the decoder
/// configuration a revision pins decides whether the mode can be
/// expressed at all.
#[test]
fn a_highlight_mode_on_a_revision_pinned_at_input_v1_is_refused() {
    let pinned_v1 = Settings {
        highlight_reconstruction: HighlightReconstruction::Rebuild,
        stages: StageVersions::from([("input".to_owned(), 1)]),
        ..Settings::default()
    };
    let error = pinned_v1.validate().unwrap_err().to_string();
    assert!(error.contains("input version 2"), "{error}");
    assert!(error.contains("reprocess"), "{error}");

    // Clipping is the neutral value, so it is expressible everywhere.
    let clipped = Settings {
        highlight_reconstruction: HighlightReconstruction::Clip,
        ..pinned_v1.clone()
    };
    assert!(clipped.validate().is_ok());

    // And so is any mode on a revision pinned at the version that reads
    // it, or at none at all (a fresh revision).
    let pinned_v2 = Settings {
        stages: StageVersions::from([("input".to_owned(), 2)]),
        ..pinned_v1.clone()
    };
    assert!(pinned_v2.validate().is_ok());
    let unpinned = Settings {
        stages: StageVersions::default(),
        ..pinned_v1
    };
    assert!(unpinned.validate().is_ok());
}

/// The neutral mode leaves no trace in a stored document, and a stored
/// one round-trips (`docs/pipeline.md` §3.4).
#[test]
fn the_highlight_mode_is_omitted_when_neutral_and_round_trips_otherwise() {
    let neutral = Settings::default();
    let json: serde_json::Value = serde_json::from_str(&neutral.to_json()).unwrap();
    assert!(json.get("highlight_reconstruction").is_none());

    let asked = Settings {
        highlight_reconstruction: HighlightReconstruction::Blend,
        ..Settings::default()
    };
    let json = asked.to_json();
    assert!(
        json.contains("\"highlight_reconstruction\":\"blend\""),
        "{json}"
    );
    assert_eq!(
        Settings::parse(&json).unwrap().highlight_reconstruction,
        HighlightReconstruction::Blend
    );
}

/// The decision of ADR 0048 §5: a revision pinned at `local_adjustments`
/// v1 cannot express a range, and the pinning rule keeps it at v1 — so the
/// only honest outcome is a refusal, never a slider that does nothing.
#[test]
fn a_range_on_a_revision_pinned_at_v1_is_refused() {
    let s = Settings {
        local_adjustments: vec![ranged_adjustment()],
        stages: StageVersions::from([("local_adjustments".to_owned(), 1)]),
        ..Settings::default()
    };
    let message = match s.validate() {
        Err(LeylineError::InvalidSettings(message)) => message,
        other => panic!("expected a refusal, got {other:?}"),
    };
    // The message has to name the remedy, since the user cannot guess it.
    assert!(message.contains("reprocess"), "{message}");
}

#[test]
fn a_range_is_accepted_at_v2_and_on_unpinned_settings() {
    let at_v2 = Settings {
        local_adjustments: vec![ranged_adjustment()],
        stages: StageVersions::from([("local_adjustments".to_owned(), 2)]),
        ..Settings::default()
    };
    at_v2.validate().unwrap();

    // Built in memory, nothing pinned yet: the engine will pin the
    // current version when the revision is written.
    let unpinned = Settings {
        local_adjustments: vec![ranged_adjustment()],
        ..Settings::default()
    };
    unpinned.validate().unwrap();
}

fn neighbourhood_adjustment() -> LocalAdjustment {
    LocalAdjustment {
        adjustments: LocalAdjustmentValues {
            texture: Some(-40),
            ..radial_adjustment().adjustments
        },
        ..radial_adjustment()
    }
}

/// ADR 0108 §5: the capability rule for the fourth time. Versions 1 to 3
/// have no code for the five neighbourhood operators, so a revision
/// pinned at any of them is refused rather than rendered with a slider
/// that silently does nothing.
#[test]
fn a_neighbourhood_value_below_local_adjustments_v4_is_refused() {
    for pinned in [1, 2, 3] {
        let s = Settings {
            local_adjustments: vec![neighbourhood_adjustment()],
            stages: StageVersions::from([("local_adjustments".to_owned(), pinned)]),
            ..Settings::default()
        };
        let message = match s.validate() {
            Err(LeylineError::InvalidSettings(message)) => message,
            other => panic!("expected a refusal at v{pinned}, got {other:?}"),
        };
        // The message names the remedy, as ADR 0048 §5's does: the user
        // cannot guess that reprocessing is what unblocks them.
        assert!(message.contains("reprocess"), "v{pinned}: {message}");
        assert!(message.contains("version 4"), "v{pinned}: {message}");
    }
}

/// The sixth application of the same rule (ADR 0116 §4): the defringe
/// pair needs `local_adjustments` v5, and an earlier pin is refused by
/// name rather than rendered without it.
#[test]
fn a_local_defringe_on_a_revision_pinned_below_v5_is_refused() {
    let with_defringe = LocalAdjustment {
        mask: Mask::Everything,
        range: None,
        opacity: 1.0,
        adjustments: LocalAdjustmentValues {
            defringe_purple: Some(60),
            ..LocalAdjustmentValues::default()
        },
    };
    for pinned in [1u16, 2, 3, 4] {
        let refused = Settings {
            local_adjustments: vec![with_defringe.clone()],
            stages: StageVersions::from([("local_adjustments".to_owned(), pinned)]),
            ..Settings::default()
        };
        let message = match refused.validate() {
            Err(LeylineError::InvalidSettings(message)) => message,
            other => panic!("v{pinned}: expected a refusal, got {other:?}"),
        };
        assert!(message.contains("version 5"), "v{pinned}: {message}");
        assert!(message.contains("reprocess"), "v{pinned}: {message}");
    }

    // Accepted at v5, and on settings nothing has pinned yet.
    Settings {
        local_adjustments: vec![with_defringe.clone()],
        stages: StageVersions::from([("local_adjustments".to_owned(), 5)]),
        ..Settings::default()
    }
    .validate()
    .unwrap();
    Settings {
        local_adjustments: vec![with_defringe],
        ..Settings::default()
    }
    .validate()
    .unwrap();

    // And an adjustment that never asks for it stays valid at v4 — the
    // rule refuses an inexpressible setting, not an old version.
    Settings {
        local_adjustments: vec![LocalAdjustment {
            mask: Mask::Everything,
            range: None,
            opacity: 1.0,
            adjustments: LocalAdjustmentValues {
                clarity: Some(20),
                ..LocalAdjustmentValues::default()
            },
        }],
        stages: StageVersions::from([("local_adjustments".to_owned(), 4)]),
        ..Settings::default()
    }
    .validate()
    .unwrap();
}

#[test]
fn a_neighbourhood_value_is_accepted_at_v4_and_on_unpinned_settings() {
    let at_v4 = Settings {
        local_adjustments: vec![neighbourhood_adjustment()],
        stages: StageVersions::from([("local_adjustments".to_owned(), 4)]),
        ..Settings::default()
    };
    at_v4.validate().unwrap();

    let unpinned = Settings {
        local_adjustments: vec![neighbourhood_adjustment()],
        ..Settings::default()
    };
    unpinned.validate().unwrap();
}

/// A revision pinned at an older version that never asks for one of the
/// five stays valid — the refusal is about the *setting*, not about the
/// pinning. The mirror of `masking_zero_...` for ADR 0096.
#[test]
fn an_old_pin_without_any_neighbourhood_value_stays_valid() {
    for pinned in [1, 2, 3] {
        let s = Settings {
            local_adjustments: vec![radial_adjustment()],
            stages: StageVersions::from([("local_adjustments".to_owned(), pinned)]),
            ..Settings::default()
        };
        s.validate().unwrap();
    }
}

/// Noise reduction has no meaningful negative, exactly as its global
/// sliders have none — while texture, clarity and sharpness do, and the
/// negative half is the point (ADR 0108 §1).
#[test]
fn local_noise_refuses_a_negative_while_texture_accepts_one() {
    let with = |values: LocalAdjustmentValues| Settings {
        local_adjustments: vec![LocalAdjustment {
            adjustments: values,
            ..radial_adjustment()
        }],
        ..Settings::default()
    };
    assert!(
        with(LocalAdjustmentValues {
            noise_luminance: Some(-1),
            ..LocalAdjustmentValues::default()
        })
        .validate()
        .is_err()
    );
    assert!(
        with(LocalAdjustmentValues {
            noise_color: Some(-1),
            ..LocalAdjustmentValues::default()
        })
        .validate()
        .is_err()
    );
    for values in [
        LocalAdjustmentValues {
            texture: Some(-100),
            ..LocalAdjustmentValues::default()
        },
        LocalAdjustmentValues {
            clarity: Some(-100),
            ..LocalAdjustmentValues::default()
        },
        LocalAdjustmentValues {
            sharpness: Some(-100),
            ..LocalAdjustmentValues::default()
        },
    ] {
        with(values).validate().unwrap();
    }
}

/// The five are omitted from a stored document when neutral, like every
/// other optional local value (`docs/pipeline.md` §3.4).
#[test]
fn the_five_neighbourhood_values_round_trip_and_are_omitted_when_absent() {
    let plain = Settings {
        local_adjustments: vec![radial_adjustment()],
        ..Settings::default()
    };
    // Read the entry's own `adjustments` object rather than the whole
    // document: `clarity` and `texture` are *also* global sliders, always
    // serialized, so a substring search over the document proves nothing.
    let json: serde_json::Value = serde_json::from_str(&plain.to_json()).unwrap();
    let stored = &json["local_adjustments"][0]["adjustments"];
    for name in [
        "clarity",
        "texture",
        "sharpness",
        "noise_luminance",
        "noise_color",
    ] {
        assert!(
            stored.get(name).is_none(),
            "{name} leaked into a neutral entry: {stored}"
        );
    }

    let set = Settings {
        local_adjustments: vec![LocalAdjustment {
            adjustments: LocalAdjustmentValues {
                clarity: Some(30),
                texture: Some(-45),
                sharpness: Some(20),
                noise_luminance: Some(15),
                noise_color: Some(10),
                ..LocalAdjustmentValues::default()
            },
            ..radial_adjustment()
        }],
        ..Settings::default()
    };
    let parsed = Settings::parse(&set.to_json()).unwrap();
    assert_eq!(parsed.local_adjustments, set.local_adjustments);
}

/// ADR 0096 §3: the capability rule, applied to sharpening's edge mask.
#[test]
fn masking_on_a_revision_pinned_at_sharpen_v1_is_refused() {
    let masked = Sharpening {
        amount: 50,
        radius: 1.0,
        masking: 40,
    };
    let refused = Settings {
        sharpening: masked.clone(),
        stages: StageVersions::from([("sharpen".to_owned(), 1)]),
        ..Settings::default()
    };
    let message = match refused.validate() {
        Err(LeylineError::InvalidSettings(message)) => message,
        other => panic!("expected a refusal, got {other:?}"),
    };
    assert!(message.contains("sharpen version 2"), "{message}");
    // The message names the remedy, since the user cannot guess it.
    assert!(message.contains("reprocess"), "{message}");

    // Accepted at v2, and on settings nothing has pinned yet.
    Settings {
        sharpening: masked.clone(),
        stages: StageVersions::from([("sharpen".to_owned(), 2)]),
        ..Settings::default()
    }
    .validate()
    .unwrap();
    Settings {
        sharpening: masked,
        ..Settings::default()
    }
    .validate()
    .unwrap();

    // A revision pinned at v1 that never asks for masking stays valid:
    // the rule refuses an inexpressible setting, not an old version.
    Settings {
        sharpening: Sharpening {
            amount: 50,
            radius: 1.0,
            masking: 0,
        },
        stages: StageVersions::from([("sharpen".to_owned(), 1)]),
        ..Settings::default()
    }
    .validate()
    .unwrap();
}

/// The same rule for ADR 0118's coloured grain, and the same three
/// cases: refused at v1, accepted at v2 and unpinned, and a v1 revision
/// that never asks for colour left alone.
#[test]
fn coloured_grain_on_a_revision_pinned_at_grain_v1_is_refused() {
    let coloured = Grain {
        amount: 60,
        size: 25,
        roughness: 50,
        color: 40,
    };
    let message = match (Settings {
        grain: coloured,
        stages: StageVersions::from([("grain".to_owned(), 1)]),
        ..Settings::default()
    })
    .validate()
    {
        Err(LeylineError::InvalidSettings(message)) => message,
        other => panic!("expected a refusal, got {other:?}"),
    };
    assert!(message.contains("grain version 2"), "{message}");
    assert!(message.contains("reprocess"), "{message}");

    for stages in [
        StageVersions::from([("grain".to_owned(), 2)]),
        StageVersions::new(),
    ] {
        Settings {
            grain: coloured,
            stages,
            ..Settings::default()
        }
        .validate()
        .unwrap();
    }

    Settings {
        grain: Grain {
            color: 0,
            ..coloured
        },
        stages: StageVersions::from([("grain".to_owned(), 1)]),
        ..Settings::default()
    }
    .validate()
    .unwrap();
}

/// The same rule for ADR 0111's manual chromatic aberration, and the
/// same three cases: refused at v1, accepted at v2 and unpinned, and a
/// v1 revision that never asks for it left alone.
#[test]
fn manual_tca_on_a_revision_pinned_at_lens_v1_is_refused() {
    let measured = LensCorrection {
        enabled: false,
        tca_red: 0.12,
        ..LensCorrection::default()
    };
    let refused = Settings {
        lens_correction: measured.clone(),
        stages: StageVersions::from([("lens".to_owned(), 1)]),
        ..Settings::default()
    };
    let message = match refused.validate() {
        Err(LeylineError::InvalidSettings(message)) => message,
        other => panic!("expected a refusal, got {other:?}"),
    };
    assert!(message.contains("lens version 2"), "{message}");
    assert!(message.contains("reprocess"), "{message}");

    Settings {
        lens_correction: measured.clone(),
        stages: StageVersions::from([("lens".to_owned(), 2)]),
        ..Settings::default()
    }
    .validate()
    .unwrap();
    Settings {
        lens_correction: measured,
        ..Settings::default()
    }
    .validate()
    .unwrap();

    // A lens correction pinned at v1 that never asks for the manual map
    // stays valid — including with the Lensfun half switched on.
    Settings {
        lens_correction: LensCorrection {
            enabled: true,
            ..LensCorrection::default()
        },
        stages: StageVersions::from([("lens".to_owned(), 1)]),
        ..Settings::default()
    }
    .validate()
    .unwrap();
}

/// A percent of the radius, and one percent is already absurd for a real
/// lens — the range is refused by name rather than clamped in silence.
#[test]
fn a_manual_tca_beyond_one_percent_is_refused() {
    for (red, blue) in [(1.5, 0.0), (0.0, -2.0), (f64::NAN, 0.0)] {
        let settings = Settings {
            lens_correction: LensCorrection {
                tca_red: red,
                tca_blue: blue,
                ..LensCorrection::default()
            },
            ..Settings::default()
        };
        assert!(
            matches!(settings.validate(), Err(LeylineError::InvalidSettings(_))),
            "{red} / {blue} should be refused"
        );
    }
}

/// The two coefficients activate the correction on their own — the
/// property ADR 0111 §5 turns into the stage's `active` predicate.
#[test]
fn manual_tca_is_asked_for_without_enabling_the_profile_correction() {
    let off = LensCorrection::default();
    assert!(!off.has_manual_tca());
    assert!(
        LensCorrection {
            tca_blue: -0.05,
            ..LensCorrection::default()
        }
        .has_manual_tca()
    );
}

#[test]
fn a_range_rejects_an_inverted_luminance_band() {
    let mut adjustment = ranged_adjustment();
    adjustment.range = Some(RangeMask {
        luminance: Some(LuminanceRange {
            min: 0.8,
            max: 0.2,
            softness: 0.1,
        }),
        color: None,
    });
    let s = Settings {
        local_adjustments: vec![adjustment],
        ..Settings::default()
    };
    assert!(matches!(
        s.validate(),
        Err(LeylineError::InvalidSettings(_))
    ));
}

#[test]
fn a_range_rejects_a_hue_band_wider_than_the_circle() {
    for (width, softness) in [(0.0, 10.0), (181.0, 10.0), (30.0, 200.0)] {
        let mut adjustment = ranged_adjustment();
        adjustment.range = Some(RangeMask {
            luminance: None,
            color: Some(ColorRange {
                center: 210.0,
                width,
                softness,
            }),
        });
        let s = Settings {
            local_adjustments: vec![adjustment],
            ..Settings::default()
        };
        assert!(
            matches!(s.validate(), Err(LeylineError::InvalidSettings(_))),
            "width {width}, softness {softness} should be refused"
        );
    }
}

/// A neutral range is absent, not present-and-empty: an adjustment without
/// one must not gain a `range` key in `settings_json` (§3.4 compatibility).
#[test]
fn an_adjustment_without_a_range_serializes_without_the_field() {
    let s = Settings {
        local_adjustments: vec![radial_adjustment()],
        ..Settings::default()
    };
    let value: serde_json::Value = serde_json::from_str(&s.to_json()).unwrap();
    let entry = &value["local_adjustments"][0];
    assert!(entry.get("range").is_none(), "{entry}");
    // And it round-trips.
    assert_eq!(Settings::parse(&s.to_json()).unwrap(), s);
}

#[test]
fn radial_mask_rejects_non_positive_radii() {
    let mut adjustment = radial_adjustment();
    adjustment.mask = Mask::Radial {
        cx: 0.5,
        cy: 0.5,
        rx: 0.0,
        ry: 0.2,
        angle: 0.0,
        feather: 0.4,
        inverted: false,
    };
    let s = Settings {
        local_adjustments: vec![adjustment],
        ..Settings::default()
    };
    assert!(matches!(
        s.validate(),
        Err(LeylineError::InvalidSettings(_))
    ));
}

#[test]
fn gradient_mask_rejects_coincident_endpoints() {
    let mut adjustment = radial_adjustment();
    adjustment.mask = Mask::Gradient {
        x0: 0.5,
        y0: 0.5,
        x1: 0.5,
        y1: 0.5,
    };
    let s = Settings {
        local_adjustments: vec![adjustment],
        ..Settings::default()
    };
    assert!(matches!(
        s.validate(),
        Err(LeylineError::InvalidSettings(_))
    ));
}

#[test]
fn brush_mask_rejects_an_empty_stroke_list() {
    let mut adjustment = radial_adjustment();
    adjustment.mask = Mask::Brush { strokes: vec![] };
    let s = Settings {
        local_adjustments: vec![adjustment],
        ..Settings::default()
    };
    assert!(matches!(
        s.validate(),
        Err(LeylineError::InvalidSettings(_))
    ));
}

#[test]
fn brush_mask_rejects_a_non_positive_stroke_radius() {
    let mut adjustment = radial_adjustment();
    adjustment.mask = Mask::Brush {
        strokes: vec![BrushStroke {
            x: 0.2,
            y: 0.6,
            radius: 0.0,
            flow: 1.0,
            hardness: 0.5,
        }],
    };
    let s = Settings {
        local_adjustments: vec![adjustment],
        ..Settings::default()
    };
    assert!(matches!(
        s.validate(),
        Err(LeylineError::InvalidSettings(_))
    ));
}

#[test]
fn local_adjustments_rejects_out_of_range_slider_values() {
    let mut adjustment = radial_adjustment();
    adjustment.adjustments.saturation = Some(200);
    let s = Settings {
        local_adjustments: vec![adjustment],
        ..Settings::default()
    };
    assert!(matches!(
        s.validate(),
        Err(LeylineError::InvalidSettings(_))
    ));
}

#[test]
fn local_adjustments_rejects_zero_temperature() {
    let mut adjustment = radial_adjustment();
    adjustment.adjustments.temperature = Some(0);
    let s = Settings {
        local_adjustments: vec![adjustment],
        ..Settings::default()
    };
    assert!(matches!(
        s.validate(),
        Err(LeylineError::InvalidSettings(_))
    ));
}

#[test]
fn rejects_malformed_json() {
    assert!(matches!(
        Settings::parse("{ not json"),
        Err(LeylineError::InvalidSettings(_))
    ));
}

#[test]
fn preset_captures_only_the_requested_groups() {
    let s = Settings::parse(SPEC_EXAMPLE).unwrap();
    let preset = PresetSettings::capture(&s, &[SettingsGroup::Tone, SettingsGroup::Presence]);
    assert_eq!(preset.schema, 1);
    assert_eq!(preset.exposure, Some(0.35));
    assert_eq!(preset.contrast, Some(12));
    assert_eq!(preset.vibrance, Some(18));
    assert_eq!(preset.saturation, Some(0));
    // Not requested: absent, not neutral.
    assert_eq!(preset.white_balance, None);
    assert_eq!(preset.lens_correction, None);
    assert_eq!(preset.rotation, None);
    assert_eq!(preset.crop, None);
}

#[test]
fn preset_json_omits_fields_of_excluded_groups() {
    let s = Settings::parse(SPEC_EXAMPLE).unwrap();
    let preset = PresetSettings::capture(&s, &[SettingsGroup::WhiteBalance]);
    let json = preset.to_json();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(value.get("white_balance").is_some());
    assert!(value.get("exposure").is_none());
    assert!(value.get("crop").is_none());
}

#[test]
fn preset_round_trips_losslessly() {
    let s = Settings::parse(SPEC_EXAMPLE).unwrap();
    let preset = PresetSettings::capture(&s, &[SettingsGroup::Detail, SettingsGroup::Geometry]);
    let reparsed = PresetSettings::parse(&preset.to_json()).unwrap();
    assert_eq!(preset, reparsed);
}
