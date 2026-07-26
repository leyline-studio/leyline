//! Golden renders — the standing proof of the promise `docs/pipeline.md`
//! §5.1 makes: *this RAW, these settings, these pixels, in ten years*.
//!
//! Each entry pins the BLAKE3 digest of one render's RGB8 output, plus its
//! dimensions and a few sampled pixels so a failure says something more
//! useful than "the hash moved". Every case is deterministic: synthetic
//! images, fixed settings, no clock, no file system, no thread-count
//! dependence (the pipeline's parallelism is over disjoint rows).
//!
//! These fixtures were what proved the ADR 0042 migration pixel-exact
//! across the eleven `processN.rs` modules it replaced. ADR 0043 then
//! collapsed that pre-publication history onto one version per operator,
//! and the manifest was regenerated once, under that decision — the only
//! time it may ever be. From here on a changed digest means a revision
//! somewhere now renders differently, which is a defect, not a diff to
//! bless:
//!
//! ```text
//! LEYLINE_BLESS_GOLDEN=1 cargo test -p leyline-engine --test golden_renders
//! ```

use std::collections::BTreeMap;
use std::io::Cursor;

use leyline_color::DcpProfile;
use leyline_core::{
    BrushStroke, ColorGrading, ColorGradingZone, Crop, CurvePoint, HslBand, LensCorrection,
    LocalAdjustment, LocalAdjustmentValues, Mask, NoiseReduction, Point, Settings, Sharpening,
    SpotRemoval, ToneCurve, WhiteBalance,
};
use leyline_engine::{LensShot, render};
use leyline_raw::RawImage;
use serde::{Deserialize, Serialize};

/// Path of the committed manifest, relative to the crate root.
const MANIFEST: &str = "tests/golden/renders.json";

/// One pinned render.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Golden {
    width: u32,
    height: u32,
    /// BLAKE3 of the RGB8 output — the bit-identity assertion itself.
    digest: String,
    /// A handful of evenly-spaced RGB samples. They prove nothing the
    /// digest doesn't, but when a digest moves they say *how* it moved.
    samples: Vec<[u8; 3]>,
}

/// Deterministic stand-in for a decoded RAW: smooth ramps so tone
/// operators have something continuous to act on, a per-pixel hash so the
/// blurs and the dark-channel filter have real high-frequency content, and
/// a saturated block so the color operators are not exercised on grey.
fn synthetic_image(width: u32, height: u32) -> RawImage {
    let mut data = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            if (width / 4..width / 2).contains(&x) && (height / 4..height / 2).contains(&y) {
                data.extend_from_slice(&[210, 70, 45]);
                continue;
            }
            let noise = (x.wrapping_mul(31).wrapping_add(y.wrapping_mul(17)) % 32) as u8;
            data.push((x * 255 / width) as u8 ^ noise);
            data.push((y * 255 / height) as u8);
            data.push((((x + y) * 255) / (width + height)) as u8 ^ noise);
        }
    }
    RawImage {
        width,
        height,
        bits: 8,
        data,
    }
}

/// A real bundled Lensfun profile with distortion, vignetting and TCA data
/// at 20mm — the same fixture `leyline-lens` and the lens stage unit
/// tests use, so the golden exercises the actual profile lookup rather
/// than a synthetic stand-in.
fn canon_shot() -> LensShot {
    LensShot {
        camera_make: "Canon".to_owned(),
        camera_model: "Canon EOS 5D Mark III".to_owned(),
        lens_make: Some("Canon".to_owned()),
        lens_model: Some("Canon EF 16-35mm f/2.8L II USM".to_owned()),
        focal_mm: 20.0,
        aperture_f: Some(2.8),
    }
}

/// A minimal in-memory DCP carrying an identity `ColorMatrix1`. The matrix
/// values do not matter to the freeze — only that the stage runs and that
/// it keeps producing the same pixels through it.
fn sample_profile() -> DcpProfile {
    use tiff::encoder::{TiffEncoder, colortype::Gray8};
    use tiff::tags::Tag;

    /// DNG `ColorMatrix1`.
    const COLOR_MATRIX_1: u16 = 50721;
    const IDENTITY: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    let mut buffer = Cursor::new(Vec::new());
    {
        let mut encoder = TiffEncoder::new(&mut buffer).unwrap();
        let mut image = encoder.new_image::<Gray8>(1, 1).unwrap();
        let values: Vec<tiff::encoder::SRational> = IDENTITY
            .into_iter()
            .flatten()
            .map(|v| tiff::encoder::SRational {
                n: (v * 10000.0).round() as i32,
                d: 10000,
            })
            .collect();
        image
            .encoder()
            .write_tag(Tag::Unknown(COLOR_MATRIX_1), values.as_slice())
            .unwrap();
        image.write_data(&[0u8]).unwrap();
    }
    DcpProfile::parse(&buffer.into_inner()).unwrap()
}

