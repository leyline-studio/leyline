//! Benchmarks of the develop rendering pipeline (`docs/roadmap.md`
//! phase 7).
//!
//! A synthetic 3 MP gradient image stands in for a decoded RAW so the
//! numbers isolate the operators from LibRaw and the disk. The `process1`
//! group follows the pipeline sections: `neutral` is the pass-through
//! floor, `tone`, `color` and `detail` exercise the per-pixel and blur
//! operators, `geometry` the resampling ones, and `full` a realistic edit
//! touching everything. The `process2` group repeats the transfer-heavy
//! cases through the LUT pipeline (ADR 0013) for comparison. Run with
//! `cargo bench -p leyline-engine`.

use criterion::{Criterion, criterion_group, criterion_main};
use leyline_core::{Crop, NoiseReduction, Settings, Sharpening, WhiteBalance};
use leyline_engine::render;
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
/// Pinned to process 1 so recorded baselines stay comparable; the
/// `process2` group overrides the version.
fn tone_settings() -> Settings {
    Settings {
        process: 1,
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
    let mut group = c.benchmark_group("process1");
    group.sample_size(10);

    group.bench_function("neutral", |b| {
        let settings = Settings {
            process: 1,
            ..Settings::default()
        };
        b.iter(|| render(black_box(&image), black_box(&settings)).unwrap());
    });

    group.bench_function("tone", |b| {
        let settings = tone_settings();
        b.iter(|| render(black_box(&image), black_box(&settings)).unwrap());
    });

    group.bench_function("color", |b| {
        let settings = Settings {
            process: 1,
            vibrance: 30,
            saturation: 10,
            ..Settings::default()
        };
        b.iter(|| render(black_box(&image), black_box(&settings)).unwrap());
    });

    group.bench_function("detail", |b| {
        let settings = Settings {
            process: 1,
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
        b.iter(|| render(black_box(&image), black_box(&settings)).unwrap());
    });

    group.bench_function("geometry", |b| {
        let settings = Settings {
            process: 1,
            rotation: 2.0,
            crop: Some(Crop {
                x: 0.1,
                y: 0.1,
                width: 0.8,
                height: 0.8,
            }),
            ..Settings::default()
        };
        b.iter(|| render(black_box(&image), black_box(&settings)).unwrap());
    });

    group.bench_function("full", |b| {
        let settings = full_settings();
        b.iter(|| render(black_box(&image), black_box(&settings)).unwrap());
    });

    group.finish();

    // The transfer-heavy cases again through the LUT pipeline (ADR 0013).
    let mut group = c.benchmark_group("process2");
    group.sample_size(10);

    group.bench_function("tone", |b| {
        let settings = Settings {
            process: 2,
            ..tone_settings()
        };
        b.iter(|| render(black_box(&image), black_box(&settings)).unwrap());
    });

    group.bench_function("full", |b| {
        let settings = Settings {
            process: 2,
            ..full_settings()
        };
        b.iter(|| render(black_box(&image), black_box(&settings)).unwrap());
    });

    group.finish();
}

#[allow(missing_docs)]
mod harness {
    use super::*;
    criterion_group!(process1, benches);
}

criterion_main!(harness::process1);
