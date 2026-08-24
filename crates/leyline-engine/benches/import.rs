//! Benchmark of the import "chore" (`docs/roadmap.md` phase 7).
//!
//! `Library::import` holds the catalog mutex for its whole call
//! (`library.rs` doc comment on `import`), and the mutex is also on the
//! path of every interactive develop-mode operation — commits, ratings,
//! previews (`docs/adr/0023`, `docs/adr/0024`). Auto-import
//! (`docs/adr/0039-watched-folder-import.md`) and tethered capture
//! (`docs/adr/0038`) both run this exact call, one file at a time, on a
//! background thread while the user may be actively developing in the
//! foreground. This benchmark measures that one-file cost — checksum,
//! catalog write, and the thumbnail render that comes with it — so a
//! regression here (a slower codec, a heavier catalog write, a bigger
//! default thumbnail) is caught before it turns into a background chore
//! that makes interactive work feel slower than usual. Run with
//! `cargo bench -p leyline-engine`.

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use leyline_engine::{ImportOptions, Library};

/// A modest but real photo size: big enough that checksum and thumbnail
/// decode/render do real work, small enough to keep the benchmark fast.
const WIDTH: u32 = 800;
const HEIGHT: u32 = 600;

fn benches(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Library"), "ImportBench").unwrap();
    let source_dir = dir.path().join("Source");
    std::fs::create_dir(&source_dir).unwrap();
    let options = ImportOptions {
        copy_files: true,
        recursive: false,
        pair_companions: true,
    };

    let mut group = c.benchmark_group("import");
    group.sample_size(20);

    let mut sequence: u32 = 0;
    group.bench_function("single_file", |b| {
        b.iter_batched(
            || {
                sequence += 1;
                let path = source_dir.join(format!("shot-{sequence:06}.png"));
                // The sequence number is embedded literally in the first
                // pixels so every file's BLAKE3 checksum is distinct, no
                // matter how many iterations run — a per-pixel value
                // derived from `sequence` (e.g. `sequence as u8`, or even
                // `sequence % 250`) wraps within a few hundred iterations,
                // after which the PNG bytes repeat exactly and later
                // imports silently hit the duplicate-checksum skip path
                // instead of the real import this benchmark measures (this
                // is exactly what happened during development: a wrapping
                // fill made the whole benchmark ~500x too fast).
                let mut pixels = vec![128u8; WIDTH as usize * HEIGHT as usize * 3];
                pixels[..4].copy_from_slice(&sequence.to_le_bytes());
                image::save_buffer(
                    &path,
                    &pixels,
                    WIDTH,
                    HEIGHT,
                    image::ExtendedColorType::Rgb8,
                )
                .unwrap();
                path
            },
            |path| {
                let report = library
                    .import(&path, &options, |_, _| {})
                    .expect("importing a freshly written, distinct PNG never fails");
                std::hint::black_box(report);
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

#[allow(missing_docs)]
mod harness {
    use super::*;
    criterion_group!(import, benches);
}

criterion_main!(harness::import);
