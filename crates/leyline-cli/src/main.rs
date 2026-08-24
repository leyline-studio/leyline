//! Command-line client for the Leyline engine.
//!
//! The CLI is a thin client of `leyline-sdk` — it holds no logic of its
//! own, proving the "API before GUI" principle of `docs/engine-api.md` §1:
//! everything Studio will do, these commands already do.

use std::path::{Path, PathBuf};

use leyline_sdk::{
    AssetId, CameraProfile, ColorGrading, ColorGradingZone, ColorLabel, Crop, CurvePoint, Demosaic,
    ExportFormat, ExportRecipe, ExportRequest, ExportSettings, GridQuery, HighlightReconstruction,
    HslBand, ImportOptions, LensCorrection, Library, LocalAdjustment, Lut, Margins, NoiseReduction,
    Orientation, PaperSize, Param, Perspective, PickState, Point, PresetId, PreviewKind,
    PrintRecipe, PrintRequest, PrintSettings, RenderingIntent, ScanOptions, Settings,
    SettingsGroup, Sharpening, ShotRange, SpotRemoval, ToneCurve, Value, VersionId, Watermark,
    WatermarkAnchor, WhiteBalance,
};

const USAGE: &str = "\
Leyline — open-source RAW photo development

Usage:
  leyline new <library> [--name <name>]
  leyline info <library>
  leyline import <library> <source> [--reference] [--flat] [--only <name>]...
                                    --only, répétable, n'importe que ces
                                    fichiers-là parmi ceux que `scan` liste
  leyline scan <library> <source> [--flat]
                                    ce qu'un import prendrait, sans rien écrire
                                    (ADR 0065) ; « = » marque un fichier que la
                                    bibliothèque contient déjà
  leyline tether <library>
  leyline watch <library> <folder>
  leyline ls <library> [--text <query>] [--rating <min>]
               [--camera <name>] [--lens <name>] [--iso <range>]
               [--aperture <range>] [--focal <range>] [--shutter <range>]
                                    shot filters (ADR 0064); a range is written
                                    <min>-<max>, <min>- or -<max>, and speeds
                                    may be fractions: --shutter -1/500
  leyline facets <library>          the bodies, lenses and value ranges the
                                    library actually holds — what the filters
                                    above take
  leyline preview <library> <asset-id> [--kind <thumbnail|small|medium|large|full>]
  leyline export <library> <dest-dir> <version-id>...
                 [--preset <name>] [--format <f>] [--quality <1-100>] [--max-edge <px>]
                 [--avif-speed <1-10>] [--concurrency <n>]
                 [--watermark <text>] [--watermark-anchor <a>]
  leyline preset <library> <name> [--format <f>] [--quality <1-100>] [--max-edge <px>]
                 [--avif-speed <1-10>] [--concurrency <n>]
                 [--watermark <text>] [--watermark-anchor <a>]
  leyline presets <library>
  leyline exports <library> <asset-id>
  leyline print <library> <dest-dir> <version-id>...
                [--preset <name>] [--paper <a4|a3|letter|<w>x<h>mm>] [--orientation <portrait|landscape>]
                [--margins <mm>] [--dpi <n>] [--profile <path>] [--intent <intent>] [--copies <n>]
  leyline print-preset <library> <name> [print options above, minus --preset]
  leyline print-presets <library>
  leyline camera-profile <library> <file.dcp>
  leyline camera-profiles <library>
  leyline lut <library> <file.cube>
  leyline luts <library>
  leyline pair <library>            attache chaque JPEG boîtier au RAW de la même
                                    prise (ADR 0079) ; l'import le fait déjà, ceci
                                    rattrape ce qui a été importé avant
  leyline unpair <library> <asset-id>...
                                    détache, et la photo revient dans la grille
  leyline remove <library> <asset-id>...
                                    takes the photos out of the catalog; the
                                    files are left exactly where they are
  leyline delete <library> <asset-id>... --yes
                                    same, and sends the files (and their .xmp
                                    sidecars) to the system trash
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
  leyline preset-update <library> <name> <version-id> --groups <g,g,...>
                                    redefines the preset from that photo and
                                    bumps its version (ADR 0058)
  leyline preset-reapply <library> <name>
                                    re-applies it to every photo still carrying
                                    an older version of it

Options:
  --reference   Reference files in place instead of copying into Photos/
  --flat        Do not descend into subdirectories
  --format <f>  Export format: jpeg (default), png, tiff, webp, avif
  --avif-speed <1-10>
                AVIF encoder effort, 9 by default (ADR 0067): low is slow and
                thorough, 10 encodes ~6x faster for 0 to 14% more bytes. Trades
                time against size only — the image is the same. Other formats
                ignore it
  --concurrency <n>
                Photos an export batch keeps in flight, 4 by default
                (ADR 0068). Higher is faster up to about 6 and costs ~0.7 GB
                of peak memory per photo at 30 Mpx
  --intent <i>  Print rendering intent: perceptual, relative (default), saturation, absolute
  --watermark <text>
                Text watermark drawn on the export, last thing before encoding (ADR 0034)
  --watermark-anchor <a>
                bottom-right (default), bottom-left, top-right, top-left, center

