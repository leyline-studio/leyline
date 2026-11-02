//! Commands that measure or propose (auto tone, auto WB, auto TCA, keystone, cull)
//! and those that call an installed detector or processor (ADR 0105, ADR 0107).

use super::*;

/// Proposes a tone for a photo and commits it (ADR 0088 §1).
///
/// Prints what it chose before writing it: a command line is where one
/// checks what Auto decided, and five numbers on stdout are a better answer
/// than a photo that changed.
/// Measures transverse chromatic aberration and writes it into the revision
/// (ADR 0111). Same shape as [`auto_tone`]: a measurement, printed, then an
/// ordinary edit — never an automatic one.
pub(crate) fn auto_tca(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &[])?;
    let [root, version] = positional.as_slice() else {
        return Err("usage: leyline auto-tca <library> <version-id> [--dry-run]".to_owned());
    };
    let library = open(root)?;
    let version = VersionId::new(version.parse().map_err(|_| "version id must be a number")?);
    let estimate = library.estimate_tca(version).map_err(|e| e.to_string())?;
    println!(
        "red {:+.3} %  blue {:+.3} %  ({} edge samples)",
        estimate.red, estimate.blue, estimate.samples
    );
    if estimate.samples < leyline_sdk::TCA_MIN_SAMPLES {
        return Err(format!(
            "not enough usable edges on this photo ({} < {}): try a frame with \
             detail away from the centre",
            estimate.samples,
            leyline_sdk::TCA_MIN_SAMPLES
        ));
    }
    if options.switch("dry-run") {
        return Ok(());
    }
    let mut session = library.edit(version).map_err(|e| e.to_string())?;
    let lens = LensCorrection {
        tca_red: estimate.red,
        tca_blue: estimate.blue,
        ..session.settings().lens_correction.clone()
    };
    session
        .set(Param::LensCorrection, Value::LensCorrection(lens))
        .map_err(|e| e.to_string())?;
    let revision = session.commit().map_err(|e| e.to_string())?;
    println!("committed revision {revision}");
    Ok(())
}

/// ADR 0119: the photographer draws the lines that ought to be straight and
/// the engine answers with the two sliders. Nothing is detected — the lines
/// are the whole input.
pub(crate) fn keystone(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &["line"])?;
    let [root, version] = positional.as_slice() else {
        return Err(
            "usage: leyline keystone <library> <version-id> --line x1,y1,x2,y2 \
             [--line …] [--dry-run]"
                .to_owned(),
        );
    };
    let mut lines = Vec::new();
    for drawn in options.all("line") {
        let numbers: Vec<f64> = drawn
            .split(',')
            .map(|n| n.trim().parse::<f64>())
            .collect::<Result<_, _>>()
            .map_err(|_| format!("--line takes four numbers, got {drawn:?}"))?;
        let [x1, y1, x2, y2] = numbers.as_slice() else {
            return Err(format!(
                "--line takes four numbers x1,y1,x2,y2 in [0, 1], got {drawn:?}"
            ));
        };
        lines.push(GuideLine {
            a: Point { x: *x1, y: *y1 },
            b: Point { x: *x2, y: *y2 },
        });
    }

    let library = open(root)?;
    let version = VersionId::new(version.parse().map_err(|_| "version id must be a number")?);
    let solution = library
        .keystone(version, &lines)
        .map_err(|e| e.to_string())?;
    println!(
        "vertical {:+}  horizontal {:+}  ({:.2}° left)",
        solution.perspective.vertical, solution.perspective.horizontal, solution.residual_degrees
    );
    if options.switch("dry-run") {
        return Ok(());
    }
    let mut session = library.edit(version).map_err(|e| e.to_string())?;
    session
        .set(
            Param::Perspective,
            Value::Perspective(Some(solution.perspective)),
        )
        .map_err(|e| e.to_string())?;
    let revision = session.commit().map_err(|e| e.to_string())?;
    println!("committed revision {revision}");
    Ok(())
}

