//! Command-line client for the Leyline engine.
//!
//! The CLI is a thin client of `leyline-sdk` — it holds no logic of its
//! own, proving the "API before GUI" principle of `docs/engine-api.md` §1:
//! everything Studio will do, these commands already do.

// ADR 0152: no panic on a production path; `unsafe` stays allowed in this crate (§3).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use leyline_sdk::{
    AssetDescription, AssetId, CameraProfile, CameraSettings, CaptionSource, ColorGrading,
    ColorGradingZone, ColorLabel, ContactSheetRecipe, ContactSheetRequest, ContactSheetSettings,
    Crop, CurvePoint, Defringe, Demosaic, DifferenceSuspect, ExportFormat, ExportMetadata,
    ExportRecipe, ExportRecord, ExportRequest, ExportSettings, ExportVerdict, GridQuery, GuideLine,
    HighlightReconstruction, HslBand, ImportOptions, LensCorrection, Levels, Library,
    LocalAdjustment, LocalAdjustmentValues, Lut, Margins, NoiseReduction, Orientation, PaperSize,
    Param, Perspective, PickState, Point, PresetId, PreviewKind, PrintRecipe, PrintRequest,
    PrintSettings, RedEye, RenderingIntent, Reproduction, ReshapePoint, RevisionOrigin,
    ScanOptions, Settings, SettingsGroup, Sharpening, ShotRange, SpotRemoval, TetherOptions,
    TetherSetting, Value, VersionId, Watermark, WatermarkAnchor, WhiteBalance,
};

// One module per family of commands; each reads the shared parser, `open` and
// the SDK names through `use super::*`, and this file only dispatches.
mod assist;
mod capture;
mod develop;
mod export;
mod library;
mod presets;
mod print;
mod usage;

use assist::*;
use capture::*;
use develop::*;
use export::*;
use library::*;
use presets::*;
use print::*;
use usage::USAGE;

/// Restores the default disposition of `SIGPIPE`.
///
/// Rust ignores that signal at startup, so a `println!` into a pipe whose
/// reader has gone away returns `EPIPE`, and the macro's `unwrap` turns that
/// into a panic — `leyline ls <library> | head` printed a backtrace where
/// every other Unix tool exits quietly. Restoring the default makes the
/// process die from the signal, which is what a pipeline expects and what
/// `head` is counting on.
///
/// Unix only: Windows has no `SIGPIPE`, and a closed pipe surfaces there as
/// an ordinary write error.
#[cfg(unix)]
fn restore_sigpipe() {
    // SAFETY: `signal` with `SIG_DFL` on a signal this process has not
    // installed a Rust handler for; it touches no memory and is the
    // documented way to undo the runtime's startup `SIG_IGN`.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn restore_sigpipe() {}

fn main() {
    leyline_sdk::install_panic_hook();
    restore_sigpipe();
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
        Some("auto-tone") => auto_tone(&args[1..]),
        Some("auto-tca") => auto_tca(&args[1..]),
        Some("keystone") => keystone(&args[1..]),
        Some("auto-wb") => auto_wb(&args[1..]),
        Some("sample-range") => sample_range(&args[1..]),
        Some("describe") => describe(&args[1..]),
        Some("detectors") => detectors(&args[1..]),
        Some("detect") => detect_cmd(&args[1..]),
        Some("detect-check") => detect_check(&args[1..]),
        Some("cull") => cull(&args[1..]),
        Some("processors") => processors(&args[1..]),
        Some("derive") => derive_cmd(&args[1..]),
        Some("derive-check") => derive_check(&args[1..]),
        Some("rename") => rename(&args[1..]),
        Some("versions") => versions(&args[1..]),
        Some("version-create") => version_create(&args[1..]),
        Some("version-switch") => version_switch(&args[1..]),
        Some("tether") => tether(&args[1..]),
        Some("watch") => watch(&args[1..]),
        Some("ls") => ls(&args[1..]),
        Some("facets") => facets(&args[1..]),
        Some("mcp") => mcp(&args[1..]),
        Some("preview") => preview(&args[1..]),
        Some("export") => export(&args[1..]),
        Some("verify") => verify(&args[1..]),
        Some("reproduce") => reproduce(&args[1..]),
        Some("preset") => preset(&args[1..]),
        Some("presets") => presets(&args[1..]),
        Some("roots") => roots(&args[1..]),
        Some("root-add") => root_add(&args[1..]),
        Some("root-locate") => root_locate(&args[1..]),
        Some("root-forget") => root_forget(&args[1..]),
        Some("exports") => exports(&args[1..]),
        Some("print") => print_cmd(&args[1..]),
        Some("print-preset") => print_preset(&args[1..]),
        Some("print-presets") => print_presets(&args[1..]),
        Some("contact-sheet") => contact_sheet_cmd(&args[1..]),
        Some("contact-sheet-preset") => contact_sheet_preset(&args[1..]),
        Some("contact-sheet-presets") => contact_sheet_presets(&args[1..]),
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
        Some("--version") | Some("version") => {
            print!("{}", version_report());
            Ok(())
        }
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
    let library = Library::open(Path::new(root)).map_err(|e| e.to_string())?;
    library.declare_origin(RevisionOrigin::Cli);
    Ok(library)
}

/// What this build is, and what it renders with.
///
/// The decoder line is not a courtesy: LibRaw is linked dynamically
/// (ADR 0004), so the library that answers here is a property of *this*
/// machine, and `docs/pipeline.md` §5.1 counts it among the terms that must
/// match for two renders to agree bit for bit (ADR 0086). It is the one fact
/// about a render that a user cannot otherwise discover, and the first thing
/// worth having in a report of "the pixels changed".
fn version_report() -> String {
    format!(
        "leyline {}\ndecoder  LibRaw {}\n",
        env!("CARGO_PKG_VERSION"),
        leyline_sdk::decoder_version(),
    )
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
