//! Command-line client for the Leyline engine.
//!
//! The CLI is a thin client of `leyline-sdk` — it holds no logic of its
//! own, proving the "API before GUI" principle of `docs/engine-api.md` §1:
//! everything Studio will do, these commands already do.

use std::path::{Path, PathBuf};

use leyline_sdk::{
    AssetId, ColorLabel, Crop, ExportFormat, ExportRecipe, ExportRequest, ExportSettings,
    GridQuery, ImportOptions, LensCorrection, Library, NoiseReduction, Param, PickState, PresetId,
    PreviewKind, Settings, SettingsGroup, Sharpening, Value, VersionId, WhiteBalance,
};

const USAGE: &str = "\
Leyline — open-source RAW photo development

Usage:
  leyline new <library> [--name <name>]
  leyline info <library>
  leyline import <library> <source> [--reference] [--flat]
  leyline ls <library> [--text <query>] [--rating <min>]
  leyline preview <library> <asset-id> [--kind <thumbnail|small|medium|large|full>]
  leyline export <library> <dest-dir> <version-id>...
                 [--preset <name>] [--format <f>] [--quality <1-100>] [--max-edge <px>]
  leyline preset <library> <name> [--format <f>] [--quality <1-100>] [--max-edge <px>]
  leyline presets <library>
  leyline exports <library> <asset-id>
  leyline rate <library> <stars|none> <version-id>...
  leyline pick <library> <pick|reject|none> <version-id>...
  leyline label <library> <red|yellow|green|blue|purple|none> <version-id>...
  leyline develop <library> <version-id> <param> <value...>
  leyline reprocess <library> <version-id>...
  leyline history <library> <version-id>
  leyline preset-save <library> <name> <version-id> --groups <g,g,...>
  leyline preset-list <library>
  leyline preset-apply <library> <name> <version-id>...
  leyline preset-rm <library> <name>

Options:
  --reference   Reference files in place instead of copying into Photos/
  --flat        Do not descend into subdirectories
  --format <f>  Export format: jpeg (default), png, tiff, webp, avif

Develop params (docs/pipeline.md §3.2, schema 1):
  exposure rotation                 decimal
  contrast highlights shadows whites blacks vibrance saturation
                                    integer in [-100, 100]
  white-balance <kelvin> <tint>     tint integer, or `white-balance none` for as-shot
  lens-correction <on|off>
  noise-reduction <luminance> <color>
  sharpening <amount> <radius>
  crop <x> <y> <width> <height>     percent 0-100, or `crop reset`

Preset groups (docs/presets.md §3.1, comma-separated, no spaces):
  white_balance tone presence lens_correction detail geometry
                                    geometry is never included unless named
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(message) = run(&args) {
        eprintln!("error: {message}");
        std::process::exit(1);
    }
}

/// Dispatches one command; every failure becomes a printable message.
fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("new") => new(&args[1..]),
        Some("info") => info(&args[1..]),
        Some("import") => import(&args[1..]),
        Some("ls") => ls(&args[1..]),
        Some("preview") => preview(&args[1..]),
        Some("export") => export(&args[1..]),
        Some("preset") => preset(&args[1..]),
        Some("presets") => presets(&args[1..]),
        Some("exports") => exports(&args[1..]),
        Some("rate") => rate(&args[1..]),
        Some("pick") => pick(&args[1..]),
        Some("label") => label(&args[1..]),
        Some("develop") => develop(&args[1..]),
        Some("reprocess") => reprocess(&args[1..]),
        Some("history") => history(&args[1..]),
        Some("preset-save") => preset_save(&args[1..]),
        Some("preset-list") => preset_list(&args[1..]),
        Some("preset-apply") => preset_apply(&args[1..]),
        Some("preset-rm") => preset_rm(&args[1..]),
        Some("--help") | Some("help") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command {other:?}\n\n{USAGE}")),
    }
}

/// Splits positional arguments from `--switch` and `--option value`.
fn parse(args: &[String], flags_with_value: &[&str]) -> Result<(Vec<String>, Options), String> {
    let mut positional = Vec::new();
    let mut options = Options::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if let Some(name) = arg.strip_prefix("--") {
            if flags_with_value.contains(&name) {
                let value = it
                    .next()
                    .ok_or_else(|| format!("--{name} expects a value"))?;
                options.values.push((name.to_owned(), value.clone()));
            } else {
                options.switches.push(name.to_owned());
            }
        } else {
            positional.push(arg.clone());
        }
    }
    Ok((positional, options))
}