pub(crate) fn auto_tone(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &[])?;
    let [root, version] = positional.as_slice() else {
        return Err("usage: leyline auto-tone <library> <version-id> [--dry-run]".to_owned());
    };
    let library = open(root)?;
    let version = VersionId::new(version.parse().map_err(|_| "version id must be a number")?);
    let asset = library
        .catalog()
        .version_asset(version)
        .map_err(|e| e.to_string())?;
    let tone = library.auto_tone(asset).map_err(|e| e.to_string())?;
    println!(
        "exposure {:+.2}  highlights {:+}  shadows {:+}  whites {:+}  blacks {:+}",
        tone.exposure, tone.highlights, tone.shadows, tone.whites, tone.blacks
    );
    if options.switch("dry-run") {
        return Ok(());
    }
    let mut session = library.edit(version).map_err(|e| e.to_string())?;
    for (param, value) in [
        (Param::Exposure, Value::Float(tone.exposure)),
        (Param::Highlights, Value::Int(tone.highlights)),
        (Param::Shadows, Value::Int(tone.shadows)),
        (Param::Whites, Value::Int(tone.whites)),
        (Param::Blacks, Value::Int(tone.blacks)),
    ] {
        session.set(param, value).map_err(|e| e.to_string())?;
    }
    let revision = session.commit().map_err(|e| e.to_string())?;
    println!("committed revision {revision}");
    Ok(())
}

/// Proposes a white balance and writes it (`docs/adr/0091`): grey-world
/// over the frame, or the picker on one point with `--sample x,y` in unit
/// coordinates of the rendered image.
pub(crate) fn auto_wb(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &["sample"])?;
    let [root, version] = positional.as_slice() else {
        return Err(
            "usage: leyline auto-wb <library> <version-id> [--sample x,y] [--dry-run]".to_owned(),
        );
    };
    let library = open(root)?;
    let version = VersionId::new(version.parse().map_err(|_| "version id must be a number")?);
    let asset = library
        .catalog()
        .version_asset(version)
        .map_err(|e| e.to_string())?;
    let wb = match options.value("sample") {
        Some(sample) => {
            let (x, y) = sample
                .split_once(',')
                .and_then(|(x, y)| {
                    Some((x.trim().parse::<f64>().ok()?, y.trim().parse::<f64>().ok()?))
                })
                .ok_or_else(|| format!("--sample expects x,y in [0,1], got {sample:?}"))?;
            library
                .neutralize_wb(asset, x, y)
                .map_err(|e| e.to_string())?
        }
        None => library.auto_wb(asset).map_err(|e| e.to_string())?,
    };
    println!("temperature {} K  tint {:+}", wb.temperature, wb.tint);
    if options.switch("dry-run") {
        return Ok(());
    }
    let mut session = library.edit(version).map_err(|e| e.to_string())?;
    session
        .set(Param::WhiteBalance, Value::WhiteBalance(Some(wb)))
        .map_err(|e| e.to_string())?;
    let revision = session.commit().map_err(|e| e.to_string())?;
    println!("committed revision {revision}");
    Ok(())
}

/// Lists the detectors installed for this user (`docs/adr/0073`).
pub(crate) fn detectors(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &["from"])?;
    if !positional.is_empty() {
        return Err("usage: leyline detectors [--from <dir>]".to_owned());
    }
    let sources = installed(&options);
    if sources.is_empty() {
        // Not an error: no detector is shipped with Leyline, so this is
        // the normal state of a fresh installation (ADR 0073).
        println!("no detector installed");
        if options.value("from").is_none()
            && let Some(dir) = leyline_sdk::manifests_dir()
        {
            println!("manifests are read from {}", dir.display());
        }
    } else {
        for source in sources {
            for detection in &source.detections {
                println!(
                    "{}:{:<16} {} · {}",
                    source.id, detection.id, source.label, detection.label
                );
            }
        }
    }
    // A manifest that was read and thrown away is invisible at a launch
    // by design, and the whole defect here: this command is the one an
    // author runs to ask why nothing appears (ADR 0105 §4).
    for rejection in rejected(&options) {
        eprintln!(
            "ignored: {} — {}",
            rejection.path.display(),
            rejection.reason
        );
    }
    Ok(())
}

