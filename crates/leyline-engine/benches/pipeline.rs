//! Benchmarks of the develop rendering pipeline (`docs/roadmap.md`
//! phase 7).
//!
//! A synthetic 3 MP gradient image stands in for a decoded RAW so the
//! numbers isolate the operators from LibRaw and the disk. The `sections`
//! group follows the pipeline sections: `neutral` is the pass-through
//! floor, `tone`, `color` and `detail` exercise the per-pixel and blur
//! operators, `geometry` the resampling ones, and `full` a realistic edit
//! touching everything. The `lens` group measures the lens correction
//! stage (ADR 0016–0018): the same tone edit and a real bundled Lensfun
//! profile, off vs. on — distortion, vignetting and TCA all run together,
//! so the pipeline resamples the image up to three times per pixel instead
//! of once. The `stages` group prices each parameter family alone on top
//! of the neutral floor: the "what does moving *this* slider cost" table an
//! interactive UI budget is built from. The `denoise` group prices the two
//! denoise stage versions against each other (ADR 0046), both pinned
//! explicitly — every published version stays in the engine forever, so
//! every published version stays measured. Run with
//! `cargo bench -p leyline-engine`.

use criterion::{Criterion, criterion_group, criterion_main};
use leyline_color::{DcpProfile, Matrix3};
use leyline_core::{
    BrushStroke, ColorGrading, ColorGradingZone, Crop, CurvePoint, HslBand, LensCorrection,
    LocalAdjustment, LocalAdjustmentValues, Mask, NoiseReduction, Point, Settings, Sharpening,
    SpotRemoval, StageVersions, ToneCurve, WhiteBalance,
};
use leyline_engine::{LensShot, SourceColor, render};

/// The colorimetry every bench renders through: no camera matrix, so the
/// measurement is of the operators rather than of a body's profile.
const SOURCE: SourceColor = SourceColor::Camera {
    to_xyz: None,
    multipliers: None,
};
use leyline_raw::RawImage;
use std::hint::black_box;

/// Width of the synthetic frame (3:2, ~3 MP: honest ratios, fast runs).
const WIDTH: u32 = 2100;
/// Height of the synthetic frame.
const HEIGHT: u32 = 1400;

/// A deterministic 8-bit gradient-plus-texture frame: smooth ramps keep
/// the tone operators representative, the per-pixel hash gives the blurs
/// real high-frequency content to chew on.
fn synthetic_image() -> RawImage {
    let mut data = Vec::with_capacity(WIDTH as usize * HEIGHT as usize * 3);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let noise = (x.wrapping_mul(31).wrapping_add(y.wrapping_mul(17)) % 32) as u8;
            data.push((x * 255 / WIDTH) as u8 ^ noise);
            data.push((y * 255 / HEIGHT) as u8);
            data.push((((x + y) * 255) / (WIDTH + HEIGHT)) as u8 ^ noise);
        }
    }
    RawImage {
        width: WIDTH,
        height: HEIGHT,
        bits: 8,
        data,
    }
}

/// A typical tone edit: white balance, exposure and the four tone sliders.
fn tone_settings() -> Settings {
    Settings {
        white_balance: Some(WhiteBalance {
            temperature: 5200,
            tint: 8,
        }),
        exposure: 0.7,
        contrast: 25,
        highlights: -40,
        shadows: 35,
        whites: 10,
        blacks: -10,
        ..Settings::default()
    }
}

/// A real bundled Lensfun profile (Canon EOS 5D Mark III + EF 16-35mm
/// f/2.8L II USM) with distortion, vignetting and TCA calibration data all
/// present at 20mm — the same fixture `leyline-lens` and the lens stage
/// unit tests already use.
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

/// The realistic everything-on edit shared by both `full` benches.
fn full_settings() -> Settings {
    Settings {
        vibrance: 25,
        noise_reduction: NoiseReduction {
            luminance: 30,
            color: 20,
        },
        sharpening: Sharpening {
            amount: 50,
            radius: 1.0,
        },
        rotation: 1.5,
        crop: Some(Crop {
            x: 0.05,
            y: 0.05,
            width: 0.9,
            height: 0.9,
        }),
        ..tone_settings()
    }
}

