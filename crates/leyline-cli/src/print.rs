//! Print and contact sheet commands, with their presets.

use super::*;

pub(crate) const PRINT_FLAGS: &[&str] = &[
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
pub(crate) fn print_recipe(options: &Options) -> Result<PrintSettings, String> {
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

pub(crate) fn print_cmd(args: &[String]) -> Result<(), String> {
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

pub(crate) fn print_preset(args: &[String]) -> Result<(), String> {
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

pub(crate) fn print_presets(args: &[String]) -> Result<(), String> {
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

/// The page half of a contact sheet is a print's, plus the grid (ADR 0110).
pub(crate) const SHEET_FLAGS: &[&str] = &[
    "preset",
    "paper",
    "orientation",
    "margins",
    "dpi",
    "profile",
    "intent",
    "columns",
    "rows",
    "gutter",
    "captions",
    "caption-size",
];

/// Builds a [`ContactSheetSettings`] from the shared page flags — reused as
/// they are, since a sheet's page *is* a print's (ADR 0110 §1) — plus the
/// grid's own.
pub(crate) fn sheet_recipe(options: &Options) -> Result<ContactSheetSettings, String> {
    let mut settings = ContactSheetSettings::default();
    // A sheet's page defaults are the sheet's, not a print's: only the flags
    // the caller actually passed override them.
    let mut page = settings.page.clone();
    let printed = print_recipe(options)?;
    if options.value("paper").is_some() {
        page.paper = printed.paper;
    }
    if options.value("orientation").is_some() {
        page.orientation = printed.orientation;
    }
    if options.value("margins").is_some() {
        page.margins_mm = printed.margins_mm;
    }
    if options.value("dpi").is_some() {
        page.dpi = printed.dpi;
    }
    if options.value("profile").is_some() {
        page.profile = printed.profile;
    }
    if options.value("intent").is_some() {
        page.intent = printed.intent;
    }
    settings.page = page;

    if let Some(columns) = options.value("columns") {
        settings.columns = columns
            .parse()
            .map_err(|_| format!("bad columns {columns:?}"))?;
    }
    if let Some(rows) = options.value("rows") {
        settings.rows = rows.parse().map_err(|_| format!("bad rows {rows:?}"))?;
    }
    if let Some(gutter) = options.value("gutter") {
        settings.gutter_mm = gutter
            .parse()
            .map_err(|_| format!("bad gutter {gutter:?}"))?;
    }
    if let Some(captions) = options.value("captions") {
        settings.caption = match captions {
            "filename" => CaptionSource::Filename,
            "none" => CaptionSource::None,
            other => return Err(format!("unknown caption source {other:?}")),
        };
    }
    if let Some(size) = options.value("caption-size") {
        settings.caption_mm = size
            .parse()
            .map_err(|_| format!("bad caption size {size:?}"))?;
    }
    Ok(settings)
}

pub(crate) fn contact_sheet_cmd(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, SHEET_FLAGS)?;
    let [root, destination, ids @ ..] = positional.as_slice() else {
        return Err(
            "usage: leyline contact-sheet <library> <sheet.pdf> <version-id>... \
             [--preset <name>] [--paper <p>] [--orientation <o>] [--margins <mm>] \
             [--dpi <n>] [--profile <path>] [--intent <i>] [--columns <n>] \
             [--rows <n>] [--gutter <mm>] [--captions <filename|none>] \
             [--caption-size <mm>]"
                .to_owned(),
        );
    };
    let versions = version_ids(ids)?;
    let destination = PathBuf::from(destination);
    let library = open(root)?;

    let recipe = match options.value("preset") {
        Some(name) => {
            if SHEET_FLAGS
                .iter()
                .filter(|f| **f != "preset")
                .any(|f| options.value(f).is_some())
            {
                return Err(
                    "--preset already defines the recipe; drop the other sheet options".to_owned(),
                );
            }
            let stored = library
                .contact_sheet_presets()
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|p| p.name == *name)
                .ok_or_else(|| format!("no contact sheet preset named {name:?}"))?;
            ContactSheetRecipe::Preset(stored.preset)
        }
        None => ContactSheetRecipe::Adhoc(sheet_recipe(&options)?),
    };

    let progress = |done: u64, total: u64| eprint!("\rrendering {done}/{total}");
    let report = library
        .contact_sheet(
            &ContactSheetRequest {
                versions,
                recipe,
                destination,
            },
            progress,
        )
        .map_err(|e| e.to_string())?;
    eprintln!();
    for failed in &report.failed {
        println!("empty cell for v{}: {}", failed.version, failed.reason);
    }
    println!(
        "wrote {} ({} photo(s) on {} page(s))",
        report.path.display(),
        report.placed,
        report.pages
    );
    Ok(())
}

pub(crate) fn contact_sheet_preset(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(
        args,
        &SHEET_FLAGS[1..], // everything but --preset
    )?;
    let [root, name] = positional.as_slice() else {
        return Err("usage: leyline contact-sheet-preset <library> <name> \
             [--paper <p>] [--orientation <o>] [--margins <mm>] [--dpi <n>] \
             [--profile <path>] [--intent <i>] [--columns <n>] [--rows <n>] \
             [--gutter <mm>] [--captions <filename|none>] [--caption-size <mm>]"
            .to_owned());
    };
    let settings = sheet_recipe(&options)?;
    let library = open(root)?;
    let id = library
        .create_contact_sheet_preset(name, &settings)
        .map_err(|e| e.to_string())?;
    println!("created contact sheet preset {name:?} (p{id})");
    Ok(())
}

pub(crate) fn contact_sheet_presets(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root] = positional.as_slice() else {
        return Err("usage: leyline contact-sheet-presets <library>".to_owned());
    };
    let stored = open(root)?
        .contact_sheet_presets()
        .map_err(|e| e.to_string())?;
    for preset in &stored {
        println!(
            "p{:<6} {:20} {}",
            preset.preset, preset.name, preset.settings_json
        );
    }
    println!("{} preset(s)", stored.len());
    Ok(())
}