Develop params (docs/pipeline.md §3.2, schema 1):
  exposure rotation                 decimal
  contrast highlights shadows whites blacks vibrance saturation
  clarity texture dehaze           integer in [-100, 100]
  highlight-rolloff <0-100>         highlight shoulder at the output (0 = clip at white)
  highlight-reconstruction <clip|blend|rebuild>
                                    what the decoder does with channels that saturated at
                                    the sensor (ADR 0050); clip is the neutral default.
                                    Needs a revision pinned at input version 2 — reprocess
                                    an older one first
  demosaic <ahd|vng|dcb|dht>        which interpolation reconstructs the missing
                                    channels (ADR 0061); ahd is the neutral default.
                                    Needs a revision pinned at input version 3 —
                                    reprocess an older one first. Has no effect on
                                    thumbnail/small previews, which decode at half
                                    size and skip interpolation entirely
  white-balance <kelvin> <tint>     tint integer, or `white-balance none` for as-shot
  lut <path|on|off|none>            path is library-relative, as listed by `leyline luts`;
                                    on/off toggles the LUT already referenced, none removes it
  lut-strength <0-100>              how much of the look to apply (ADR 0053)
  lens-correction <on|off>
  noise-reduction <luminance> <color>
  sharpening <amount> <radius>
  crop <x> <y> <width> <height>     percent 0-100, or `crop reset`
  perspective <vertical> <horizontal>
                                    keystone correction, integers in [-100, 100]
                                    (ADR 0052), or `perspective reset`
  tone-curve <x,y> <x,y>...         points in [0,1], strictly increasing x, or `tone-curve reset`
  spot-removal <tx> <ty> <sx> <sy> <radius> <feather> <opacity>
                                    positions/radius percent 0-100, feather/opacity 0-1;
                                    appends one spot, or `spot-removal reset` to clear all
  local-adjustment <json|@file>     appends one masked local adjustment (ADR 0029, ADR 0048),
                                    written exactly as `settings_json` stores it
                                    (docs/pipeline.md §3.2), e.g.
                                    '{\"mask\":{\"type\":\"radial\",\"cx\":0.5,\"cy\":0.5,
                                    \"rx\":0.3,\"ry\":0.3,\"angle\":0,\"feather\":0.5,
                                    \"inverted\":false},\"opacity\":1,
                                    \"adjustments\":{\"exposure\":-0.5}}';
                                    @file reads the payload from a file instead
  local-adjustment rm <index>       removes the adjustment at that index
  local-adjustment reset            removes every local adjustment
  hsl-band <band> <hue> <saturation> <luminance>
                                    band is one of red/orange/yellow/green/aqua/blue/purple/magenta,
                                    each value integer in [-100, 100]
  color-grading <shadows|midtones|highlights> <hue> <saturation> <luminance>
                                    hue integer in [0, 360), saturation in [0, 100], luminance in [-100, 100]
  color-grading balance <n>        integer in [-100, 100]
  color-grading blending <n>       integer in [0, 100]
  color-grading reset              clears every zone and balance/blending
  camera-profile <path|on|off|none>
                                    path is library-relative, as listed by
                                    `leyline camera-profiles`; on/off toggles the
                                    profile already referenced, none removes it
                                    EXPERIMENTAL: DCP colors are not yet validated
                                    against reference renders

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
        Some("scan") => scan(&args[1..]),
        Some("tether") => tether(&args[1..]),
        Some("watch") => watch(&args[1..]),
        Some("ls") => ls(&args[1..]),
        Some("facets") => facets(&args[1..]),
        Some("preview") => preview(&args[1..]),
        Some("export") => export(&args[1..]),
        Some("preset") => preset(&args[1..]),
        Some("presets") => presets(&args[1..]),
        Some("exports") => exports(&args[1..]),
        Some("print") => print_cmd(&args[1..]),
        Some("print-preset") => print_preset(&args[1..]),
        Some("print-presets") => print_presets(&args[1..]),
        Some("camera-profile") => camera_profile(&args[1..]),
        Some("camera-profiles") => camera_profiles(&args[1..]),
        Some("lut") => lut_import(&args[1..]),
        Some("luts") => luts(&args[1..]),
        Some("pair") => pair(&args[1..]),
        Some("unpair") => unpair(&args[1..]),
        Some("remove") => remove(&args[1..]),
        Some("delete") => delete(&args[1..]),
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
        Some("preset-update") => preset_update(&args[1..]),
        Some("preset-reapply") => preset_reapply(&args[1..]),
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
    /// Every occurrence of a repeatable option, in the order given —
    /// `--only a --only b` selects two files (ADR 0065 §5).
    fn all(&self, name: &str) -> Vec<&str> {
        self.values
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
            .collect()
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
    let (positional, options) = parse(args, &["only"])?;
    let [root, source] = positional.as_slice() else {
        return Err(
            "usage: leyline import <library> <source> [--reference] [--flat] [--no-pair] \
             [--only <name>]..."
                .to_owned(),
        );
    };
    let library = open(root)?;
    let source = Path::new(source);
    let import_options = ImportOptions {
        copy_files: !options.switch("reference"),
        recursive: !options.switch("flat"),
        pair_companions: !options.switch("no-pair"),
    };
    let chosen = options.all("only");
    let report = if chosen.is_empty() {
        library.import(source, &import_options, |done, total| {
            eprint!("\rimporting {done}/{total}")
        })
    } else {
        // A selective import (ADR 0065 §5): each `--only` names a file the
        // scan listed, matched on its name so the whole path need not be
        // retyped. A name matching nothing is an error, not a silent
        // no-op — the user asked for that photo.
        let files = select(&library, source, &import_options, &chosen)?;
        library.import_files(source, &files, &import_options, |done, total| {
            eprint!("\rimporting {done}/{total}")
        })
    }
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

/// Resolves `--only` names against what a scan of `source` actually holds.
fn select(
    library: &Library,
    source: &Path,
    options: &ImportOptions,
    chosen: &[&str],
) -> Result<Vec<PathBuf>, String> {
    let candidates = library
        .scan_import(
            source,
            &ScanOptions {
                recursive: options.recursive,
                // Nobody is looking at pictures here.
                thumbnails: false,
            },
            |_, _| {},
        )
        .map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    for name in chosen {
        let found = candidates
            .iter()
            .find(|candidate| candidate.filename == *name)
            .ok_or_else(|| format!("--only {name:?}: no such file under {}", source.display()))?;
        files.push(found.path.clone());
    }
    Ok(files)
}

/// Lists what an import would take, without importing anything (ADR 0065).
fn scan(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &[])?;
    let [root, source] = positional.as_slice() else {
        return Err("usage: leyline scan <library> <source> [--flat]".to_owned());
    };
    let library = open(root)?;
    let candidates = library
        .scan_import(
            Path::new(source),
            &ScanOptions {
                recursive: !options.switch("flat"),
                thumbnails: false,
            },
            |done, total| eprint!("\rscanning {done}/{total}"),
        )
        .map_err(|e| e.to_string())?;
    if !candidates.is_empty() {
        eprintln!();
    }
    for candidate in &candidates {
        println!(
            "{} {:<24} {:>10} {} {}",
            // The mark is the point of the listing: it says which ones
            // `import` would refuse as duplicates.
            if candidate.already_imported { "=" } else { " " },
            candidate.filename,
            candidate.file_size,
            candidate.camera.as_deref().unwrap_or("-"),
            candidate.path.display(),
        );
    }
    let already = candidates.iter().filter(|c| c.already_imported).count();
    println!(
        "{} candidate(s), {already} already in the library",
        candidates.len()
    );
    Ok(())
}

