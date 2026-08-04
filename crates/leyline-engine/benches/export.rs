//! Benchmarks of the **export** path (`docs/competitive-plan.md` B2).
//!
//! ADR 0041 deliberately left export and print out of its optimisations:
//! they render at full resolution, with no stage cache, bit for bit as
//! before. That was the right call for an ADR about interaction — but it
//! left the export path unmeasured, and "a batch of 500 modern files" is a
//! real use nobody had ever timed.
//!
//! Three costs make up an export, and they are priced separately here
//! because they scale differently and only one of them is ours:
//!
//! * `render` — the develop pipeline at full resolution, at today's 10 Mpx
//!   reference size and at the 45 Mpx of a current body;
//! * `encode` — the codec, per format, on a 45 Mpx frame;
//! * `decode` — LibRaw on a real RAW file, priced only when one is given
//!   through `LEYLINE_TEST_RAW` (the other groups stay synthetic so they
//!   measure operators rather than a disk).
//!
//! Run with `cargo bench -p leyline-engine --bench export`. AVIF is priced
//! on the 10 Mpx frame as well as the 45 Mpx one: it is by far the slowest
//! encoder, and knowing how it scales matters more than one absolute number.

use criterion::{Criterion, criterion_group, criterion_main};
use leyline_core::{Crop, NoiseReduction, Settings, Sharpening, WhiteBalance};
use leyline_engine::{SourceColor, render};
use leyline_export::{ExportFormat, ExportSettings};
use leyline_raw::RawImage;
use std::hint::black_box;

/// The colorimetry every bench renders through: no camera matrix, so the
/// measurement is of the operators rather than of a body's profile.
const SOURCE: SourceColor = SourceColor::Camera {
    to_xyz: None,
    multipliers: None,
};

/// The 10 Mpx frame ADR 0041 measured the preview path on — the same CR2
/// geometry, so the two documents' numbers can be read side by side.
const REFERENCE: (u32, u32) = (3888, 2592);

/// A current full-frame body: 45 Mpx, 3:2. What "the files people actually
/// shoot today" means in this document.
const MODERN: (u32, u32) = (8192, 5464);

/// A deterministic gradient-plus-texture frame, the same recipe the
/// pipeline bench uses: smooth ramps for the tone operators, a per-pixel
/// hash so the blurs have real high-frequency content to chew on.
fn synthetic_image(width: u32, height: u32) -> RawImage {
    let mut data = Vec::with_capacity(width as usize * height as usize * 3);
    for y in 0..height {
        for x in 0..width {
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

/// The realistic everything-on edit: the same one the pipeline bench calls
/// `full`, so an export number can be compared with a preview number.
fn full_settings() -> Settings {
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
        ..Settings::default()
    }
}

fn benches(c: &mut Criterion) {
    // What the develop pipeline costs at export resolution — no proxy, no
    // stage cache, which is exactly what ADR 0041 §1 excluded here.
    let mut group = c.benchmark_group("export_render");
    group.sample_size(10);
    for (name, (width, height)) in [("10mpx", REFERENCE), ("45mpx", MODERN)] {
        let image = synthetic_image(width, height);
        for (edit, settings) in [("neutral", Settings::default()), ("full", full_settings())] {
            group.bench_function(format!("{name}_{edit}"), |b| {
                b.iter(|| {
                    render(
                        black_box(&image),
                        black_box(&settings),
                        None,
                        None,
                        None,
                        &Default::default(),
                        SOURCE,
                    )
                    .unwrap()
                });
            });
        }
    }
    group.finish();

    // What the codecs cost on the rendered pixels. Written to a temporary
    // directory: encoding to a file is what an export does, and leaving the
    // write out would price something nobody runs.
    let mut group = c.benchmark_group("export_encode");
    group.sample_size(10);
    let dir = tempfile::tempdir().expect("a temporary directory");
    let modern = synthetic_image(MODERN.0, MODERN.1);
    for format in [
        ExportFormat::Jpeg,
        ExportFormat::Png,
        ExportFormat::Tiff,
        ExportFormat::Webp,
        ExportFormat::Avif,
    ] {
        let settings = ExportSettings {
            format,
            ..ExportSettings::default()
        };
        let path = dir.path().join(format!("45mpx.{}", format.extension()));
        group.bench_function(format!("45mpx_{}", format.extension()), |b| {
            b.iter(|| {
                leyline_export::encode(
                    black_box(&path),
                    modern.width,
                    modern.height,
                    black_box(&modern.data),
                    black_box(&settings),
                )
                .unwrap()
            });
        });
    }

    // AVIF again at the reference size: the one encoder whose scaling is
    // worth knowing on its own.
    let reference = synthetic_image(REFERENCE.0, REFERENCE.1);
    let settings = ExportSettings {
        format: ExportFormat::Avif,
        ..ExportSettings::default()
    };
    let path = dir.path().join("10mpx.avif");
    group.bench_function("10mpx_avif", |b| {
        b.iter(|| {
            leyline_export::encode(
                black_box(&path),
                reference.width,
                reference.height,
                black_box(&reference.data),
                black_box(&settings),
            )
            .unwrap()
        });
    });
    group.finish();

    // The third of the export's three costs, and the only one that is not
    // ours: LibRaw. Priced only when a real file is offered, since a
    // synthetic buffer cannot stand in for a sensor's data.
    let Ok(raw) = std::env::var("LEYLINE_TEST_RAW") else {
        return;
    };
    let raw = std::path::PathBuf::from(raw);
    let mut group = c.benchmark_group("export_decode");
    group.sample_size(10);
    group.bench_function("libraw_full_size", |b| {
        b.iter(|| {
            leyline_raw::decode(black_box(&raw), &leyline_raw::DecodeParams::default()).unwrap()
        });
    });
    group.finish();
}

#[allow(missing_docs)]
mod harness {
    use super::*;
    criterion_group!(export, benches);
}

criterion_main!(harness::export);
