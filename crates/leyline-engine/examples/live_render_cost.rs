//! What one frame of a slider drag costs at each preview class — the E0
//! measurement of `docs/measured-findings.md` §B5.
//!
//! ```text
//! cargo run --release -p leyline-engine --example live_render_cost -- <raw-file>...
//! ```
//!
//! For each file: imports it into a throw-away library, then for each class
//! (Small 1024 px, Medium 2048 px, Large 4096 px) renders a drag on an early
//! slider (exposure: the whole pipeline after the decode is replayed), on
//! contrast with the rest of the tonal block active, on saturation in a
//! cropped photograph, on clarity over a little texture, on dehaze, on coloured grain, and on a late one
//! (sharpening: the stage cache's best case), and prints the
//! median of the frames after the first, in milliseconds. Every render goes
//! through `Library::preview_live`, the path Studio's drag uses.
//!
//! `LEYLINE_E0_DUMP=<dir>` also writes each drag's last frame there as a
//! binary PPM, so a run with `LEYLINE_GPU=off` can be compared with one on
//! the GPU (ADR 0156 §3).

use std::path::{Path, PathBuf};
use std::time::Instant;

use leyline_core::{PreviewKind, Settings};
use leyline_engine::{ImportOptions, Library};

/// Frames per drag; the first warms the decode and the stage cache and is
/// left out of the median.
const FRAMES: usize = 9;

fn main() -> Result<(), String> {
    let files: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if files.is_empty() {
        return Err("usage: live_render_cost <raw-file>...".to_owned());
    }
    println!("file\tclass\tslider\tmedian_ms\tframe_px");
    for file in files {
        measure(&file)?;
    }
    Ok(())
}

fn measure(file: &Path) -> Result<(), String> {
    let scratch = tempfile::tempdir().map_err(|e| e.to_string())?;
    let shoot = scratch.path().join("shoot");
    std::fs::create_dir(&shoot).map_err(|e| e.to_string())?;
    let name = file.file_name().ok_or("not a file")?;
    std::fs::copy(file, shoot.join(name)).map_err(|e| e.to_string())?;

    let library =
        Library::create(&scratch.path().join("Library"), "E0").map_err(|e| e.to_string())?;
    let report = library
        .import(
            &shoot,
            &ImportOptions {
                copy_files: true,
                recursive: false,
                pair_companions: false,
                thumbnails: false,
            },
            |_, _| (),
        )
        .map_err(|e| e.to_string())?;
    let imported = report.imported.first().ok_or("nothing imported")?;
    let (asset, version) = (imported.registered.asset, imported.registered.version);
    eprintln!(
        "preview backend: {}",
        library
            .preview_backend()
            .unwrap_or_else(|| "CPU".to_owned())
    );
    let head = library
        .edit(version)
        .map_err(|e| e.to_string())?
        .settings()
        .clone();

    for (kind, class) in [
        (PreviewKind::Small, "small-1024"),
        (PreviewKind::Medium, "medium-2048"),
        (PreviewKind::Large, "large-4096"),
    ] {
        for slider in [
            "exposure",
            "tone",
            "colour",
            "sharpening",
            "clarity",
            "dehaze",
            "grain",
        ] {
            let mut times = Vec::with_capacity(FRAMES);
            let mut size = (0, 0);
            let mut last = None;
            for frame in 0..FRAMES {
                let settings = dragged(&head, slider, frame);
                let started = Instant::now();
                let image = library
                    .preview_live(asset, kind, &settings)
                    .map_err(|e| e.to_string())?;
                times.push(started.elapsed().as_secs_f64() * 1000.0);
                size = (image.width(), image.height());
                last = Some(image);
            }
            if let (Ok(dir), Some(image)) = (std::env::var("LEYLINE_E0_DUMP"), last) {
                let path = Path::new(&dir)
                    .join(format!("{}-{class}-{slider}.ppm", name.to_string_lossy()));
                let mut ppm = format!("P6\n{} {}\n255\n", size.0, size.1).into_bytes();
                ppm.extend_from_slice(image.data());
                std::fs::write(path, ppm).map_err(|e| e.to_string())?;
            }
            let mut steady = times[1..].to_vec();
            steady.sort_by(f64::total_cmp);
            println!(
                "{}\t{class}\t{slider}\t{:.0}\t{}x{}",
                name.to_string_lossy(),
                steady[steady.len() / 2],
                size.0,
                size.1
            );
        }
    }
    Ok(())
}

/// The settings at one frame of a drag on `slider`: a different value per
/// frame, as a pointer moving across the track produces.
fn dragged(head: &Settings, slider: &str, frame: usize) -> Settings {
    let mut settings = head.clone();
    let step = frame as f64 / FRAMES as f64;
    match slider {
        "exposure" => settings.exposure = 0.1 + step,
        // The Basic panel at work: contrast dragged while the other tonal
        // sliders, levels and the parametric curve hold their values.
        "tone" => {
            settings.contrast = 10 + frame as i32 * 5;
            settings.highlights = -30;
            settings.shadows = 25;
            settings.whites = 10;
            settings.blacks = -5;
            settings.levels.gamma = 1.2;
            settings.parametric_curve.darks = 15;
        }
        // A cropped, vignetted photograph, its saturation dragged with
        // vibrance, the HSL mixer and a shadow grade held.
        "colour" => {
            settings.saturation = -20 + frame as i32 * 5;
            settings.vibrance = 20;
            settings.hsl[0].hue = 30;
            settings.hsl[3].saturation = -40;
            settings.hsl[5].luminance = 25;
            settings.color_grading.shadows.hue = 220;
            settings.color_grading.shadows.saturation = 30;
            settings.vignette.amount = -30;
            settings.crop = Some(leyline_core::Crop {
                x: 0.1,
                y: 0.1,
                width: 0.8,
                height: 0.75,
            });
        }
        // Clarity dragged over a little texture: both local-contrast
        // radii, the reduced blur and the literal one at small classes.
        "clarity" => {
            settings.clarity = 10 + frame as i32 * 5;
            settings.texture = 15;
        }
        // Dehaze dragged: two minimum filters and the atmospheric light,
        // a global reduction, on every frame.
        "dehaze" => settings.dehaze = 10 + frame as i32 * 5,
        // Grain dragged, with colour: v2's three salted fields as well as
        // the shared one, at the end of the pipeline.
        "grain" => {
            settings.grain.amount = 20 + frame as i32 * 5;
            settings.grain.color = 40;
        }
        _ => {
            settings.sharpening.amount = 20 + frame as i32 * 5;
            settings.sharpening.radius = 1.0;
        }
    }
    settings
}
