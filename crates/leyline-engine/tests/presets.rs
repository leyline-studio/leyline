//! Integration tests: preset capture and application
//! (`docs/engine-api.md` §10.3, `docs/presets.md`).

use leyline_catalog::{CHECKSUM_LEN, Catalog, NewAsset, RegisteredAsset};
use leyline_core::{
    CURRENT_SCHEMA, CameraProfile, ColorGrading, Crop, CurvePoint, Defringe, Demosaic, Grain,
    HighlightReconstruction, HslBand, LensCorrection, LocalAdjustment, LocalAdjustmentValues, Lut,
    Mask, MediaType, NoiseReduction, OutputRendering, Perspective, Point, PresetSettings, RedEye,
    ReshapePoint, Settings, SettingsGroup, Sharpening, SpotRemoval, ToneCurve, VersionId, Vignette,
    WhiteBalance,
};
use leyline_engine::{EditSession, Param, Value, apply_batch, capture};

fn catalog_with_asset(dir: &tempfile::TempDir, name: &str) -> (Catalog, RegisteredAsset) {
    let mut catalog = Catalog::create(&dir.path().join("catalog.db"), "Presets").unwrap();
    let new = NewAsset {
        folder: catalog.ensure_folder("Photos").unwrap(),
        filename: name.to_owned(),
        extension: "CR3".to_owned(),
        media_type: MediaType::Raw,
        file_size: 1,
        checksum: [0xAB; CHECKSUM_LEN],
        width: None,
        height: None,
        capture_date: None,
        capture_offset_minutes: None,
    };
    let registered = catalog
        .add_asset(&new, &leyline_engine::neutral_settings())
        .unwrap();
    (catalog, registered)
}

#[test]
fn capture_then_apply_reproduces_only_the_included_groups() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, source) = catalog_with_asset(&dir, "IMG_0001.CR3");
    {
        let mut session = EditSession::open(&mut catalog, source.version).unwrap();
        session.set(Param::Contrast, Value::Int(30)).unwrap();
        session.set(Param::Rotation, Value::Float(5.0)).unwrap();
        session.commit().unwrap();
    }

    let preset = capture(&catalog, source.version, &[SettingsGroup::Tone]).unwrap();
    assert_eq!(preset.contrast, Some(30));
    assert_eq!(preset.rotation, None); // Geometry not requested.

    let folder = catalog.ensure_folder("Photos").unwrap();
    let target = catalog
        .add_asset(
            &NewAsset {
                folder,
                filename: "IMG_0002.CR3".to_owned(),
                extension: "CR3".to_owned(),
                media_type: MediaType::Raw,
                file_size: 1,
                checksum: [0xCD; CHECKSUM_LEN],
                width: None,
                height: None,
                capture_date: None,
                capture_offset_minutes: None,
            },
            &Settings::default(),
        )
        .unwrap()
        .version;

    let report = apply_batch(&mut catalog, &preset, &[target], |_, _| {});
    assert_eq!(report.applied, [target]);
    assert!(report.failed.is_empty());

    let head = catalog.version_head(target).unwrap();
    let settings = Settings::parse(&catalog.revision(head).unwrap().settings_json).unwrap();
    assert_eq!(settings.contrast, 30);
    assert_eq!(settings.rotation, 0.0); // Untouched: not in the preset.
}

#[test]
fn apply_always_creates_a_new_revision_never_an_amendment() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, target) = catalog_with_asset(&dir, "IMG_0001.CR3");
    let initial_head = catalog.version_head(target.version).unwrap();

    let preset = PresetSettings {
        schema: CURRENT_SCHEMA,
        groups: vec![SettingsGroup::Tone],
        exposure: Some(0.5),
        contrast: Some(10),
        highlights: Some(0),
        shadows: Some(0),
        whites: Some(0),
        blacks: Some(0),
        ..PresetSettings::default()
    };

    apply_batch(&mut catalog, &preset, &[target.version], |_, _| {});
    apply_batch(&mut catalog, &preset, &[target.version], |_, _| {});

    let history = catalog.version_history(target.version).unwrap();
    // Initial revision + two applications: three, none amended into another.
    assert_eq!(history.len(), 3);
    assert_ne!(history[0].revision, initial_head);
}

#[test]
fn one_failure_does_not_stop_the_batch() {
    let dir = tempfile::tempdir().unwrap();
    let (mut catalog, ok) = catalog_with_asset(&dir, "IMG_0001.CR3");
    let missing = VersionId::new(999);

    let preset = PresetSettings {
        schema: CURRENT_SCHEMA,
        groups: vec![SettingsGroup::WhiteBalance],
        white_balance: Some(None),
        ..PresetSettings::default()
    };

    let report = apply_batch(&mut catalog, &preset, &[missing, ok.version], |_, _| {});
    assert_eq!(report.applied, [ok.version]);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].version, missing);
}