/// The detectors to consider: those installed for this user, or those in
/// `--from <dir>`. The flag is what lets a detector author try an
/// executable before installing it (ADR 0105 §2).
pub(crate) fn installed(options: &Options) -> Vec<leyline_sdk::DetectorSource> {
    match options.value("from") {
        Some(dir) => leyline_sdk::discover_in(Path::new(dir)),
        None => leyline_sdk::discover(),
    }
}

/// The manifests the same directory holds and discovery declined.
pub(crate) fn rejected(options: &Options) -> Vec<leyline_sdk::Rejection> {
    match options.value("from") {
        Some(dir) => leyline_sdk::rejected_in(Path::new(dir)),
        None => leyline_sdk::manifests_dir()
            .map(|dir| leyline_sdk::rejected_in(&dir))
            .unwrap_or_default(),
    }
}

/// Splits `detector:detection`, the key both detector commands take.
pub(crate) fn detector_key(
    key: &str,
    options: &Options,
) -> Result<(leyline_sdk::DetectorSource, String), String> {
    let (source_id, detection) = key
        .split_once(':')
        .ok_or_else(|| format!("expected <detector>:<detection>, got {key:?}"))?;
    let source = installed(options)
        .into_iter()
        .find(|source| source.id == source_id)
        .ok_or_else(|| format!("no detector named {source_id} is installed"))?;
    Ok((source, detection.to_owned()))
}

/// Runs a detection and appends the coverage it returns as a local
/// adjustment (`docs/adr/0105` §2).
pub(crate) fn detect_cmd(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &["from"])?;
    let [root, version, key] = positional.as_slice() else {
        return Err(
            "usage: leyline detect <library> <version-id> <detector>:<detection> [--from <dir>]"
                .to_owned(),
        );
    };
    let (source, detection) = detector_key(key, &options)?;
    let library = open(root)?;
    let version = VersionId::new(version.parse().map_err(|_| "version id must be a number")?);
    let asset = library
        .catalog()
        .version_asset(version)
        .map_err(|e| e.to_string())?;
    let preview = library
        .preview(asset, PreviewKind::Medium)
        .map_err(|e| e.to_string())?;
    let (width, height, samples) = leyline_sdk::detect_coverage(&source, &detection, &preview.path)
        .map_err(|e| e.to_string())?;
    let mask = library
        .store_mask_coverage(width, height, &samples)
        .map_err(|e| e.to_string())?;

    // Neutral values, like Studio's: the detection chose *where*, the
    // photographer still chooses *what* (ADR 0105 §2).
    let mut session = library.edit(version).map_err(|e| e.to_string())?;
    let index = session.settings().local_adjustments.len();
    let entry = LocalAdjustment {
        mask,
        range: None,
        opacity: 1.0,
        adjustments: LocalAdjustmentValues::default(),
    };
    session
        .set(
            Param::LocalAdjustment(index),
            Value::LocalAdjustment(Some(entry)),
        )
        .map_err(|e| e.to_string())?;
    let revision = session.commit().map_err(|e| e.to_string())?;
    println!("local adjustment {index} carries the detected coverage");
    println!("committed revision {revision}");
    Ok(())
}

/// Checks that a detector speaks the protocol (`docs/adr/0105` §3).
pub(crate) fn detect_check(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &["from"])?;
    let [key] = positional.as_slice() else {
        return Err("usage: leyline detect-check <detector>:<detection> [--from <dir>]".to_owned());
    };
    let (source, detection) = detector_key(key, &options)?;
    let report = leyline_sdk::check_conformance(&source, &detection);
    for failure in &report.failures {
        eprintln!("fail: {failure}");
    }
    for warning in &report.warnings {
        eprintln!("warn: {warning}");
    }
    if report.passed() {
        println!("{key} speaks the protocol");
        // Said every time, so nobody reads a pass as an endorsement.
        println!("(the protocol only — the quality of the segmentation is not checked)");
        return Ok(());
    }
    Err(format!("{key} does not speak the protocol"))
}