fn benches(c: &mut Criterion) {
    let image = synthetic_image();
    let mut group = c.benchmark_group("sections");
    group.sample_size(10);

    group.bench_function("neutral", |b| {
        let settings = Settings {
            ..Settings::default()
        };
        b.iter(|| render(black_box(&image), black_box(&settings), None, None, SOURCE).unwrap());
    });

    group.bench_function("tone", |b| {
        let settings = tone_settings();
        b.iter(|| render(black_box(&image), black_box(&settings), None, None, SOURCE).unwrap());
    });

    group.bench_function("color", |b| {
        let settings = Settings {
            vibrance: 30,
            saturation: 10,
            ..Settings::default()
        };
        b.iter(|| render(black_box(&image), black_box(&settings), None, None, SOURCE).unwrap());
    });

    group.bench_function("detail", |b| {
        let settings = Settings {
            noise_reduction: NoiseReduction {
                luminance: 40,
                color: 30,
            },
            sharpening: Sharpening {
                amount: 60,
                radius: 1.2,
            },
            ..Settings::default()
        };
        b.iter(|| render(black_box(&image), black_box(&settings), None, None, SOURCE).unwrap());
    });

    group.bench_function("geometry", |b| {
        let settings = Settings {
            rotation: 2.0,
            crop: Some(Crop {
                x: 0.1,
                y: 0.1,
                width: 0.8,
                height: 0.8,
            }),
            ..Settings::default()
        };
        b.iter(|| render(black_box(&image), black_box(&settings), None, None, SOURCE).unwrap());
    });

    group.bench_function("full", |b| {
        let settings = full_settings();
        b.iter(|| render(black_box(&image), black_box(&settings), None, None, SOURCE).unwrap());
    });

    group.finish();

    // Lens correction (ADR 0016–0018): same tone edit and shot, disabled
    // vs. enabled (distortion + vignetting + TCA all at once, the only
    // combination `lens_correction.enabled` can produce).
    let mut group = c.benchmark_group("lens");
    group.sample_size(10);
    let shot = canon_shot();

    group.bench_function("baseline_no_lens_correction", |b| {
        let settings = tone_settings();
        b.iter(|| {
            render(
                black_box(&image),
                black_box(&settings),
                Some(black_box(&shot)),
                None,
                SOURCE,
            )
            .unwrap()
        });
    });

    group.bench_function("lens_correction", |b| {
        let settings = Settings {
            lens_correction: LensCorrection {
                enabled: true,
                profile: "auto".to_owned(),
            },
            ..tone_settings()
        };
        b.iter(|| {
            render(
                black_box(&image),
                black_box(&settings),
                Some(black_box(&shot)),
                None,
                SOURCE,
            )
            .unwrap()
        });
    });

    // Same as above, but with a real bundled profile that has distortion
    // calibration and no TCA calibration at all (Canon EF 17-35mm f/2.8L
    // USM, `leyline-lens`'s own no-TCA fixture): isolates the cost of the
    // TCA pass's early-exit vs. running a full identity resample.
    let no_tca_shot = LensShot {
        camera_make: "Canon".to_owned(),
        camera_model: "Canon EOS 5D Mark III".to_owned(),
        lens_make: Some("Canon".to_owned()),
        lens_model: Some("Canon EF 17-35mm f/2.8L USM".to_owned()),
        focal_mm: 20.0,
        aperture_f: None,
    };
    group.bench_function("lens_correction_no_tca", |b| {
        let settings = Settings {
            lens_correction: LensCorrection {
                enabled: true,
                profile: "auto".to_owned(),
            },
            ..tone_settings()
        };
        b.iter(|| {
            render(
                black_box(&image),
                black_box(&settings),
                Some(black_box(&no_tca_shot)),
                None,
                SOURCE,
            )
            .unwrap()
        });
    });

    group.finish();

    // Every parameter family the pipeline exposes, each one alone on top
    // of the neutral floor: this is the "what does
    // moving *this* slider cost" table, the one an interactive UI budget is
    // built from. `neutral` is the floor to subtract; `full` is everything
    // at once, the worst case a single edit can reach.
    let mut group = c.benchmark_group("stages");
    group.sample_size(10);
    let profile = sample_profile();

    for (name, settings) in stage_cases() {
        group.bench_function(name, |b| {
            b.iter(|| {
                render(
                    black_box(&image),
                    black_box(&settings),
                    Some(black_box(&shot)),
                    Some(black_box(&profile)),
                    SOURCE,
                )
                .unwrap()
            });
        });
    }

    group.finish();

    // The two denoise stage versions at identical slider values, each pinned
    // explicitly: what ADR 0046 costs against the Gaussian blur it replaces.
    // Both remain in the engine forever, so both stay priced forever.
    let mut group = c.benchmark_group("denoise");
    group.sample_size(10);

    for version in [1u16, 2u16] {
        let settings = Settings {
            noise_reduction: NoiseReduction {
                luminance: 40,
                color: 30,
            },
            stages: StageVersions::from([
                ("noise_luminance".to_owned(), version),
                ("noise_color".to_owned(), version),
            ]),
            ..Settings::default()
        };
        let name = if version == 1 {
            "v1_gaussian"
        } else {
            "v2_wavelet"
        };
        group.bench_function(name, |b| {
            b.iter(|| render(black_box(&image), black_box(&settings), None, None, SOURCE).unwrap());
        });
    }

    group.finish();
}

