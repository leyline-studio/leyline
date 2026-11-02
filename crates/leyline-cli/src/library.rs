//! Commands on the library itself: creation, roots, import and scan, listing,
//! classification (rate, pick, label), pairing, removal, renaming, description.

use leyline_sdk::Sort;

use super::*;

pub(crate) fn new(args: &[String]) -> Result<(), String> {
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

pub(crate) fn info(args: &[String]) -> Result<(), String> {
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

pub(crate) fn import(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &["only"])?;
    let [root, source] = positional.as_slice() else {
        return Err(
            "usage: leyline import <library> <source> [--reference] [--flat] [--no-pair] \
             [--thumbnails] [--only <name>]..."
                .to_owned(),
        );
    };
    let library = open(root)?;
    let source = Path::new(source);
    let import_options = ImportOptions {
        copy_files: !options.switch("reference"),
        recursive: !options.switch("flat"),
        pair_companions: !options.switch("no-pair"),
        // Nobody is looking at pictures here, and this process exits before a
        // background pass could draw one (ADR 0082 §4). `--thumbnails` warms
        // the cache synchronously instead, below.
        thumbnails: false,
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
    // Synchronously, and only when asked: this process exits as soon as it
    // returns, so a background pass would be killed before drawing anything
    // (ADR 0082 §4).
    if options.switch("thumbnails") && !report.imported.is_empty() {
        eprint!("warming thumbnails...");
        let assets: Vec<_> = report
            .imported
            .iter()
            .map(|imported| imported.registered.asset)
            .collect();
        library.warm_thumbnails(&assets);
        eprintln!(" done");
    }
    Ok(())
}

/// Resolves `--only` names against what a scan of `source` actually holds.
pub(crate) fn select(
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
                // `--only` resolves names against the folder; the import
                // that follows asks the fingerprint question itself.
                exact: false,
            },
            |_, _| {},
        )
        .map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    for name in chosen {
        let found = candidates
            .candidates
            .iter()
            .find(|candidate| candidate.filename == *name)
            .ok_or_else(|| format!("--only {name:?}: no such file under {}", source.display()))?;
        files.push(found.path.clone());
    }
    Ok(files)
}

/// Lists what an import would take, without importing anything (ADR 0065).
pub(crate) fn scan(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &[])?;
    let [root, source] = positional.as_slice() else {
        return Err("usage: leyline scan <library> <source> [--flat] [--exact]".to_owned());
    };
    let library = open(root)?;
    let candidates = library
        .scan_import(
            Path::new(source),
            &ScanOptions {
                recursive: !options.switch("flat"),
                thumbnails: false,
                // Reads every file in full, and says so in the help
                // (ADR 0095 §2): asked for, never a surprise.
                exact: options.switch("exact"),
            },
            |done, total| eprint!("\rscanning {done}/{total}"),
        )
        .map_err(|e| e.to_string())?;
    if !candidates.candidates.is_empty() {
        eprintln!();
    }
    for candidate in &candidates.candidates {
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
        // Only the fingerprint can name the asset (ADR 0095 §3), so this
        // line appears under --exact and nowhere else.
        if let Some(asset) = candidate.duplicate_of {
            println!("    ↳ same content as asset {asset}");
        }
    }
    let already = candidates
        .candidates
        .iter()
        .filter(|c| c.already_imported)
        .count();
    println!(
        "{} candidate(s), {already} already in the library",
        candidates.candidates.len()
    );
    Ok(())
}