/// Parsed `--` options of one invocation.
#[derive(Default)]
struct Options {
    switches: Vec<String>,
    values: Vec<(String, String)>,
}

impl Options {
    fn switch(&self, name: &str) -> bool {
        self.switches.iter().any(|s| s == name)
    }
    fn value(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

fn open(root: &str) -> Result<Library, String> {
    Library::open(Path::new(root)).map_err(|e| e.to_string())
}

fn new(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &["name"])?;
    let [root] = positional.as_slice() else {
        return Err("usage: leyline new <library> [--name <name>]".to_owned());
    };
    let name = options
        .value("name")
        .map(str::to_owned)
        .or_else(|| {
            Path::new(root)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "Leyline".to_owned());
    let library = Library::create(Path::new(root), &name).map_err(|e| e.to_string())?;
    println!("created library {:?} at {}", name, library.root().display());
    Ok(())
}

fn info(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root] = positional.as_slice() else {
        return Err("usage: leyline info <library>".to_owned());
    };
    let library = open(root)?;
    let identity = library.catalog().library().map_err(|e| e.to_string())?;
    let assets = library
        .catalog()
        .count(&GridQuery::default())
        .map_err(|e| e.to_string())?;
    let schema = library
        .catalog()
        .user_version()
        .map_err(|e| e.to_string())?;
    println!("name:    {}", identity.name);
    println!("uuid:    {}", identity.uuid);
    println!("schema:  v{schema}");
    println!("assets:  {assets}");
    Ok(())
}

fn import(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &[])?;
    let [root, source] = positional.as_slice() else {
        return Err("usage: leyline import <library> <source> [--reference] [--flat]".to_owned());
    };
    let library = open(root)?;
    let report = library
        .import(
            Path::new(source),
            &ImportOptions {
                copy_files: !options.switch("reference"),
                recursive: !options.switch("flat"),
            },
            |done, total| eprint!("\rimporting {done}/{total}"),
        )
        .map_err(|e| e.to_string())?;
    if !report.imported.is_empty() || !report.skipped.is_empty() {
        eprintln!();
    }
    for imported in &report.imported {
        println!(
            "imported {} (asset {})",
            imported.relative_path, imported.registered.asset
        );
    }
    for skipped in &report.skipped {
        println!("skipped  {}: {}", skipped.path.display(), skipped.reason);
    }
    println!(
        "{} imported, {} skipped",
        report.imported.len(),
        report.skipped.len()
    );
    Ok(())
}

fn ls(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &["text", "rating"])?;
    let [root] = positional.as_slice() else {
        return Err("usage: leyline ls <library> [--text <query>] [--rating <min>]".to_owned());
    };
    let library = open(root)?;
    let query = GridQuery {
        text: options.value("text").map(str::to_owned),
        rating_at_least: options
            .value("rating")
            .map(|r| r.parse().map_err(|_| format!("bad rating {r:?}")))
            .transpose()?,
        ..GridQuery::default()
    };
    let items = library.catalog().grid(&query).map_err(|e| e.to_string())?;
    for item in &items {
        let stars = match item.rating {
            Some(n) => "*".repeat(n as usize),
            None => String::new(),
        };
        println!(
            "v{:<6} a{:<6} {:5} {}",
            item.version_id, item.asset_id, stars, item.filename
        );
    }
    println!("{} version(s)", items.len());
    Ok(())
}

fn preview(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &["kind"])?;
    let [root, asset] = positional.as_slice() else {
        return Err("usage: leyline preview <library> <asset-id> [--kind <k>]".to_owned());
    };
    let asset = AssetId::new(
        asset
            .parse()
            .map_err(|_| format!("bad asset id {asset:?}"))?,
    );
    let kind = match options.value("kind").unwrap_or("small") {
        "thumbnail" => PreviewKind::Thumbnail,
        "small" => PreviewKind::Small,
        "medium" => PreviewKind::Medium,
        "large" => PreviewKind::Large,
        "full" => PreviewKind::Full,
        other => return Err(format!("unknown preview kind {other:?}")),
    };
    let library = open(root)?;
    let file = library.preview(asset, kind).map_err(|e| e.to_string())?;
    let freshness = if file.freshly_generated {
        "generated"
    } else {
        "cached"
    };
    println!(
        "{} ({}x{}, {freshness})",
        file.path.display(),
        file.width,
        file.height
    );
    Ok(())
}