/// One entry per parameter family, each isolated on the neutral
/// base so the delta against `neutral` is that family's own cost.
fn stage_cases() -> Vec<(&'static str, Settings)> {
    let base = Settings {
        ..Settings::default()
    };
    vec![
        ("neutral", base.clone()),
        (
            "white_balance",
            Settings {
                white_balance: Some(WhiteBalance {
                    temperature: 5200,
                    tint: 8,
                }),
                ..base.clone()
            },
        ),
        (
            "exposure",
            Settings {
                exposure: 0.7,
                ..base.clone()
            },
        ),
        (
            "tone_sliders",
            Settings {
                contrast: 25,
                highlights: -40,
                shadows: 35,
                whites: 10,
                blacks: -10,
                ..base.clone()
            },
        ),
        (
            "tone_curve",
            Settings {
                tone_curve: ToneCurve {
                    points: vec![
                        CurvePoint { x: 0.0, y: 0.02 },
                        CurvePoint { x: 0.25, y: 0.18 },
                        CurvePoint { x: 0.75, y: 0.82 },
                        CurvePoint { x: 1.0, y: 0.98 },
                    ],
                },
                ..base.clone()
            },
        ),
        (
            "clarity",
            Settings {
                clarity: 40,
                ..base.clone()
            },
        ),
        (
            "texture",
            Settings {
                texture: 40,
                ..base.clone()
            },
        ),
        (
            "dehaze",
            Settings {
                dehaze: 40,
                ..base.clone()
            },
        ),
        (
            "vibrance_saturation",
            Settings {
                vibrance: 30,
                saturation: 10,
                ..base.clone()
            },
        ),
        (
            "hsl",
            Settings {
                hsl: [HslBand {
                    hue: 10,
                    saturation: 20,
                    luminance: -10,
                }; 8],
                ..base.clone()
            },
        ),
        (
            "color_grading",
            Settings {
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
                ..base.clone()
            },
        ),
        (
            "camera_profile",
            Settings {
                camera_profile: Some(leyline_core::CameraProfile {
                    enabled: true,
                    path: "Profiles/Camera/bench.dcp".to_owned(),
                    checksum: format!("blake3:{}", "00".repeat(32)),
                }),
                ..base.clone()
            },
        ),
        (
            "spot_removal_8",
            Settings {
                spot_removal: (0..8)
                    .map(|i| {
                        let t = f64::from(i) / 16.0 + 0.1;
                        SpotRemoval {
                            target: Point { x: t, y: 0.4 },
                            source: Point { x: t, y: 0.6 },
                            radius: 0.03,
                            feather: 0.5,
                            opacity: 1.0,
                        }
                    })
                    .collect(),
                ..base.clone()
            },
        ),
        (
            "local_radial",
            Settings {
                local_adjustments: vec![LocalAdjustment {
                    mask: Mask::Radial {
                        cx: 0.5,
                        cy: 0.5,
                        rx: 0.3,
                        ry: 0.25,
                        angle: 15.0,
                        feather: 0.5,
                        inverted: false,
                    },
                    range: None,
                    opacity: 1.0,
                    adjustments: local_values(),
                }],
                ..base.clone()
            },
        ),
        (
            "local_gradient",
            Settings {
                local_adjustments: vec![LocalAdjustment {
                    mask: Mask::Gradient {
                        x0: 0.0,
                        y0: 0.0,
                        x1: 0.0,
                        y1: 0.6,
                    },
                    range: None,
                    opacity: 1.0,
                    adjustments: local_values(),
                }],
                ..base.clone()
            },
        ),
        (
            "local_brush_64_dabs",
            Settings {
                local_adjustments: vec![LocalAdjustment {
                    mask: Mask::Brush {
                        strokes: (0..64)
                            .map(|i| BrushStroke {
                                x: 0.1 + f64::from(i) / 80.0,
                                y: 0.5,
                                radius: 0.05,
                                flow: 0.6,
                                hardness: 0.4,
                            })
                            .collect(),
                    },
                    range: None,
                    opacity: 1.0,
                    adjustments: local_values(),
                }],
                ..base.clone()
            },
        ),
        (
            "noise_reduction",
            Settings {
                noise_reduction: NoiseReduction {
                    luminance: 40,
                    color: 30,
                },
                ..base.clone()
            },
        ),
        (
            "sharpening",
            Settings {
                sharpening: Sharpening {
                    amount: 60,
                    radius: 1.2,
                },
                ..base.clone()
            },
        ),
        (
            "geometry",
            Settings {
                rotation: 2.0,
                crop: Some(Crop {
                    x: 0.1,
                    y: 0.1,
                    width: 0.8,
                    height: 0.8,
                }),
                ..base.clone()
            },
        ),
        (
            "lens_correction",
            Settings {
                lens_correction: LensCorrection {
                    enabled: true,
                    profile: "auto".to_owned(),
                },
                ..base.clone()
            },
        ),
        ("full", full_settings()),
    ]
}

/// The tonal/color values a local adjustment re-parameterizes, all set so
/// no per-pixel branch inside the masked path gets skipped.
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

/// A minimal in-memory DCP profile (identity `ColorMatrix1`): the matrix's
/// *values* do not change the per-pixel cost, only that a profile is
/// applied at all.
fn sample_profile() -> DcpProfile {
    use std::io::Cursor;
    use tiff::encoder::{TiffEncoder, colortype::Gray8};
    use tiff::tags::Tag;

    const IDENTITY: Matrix3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    /// DNG `ColorMatrix1`.
    const COLOR_MATRIX_1: u16 = 50721;

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

#[allow(missing_docs)]
mod harness {
    use super::*;
    criterion_group!(pipeline, benches);
}

criterion_main!(harness::pipeline);
