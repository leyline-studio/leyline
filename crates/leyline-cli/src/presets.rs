//! Develop preset commands (`docs/presets.md`).

use super::*;

/// Finds a stored develop preset by name.
pub(crate) fn find_preset(library: &Library, name: &str) -> Result<PresetId, String> {
    library
        .presets()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|p| p.name == name)
        .map(|p| p.preset)
        .ok_or_else(|| format!("no develop preset named {name:?}"))
}

pub(crate) fn preset_save(args: &[String]) -> Result<(), String> {
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
pub(crate) fn preset_update(args: &[String]) -> Result<(), String> {
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
pub(crate) fn preset_reapply(args: &[String]) -> Result<(), String> {
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

pub(crate) fn preset_list(args: &[String]) -> Result<(), String> {
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

pub(crate) fn preset_apply(args: &[String]) -> Result<(), String> {
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

pub(crate) fn preset_rm(args: &[String]) -> Result<(), String> {
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

pub(crate) fn preset(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(
        args,
        &[
            "format",
            "quality",
            "avif-speed",
            "max-edge",
            "metadata",
            "watermark",
            "watermark-anchor",
            "watermark-size",
            "watermark-color",
            "watermark-opacity",
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

pub(crate) fn presets(args: &[String]) -> Result<(), String> {
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