/// Parses the version ids at the tail of a classement command.
fn version_ids(ids: &[String]) -> Result<Vec<VersionId>, String> {
    if ids.is_empty() {
        return Err("expected at least one version id".to_owned());
    }
    ids.iter()
        .map(|id| {
            id.parse()
                .map(VersionId::new)
                .map_err(|_| format!("bad version id {id:?}"))
        })
        .collect()
}

fn rate(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, stars, ids @ ..] = positional.as_slice() else {
        return Err("usage: leyline rate <library> <stars|none> <version-id>...".to_owned());
    };
    let rating = match stars.as_str() {
        "none" => None,
        n => Some(n.parse().map_err(|_| format!("bad rating {n:?}"))?),
    };
    let versions = version_ids(ids)?;
    open(root)?
        .catalog_mut()
        .set_rating(&versions, rating)
        .map_err(|e| e.to_string())?;
    println!("rated {} version(s)", versions.len());
    Ok(())
}

fn pick(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, state, ids @ ..] = positional.as_slice() else {
        return Err("usage: leyline pick <library> <pick|reject|none> <version-id>...".to_owned());
    };
    let state = match state.as_str() {
        "pick" => PickState::Pick,
        "reject" => PickState::Reject,
        "none" => PickState::None,
        other => return Err(format!("unknown pick state {other:?}")),
    };
    let versions = version_ids(ids)?;
    open(root)?
        .catalog_mut()
        .set_pick(&versions, state)
        .map_err(|e| e.to_string())?;
    println!("flagged {} version(s)", versions.len());
    Ok(())
}

fn label(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, color, ids @ ..] = positional.as_slice() else {
        return Err(
            "usage: leyline label <library> <red|yellow|green|blue|purple|none> <version-id>..."
                .to_owned(),
        );
    };
    let color = match color.as_str() {
        "red" => Some(ColorLabel::Red),
        "yellow" => Some(ColorLabel::Yellow),
        "green" => Some(ColorLabel::Green),
        "blue" => Some(ColorLabel::Blue),
        "purple" => Some(ColorLabel::Purple),
        "none" => None,
        other => return Err(format!("unknown color label {other:?}")),
    };
    let versions = version_ids(ids)?;
    open(root)?
        .catalog_mut()
        .set_color_label(&versions, color)
        .map_err(|e| e.to_string())?;
    println!("labeled {} version(s)", versions.len());
    Ok(())
}

fn develop(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, version, param, rest @ ..] = positional.as_slice() else {
        return Err("usage: leyline develop <library> <version-id> <param> <value...>".to_owned());
    };
    let version = VersionId::new(
        version
            .parse()
            .map_err(|_| format!("bad version id {version:?}"))?,
    );
    let at = |i: usize| -> Result<&str, String> {
        rest.get(i)
            .map(String::as_str)
            .ok_or_else(|| format!("{param} expects {} value(s)", i + 1))
    };
    let float_at = |i: usize| -> Result<f64, String> {
        let v = at(i)?;
        v.parse().map_err(|_| format!("bad decimal {v:?}"))
    };
    let int_at = |i: usize| -> Result<i32, String> {
        let v = at(i)?;
        v.parse().map_err(|_| format!("bad integer {v:?}"))
    };
    let (param, value) = match param.as_str() {
        "exposure" => (Param::Exposure, Value::Float(float_at(0)?)),
        "rotation" => (Param::Rotation, Value::Float(float_at(0)?)),
        "contrast" => (Param::Contrast, Value::Int(int_at(0)?)),
        "highlights" => (Param::Highlights, Value::Int(int_at(0)?)),
        "shadows" => (Param::Shadows, Value::Int(int_at(0)?)),
        "whites" => (Param::Whites, Value::Int(int_at(0)?)),
        "blacks" => (Param::Blacks, Value::Int(int_at(0)?)),
        "vibrance" => (Param::Vibrance, Value::Int(int_at(0)?)),
        "saturation" => (Param::Saturation, Value::Int(int_at(0)?)),
        "white-balance" => {
            let wb = match at(0)? {
                "none" => None,
                _ => Some(WhiteBalance {
                    temperature: int_at(0)?
                        .try_into()
                        .map_err(|_| "temperature must be positive".to_owned())?,
                    tint: int_at(1)?,
                }),
            };
            (Param::WhiteBalance, Value::WhiteBalance(wb))
        }
        "lens-correction" => {
            let enabled = match at(0)? {
                "on" => true,
                "off" => false,
                other => return Err(format!("expected on/off, got {other:?}")),
            };
            (
                Param::LensCorrection,
                Value::LensCorrection(LensCorrection {
                    enabled,
                    profile: "auto".to_owned(),
                }),
            )
        }
        "noise-reduction" => (
            Param::NoiseReduction,
            Value::NoiseReduction(NoiseReduction {
                luminance: int_at(0)?,
                color: int_at(1)?,
            }),
        ),
        "sharpening" => (
            Param::Sharpening,
            Value::Sharpening(Sharpening {
                amount: int_at(0)?,
                radius: float_at(1)?,
            }),
        ),
        "crop" => {
            let crop = match at(0)? {
                "none" | "reset" => None,
                _ => Some(Crop {
                    x: float_at(0)? / 100.0,
                    y: float_at(1)? / 100.0,
                    width: float_at(2)? / 100.0,
                    height: float_at(3)? / 100.0,
                }),
            };
            (Param::Crop, Value::Crop(crop))
        }
        other => return Err(format!("unknown develop parameter {other:?}")),
    };

    let library = open(root)?;
    let mut session = library.edit(version).map_err(|e| e.to_string())?;
    session.set(param, value).map_err(|e| e.to_string())?;
    let revision = session.commit().map_err(|e| e.to_string())?;
    drop(session);
    println!("committed revision {revision}");
    Ok(())
}