/// A development in which **every** field a category claims is off its
/// neutral value.
///
/// Written as a full struct literal with no `..` on purpose (ADR 0132 §7):
/// adding a field to `Settings` stops this file compiling, which is the
/// moment to decide which category claims it — or to add it to §3's list of
/// refusals. A `..Default::default()` here would let the next field slip in
/// unclaimed exactly as the sixteen of ADR 0132's context did.
fn a_development_with_nothing_left_neutral() -> Settings {
    let neutral = leyline_engine::neutral_settings();
    let checksum = format!("blake3:{}", "ab".repeat(32));
    Settings {
        // Refused a category (ADR 0132 §3): carried over unchanged so the
        // assertion below compares them against themselves.
        schema: neutral.schema,
        stages: neutral.stages.clone(),
        source_encoding: neutral.source_encoding,
        extra: neutral.extra.clone(),

        camera_profile: Some(CameraProfile {
            enabled: true,
            path: "Profiles/a.dcp".to_owned(),
            checksum: checksum.clone(),
        }),
        lut: Some(Lut {
            enabled: true,
            path: "Luts/a.cube".to_owned(),
            checksum,
            strength: 60,
        }),
        white_balance: Some(WhiteBalance {
            temperature: 5400,
            tint: 4,
        }),
        exposure: 0.35,
        contrast: 12,
        highlights: -40,
        shadows: 25,
        whites: 7,
        blacks: -5,
        clarity: 11,
        texture: 13,
        dehaze: 17,
        vibrance: 18,
        saturation: 9,
        monochrome: true,
        tone_curve: ToneCurve {
            points: vec![
                CurvePoint { x: 0.0, y: 0.0 },
                CurvePoint { x: 0.4, y: 0.5 },
                CurvePoint { x: 1.0, y: 1.0 },
            ],
            ..ToneCurve::default()
        },
        hsl: [HslBand {
            hue: 5,
            saturation: -10,
            luminance: 15,
        }; 8],
        color_grading: ColorGrading {
            balance: 20,
            blending: 30,
            ..ColorGrading::default()
        },
        spot_removal: vec![SpotRemoval {
            target: Point { x: 0.3, y: 0.3 },
            source: Point { x: 0.6, y: 0.6 },
            radius: 0.05,
            feather: 0.5,
            opacity: 1.0,
        }],
        reshape: vec![ReshapePoint {
            from: Point { x: 0.4, y: 0.4 },
            to: Point { x: 0.45, y: 0.4 },
            radius: 0.2,
            strength: 0.5,
        }],
        red_eye: vec![RedEye {
            center: Point { x: 0.5, y: 0.5 },
            radius: 0.03,
            feather: 0.4,
            darken: 0.8,
        }],
        local_adjustments: vec![LocalAdjustment {
            mask: Mask::Everything,
            range: None,
            opacity: 1.0,
            adjustments: LocalAdjustmentValues {
                exposure: Some(0.5),
                ..LocalAdjustmentValues::default()
            },
        }],
        lens_correction: LensCorrection {
            enabled: true,
            ..LensCorrection::default()
        },
        noise_reduction: NoiseReduction {
            luminance: 15,
            color: 25,
        },
        sharpening: Sharpening {
            amount: 40,
            ..Sharpening::default()
        },
        defringe: Defringe {
            purple: 6,
            green: 4,
        },
        output_rendering: OutputRendering {
            highlight_rolloff: 42,
        },
        highlight_reconstruction: HighlightReconstruction::Blend,
        demosaic: Demosaic::Dcb,
        rotation: 5.0,
        perspective: Some(Perspective {
            vertical: 10,
            horizontal: -6,
        }),
        crop: Some(Crop {
            x: 0.1,
            y: 0.2,
            width: 0.8,
            height: 0.7,
        }),
        vignette: Vignette {
            amount: -30,
            ..Vignette::default()
        },
        grain: Grain {
            amount: 20,
            ..Grain::default()
        },
    }
}

/// ADR 0132 §7 — the guard that keeps the categories covering the panel.
///
/// Behavioural rather than a list of field names, so it is not a second
/// place to forget something: a field with no category fails it, and so does
/// a category whose `param_values` arm was never written.
#[test]
fn every_category_carries_its_settings_all_the_way_across() {
    const EVERY_GROUP: &[SettingsGroup] = &[
        SettingsGroup::WhiteBalance,
        SettingsGroup::Tone,
        SettingsGroup::Presence,
        SettingsGroup::ToneCurve,
        SettingsGroup::ColorMixer,
        SettingsGroup::ColorGrading,
        SettingsGroup::CameraProfile,
        SettingsGroup::CreativeLut,
        SettingsGroup::Effects,
        SettingsGroup::LensCorrection,
        SettingsGroup::Detail,
        SettingsGroup::Rendering,
        SettingsGroup::Geometry,
        SettingsGroup::Reshape,
        SettingsGroup::SpotRemoval,
        SettingsGroup::RedEye,
        SettingsGroup::LocalAdjustments,
    ];

    let moved = a_development_with_nothing_left_neutral();
    let captured = PresetSettings::capture(&moved, EVERY_GROUP);
    let landed = leyline_engine::overlay(&leyline_engine::neutral_settings(), &captured).unwrap();

    assert_eq!(
        landed, moved,
        "a setting was left behind between capture and apply — either it \
         belongs to no category (ADR 0132 §1) or its `param_values` arm is \
         missing (§7)"
    );
}