// ---------------------------------------------------------------------------
// Settings fragments, each exercising one operator family
// ---------------------------------------------------------------------------

fn white_balance() -> Option<WhiteBalance> {
    Some(WhiteBalance {
        temperature: 5200,
        tint: 8,
    })
}

fn tone(settings: Settings) -> Settings {
    Settings {
        white_balance: white_balance(),
        exposure: 0.7,
        contrast: 25,
        highlights: -40,
        shadows: 35,
        whites: 10,
        blacks: -10,
        ..settings
    }
}

fn color(settings: Settings) -> Settings {
    Settings {
        vibrance: 30,
        saturation: 12,
        ..settings
    }
}

fn detail(settings: Settings) -> Settings {
    Settings {
        noise_reduction: NoiseReduction {
            luminance: 40,
            color: 30,
        },
        sharpening: Sharpening {
            amount: 60,
            radius: 1.2,
        },
        ..settings
    }
}

fn geometry(settings: Settings) -> Settings {
    Settings {
        rotation: 2.0,
        crop: Some(Crop {
            x: 0.1,
            y: 0.1,
            width: 0.8,
            height: 0.8,
        }),
        ..settings
    }
}

fn lens(settings: Settings) -> Settings {
    Settings {
        lens_correction: LensCorrection {
            enabled: true,
            profile: "auto".to_owned(),
        },
        ..settings
    }
}

fn tone_curve(settings: Settings) -> Settings {
    Settings {
        tone_curve: ToneCurve {
            points: vec![
                CurvePoint { x: 0.0, y: 0.03 },
                CurvePoint { x: 0.3, y: 0.22 },
                CurvePoint { x: 0.7, y: 0.79 },
                CurvePoint { x: 1.0, y: 0.97 },
            ],
        },
        ..settings
    }
}

fn spots(settings: Settings) -> Settings {
    Settings {
        spot_removal: vec![
            SpotRemoval {
                target: Point { x: 0.3, y: 0.4 },
                source: Point { x: 0.6, y: 0.7 },
                radius: 0.08,
                feather: 0.5,
                opacity: 0.9,
            },
            SpotRemoval {
                target: Point { x: 0.7, y: 0.2 },
                source: Point { x: 0.4, y: 0.5 },
                radius: 0.05,
                feather: 0.2,
                opacity: 1.0,
            },
        ],
        ..settings
    }
}

fn local_values() -> LocalAdjustmentValues {
    LocalAdjustmentValues {
        temperature: Some(6500),
        tint: Some(5),
        exposure: Some(0.5),
        contrast: Some(20),
        highlights: Some(-20),
        shadows: Some(20),
        whites: Some(5),
        blacks: Some(-5),
        vibrance: Some(15),
        saturation: Some(10),
    }
}

fn locals(settings: Settings) -> Settings {
    Settings {
        local_adjustments: vec![
            LocalAdjustment {
                mask: Mask::Radial {
                    cx: 0.45,
                    cy: 0.5,
                    rx: 0.3,
                    ry: 0.25,
                    angle: 15.0,
                    feather: 0.5,
                    inverted: false,
                },
                opacity: 0.9,
                adjustments: local_values(),
            },
            LocalAdjustment {
                mask: Mask::Gradient {
                    x0: 0.0,
                    y0: 0.0,
                    x1: 0.0,
                    y1: 0.6,
                },
                opacity: 0.7,
                adjustments: local_values(),
            },
            LocalAdjustment {
                mask: Mask::Brush {
                    strokes: (0..12)
                        .map(|i| BrushStroke {
                            x: 0.2 + f64::from(i) / 30.0,
                            y: 0.55,
                            radius: 0.06,
                            flow: 0.6,
                            hardness: 0.4,
                        })
                        .collect(),
                },
                opacity: 1.0,
                adjustments: local_values(),
            },
        ],
        ..settings
    }
}

fn hsl_grading(settings: Settings) -> Settings {
    Settings {
        hsl: [
            HslBand {
                hue: 10,
                saturation: 20,
                luminance: -10,
            },
            HslBand {
                hue: -15,
                saturation: 30,
                luminance: 5,
            },
            HslBand {
                hue: 5,
                saturation: -20,
                luminance: 15,
            },
            HslBand {
                hue: 20,
                saturation: 10,
                luminance: -5,
            },
            HslBand {
                hue: -10,
                saturation: 25,
                luminance: 0,
            },
            HslBand {
                hue: 30,
                saturation: -15,
                luminance: 10,
            },
            HslBand {
                hue: 0,
                saturation: 40,
                luminance: -20,
            },
            HslBand {
                hue: -25,
                saturation: 5,
                luminance: 8,
            },
        ],
        color_grading: ColorGrading {
            shadows: ColorGradingZone {
                hue: 220,
                saturation: 30,
                luminance: -5,
            },
            midtones: ColorGradingZone {
                hue: 40,
                saturation: 15,
                luminance: 0,
            },
            highlights: ColorGradingZone {
                hue: 55,
                saturation: 25,
                luminance: 5,
            },
            balance: 10,
            blending: 50,
        },
        ..settings
    }
}