/// Goes through a library and proposes what to reject (`docs/adr/0084`).
///
/// Prints the proposal and writes nothing, unless `--apply` is given: the
/// product rule of ADR 0084 §2 is that a photographer sees a culling
/// verdict before it is applied, and a shell is where "sees it" means
/// "reads it". `--apply` is the deliberate second step, not a default.
pub(crate) fn cull(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &["burst-distance"])?;
    let [root, ids @ ..] = positional.as_slice() else {
        return Err(
            "usage: leyline cull <library> [asset-id...] [--burst-distance <bits>] [--apply]"
                .to_owned(),
        );
    };
    let library = open(root)?;
    let burst_distance = match options.value("burst-distance") {
        Some(value) => value
            .parse()
            .map_err(|_| format!("--burst-distance wants a number of bits, got {value:?}"))?,
        None => leyline_sdk::DEFAULT_BURST_DISTANCE,
    };

    // No ids means the whole library: culling is a pass over a shoot, and
    // making the common case type out fifteen thousand numbers would be a
    // joke at the user's expense.
    let assets = if ids.is_empty() {
        library
            .catalog()
            .grid(&GridQuery::default())
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|item| item.asset_id)
            .collect()
    } else {
        ids.iter()
            .map(|id| {
                id.parse()
                    .map(AssetId::new)
                    .map_err(|_| format!("bad asset id {id:?}"))
            })
            .collect::<Result<Vec<_>, String>>()?
    };

    let proposal = library
        .cull(
            &assets,
            &leyline_sdk::CullOptions { burst_distance },
            |done, total| {
                if done == total {
                    println!("measured {done}/{total}");
                }
            },
        )
        .map_err(|e| e.to_string())?;

    for entry in &proposal.entries {
        let verdict = match entry.verdict {
            leyline_sdk::Verdict::Keep => continue,
            leyline_sdk::Verdict::Pick => "keep    (sharpest of its burst)".to_owned(),
            leyline_sdk::Verdict::Reject(reason) => format!("reject  ({})", cull_reason(reason)),
        };
        println!(
            "asset {:<6} version {:<6} burst {:<4} focus {:>7.3}  {verdict}",
            entry.asset.get(),
            entry.version.get(),
            entry.burst,
            entry.quality.focus
        );
    }
    for (asset, reason) in &proposal.skipped {
        eprintln!("skipped asset {}: {reason}", asset.get());
    }

    let rejects = proposal.rejects();
    println!(
        "{} photo(s) in {} burst(s); {} proposed for rejection",
        proposal.entries.len(),
        proposal.bursts(),
        rejects.len()
    );

    if !options.switch("apply") {
        // Said every time: a proposal that looks like a result is the one
        // way this feature could quietly throw a shoot away.
        println!("nothing was written — pass --apply to flag them, or `leyline pick` a subset");
        return Ok(());
    }
    if rejects.is_empty() {
        return Ok(());
    }
    library
        .catalog_mut()
        .set_pick(&rejects, PickState::Reject)
        .map_err(|e| e.to_string())?;
    println!("flagged {} version(s) as rejected", rejects.len());
    Ok(())
}

/// One reason, in the words a shell should print.
pub(crate) fn cull_reason(reason: leyline_sdk::RejectReason) -> String {
    match reason {
        leyline_sdk::RejectReason::Softer { sharper } => {
            format!("asset {} of the same burst is sharper", sharper.get())
        }
        leyline_sdk::RejectReason::Blown => "blown highlights over most of the frame".to_owned(),
        leyline_sdk::RejectReason::Black => "nothing in it — black frame".to_owned(),
    }
}

