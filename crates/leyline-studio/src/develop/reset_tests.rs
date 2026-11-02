use super::*;
use leyline_sdk::{
    ColorGradingZone, Grain, HighlightReconstruction, Lut, Mask, Perspective, Vignette,
    WhiteBalance,
};

/// A revision with **every** group away from neutral: the fixture a
/// group reset has to be measured against, since the whole question is
/// what it leaves alone (ADR 0112 §4).
fn everything() -> Settings {
    let mut settings = Settings {
        white_balance: Some(WhiteBalance {
            temperature: 4200,
            tint: 12,
        }),
        moire: 30,
        levels: leyline_sdk::Levels {
            gamma: 1.2,
            ..leyline_sdk::Levels::default()
        },
        exposure: 0.8,
        contrast: 20,
        highlights: -30,
        shadows: 25,
        whites: 10,
        blacks: -10,
        texture: 15,
        clarity: 20,
        dehaze: 10,
        vibrance: 12,
        saturation: -8,
        monochrome: true,
        highlight_reconstruction: HighlightReconstruction::Rebuild,
        lens_correction: LensCorrection {
            enabled: true,
            tca_red: 0.05,
            ..LensCorrection::default()
        },
        defringe: leyline_sdk::Defringe {
            purple: 40,
            green: 20,
        },
        noise_reduction: NoiseReduction {
            luminance: 30,
            color: 40,
        },
        sharpening: Sharpening {
            amount: 60,
            radius: 1.5,
            masking: 20,
        },
        vignette: Vignette {
            amount: -40,
            ..Vignette::default()
        },
        grain: Grain {
            amount: 30,
            ..Grain::default()
        },
        tone_curve: ToneCurve {
            points: vec![
                CurvePoint { x: 0.0, y: 0.05 },
                CurvePoint { x: 1.0, y: 0.95 },
            ],
            ..ToneCurve::default()
        },
        color_grading: ColorGrading {
            shadows: ColorGradingZone {
                hue: 210,
                saturation: 20,
                luminance: 0,
            },
            ..ColorGrading::default()
        },
        red_eye: vec![RedEye {
            center: Point { x: 0.5, y: 0.5 },
            radius: 0.05,
            feather: 0.5,
            darken: 0.6,
        }],
        spot_removal: vec![SpotRemoval {
            target: Point { x: 0.2, y: 0.3 },
            source: Point { x: 0.4, y: 0.5 },
            radius: 0.04,
            feather: 0.5,
            opacity: 1.0,
        }],
        local_adjustments: vec![
            leyline_sdk::LocalAdjustment {
                mask: Mask::Everything,
                range: None,
                opacity: 1.0,
                adjustments: leyline_sdk::LocalAdjustmentValues {
                    exposure: Some(0.3),
                    ..Default::default()
                },
            },
            leyline_sdk::LocalAdjustment {
                mask: Mask::Everything,
                range: None,
                opacity: 1.0,
                adjustments: leyline_sdk::LocalAdjustmentValues {
                    contrast: Some(10),
                    ..Default::default()
                },
            },
        ],
        rotation: 3.5,
        perspective: Some(Perspective {
            vertical: 12,
            horizontal: -8,
        }),
        crop: Some(Crop {
            x: 0.1,
            y: 0.1,
            width: 0.8,
            height: 0.8,
        }),
        lut: Some(Lut {
            enabled: true,
            path: "Profiles/Lut/a.cube".to_owned(),
            checksum: format!("blake3:{}", "a".repeat(64)),
            strength: 80,
        }),
        camera_profile: Some(leyline_sdk::CameraProfile {
            enabled: true,
            path: "Profiles/Camera/a.dcp".to_owned(),
            checksum: format!("blake3:{}", "b".repeat(64)),
        }),
        reshape: vec![leyline_sdk::ReshapePoint {
            from: Point { x: 0.4, y: 0.4 },
            to: Point { x: 0.45, y: 0.42 },
            radius: 0.1,
            strength: 1.0,
        }],
        ..Settings::default()
    };
    settings.hsl[0].saturation = 30;
    settings.hsl[3].hue = -20;
    settings.output_rendering.highlight_rolloff = 20;
    settings
}

/// What a group's reset touches, as parameter names — the list *is* the
/// guarantee: a session only ever changes the parameters it is handed,
/// so a group that never names another group's parameter cannot reach
/// it (ADR 0112 §4).
fn touched(group: &str) -> Vec<String> {
    let mut names: Vec<String> = reset_group(group, &everything())
        .into_iter()
        .map(|(param, _)| format!("{param:?}"))
        .collect();
    names.sort();
    names
}