fn ls(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(
        args,
        &[
            "text", "rating", "camera", "lens", "iso", "aperture", "focal", "shutter",
        ],
    )?;
    let [root] = positional.as_slice() else {
        return Err(
            "usage: leyline ls <library> [--text <query>] [--rating <min>]\n\
                    \x20                      [--camera <name>] [--lens <name>]\n\
                    \x20                      [--iso <range>] [--aperture <range>]\n\
                    \x20                      [--focal <range>] [--shutter <range>]"
                .to_owned(),
        );
    };
    let library = open(root)?;
    let query = GridQuery {
        text: options.value("text").map(str::to_owned),
        rating_at_least: options
            .value("rating")
            .map(|r| r.parse().map_err(|_| format!("bad rating {r:?}")))
            .transpose()?,
        camera: options.value("camera").map(str::to_owned),
        lens: options.value("lens").map(str::to_owned),
        iso: shot_range("iso", options.value("iso"))?,
        aperture: shot_range("aperture", options.value("aperture"))?,
        focal_length: shot_range("focal", options.value("focal"))?,
        shutter_speed: shot_range("shutter", options.value("shutter"))?,
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

/// One `--iso`/`--aperture`/`--focal`/`--shutter` value, read by the engine
/// so the CLI and Studio cannot drift on what `1/200-` means.
fn shot_range(name: &str, text: Option<&str>) -> Result<ShotRange, String> {
    match text {
        None => Ok(ShotRange::default()),
        Some(text) => ShotRange::parse(text).map_err(|e| format!("--{name}: {e}")),
    }
}

/// Prints what the library was shot with: the values the `ls` filters take,
/// listed from the catalog itself so nothing has to be guessed (ADR 0064 §3).
fn facets(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root] = positional.as_slice() else {
        return Err("usage: leyline facets <library>".to_owned());
    };
    let library = open(root)?;
    let facets = library.catalog().shot_facets().map_err(|e| e.to_string())?;
    for camera in &facets.cameras {
        println!("camera   {camera}");
    }
    for lens in &facets.lenses {
        println!("lens     {lens}");
    }
    let bounds = |name: &str, range: Option<(f64, f64)>, show: &dyn Fn(f64) -> String| {
        if let Some((min, max)) = range {
            println!("{name:<8} {} – {}", show(min), show(max));
        }
    };
    let plain = |value: f64| format!("{value}");
    bounds("iso", facets.iso, &plain);
    bounds("aperture", facets.aperture, &|value| format!("f/{value}"));
    bounds("focal", facets.focal_length, &|value| format!("{value}mm"));
    // A speed reads as the fraction it was shot at: `1/30`, not
    // `0.03333333333333333s`, which is the same number and no help at all.
    bounds("shutter", facets.shutter_speed, &|value| {
        if value > 0.0 && value < 1.0 {
            format!("1/{}s", (1.0 / value).round())
        } else {
            format!("{value}s")
        }
    });
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
fn asset_ids(ids: &[String]) -> Result<Vec<AssetId>, String> {
    if ids.is_empty() {
        return Err("expected at least one asset id".to_owned());
    }
    ids.iter()
        .map(|id| {
            id.parse()
                .map(AssetId::new)
                .map_err(|_| format!("bad asset id {id:?}"))
        })
        .collect()
}

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

/// Imports a `.dcp` camera profile into the library (ADR 0035). The
/// library-relative path it prints is what `develop <v> camera-profile
/// <path>` takes.
fn camera_profile(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, source] = positional.as_slice() else {
        return Err("usage: leyline camera-profile <library> <file.dcp>".to_owned());
    };
    let imported = open(root)?
        .import_camera_profile(Path::new(source))
        .map_err(|e| e.to_string())?;
    println!("{}  {}", imported.relative_path, imported.checksum);
    // The matrix path is implemented to spec but unvalidated against
    // Adobe's own renders (`docs/pipeline.md` process 11) — say so at the
    // point the user opts in, not only in the docs.
    eprintln!(
        "warning: camera profiles are experimental; their colors have not been validated \
         against reference renders"
    );
    Ok(())
}

fn camera_profiles(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root] = positional.as_slice() else {
        return Err("usage: leyline camera-profiles <library>".to_owned());
    };
    let profiles = open(root)?.camera_profiles().map_err(|e| e.to_string())?;
    for profile in &profiles {
        println!("{}  {}", profile.relative_path, profile.checksum);
    }
    println!("{} camera profile(s)", profiles.len());
    Ok(())
}

