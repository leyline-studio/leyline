//! Command-line client for the Leyline engine.
//!
//! The CLI is a thin client of `leyline-sdk` — it holds no logic of its
//! own, proving the "API before GUI" principle of `docs/engine-api.md` §1:
//! everything Studio will do, these commands already do.

use std::path::{Path, PathBuf};

use leyline_sdk::{
    AssetId, ColorLabel, ExportFormat, ExportSettings, GridQuery, ImportOptions, Library, Param,
    PickState, PreviewKind, Settings, Value, VersionId,
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
                 [--preset <name>] [--png] [--quality <1-100>] [--max-edge <px>]
  leyline preset <library> <name> [--png] [--quality <1-100>] [--max-edge <px>]
  leyline presets <library>
  leyline exports <library> <asset-id>
  leyline rate <library> <stars|none> <version-id>...
  leyline pick <library> <pick|reject|none> <version-id>...
  leyline label <library> <red|yellow|green|blue|purple|none> <version-id>...
  leyline develop <library> <version-id> <param> <value>
  leyline history <library> <version-id>

Options:
  --reference   Reference files in place instead of copying into Photos/
  --flat        Do not descend into subdirectories

Develop params (docs/pipeline.md §3.2, schema 1):
  exposure rotation                 decimal
  contrast highlights shadows whites blacks vibrance saturation
                                    integer in [-100, 100]
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
        Some("history") => history(&args[1..]),
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
    let mut library = open(root)?;
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
    let mut library = open(root)?;
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
    let [root, version, param, value] = positional.as_slice() else {
        return Err("usage: leyline develop <library> <version-id> <param> <value>".to_owned());
    };
    let version = VersionId::new(
        version
            .parse()
            .map_err(|_| format!("bad version id {version:?}"))?,
    );
    let float = |v: &str| -> Result<Value, String> {
        Ok(Value::Float(
            v.parse().map_err(|_| format!("bad decimal {v:?}"))?,
        ))
    };
    let int = |v: &str| -> Result<Value, String> {
        Ok(Value::Int(
            v.parse().map_err(|_| format!("bad integer {v:?}"))?,
        ))
    };
    let (param, value) = match param.as_str() {
        "exposure" => (Param::Exposure, float(value)?),
        "rotation" => (Param::Rotation, float(value)?),
        "contrast" => (Param::Contrast, int(value)?),
        "highlights" => (Param::Highlights, int(value)?),
        "shadows" => (Param::Shadows, int(value)?),
        "whites" => (Param::Whites, int(value)?),
        "blacks" => (Param::Blacks, int(value)?),
        "vibrance" => (Param::Vibrance, int(value)?),
        "saturation" => (Param::Saturation, int(value)?),
        other => return Err(format!("unknown develop parameter {other:?}")),
    };

    let mut library = open(root)?;
    let mut session = library.edit(version).map_err(|e| e.to_string())?;
    session.set(param, value).map_err(|e| e.to_string())?;
    let revision = session.commit().map_err(|e| e.to_string())?;
    drop(session);
    println!("committed revision {revision}");
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

/// Builds an [`ExportSettings`] from the shared recipe flags.
fn recipe(options: &Options) -> Result<ExportSettings, String> {
    let mut settings = ExportSettings::default();
    if options.switch("png") {
        settings.format = ExportFormat::Png;
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
    let (positional, options) = parse(args, &["preset", "quality", "max-edge"])?;
    let [root, destination, ids @ ..] = positional.as_slice() else {
        return Err(
            "usage: leyline export <library> <dest-dir> <version-id>... \
             [--preset <name>] [--png] [--quality <q>] [--max-edge <px>]"
                .to_owned(),
        );
    };
    let versions = version_ids(ids)?;
    let destination = PathBuf::from(destination);
    let mut library = open(root)?;

    let progress = |done: u64, total: u64| eprint!("\rexporting {done}/{total}");
    let report = match options.value("preset") {
        Some(name) => {
            if options.switch("png")
                || options.value("quality").is_some()
                || options.value("max-edge").is_some()
            {
                return Err("--preset already defines the recipe; \
                     drop --png/--quality/--max-edge"
                    .to_owned());
            }
            let stored = library
                .export_presets()
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|p| p.name == *name)
                .ok_or_else(|| format!("no export preset named {name:?}"))?;
            library
                .export_with_preset(&versions, stored.preset, &destination, progress)
                .map_err(|e| e.to_string())?
        }
        None => {
            let settings = recipe(&options)?;
            library
                .export_batch(&versions, &settings, &destination, progress)
                .map_err(|e| e.to_string())?
        }
    };
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
            versions.len()
        ))
    }
}

fn preset(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &["quality", "max-edge"])?;
    let [root, name] = positional.as_slice() else {
        return Err("usage: leyline preset <library> <name> \
             [--png] [--quality <q>] [--max-edge <px>]"
            .to_owned());
    };
    let settings = recipe(&options)?;
    let mut library = open(root)?;
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