/// And what it puts there: the neutral value, read from the engine's own
/// neutral revision rather than from a literal in this file.
fn values(group: &str) -> Vec<Value> {
    reset_group(group, &everything())
        .into_iter()
        .map(|(_, value)| value)
        .collect()
}

#[test]
fn every_group_resets_its_own_fields_and_names_no_other() {
    let neutral = leyline_sdk::neutral_settings();

    assert_eq!(
        touched("detail"),
        ["Moire", "NoiseReduction", "Sharpening"],
        "detail owns the two pairs and the moiré beside them (ADR 0165 §3)"
    );
    assert_eq!(
        values("detail"),
        [
            Value::NoiseReduction(neutral.noise_reduction.clone()),
            Value::Sharpening(neutral.sharpening.clone()),
            Value::Int(neutral.moire),
        ]
    );
    assert_eq!(touched("effects"), ["Grain", "Vignette"]);
    // The lens group owns both aberration pairs (ADR 0113 §5).
    assert_eq!(touched("lens"), ["Defringe", "LensCorrection"]);
    assert_eq!(touched("tone-curve"), ["ToneCurve"]);
    assert_eq!(touched("color-grading"), ["ColorGrading"]);
    assert_eq!(touched("red-eye"), ["RedEye"]);
    assert_eq!(touched("spot-removal"), ["SpotRemoval"]);
    assert_eq!(touched("lut"), ["Lut"]);
    assert_eq!(touched("crop"), ["Crop", "Rotation"]);
    assert_eq!(touched("geometry"), ["Perspective"]);

    // The mixer is eight parameters and the fixture moved two of them:
    // an untouched band is not written back.
    assert_eq!(touched("hsl"), ["HslBand(0)", "HslBand(3)"]);

    // Local adjustments are removed by index, last first — the only
    // order in which the indices stay true.
    assert_eq!(
        reset_group("local", &everything())
            .into_iter()
            .map(|(param, _)| format!("{param:?}"))
            .collect::<Vec<_>>(),
        ["LocalAdjustment(1)", "LocalAdjustment(0)"]
    );

    // Basic is the wide one, and the list is the point: it names the
    // tone and colour parameters and **nothing** of detail, effects,
    // geometry or the mask tools.
    let basic = touched("basic");
    for owned in [
        "Blacks",
        "Clarity",
        "Contrast",
        "Dehaze",
        "Exposure",
        "Highlights",
        "Monochrome",
        "Saturation",
        "Shadows",
        "Texture",
        "Vibrance",
        "WhiteBalance",
        "Whites",
    ] {
        assert!(
            basic.contains(&owned.to_owned()),
            "basic should reset {owned}"
        );
    }
    for foreign in [
        "Sharpening",
        "NoiseReduction",
        "Vignette",
        "Grain",
        "Crop",
        "Rotation",
        "ToneCurve",
        "RedEye",
        "SpotRemoval",
    ] {
        assert!(
            !basic.contains(&foreign.to_owned()),
            "basic must not touch {foreign}"
        );
    }
}

#[test]
fn a_group_already_at_neutral_writes_nothing() {
    let neutral = leyline_sdk::neutral_settings();
    for group in GROUPS.iter().chain(["all"].iter()) {
        assert!(
            reset_group(group, &neutral).is_empty(),
            "{group} proposed a change on an untouched photo"
        );
    }
}

/// `GROUPS` is walked by name, so a name that is not an arm of the
/// `match` would silently contribute nothing to `all` — the fixture
/// moves every group, so every entry must have something to say about
/// it (ADR 0124 §4).
#[test]
fn every_named_group_is_a_real_arm() {
    let loaded = everything();
    for group in GROUPS {
        assert!(
            !reset_group(group, &loaded).is_empty(),
            "{group} is named in GROUPS but resets nothing"
        );
    }
}

#[test]
fn resetting_everything_is_exactly_the_union_of_the_groups() {
    let mut union: Vec<String> = GROUPS.iter().flat_map(|group| touched(group)).collect();
    union.sort();
    assert_eq!(touched("all"), union);

    // And it is one call, not fifteen revisions: the caller commits
    // the whole list at once.
    assert!(touched("all").len() > touched("basic").len());
}

/// The three headers that are not settings: a view toggle and two lists.
#[test]
fn the_headers_that_are_not_settings_reset_nothing() {
    let loaded = everything();
    for group in ["proof", "versions", "history", "nonsense"] {
        assert!(reset_group(group, &loaded).is_empty(), "{group}");
    }
}