/// Migrates each version to the engine's current process version
/// (`docs/engine-api.md` §10.4): same parameter values, re-rendered under a
/// newer process contract (e.g. picking up lens correction).
fn reprocess(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, versions @ ..] = positional.as_slice() else {
        return Err("usage: leyline reprocess <library> <version-id>...".to_owned());
    };
    if versions.is_empty() {
        return Err("usage: leyline reprocess <library> <version-id>...".to_owned());
    }
    let ids = versions
        .iter()
        .map(|v| {
            v.parse()
                .map(VersionId::new)
                .map_err(|_| format!("bad version id {v:?}"))
        })
        .collect::<Result<Vec<_>, String>>()?;

    let library = open(root)?;
    let report = library
        .reprocess(&ids, |_, _| {})
        .map_err(|e| e.to_string())?;
    println!(
        "reprocessed {}, already current {}, failed {}",
        report.reprocessed.len(),
        report.already_current.len(),
        report.failed.len()
    );
    for failed in &report.failed {
        println!("  failed: version {} — {}", failed.version, failed.reason);
    }
    Ok(())
}

fn history(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, version] = positional.as_slice() else {
        return Err("usage: leyline history <library> <version-id>".to_owned());
    };
    let version = VersionId::new(
        version
            .parse()
            .map_err(|_| format!("bad version id {version:?}"))?,
    );
    let library = open(root)?;
    let chain = library
        .catalog()
        .version_history(version)
        .map_err(|e| e.to_string())?;
    for (index, row) in chain.iter().enumerate() {
        let marker = if index == 0 { "HEAD" } else { "    " };
        let settings = Settings::parse(&row.settings_json).map_err(|e| e.to_string())?;
        println!(
            "{marker} r{:<6} exposure {:+.2}  contrast {:+}  (schema {}, process {})",
            row.revision, settings.exposure, settings.contrast, settings.schema, settings.process
        );
    }
    Ok(())
}

/// Parses a comma-separated list of preset group names (`docs/presets.md` §3.1).
fn groups(value: &str) -> Result<Vec<SettingsGroup>, String> {
    value
        .split(',')
        .map(|name| match name {
            "white_balance" => Ok(SettingsGroup::WhiteBalance),
            "tone" => Ok(SettingsGroup::Tone),
            "presence" => Ok(SettingsGroup::Presence),
            "lens_correction" => Ok(SettingsGroup::LensCorrection),
            "detail" => Ok(SettingsGroup::Detail),
            "geometry" => Ok(SettingsGroup::Geometry),
            other => Err(format!("unknown preset group {other:?}")),
        })
        .collect()
}