/// Lists the pixel processors installed for this user (`docs/adr/0107` §7).
pub(crate) fn processors(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &["from"])?;
    if !positional.is_empty() {
        return Err("usage: leyline processors [--from <dir>]".to_owned());
    }
    let sources = processors_installed(&options);
    if sources.is_empty() {
        // The normal state of a fresh installation: no processor ships
        // with Leyline, and the free application is complete without one.
        println!("no pixel processor installed");
        if options.value("from").is_none()
            && let Some(dir) = leyline_sdk::derive::manifests_dir()
        {
            println!("manifests are read from {}", dir.display());
        }
    } else {
        for source in sources {
            for operation in &source.operations {
                println!(
                    "{}:{:<16} {} · {}",
                    source.id, operation.id, source.label, operation.label
                );
            }
        }
    }
    // Same reason as `detectors`: a manifest read and thrown away is
    // invisible by design, and this is the command an author runs to ask
    // why nothing appears (ADR 0105 §4, applied before it could happen
    // twice).
    for rejection in processors_rejected(&options) {
        eprintln!(
            "ignored: {} — {}",
            rejection.path.display(),
            rejection.reason
        );
    }
    Ok(())
}

/// The processors to consider: those installed for this user, or those in
/// `--from <dir>` — what lets an author try an executable before installing
/// it.
pub(crate) fn processors_installed(options: &Options) -> Vec<leyline_sdk::derive::ProcessorSource> {
    match options.value("from") {
        Some(dir) => leyline_sdk::derive::discover_in(Path::new(dir)),
        None => leyline_sdk::derive::discover(),
    }
}

/// The manifests the same directory holds and discovery declined.
pub(crate) fn processors_rejected(options: &Options) -> Vec<leyline_sdk::derive::Rejection> {
    match options.value("from") {
        Some(dir) => leyline_sdk::derive::rejected_in(Path::new(dir)),
        None => leyline_sdk::derive::manifests_dir()
            .map(|dir| leyline_sdk::derive::rejected_in(&dir))
            .unwrap_or_default(),
    }
}

/// Splits `processor:operation`, the key both processor commands take.
pub(crate) fn processor_key(
    key: &str,
    options: &Options,
) -> Result<(leyline_sdk::derive::ProcessorSource, String), String> {
    let (source_id, operation) = key
        .split_once(':')
        .ok_or_else(|| format!("expected <processor>:<operation>, got {key:?}"))?;
    let source = processors_installed(options)
        .into_iter()
        .find(|source| source.id == source_id)
        .ok_or_else(|| format!("no processor named {source_id} is installed"))?;
    Ok((source, operation.to_owned()))
}

/// Runs a pixel processor and files what it returns as a new asset
/// (`docs/adr/0107`).
pub(crate) fn derive_cmd(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &["from"])?;
    let [root, version, key] = positional.as_slice() else {
        return Err(
            "usage: leyline derive <library> <version-id> <processor>:<operation> [--from <dir>]"
                .to_owned(),
        );
    };
    let (source, operation) = processor_key(key, &options)?;
    let library = open(root)?;
    let version = VersionId::new(version.parse().map_err(|_| "version id must be a number")?);
    let derived = library
        .derive(version, &source, &operation)
        .map_err(|e| e.to_string())?;
    let details = library
        .catalog()
        .asset_details(derived)
        .map_err(|e| e.to_string())?;
    println!("asset {derived}: {}", details.relative_path);
    // Said plainly, because it is the decision of ADR 0102 and the thing a
    // user is most likely to misread: this is a new photograph, not a
    // slider that can be dialled back.
    println!("a new asset, carrying the development of the one it was made from");
    Ok(())
}

/// Checks that a pixel processor speaks the protocol (`docs/adr/0107` §8).
pub(crate) fn derive_check(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &["from"])?;
    let [key] = positional.as_slice() else {
        return Err(
            "usage: leyline derive-check <processor>:<operation> [--from <dir>]".to_owned(),
        );
    };
    let (source, operation) = processor_key(key, &options)?;
    let report = leyline_sdk::derive::check_conformance(&source, &operation);
    for failure in &report.failures {
        eprintln!("fail: {failure}");
    }
    for warning in &report.warnings {
        eprintln!("warn: {warning}");
    }
    if report.passed() {
        println!("{key} speaks the protocol");
        // Said every time, so nobody reads a pass as an endorsement.
        println!("(the protocol only — the quality of the processing is not checked)");
        return Ok(());
    }
    Err(format!("{key} does not speak the protocol"))
}