fn presence(settings: Settings) -> Settings {
    Settings {
        clarity: 40,
        texture: 30,
        dehaze: 35,
        ..settings
    }
}

// ---------------------------------------------------------------------------
// The case matrix
// ---------------------------------------------------------------------------

/// Every case: `(key, settings)`.
///
/// One `neutral` floor, one case per operator family, and an `everything`
/// case running them all together — the combination most likely to expose a
/// stage ordering mistake.
fn cases() -> Vec<(String, Settings)> {
    let base = Settings::default();
    let mut named: Vec<(&str, Settings)> = vec![
        ("neutral", base.clone()),
        ("tone", tone(base.clone())),
        ("color", color(base.clone())),
        ("detail", detail(base.clone())),
        ("geometry", geometry(base.clone())),
        ("lens", lens(base.clone())),
        ("tone_curve", tone_curve(base.clone())),
        ("spots", spots(base.clone())),
        ("locals", locals(base.clone())),
        ("hsl_grading", hsl_grading(base.clone())),
        ("presence", presence(base.clone())),
    ];

    let all = presence(hsl_grading(locals(spots(tone_curve(lens(detail(color(
        tone(base.clone()),
    ))))))));
    named.push(("everything", geometry(all)));

    named
        .into_iter()
        .map(|(name, settings)| (name.to_owned(), settings))
        .collect()
}

/// Renders one case and reduces it to its pinned form.
fn capture(settings: &Settings) -> Golden {
    // Small enough to keep the suite fast, large enough that the blur and
    // resampling stages have real neighbourhoods to work with.
    let image = synthetic_image(96, 64);
    let shot = canon_shot();
    let profile = sample_profile();
    let rendered = render(&image, settings, Some(&shot), Some(&profile))
        .unwrap_or_else(|e| panic!("render failed: {e}"));

    let digest = blake3::hash(&rendered.data).to_hex().to_string();
    let step = (rendered.data.len() / 3 / 16).max(1);
    let samples = (0..16)
        .filter_map(|i| {
            let at = i * step * 3;
            rendered
                .data
                .get(at..at + 3)
                .map(|px| [px[0], px[1], px[2]])
        })
        .collect();
    Golden {
        width: rendered.width,
        height: rendered.height,
        digest,
        samples,
    }
}

#[test]
fn the_pipeline_still_renders_exactly_what_it_rendered_before() {
    let current: BTreeMap<String, Golden> = cases()
        .into_iter()
        .map(|(key, settings)| (key, capture(&settings)))
        .collect();

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(MANIFEST);

    if std::env::var_os("LEYLINE_BLESS_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let json = serde_json::to_string_pretty(&current).unwrap();
        std::fs::write(&path, format!("{json}\n")).unwrap();
        eprintln!(
            "blessed {} golden renders -> {}",
            current.len(),
            path.display()
        );
        return;
    }

    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\nrun with LEYLINE_BLESS_GOLDEN=1 to capture it",
            path.display()
        )
    });
    let expected: BTreeMap<String, Golden> = serde_json::from_str(&raw).unwrap();

    let mut drifted = Vec::new();
    for (key, want) in &expected {
        match current.get(key) {
            None => drifted.push(format!("{key}: case disappeared")),
            Some(got) if got != want => drifted.push(format!(
                "{key}: {}x{} {} -> {}x{} {}\n    expected samples {:?}\n    actual   samples {:?}",
                want.width,
                want.height,
                &want.digest[..16],
                got.width,
                got.height,
                &got.digest[..16],
                &want.samples[..4.min(want.samples.len())],
                &got.samples[..4.min(got.samples.len())],
            )),
            Some(_) => {}
        }
    }
    for key in current.keys() {
        if !expected.contains_key(key) {
            drifted.push(format!("{key}: new case, not yet pinned"));
        }
    }

    assert!(
        drifted.is_empty(),
        "{} of {} golden renders drifted — a revision somewhere now renders \
         differently (docs/pipeline.md §3.3):\n  {}",
        drifted.len(),
        expected.len(),
        drifted.join("\n  ")
    );
}