/// Imports a `.cube` LUT into the library (ADR 0053 §1).
fn lut_import(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, source] = positional.as_slice() else {
        return Err("usage: leyline lut <library> <file.cube>".to_owned());
    };
    let imported = open(root)?
        .import_lut(Path::new(source))
        .map_err(|e| e.to_string())?;
    println!("{}  {}", imported.relative_path, imported.checksum);
    Ok(())
}

fn luts(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root] = positional.as_slice() else {
        return Err("usage: leyline luts <library>".to_owned());
    };
    let luts = open(root)?.luts().map_err(|e| e.to_string())?;
    for lut in &luts {
        println!("{}  {}", lut.relative_path, lut.checksum);
    }
    println!("{} LUT(s)", luts.len());
    Ok(())
}

/// `remove` — out of the catalog, files untouched (ADR 0060 §1).
/// `pair` — the retroactive pass of ADR 0079 §7.
///
/// An import pairs as it goes; this is for what was imported before, and for
/// a library whose RAW arrived long after its JPEG. Idempotent.
fn pair(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root] = positional.as_slice() else {
        return Err("usage: leyline pair <library>".to_owned());
    };
    let library = open(root)?;
    let paired = library.pair_assets().map_err(|e| e.to_string())?;
    for (master, companion) in &paired {
        let master = library
            .catalog()
            .asset_relative_path(*master)
            .map_err(|e| e.to_string())?;
        let companion = library
            .catalog()
            .asset_relative_path(*companion)
            .map_err(|e| e.to_string())?;
        println!("  {companion} -> {master}");
    }
    println!("{} pair(s)", paired.len());
    Ok(())
}

/// `unpair` — detaches, master or companion (ADR 0079 §6).
fn unpair(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, ids @ ..] = positional.as_slice() else {
        return Err("usage: leyline unpair <library> <asset-id>...".to_owned());
    };
    let assets = asset_ids(ids)?;
    let detached = open(root)?
        .unpair_assets(&assets)
        .map_err(|e| e.to_string())?;
    println!("{detached} photo(s) back in the grid");
    Ok(())
}

fn remove(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, ids @ ..] = positional.as_slice() else {
        return Err("usage: leyline remove <library> <asset-id>...".to_owned());
    };
    let assets = asset_ids(ids)?;
    let report = open(root)?
        .remove_assets(&assets)
        .map_err(|e| e.to_string())?;
    println!(
        "removed {} asset(s) from the catalog; no file was touched",
        report.removed.len()
    );
    Ok(())
}

