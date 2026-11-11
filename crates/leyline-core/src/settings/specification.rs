use super::*;

/// `docs/pipeline.md` §3.2 owns the list of `settings_json` keys and
/// their neutral values. A field that exists here and is named nowhere
/// there is a parameter a user can find in a stored revision and cannot
/// look up — which is how `demosaic` spent two weeks unlisted after
/// [ADR 0061](../../../../docs/adr/0061-demosaic-algorithm.md) added it.
///
/// Serialization is what is compared, not the Rust identifiers: the JSON
/// keys are the contract, and `serde` may rename one.
#[test]
fn every_settings_key_is_named_in_the_pipeline_specification() {
    // Everything set away from neutral, so nothing is skipped by
    // `skip_serializing_if` — the whole surface, in one document.
    let all = Settings {
        // The six fields the first version of this list did not move off
        // their neutral, found the day `SETTINGS_KEYS` was compared with
        // what this actually serializes (ADR 0142 §3) — the same way
        // `demosaic` was found before them.
        stages: StageVersions::from([("input".to_owned(), 2)]),
        monochrome: true,
        parametric_curve: ParametricCurve {
            shadows: 10,
            ..ParametricCurve::default()
        },
        levels: Levels {
            gamma: 1.2,
            ..Levels::default()
        },
        moire: 30,
        red_eye: vec![RedEye {
            center: Point { x: 0.5, y: 0.5 },
            radius: 0.05,
            feather: 0.4,
            darken: 0.8,
        }],
        backdrop: Backdrop {
            regions: vec![BackdropRegion {
                source: vec![Point { x: 0.0, y: 0.0 }; 3],
                zone: BackdropZone::OutsideSource,
                holes: Vec::new(),
            }],
            ..Backdrop::default()
        },
        source_encoding: SourceEncoding::LinearWorkspace,
        // `extra` stays empty: it is `#[serde(flatten)]`, so it has no
        // key of its own — what it holds is spliced in at the top level
        // (§3.4's round-trip of a key from a future version), and a
        // client meeting one of those shows it raw, which is honest.

        // Every field that `skip_serializing_if` can drop, pushed off its
        // neutral value — the first version of this test left `demosaic`
        // at its default and therefore did not notice it was serializing
        // nothing to check.
        demosaic: Demosaic::Dcb,
        highlight_reconstruction: HighlightReconstruction::Rebuild,
        defringe: Defringe {
            purple: 40,
            green: 20,
        },
        reshape: vec![ReshapePoint {
            from: Point { x: 0.4, y: 0.4 },
            to: Point { x: 0.45, y: 0.42 },
            radius: 0.1,
            strength: 1.0,
        }],
        spot_removal: vec![SpotRemoval {
            target: Point { x: 0.5, y: 0.5 },
            source: Point { x: 0.4, y: 0.4 },
            radius: 0.05,
            feather: 0.5,
            opacity: 1.0,
        }],
        local_adjustments: vec![LocalAdjustment {
            mask: Mask::Everything,
            range: Some(RangeMask::default()),
            opacity: 1.0,
            adjustments: LocalAdjustmentValues::default(),
        }],
        white_balance: Some(WhiteBalance::default()),
        camera_profile: Some(CameraProfile {
            enabled: true,
            path: "Profiles/Camera/x.dcp".to_owned(),
            checksum: "blake3:0".to_owned(),
        }),
        lut: Some(Lut {
            enabled: true,
            path: "Profiles/LUT/x.cube".to_owned(),
            checksum: "blake3:0".to_owned(),
            strength: 100,
        }),
        tone_curve: ToneCurve {
            points: vec![CurvePoint { x: 0.0, y: 0.0 }],
            ..ToneCurve::default()
        },
        perspective: Some(Perspective::default()),
        crop: Some(Crop {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }),
        ..Settings::default()
    };
    let serde_json::Value::Object(document) =
        serde_json::from_str::<serde_json::Value>(&all.to_json()).unwrap()
    else {
        panic!("settings serialize to an object");
    };

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/pipeline.md");
    let spec = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

    // The published list and the format itself, in step (ADR 0142 §3).
    let serialized: Vec<&str> = document.keys().map(String::as_str).collect();
    assert_eq!(
        serialized, SETTINGS_KEYS,
        "SETTINGS_KEYS and what a full settings value serializes to have \
             drifted apart — a client labelling a change would meet a key it \
             has never heard of"
    );

    let unlisted: Vec<&String> = document
        .keys()
        // `schema` and `stages` are the two reserved fields, documented
        // in their own table rather than among the parameters.
        .filter(|key| !spec.contains(&format!("`{key}`")))
        .collect();
    assert!(
        unlisted.is_empty(),
        "{} settings key(s) are stored in revisions and named nowhere in \
             docs/pipeline.md — a parameter a reader cannot look up:\n  {:?}",
        unlisted.len(),
        unlisted
    );
}