pub(crate) fn ls(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(
        args,
        &[
            "text",
            "rating",
            "camera",
            "lens",
            "iso",
            "aperture",
            "focal",
            "shutter",
            "collection",
        ],
    )?;
    let [root] = positional.as_slice() else {
        return Err(
            "usage: leyline ls <library> [--text <query>] [--rating <min>]\n\
                    \x20                      [--camera <name>] [--lens <name>]\n\
                    \x20                      [--iso <range>] [--aperture <range>]\n\
                    \x20                      [--focal <range>] [--shutter <range>]\n\
                    \x20                      [--collection <id>]"
                .to_owned(),
        );
    };
    let library = open(root)?;
    let collection = options.value("collection").map(collection_id).transpose()?;
    // A manual collection is listed in the order its owner gave it; a smart
    // one has no order of its own and keeps the grid's (ADR 0169 §1).
    let sort = match collection {
        Some(id)
            if library
                .smart_rules(id)
                .map_err(|e| e.to_string())?
                .is_none() =>
        {
            Sort::CollectionOrder
        }
        _ => GridQuery::default().sort,
    };
    let query = GridQuery {
        collection,
        sort,
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
pub(crate) fn shot_range(name: &str, text: Option<&str>) -> Result<ShotRange, String> {
    match text {
        None => Ok(ShotRange::default()),
        Some(text) => ShotRange::parse(text).map_err(|e| format!("--{name}: {e}")),
    }
}

/// Prints what the library was shot with: the values the `ls` filters take,
/// listed from the catalog itself so nothing has to be guessed (ADR 0064 §3).
pub(crate) fn facets(args: &[String]) -> Result<(), String> {
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

/// Serves the library to an agent over stdin/stdout (ADR 0154). Its MCP
/// client launches this; stdout is the protocol, so nothing else may print.
pub(crate) fn mcp(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &["library"])?;
    let (Some(root), true) = (options.value("library"), positional.is_empty()) else {
        return Err("usage: leyline mcp --library <library>".to_owned());
    };
    leyline_mcp::serve_stdio(Path::new(root))
}

pub(crate) fn preview(args: &[String]) -> Result<(), String> {
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

/// `remove` — out of the catalog, files untouched (ADR 0060 §1).
/// `pair` — the retroactive pass of ADR 0079 §7.
///
/// An import pairs as it goes; this is for what was imported before, and for
/// a library whose RAW arrived long after its JPEG. Idempotent.
pub(crate) fn pair(args: &[String]) -> Result<(), String> {
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
pub(crate) fn unpair(args: &[String]) -> Result<(), String> {
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

pub(crate) fn remove(args: &[String]) -> Result<(), String> {
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
pub(crate) fn delete(args: &[String]) -> Result<(), String> {
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

pub(crate) fn rate(args: &[String]) -> Result<(), String> {
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

pub(crate) fn pick(args: &[String]) -> Result<(), String> {
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

pub(crate) fn label(args: &[String]) -> Result<(), String> {
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

/// Renames files on disk from a template (`docs/adr/0100`).
pub(crate) fn rename(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, template, assets @ ..] = positional.as_slice() else {
        return Err("usage: leyline rename <library> <template> <asset-id>...".to_owned());
    };
    if assets.is_empty() {
        return Err("usage: leyline rename <library> <template> <asset-id>...".to_owned());
    }
    let library = open(root)?;
    let assets = assets
        .iter()
        .map(|id| {
            id.parse()
                .map(AssetId::new)
                .map_err(|_| format!("bad asset id {id:?}"))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let report = library
        .rename(&assets, template)
        .map_err(|e| e.to_string())?;
    for renamed in &report.renamed {
        println!("{} -> {}", renamed.from, renamed.to);
    }
    for failed in &report.failed {
        eprintln!("asset {}: {}", failed.asset, failed.reason);
    }
    println!(
        "{} renamed, {} refused",
        report.renamed.len(),
        report.failed.len()
    );
    // A batch that renamed nothing because every name was refused is a
    // failure the shell should see.
    if report.renamed.is_empty() && !report.failed.is_empty() {
        return Err("nothing was renamed".to_owned());
    }
    Ok(())
}

/// Reads or writes what someone wrote about a photograph
/// (`docs/adr/0099`). Each flag is optional; `--clear` empties the row.
pub(crate) fn describe(args: &[String]) -> Result<(), String> {
    let fields = [
        "title",
        "caption",
        "creator",
        "copyright",
        "credit",
        "city",
        "state",
        "country",
    ];
    let (positional, options) = parse(args, &fields)?;
    let [root, asset] = positional.as_slice() else {
        return Err("usage: leyline describe <library> <asset-id> [--title <t>] ...".to_owned());
    };
    let library = open(root)?;
    let asset = AssetId::new(asset.parse().map_err(|_| "asset id must be a number")?);

    let existing = library
        .catalog()
        .description(asset)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let given: Vec<(&str, &str)> = fields
        .iter()
        .filter_map(|name| options.value(name).map(|value| (*name, value)))
        .collect();

    if given.is_empty() && !options.switch("clear") {
        // Nothing to write: show what is written.
        for (name, value) in [
            ("title", &existing.title),
            ("caption", &existing.caption),
            ("creator", &existing.creator),
            ("copyright", &existing.copyright),
            ("credit", &existing.credit),
            ("city", &existing.city),
            ("state", &existing.state),
            ("country", &existing.country),
        ] {
            if let Some(value) = value {
                println!("{name:<10} {value}");
            }
        }
        return Ok(());
    }

    let mut written = if options.switch("clear") {
        AssetDescription::default()
    } else {
        existing
    };
    for (name, value) in given {
        let value = (!value.is_empty()).then(|| value.to_owned());
        match name {
            "title" => written.title = value,
            "caption" => written.caption = value,
            "creator" => written.creator = value,
            "copyright" => written.copyright = value,
            "credit" => written.credit = value,
            "city" => written.city = value,
            "state" => written.state = value,
            _ => written.country = value,
        }
    }
    library
        .set_description(asset, &written)
        .map_err(|e| e.to_string())?;
    println!("described asset {asset}");
    Ok(())
}

/// Lists the roots a library references, and whether each is reachable now
/// (ADR 0085 §8).
pub(crate) fn roots(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root] = positional.as_slice() else {
        return Err("usage: leyline roots <library>".to_owned());
    };
    let statuses = open(root)?.roots().map_err(|e| e.to_string())?;
    for status in &statuses {
        // "offline" rather than "missing", and the distinction is the point:
        // an unplugged disk is not a deleted photograph (ADR 0085 §5).
        let where_ = match &status.location {
            Some(path) => path.display().to_string(),
            None => "offline".to_owned(),
        };
        println!(
            "r{:<4} {:24} {:38} {}",
            status.root.id, status.root.name, status.root.uuid, where_
        );
    }
    println!("{} root(s)", statuses.len());
    Ok(())
}

/// Adds a folder as a root this library may reference photos inside.
pub(crate) fn root_add(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &["name"])?;
    let [root, folder] = positional.as_slice() else {
        return Err("usage: leyline root-add <library> <folder> [--name <name>]".to_owned());
    };
    let path = std::path::Path::new(folder);
    let name = options.value("name").map(str::to_owned).unwrap_or_else(|| {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| folder.clone())
    });
    let added = open(root)?
        .add_root(path, &name)
        .map_err(|e| e.to_string())?;
    println!("r{} {} {}", added.id, added.name, added.uuid);
    Ok(())
}

/// Tells the library where a root went.
pub(crate) fn root_locate(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, uuid, folder] = positional.as_slice() else {
        return Err("usage: leyline root-locate <library> <uuid> <folder>".to_owned());
    };
    open(root)?
        .locate_root(uuid, std::path::Path::new(folder))
        .map_err(|e| e.to_string())?;
    println!("located {uuid}");
    Ok(())
}

/// Stops referencing a root. Refused while it still holds photographs.
pub(crate) fn root_forget(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, uuid] = positional.as_slice() else {
        return Err("usage: leyline root-forget <library> <uuid>".to_owned());
    };
    open(root)?.forget_root(uuid).map_err(|e| e.to_string())?;
    println!("forgot {uuid}");
    Ok(())
}