/// `delete` — out of the catalog *and* to the system trash (ADR 0060 §2).
///
/// `--yes` is mandatory rather than a convenience: this is the one command
/// in the CLI that takes a photo off the user's disk, and a shell has no
/// confirmation dialog to fall back on.
fn delete(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &[])?;
    let [root, ids @ ..] = positional.as_slice() else {
        return Err("usage: leyline delete <library> <asset-id>... --yes\n\
             sends the files to the system trash; use `leyline remove` to \
             keep them"
            .to_owned());
    };
    if !options.switch("yes") {
        return Err("refusing to delete files without --yes".to_owned());
    }
    let assets = asset_ids(ids)?;
    let report = open(root)?
        .delete_assets(&assets)
        .map_err(|e| e.to_string())?;
    println!(
        "removed {} asset(s); {} file(s) sent to the trash",
        report.removed.len(),
        report.trashed.len()
    );
    for (path, reason) in &report.failed {
        eprintln!("could not trash {}: {reason}", path.display());
    }
    if report.failed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} file(s) could not be trashed",
            report.failed.len()
        ))
    }
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

    let library = open(root)?;
    let mut session = library.edit(version).map_err(|e| e.to_string())?;

    let (param, value) = match param.as_str() {
        "exposure" => (Param::Exposure, Value::Float(float_at(0)?)),
        "rotation" => (Param::Rotation, Value::Float(float_at(0)?)),
        "contrast" => (Param::Contrast, Value::Int(int_at(0)?)),
        "highlights" => (Param::Highlights, Value::Int(int_at(0)?)),
        "shadows" => (Param::Shadows, Value::Int(int_at(0)?)),
        "whites" => (Param::Whites, Value::Int(int_at(0)?)),
        "blacks" => (Param::Blacks, Value::Int(int_at(0)?)),
        "clarity" => (Param::Clarity, Value::Int(int_at(0)?)),
        "texture" => (Param::Texture, Value::Int(int_at(0)?)),
        "dehaze" => (Param::Dehaze, Value::Int(int_at(0)?)),
        "highlight-rolloff" => (Param::HighlightRolloff, Value::Int(int_at(0)?)),
        "highlight-reconstruction" => {
            let mode = match at(0)? {
                "clip" | "none" => HighlightReconstruction::Clip,
                "blend" => HighlightReconstruction::Blend,
                "rebuild" => HighlightReconstruction::Rebuild,
                other => {
                    return Err(format!(
                        "unknown highlight reconstruction {other:?}, expected clip/blend/rebuild"
                    ));
                }
            };
            (
                Param::HighlightReconstruction,
                Value::HighlightReconstruction(mode),
            )
        }
        "demosaic" => {
            let algorithm = match at(0)? {
                "ahd" => Demosaic::Ahd,
                "vng" => Demosaic::Vng,
                "dcb" => Demosaic::Dcb,
                "dht" => Demosaic::Dht,
                other => {
                    return Err(format!(
                        "unknown demosaic algorithm {other:?}, expected ahd/vng/dcb/dht"
                    ));
                }
            };
            (Param::Demosaic, Value::Demosaic(algorithm))
        }
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
        "camera-profile" => {
            let profile = match at(0)? {
                "none" | "reset" => None,
                state @ ("on" | "off") => {
                    let mut profile =
                        session.settings().camera_profile.clone().ok_or_else(|| {
                            "no camera profile referenced yet; pass a library-relative path first"
                                .to_owned()
                        })?;
                    profile.enabled = state == "on";
                    Some(profile)
                }
                // The checksum is never typed by hand: it comes from the
                // engine's own listing of what has been imported, so the
                // revision records the bytes that are on disk right now.
                path => {
                    let imported = library
                        .camera_profiles()
                        .map_err(|e| e.to_string())?
                        .into_iter()
                        .find(|p| p.relative_path == path)
                        .ok_or_else(|| {
                            format!(
                                "unknown camera profile {path:?}; \
                                 run `leyline camera-profiles <library>` to list them"
                            )
                        })?;
                    Some(CameraProfile {
                        enabled: true,
                        path: imported.relative_path,
                        checksum: imported.checksum,
                    })
                }
            };
            (Param::CameraProfile, Value::CameraProfile(profile))
        }
        // The same shape as `camera-profile`: the checksum is never typed by
        // hand, it comes from the engine's own listing of what was imported
        // (ADR 0053 §1).
        "lut" => {
            let lut = match at(0)? {
                "none" | "reset" => None,
                state @ ("on" | "off") => {
                    let mut lut = session.settings().lut.clone().ok_or_else(|| {
                        "no LUT referenced yet; pass a library-relative path first".to_owned()
                    })?;
                    lut.enabled = state == "on";
                    Some(lut)
                }
                path => {
                    let imported = library
                        .luts()
                        .map_err(|e| e.to_string())?
                        .into_iter()
                        .find(|l| l.relative_path == path)
                        .ok_or_else(|| {
                            format!(
                                "unknown LUT {path:?}; run `leyline luts <library>` to list them"
                            )
                        })?;
                    // A path given alone keeps whatever dose was already set,
                    // so swapping looks does not silently reset it.
                    let strength = session
                        .settings()
                        .lut
                        .as_ref()
                        .map_or(100, |previous| previous.strength);
                    Some(Lut {
                        enabled: true,
                        path: imported.relative_path,
                        checksum: imported.checksum,
                        strength,
                    })
                }
            };
            (Param::Lut, Value::Lut(lut))
        }
        "lut-strength" => {
            let mut lut = session
                .settings()
                .lut
                .clone()
                .ok_or_else(|| "no LUT referenced yet; pass a path first".to_owned())?;
            lut.strength = int_at(0)?;
            (Param::Lut, Value::Lut(Some(lut)))
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
        "perspective" => {
            let perspective = match at(0)? {
                "none" | "reset" => None,
                _ => Some(Perspective {
                    vertical: int_at(0)?,
                    horizontal: int_at(1)?,
                }),
            };
            (Param::Perspective, Value::Perspective(perspective))
        }
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
        "tone-curve" => {
            let points = if at(0)? == "reset" {
                Vec::new()
            } else {
                rest.iter()
                    .map(|p| {
                        let (x, y) = p
                            .split_once(',')
                            .ok_or_else(|| format!("bad point {p:?}, expected x,y"))?;
                        Ok(CurvePoint {
                            x: x.parse().map_err(|_| format!("bad decimal {x:?}"))?,
                            y: y.parse().map_err(|_| format!("bad decimal {y:?}"))?,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?
            };
            (Param::ToneCurve, Value::ToneCurve(ToneCurve { points }))
        }
        "spot-removal" => {
            let spots = if at(0)? == "reset" {
                Vec::new()
            } else {
                let spot = SpotRemoval {
                    target: Point {
                        x: float_at(0)? / 100.0,
                        y: float_at(1)? / 100.0,
                    },
                    source: Point {
                        x: float_at(2)? / 100.0,
                        y: float_at(3)? / 100.0,
                    },
                    radius: float_at(4)? / 100.0,
                    feather: float_at(5)?,
                    opacity: float_at(6)?,
                };
                let mut spots = session.settings().spot_removal.clone();
                spots.push(spot);
                spots
            };
            (Param::SpotRemoval, Value::SpotRemoval(spots))
        }
        // The only command taking a JSON payload (ADR 0049 §4), and the only
        // one whose argument can stand for several edits at once — so it
        // commits on its own instead of falling through to the single
        // `set` below.
        "local-adjustment" => {
            let updates = local_adjustment_updates(
                at(0)?,
                rest.get(1).map(String::as_str),
                &session.settings().local_adjustments,
            )?;
            for (param, value) in updates {
                session.set(param, value).map_err(|e| e.to_string())?;
            }
            let revision = session.commit().map_err(|e| e.to_string())?;
            drop(session);
            println!("committed revision {revision}");
            return Ok(());
        }
        "hsl-band" => {
            const HSL_BAND_NAMES: [&str; 8] = [
                "red", "orange", "yellow", "green", "aqua", "blue", "purple", "magenta",
            ];
            let name = at(0)?;
            let index = HSL_BAND_NAMES
                .iter()
                .position(|&n| n == name)
                .ok_or_else(|| {
                    format!(
                        "unknown hsl band {name:?}, expected one of {}",
                        HSL_BAND_NAMES.join("/")
                    )
                })?;
            let band = HslBand {
                hue: int_at(1)?,
                saturation: int_at(2)?,
                luminance: int_at(3)?,
            };
            (Param::HslBand(index), Value::HslBand(band))
        }
        "color-grading" => {
            let mut grading = session.settings().color_grading;
            match at(0)? {
                "reset" => grading = ColorGrading::default(),
                "balance" => grading.balance = int_at(1)?,
                "blending" => grading.blending = int_at(1)?,
                zone_name @ ("shadows" | "midtones" | "highlights") => {
                    let zone = ColorGradingZone {
                        hue: int_at(1)?,
                        saturation: int_at(2)?,
                        luminance: int_at(3)?,
                    };
                    match zone_name {
                        "shadows" => grading.shadows = zone,
                        "midtones" => grading.midtones = zone,
                        _ => grading.highlights = zone,
                    }
                }
                other => {
                    return Err(format!(
                        "unknown color-grading target {other:?}, expected shadows/midtones/highlights/balance/blending/reset"
                    ));
                }
            }
            (Param::ColorGrading, Value::ColorGrading(grading))
        }
        other => return Err(format!("unknown develop parameter {other:?}")),
    };

    session.set(param, value).map_err(|e| e.to_string())?;
    let revision = session.commit().map_err(|e| e.to_string())?;
    drop(session);
    println!("committed revision {revision}");
    Ok(())
}

/// Decodes the `local-adjustment` develop argument (ADR 0049 §4) into the
/// edits it stands for, against the adjustments `current`ly stored.
///
/// `action` is `reset`, `rm`, or the payload itself — a serialized
/// [`LocalAdjustment`] exactly as `settings_json` holds it, or `@path` to read
/// that payload from a file (a brush stroke's dabs do not fit on a command
/// line). `argument` is the index `rm` removes, and is ignored otherwise.
///
/// A payload appends, so its index is the current length — the append
/// convention of [`Value::LocalAdjustment`]. `reset` removes from the last
/// index down, since removing an entry shifts every later one.
fn local_adjustment_updates(
    action: &str,
    argument: Option<&str>,
    current: &[LocalAdjustment],
) -> Result<Vec<(Param, Value)>, String> {
    match action {
        "reset" => Ok((0..current.len())
            .rev()
            .map(|i| (Param::LocalAdjustment(i), Value::LocalAdjustment(None)))
            .collect()),
        "rm" => {
            let index =
                argument.ok_or_else(|| "local-adjustment rm expects an index".to_owned())?;
            let index: usize = index.parse().map_err(|_| format!("bad index {index:?}"))?;
            if index >= current.len() {
                return Err(format!(
                    "no local adjustment at index {index}; there {} {}",
                    if current.len() == 1 { "is" } else { "are" },
                    current.len()
                ));
            }
            Ok(vec![(
                Param::LocalAdjustment(index),
                Value::LocalAdjustment(None),
            )])
        }
        payload => {
            let json = match payload.strip_prefix('@') {
                Some(path) => {
                    std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?
                }
                None => payload.to_owned(),
            };
            let adjustment: LocalAdjustment = serde_json::from_str(&json)
                .map_err(|e| format!("bad local adjustment payload: {e}"))?;
            Ok(vec![(
                Param::LocalAdjustment(current.len()),
                Value::LocalAdjustment(Some(adjustment)),
            )])
        }
    }
}

/// Migrates each version to the engine's current stage versions
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
        let stages = settings
            .stages
            .iter()
            .map(|(name, version)| format!("{name}:{version}"))
            .collect::<Vec<_>>()
            .join(" ");
        println!(
            "{marker} r{:<6} exposure {:+.2}  contrast {:+}  (schema {}, stages [{stages}])",
            row.revision, settings.exposure, settings.contrast, settings.schema
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

/// Redefines a preset from a photo's current settings (ADR 0058 §6).
fn preset_update(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &["groups"])?;
    let [root, name, version] = positional.as_slice() else {
        return Err(
            "usage: leyline preset-update <library> <name> <version-id> --groups <g,g,...>"
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
    let preset = find_preset(&library, name)?;
    let revision = library
        .update_preset(preset, version, &groups)
        .map_err(|e| e.to_string())?;
    println!("preset {name:?} is now at version {revision}");
    println!(
        "photos developed with an earlier version keep their pixels; `preset-reapply` moves them forward"
    );
    Ok(())
}

/// Re-applies a preset to the photos still carrying an older version of it
/// (ADR 0058 §6) — ordinary revisions, undoable one by one.
fn preset_reapply(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, name] = positional.as_slice() else {
        return Err("usage: leyline preset-reapply <library> <name>".to_owned());
    };
    let library = open(root)?;
    let preset = find_preset(&library, name)?;
    let current = library.preset(preset).map_err(|e| e.to_string())?.revision;
    let outdated: Vec<VersionId> = library
        .versions_from_preset(preset)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|&(_, revision)| revision < current)
        .map(|(version, _)| version)
        .collect();
    if outdated.is_empty() {
        println!("nothing to do: no photo carries an earlier version of {name:?}");
        return Ok(());
    }
    let report = library
        .apply_preset(preset, &outdated, |done, total| {
            eprint!("\rre-applying {done}/{total}")
        })
        .map_err(|e| e.to_string())?;
    eprintln!();
    println!(
        "{} photo(s) moved to version {current}, {} failed",
        report.applied.len(),
        report.failed.len()
    );
    for failure in &report.failed {
        eprintln!("  {} — {}", failure.version, failure.reason);
    }
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
            "{:<6} {}{:20} v{}  {}",
            preset.preset,
            if preset.favourite { "★ " } else { "  " },
            preset.name,
            preset.revision,
            preset.preset_json
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

/// Connects to a USB camera and imports every shot as it's taken
/// (`docs/adr/0038-tethered-capture.md`), until the camera disconnects or
/// the process is interrupted (Ctrl+C).
fn tether(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root] = positional.as_slice() else {
        return Err("usage: leyline tether <library>".to_owned());
    };
    let library = open(root)?;
    let events = library.subscribe();
    library.tether_connect().map_err(|e| e.to_string())?;
    eprintln!("connected — waiting for shots (Ctrl+C to stop)");
    loop {
        match events.recv() {
            Ok(leyline_sdk::Event::AssetsAdded { asset_ids }) => {
                for asset in asset_ids {
                    println!("captured asset {asset}");
                }
            }
            Ok(leyline_sdk::Event::TetherDisconnected { reason }) => {
                match reason {
                    Some(reason) => eprintln!("disconnected: {reason}"),
                    None => eprintln!("disconnected"),
                }
                return Ok(());
            }
            Ok(_) => {}
            Err(_) => return Ok(()),
        }
    }
}

/// Watches a folder and imports every file that settles there
/// (`docs/adr/0039-watched-folder-import.md`), until the process is
/// interrupted (Ctrl+C).
fn watch(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, folder] = positional.as_slice() else {
        return Err("usage: leyline watch <library> <folder>".to_owned());
    };
    let library = open(root)?;
    let events = library.subscribe();
    library
        .watch_start(Path::new(folder))
        .map_err(|e| e.to_string())?;
    eprintln!("watching {folder} — waiting for files (Ctrl+C to stop)");
    loop {
        match events.recv() {
            Ok(leyline_sdk::Event::AssetsAdded { asset_ids }) => {
                for asset in asset_ids {
                    println!("imported asset {asset}");
                }
            }
            Ok(leyline_sdk::Event::WatchStopped { reason }) => {
                match reason {
                    Some(reason) => eprintln!("stopped: {reason}"),
                    None => eprintln!("stopped"),
                }
                return Ok(());
            }
            Ok(_) => {}
            Err(_) => return Ok(()),
        }
    }
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
    if let Some(speed) = options.value("avif-speed") {
        settings.avif_speed = speed
            .parse()
            .map_err(|_| format!("bad avif speed {speed:?}"))?;
    }
    if let Some(edge) = options.value("max-edge") {
        settings.max_edge = Some(edge.parse().map_err(|_| format!("bad max edge {edge:?}"))?);
    }
    // Only the line, like Studio's dialog: the rest of the decoration keeps
    // the recipe defaults (ADR 0051 §3), and a preset's `settings_json` is
    // where other values are written.
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
        match &mut settings.watermark {
            Some(watermark) => watermark.anchor = anchor,
            None => return Err("--watermark-anchor needs --watermark".to_owned()),
        }
    }
    settings.validate().map_err(|e| e.to_string())?;
    Ok(settings)
}

fn export(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(
        args,
        &[
            "preset",
            "format",
            "quality",
            "avif-speed",
            "concurrency",
            "max-edge",
            "watermark",
            "watermark-anchor",
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
                || options.value("watermark").is_some()
            {
                return Err("--preset already defines the recipe; \
                     drop --format/--quality/--avif-speed/--max-edge/--watermark"
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

const PRINT_FLAGS: &[&str] = &[
    "preset",
    "paper",
    "orientation",
    "margins",
    "dpi",
    "profile",
    "intent",
    "copies",
];

/// Builds a [`PrintSettings`] from the shared recipe flags.
fn print_recipe(options: &Options) -> Result<PrintSettings, String> {
    let mut settings = PrintSettings::default();
    if let Some(paper) = options.value("paper") {
        settings.paper = match paper {
            "a4" => PaperSize::A4,
            "a3" => PaperSize::A3,
            "letter" => PaperSize::Letter,
            custom => {
                let (w, h) = custom
                    .strip_suffix("mm")
                    .and_then(|wh| wh.split_once('x'))
                    .ok_or_else(|| format!("unknown paper size {custom:?}"))?;
                PaperSize::Custom {
                    width_mm: w.parse().map_err(|_| format!("bad paper width {w:?}"))?,
                    height_mm: h.parse().map_err(|_| format!("bad paper height {h:?}"))?,
                }
            }
        };
    }
    if let Some(orientation) = options.value("orientation") {
        settings.orientation = match orientation {
            "portrait" => Orientation::Portrait,
            "landscape" => Orientation::Landscape,
            other => return Err(format!("unknown orientation {other:?}")),
        };
    }
    if let Some(margin) = options.value("margins") {
        let mm: f32 = margin
            .parse()
            .map_err(|_| format!("bad margins {margin:?}"))?;
        settings.margins_mm = Margins {
            top_mm: mm,
            right_mm: mm,
            bottom_mm: mm,
            left_mm: mm,
        };
    }
    if let Some(dpi) = options.value("dpi") {
        settings.dpi = dpi.parse().map_err(|_| format!("bad dpi {dpi:?}"))?;
    }
    if let Some(profile) = options.value("profile") {
        settings.profile = Some(PathBuf::from(profile));
    }
    if let Some(intent) = options.value("intent") {
        settings.intent = match intent {
            "perceptual" => RenderingIntent::Perceptual,
            "relative" => RenderingIntent::RelativeColorimetric,
            "saturation" => RenderingIntent::Saturation,
            "absolute" => RenderingIntent::AbsoluteColorimetric,
            other => return Err(format!("unknown rendering intent {other:?}")),
        };
    }
    Ok(settings)
}

fn print_cmd(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, PRINT_FLAGS)?;
    let [root, destination, ids @ ..] = positional.as_slice() else {
        return Err("usage: leyline print <library> <dest-dir> <version-id>... \
             [--preset <name>] [--paper <p>] [--orientation <o>] [--margins <mm>] \
             [--dpi <n>] [--profile <path>] [--intent <i>] [--copies <n>]"
            .to_owned());
    };
    let versions = version_ids(ids)?;
    let destination = PathBuf::from(destination);
    let library = open(root)?;

    let progress = |done: u64, total: u64| eprint!("\rprinting {done}/{total}");
    let recipe_kind = match options.value("preset") {
        Some(name) => {
            if PRINT_FLAGS
                .iter()
                .filter(|f| **f != "preset" && **f != "copies")
                .any(|f| options.value(f).is_some())
            {
                return Err(
                    "--preset already defines the recipe; drop the other print options".to_owned(),
                );
            }
            let stored = library
                .print_presets()
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|p| p.name == *name)
                .ok_or_else(|| format!("no print preset named {name:?}"))?;
            PrintRecipe::Preset(stored.preset)
        }
        None => PrintRecipe::Adhoc(print_recipe(&options)?),
    };
    let copies = options
        .value("copies")
        .map(|c| c.parse().map_err(|_| format!("bad copies {c:?}")))
        .transpose()?
        .unwrap_or(1);
    let request = PrintRequest {
        versions,
        recipe: recipe_kind,
        destination_dir: destination,
        copies,
    };
    let report = library
        .print(&request, progress)
        .map_err(|e| e.to_string())?;
    eprintln!();
    for printed in &report.printed {
        println!("printed  {}", printed.path.display());
    }
    for failed in &report.failed {
        println!("failed   v{}: {}", failed.version, failed.reason);
    }
    if report.failed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} of {} print(s) failed",
            report.failed.len(),
            report.printed.len() + report.failed.len()
        ))
    }
}

fn print_preset(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(
        args,
        &[
            "paper",
            "orientation",
            "margins",
            "dpi",
            "profile",
            "intent",
        ],
    )?;
    let [root, name] = positional.as_slice() else {
        return Err("usage: leyline print-preset <library> <name> \
             [--paper <p>] [--orientation <o>] [--margins <mm>] [--dpi <n>] \
             [--profile <path>] [--intent <i>]"
            .to_owned());
    };
    let settings = print_recipe(&options)?;
    let library = open(root)?;
    let id = library
        .create_print_preset(name, &settings)
        .map_err(|e| e.to_string())?;
    println!("created print preset {name:?} (p{id})");
    Ok(())
}

fn print_presets(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root] = positional.as_slice() else {
        return Err("usage: leyline print-presets <library>".to_owned());
    };
    let stored = open(root)?.print_presets().map_err(|e| e.to_string())?;
    for preset in &stored {
        println!(
            "p{:<6} {:20} {}",
            preset.preset, preset.name, preset.settings_json
        );
    }
    println!("{} preset(s)", stored.len());
    Ok(())
}

