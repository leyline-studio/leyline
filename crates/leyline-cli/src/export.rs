//! Export, and the provenance commands that read an export back (`verify`, `reproduce`).

use super::*;

/// Builds an [`ExportSettings`] from the shared recipe flags.
pub(crate) fn recipe(options: &Options) -> Result<ExportSettings, String> {
    let mut settings = ExportSettings::default();
    if let Some(format) = options.value("format") {
        settings.format = match format {
            "jpeg" | "jpg" => ExportFormat::Jpeg,
            "png" => ExportFormat::Png,
            "tiff" | "tif" => ExportFormat::Tiff,
            "webp" => ExportFormat::Webp,
            "avif" => ExportFormat::Avif,
            other => return Err(format!("unknown export format {other:?}")),
        };
    }
    if let Some(quality) = options.value("quality") {
        settings.quality = quality
            .parse()
            .map_err(|_| format!("bad quality {quality:?}"))?;
    }
    if let Some(speed) = options.value("avif-speed") {
        settings.avif_speed = speed
            .parse()
            .map_err(|_| format!("bad avif speed {speed:?}"))?;
    }
    if let Some(edge) = options.value("max-edge") {
        settings.max_edge = Some(edge.parse().map_err(|_| format!("bad max edge {edge:?}"))?);
    }
    if let Some(metadata) = options.value("metadata") {
        settings.metadata = match metadata {
            "full" => ExportMetadata::Full,
            "without-location" => ExportMetadata::WithoutLocation,
            "none" => ExportMetadata::None,
            other => {
                return Err(format!(
                    "unknown metadata policy {other:?}, expected full/without-location/none"
                ));
            }
        };
    }
    // The line, then its decoration (ADR 0106 §1). The ranges are not
    // restated here: `validate` below owns them, and a client that copied
    // them would be a second place to correct them.
    if let Some(text) = options.value("watermark") {
        settings.watermark = Some(Watermark {
            text: text.to_owned(),
            ..Watermark::default()
        });
    }
    if let Some(anchor) = options.value("watermark-anchor") {
        let anchor = match anchor {
            "bottom-right" => WatermarkAnchor::BottomRight,
            "bottom-left" => WatermarkAnchor::BottomLeft,
            "top-right" => WatermarkAnchor::TopRight,
            "top-left" => WatermarkAnchor::TopLeft,
            "center" => WatermarkAnchor::Center,
            other => {
                return Err(format!(
                    "unknown watermark anchor {other:?}, expected \
                     bottom-right/bottom-left/top-right/top-left/center"
                ));
            }
        };
        decorate(&mut settings.watermark, "--watermark-anchor", |w| {
            w.anchor = anchor;
        })?;
    }
    if let Some(size) = options.value("watermark-size") {
        let size = size
            .parse()
            .map_err(|_| format!("bad watermark size {size:?}"))?;
        decorate(&mut settings.watermark, "--watermark-size", |w| {
            w.size = size
        })?;
    }
    if let Some(color) = options.value("watermark-color") {
        let color = color.to_owned();
        decorate(&mut settings.watermark, "--watermark-color", |w| {
            w.color = color;
        })?;
    }
    if let Some(opacity) = options.value("watermark-opacity") {
        let opacity = opacity
            .parse()
            .map_err(|_| format!("bad watermark opacity {opacity:?}"))?;
        decorate(&mut settings.watermark, "--watermark-opacity", |w| {
            w.opacity = opacity;
        })?;
    }
    settings.validate().map_err(|e| e.to_string())?;
    Ok(settings)
}

/// Applies one decoration to the watermark being built, or says which
/// option was given without a line to decorate (ADR 0106 §1).
pub(crate) fn decorate(
    watermark: &mut Option<Watermark>,
    option: &str,
    set: impl FnOnce(&mut Watermark),
) -> Result<(), String> {
    match watermark {
        Some(watermark) => {
            set(watermark);
            Ok(())
        }
        None => Err(format!("{option} needs --watermark")),
    }
}

/// Once per batch, before it starts: a policy that asks for metadata in a
/// format that cannot carry it would otherwise look applied (ADR 0157 §5).
pub(crate) fn say_when_the_format_drops_metadata(settings: &ExportSettings) {
    if settings.metadata != ExportMetadata::None && !settings.format.carries_metadata() {
        eprintln!(
            "note: {} files carry no metadata — the encoder offers no way to write it",
            settings.format.extension()
        );
    }
}

/// `leyline verify <library> <file>` (ADR 0153 §4). The exit code is 0 only
/// for *intact*, so a script can gate on it.
pub(crate) fn verify(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, file] = positional.as_slice() else {
        return Err("usage: leyline verify <library> <file>".to_owned());
    };
    let library = open(root)?;
    let describe = |row: &ExportRecord| {
        format!(
            "asset {} version {} revision {}, {} to {}",
            row.asset,
            row.version.map_or("deleted".to_owned(), |v| v.to_string()),
            row.revision.map_or("deleted".to_owned(), |r| r.to_string()),
            row.format,
            row.destination
        )
    };
    match library
        .verify_export(Path::new(file))
        .map_err(|e| e.to_string())?
    {
        ExportVerdict::Intact(row) => {
            println!("intact: {}", describe(&row));
            Ok(())
        }
        ExportVerdict::Modified(row) => {
            println!("modified since export: {}", describe(&row));
            Err("the file is not the one written".to_owned())
        }
        ExportVerdict::Unknown => {
            println!("unknown to this library");
            Err("no export of this library matches the file".to_owned())
        }
    }
}