/// Finds a stored develop preset by name.
fn find_preset(library: &Library, name: &str) -> Result<PresetId, String> {
    library
        .presets()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.preset)
        .ok_or_else(|| format!("no develop preset named {name:?}"))
}

fn preset_save(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &["groups"])?;
    let [root, name, version] = positional.as_slice() else {
        return Err(
            "usage: leyline preset-save <library> <name> <version-id> --groups <g,g,...>"
                .to_owned(),
        );
    };
    let version = VersionId::new(
        version
            .parse()
            .map_err(|_| format!("bad version id {version:?}"))?,
    );
    let groups = groups(
        options
            .value("groups")
            .ok_or("--groups is required, e.g. --groups tone,presence")?,
    )?;
    let library = open(root)?;
    let id = library
        .create_preset(name, version, &groups)
        .map_err(|e| e.to_string())?;
    println!("created preset {name:?} ({id})");
    Ok(())
}

fn preset_list(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root] = positional.as_slice() else {
        return Err("usage: leyline preset-list <library>".to_owned());
    };
    let stored = open(root)?.presets().map_err(|e| e.to_string())?;
    for preset in &stored {
        println!(
            "{:<6} {:20} {}",
            preset.preset, preset.name, preset.preset_json
        );
    }
    println!("{} preset(s)", stored.len());
    Ok(())
}

fn preset_apply(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, name, ids @ ..] = positional.as_slice() else {
        return Err("usage: leyline preset-apply <library> <name> <version-id>...".to_owned());
    };
    let library = open(root)?;
    let preset = find_preset(&library, name)?;
    let versions = version_ids(ids)?;
    let report = library
        .apply_preset(preset, &versions, |done, total| {
            eprint!("\rapplying {done}/{total}")
        })
        .map_err(|e| e.to_string())?;
    eprintln!();
    for version in &report.applied {
        println!("applied  v{version}");
    }
    for failed in &report.failed {
        println!("failed   v{}: {}", failed.version, failed.reason);
    }
    if report.failed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} of {} application(s) failed",
            report.failed.len(),
            versions.len()
        ))
    }
}

fn preset_rm(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, name] = positional.as_slice() else {
        return Err("usage: leyline preset-rm <library> <name>".to_owned());
    };
    let library = open(root)?;
    let preset = find_preset(&library, name)?;
    library.delete_preset(preset).map_err(|e| e.to_string())?;
    println!("deleted preset {name:?}");
    Ok(())
}

/// Builds an [`ExportSettings`] from the shared recipe flags.
fn recipe(options: &Options) -> Result<ExportSettings, String> {
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
    if let Some(edge) = options.value("max-edge") {
        settings.max_edge = Some(edge.parse().map_err(|_| format!("bad max edge {edge:?}"))?);
    }
    Ok(settings)
}

fn export(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &["preset", "format", "quality", "max-edge"])?;
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
                || options.value("max-edge").is_some()
            {
                return Err("--preset already defines the recipe; \
                     drop --format/--quality/--max-edge"
                    .to_owned());
            }
            let stored = library
                .export_presets()
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|p| p.name == *name)
                .ok_or_else(|| format!("no export preset named {name:?}"))?;
            ExportRecipe::Preset(stored.preset)
        }
        None => ExportRecipe::Adhoc(recipe(&options)?),
    };
    let request = ExportRequest {
        versions,
        recipe: recipe_kind,
        destination_dir: destination,
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

fn preset(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &["format", "quality", "max-edge"])?;
    let [root, name] = positional.as_slice() else {
        return Err("usage: leyline preset <library> <name> \
             [--format <f>] [--quality <q>] [--max-edge <px>]"
            .to_owned());
    };
    let settings = recipe(&options)?;
    let library = open(root)?;
    let id = library
        .create_export_preset(name, &settings)
        .map_err(|e| e.to_string())?;
    println!("created preset {name:?} (p{id})");
    Ok(())
}

fn presets(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root] = positional.as_slice() else {
        return Err("usage: leyline presets <library>".to_owned());
    };
    let stored = open(root)?.export_presets().map_err(|e| e.to_string())?;
    for preset in &stored {
        println!(
            "p{:<6} {:20} {}",
            preset.preset, preset.name, preset.settings_json
        );
    }
    println!("{} preset(s)", stored.len());
    Ok(())
}

fn exports(args: &[String]) -> Result<(), String> {
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