fn preset(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(
        args,
        &[
            "format",
            "quality",
            "avif-speed",
            "max-edge",
            "watermark",
            "watermark-anchor",
        ],
    )?;
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

#[cfg(test)]
mod tests {
    use leyline_sdk::{LocalAdjustmentValues, Mask};

    use super::*;

    /// One stored radial adjustment, as a payload and as a value.
    fn radial_json() -> &'static str {
        r#"{"mask":{"type":"radial","cx":0.5,"cy":0.5,"rx":0.3,"ry":0.2,
            "angle":0,"feather":0.5,"inverted":false},
            "opacity":1,"adjustments":{"exposure":-0.5}}"#
    }

    fn radial() -> LocalAdjustment {
        LocalAdjustment {
            mask: Mask::Radial {
                cx: 0.5,
                cy: 0.5,
                rx: 0.3,
                ry: 0.2,
                angle: 0.0,
                feather: 0.5,
                inverted: false,
            },
            range: None,
            opacity: 1.0,
            adjustments: LocalAdjustmentValues {
                exposure: Some(-0.5),
                ..LocalAdjustmentValues::default()
            },
        }
    }

    #[test]
    fn a_payload_appends_at_the_current_length() {
        assert_eq!(
            local_adjustment_updates(radial_json(), None, &[]),
            Ok(vec![(
                Param::LocalAdjustment(0),
                Value::LocalAdjustment(Some(radial()))
            )])
        );
        let existing = [radial(), radial()];
        assert_eq!(
            local_adjustment_updates(radial_json(), None, &existing),
            Ok(vec![(
                Param::LocalAdjustment(2),
                Value::LocalAdjustment(Some(radial()))
            )])
        );
    }

    #[test]
    fn an_at_prefixed_payload_is_read_from_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mask.json");
        std::fs::write(&file, radial_json()).unwrap();
        assert_eq!(
            local_adjustment_updates(&format!("@{}", file.display()), None, &[]),
            Ok(vec![(
                Param::LocalAdjustment(0),
                Value::LocalAdjustment(Some(radial()))
            )])
        );
        assert!(
            local_adjustment_updates("@no/such/file.json", None, &[])
                .unwrap_err()
                .starts_with("cannot read ")
        );
    }

    #[test]
    fn a_malformed_payload_is_named_rather_than_ignored() {
        let error = local_adjustment_updates("{\"mask\":\"radial\"}", None, &[]).unwrap_err();
        assert!(
            error.starts_with("bad local adjustment payload: "),
            "{error}"
        );
    }

    #[test]
    fn rm_removes_one_index_and_refuses_a_missing_one() {
        assert_eq!(
            local_adjustment_updates("rm", Some("1"), &[radial(), radial()]),
            Ok(vec![(
                Param::LocalAdjustment(1),
                Value::LocalAdjustment(None)
            )])
        );
        assert_eq!(
            local_adjustment_updates("rm", Some("2"), &[radial(), radial()]),
            Err("no local adjustment at index 2; there are 2".to_owned())
        );
        assert_eq!(
            local_adjustment_updates("rm", None, &[radial()]),
            Err("local-adjustment rm expects an index".to_owned())
        );
        assert!(
            local_adjustment_updates("rm", Some("x"), &[radial()])
                .unwrap_err()
                .starts_with("bad index ")
        );
    }

    /// Removing shifts every later index down, so a reset that walked
    /// forwards would run off the end of a shrinking list.
    #[test]
    fn reset_removes_from_the_last_index_down() {
        assert_eq!(
            local_adjustment_updates("reset", None, &[radial(), radial(), radial()]),
            Ok(vec![
                (Param::LocalAdjustment(2), Value::LocalAdjustment(None)),
                (Param::LocalAdjustment(1), Value::LocalAdjustment(None)),
                (Param::LocalAdjustment(0), Value::LocalAdjustment(None)),
            ])
        );
        assert_eq!(local_adjustment_updates("reset", None, &[]), Ok(vec![]));
    }
}