/// `leyline reproduce <library> <file> <dest-dir>` (ADR 0153 §5). The exit
/// code is 0 only for *bit for bit*: *same pixels* is no failure of the
/// promise, but it is not the file either.
pub(crate) fn reproduce(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, file, destination] = positional.as_slice() else {
        return Err("usage: leyline reproduce <library> <file> <dest-dir>".to_owned());
    };
    let library = open(root)?;
    let verdict = library
        .reproduce_export(Path::new(file), Path::new(destination))
        .map_err(|e| e.to_string())?;
    println!("reproduced: {}", verdict.reproduced.display());
    match verdict.outcome {
        Reproduction::BitForBit => {
            println!("bit for bit");
            Ok(())
        }
        Reproduction::SamePixels => {
            println!("same pixels, different bytes: the photograph's metadata changed since");
            Err("the bytes differ".to_owned())
        }
        Reproduction::PixelsNotCompared => {
            println!("different bytes; pixels not compared (no AVIF decoder)");
            Err("the bytes differ".to_owned())
        }
        Reproduction::DifferentPixels(suspect) => {
            match suspect {
                DifferenceSuspect::Decoder { recorded, loaded } => println!(
                    "different pixels: exported with decoder {recorded}, {loaded} loaded now"
                ),
                DifferenceSuspect::FileModified => {
                    println!("different pixels: the file was changed after the export")
                }
                DifferenceSuspect::PlatformOrToolchain => println!(
                    "different pixels: same decoder; the platform or the toolchain differs"
                ),
            }
            Err("the pixels differ".to_owned())
        }
    }
}

pub(crate) fn export(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(
        args,
        &[
            "preset",
            "format",
            "quality",
            "avif-speed",
            "concurrency",
            "max-edge",
            "metadata",
            "watermark",
            "watermark-anchor",
            "watermark-size",
            "watermark-color",
            "watermark-opacity",
        ],
    )?;
    let [root, destination, ids @ ..] = positional.as_slice() else {
        return Err(
            "usage: leyline export <library> <dest-dir> <version-id>... \
             [--preset <name>] [--format <f>] [--quality <q>] [--max-edge <px>]"
                .to_owned(),
        );
    };
    let versions = version_ids(ids)?;
    let destination = PathBuf::from(destination);
    let library = open(root)?;

    let progress = |done: u64, total: u64| eprint!("\rexporting {done}/{total}");
    let recipe_kind = match options.value("preset") {
        Some(name) => {
            if options.value("format").is_some()
                || options.value("quality").is_some()
                || options.value("avif-speed").is_some()
                || options.value("max-edge").is_some()
                || options.value("metadata").is_some()
                || options.value("watermark").is_some()
            {
                return Err("--preset already defines the recipe; \
                     drop --format/--quality/--avif-speed/--max-edge/--metadata/--watermark"
                    .to_owned());
            }
            let stored = library
                .export_presets()
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|p| p.name == *name)
                .ok_or_else(|| format!("no export preset named {name:?}"))?;
            if let Ok(settings) = ExportSettings::parse(&stored.settings_json) {
                say_when_the_format_drops_metadata(&settings);
            }
            ExportRecipe::Preset(stored.preset)
        }
        None => {
            let settings = recipe(&options)?;
            say_when_the_format_drops_metadata(&settings);
            ExportRecipe::Adhoc(settings)
        }
    };
    let concurrency = match options.value("concurrency") {
        Some(value) => Some(
            value
                .parse::<usize>()
                .map_err(|_| format!("bad concurrency {value:?}"))?,
        ),
        None => None,
    };
    let request = ExportRequest {
        versions,
        recipe: recipe_kind,
        destination_dir: destination,
        concurrency,
    };
    let report = library
        .export(&request, progress)
        .map_err(|e| e.to_string())?;
    eprintln!();
    for exported in &report.exported {
        println!("exported {}", exported.path.display());
    }
    for failed in &report.failed {
        println!("failed   v{}: {}", failed.version, failed.reason);
    }
    if report.failed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} of {} export(s) failed",
            report.failed.len(),
            report.exported.len() + report.failed.len()
        ))
    }
}

pub(crate) fn exports(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, asset] = positional.as_slice() else {
        return Err("usage: leyline exports <library> <asset-id>".to_owned());
    };
    let asset = AssetId::new(
        asset
            .parse()
            .map_err(|_| format!("bad asset id {asset:?}"))?,
    );
    let library = open(root)?;
    let history = library
        .catalog()
        .export_history(asset)
        .map_err(|e| e.to_string())?;
    for record in &history {
        let preset = match record.preset {
            Some(id) => format!("p{id}"),
            None => "-".to_owned(),
        };
        println!(
            "{:13} {:4} {:6} {}",
            record.exported_at, record.format, preset, record.destination
        );
    }
    println!("{} export(s)", history.len());
    Ok(())
}
