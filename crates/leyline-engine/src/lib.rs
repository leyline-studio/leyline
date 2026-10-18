//! Leyline orchestration engine: jobs, events and rendering.
//!
//! The develop renderer lives here: [`render`] turns a decoded RAW image and
//! a revision's settings into pixels, honoring the freeze contract of
//! `docs/pipeline.md` §5.1 — a revision names the version of each stage it
//! renders through (ADR 0042, ADR 0043), and every published stage version
//! stays rendable forever, frozen in its own module.
//!
//! Editing goes through [`EditSession`], which owns the coalescence policy
//! of `docs/engine-api.md` §10.1 on top of the catalog's revision mechanics.

// ADR 0152: no panic on a production path.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]
#![forbid(unsafe_code)]

mod auto_tca;
mod auto_tone;
mod camera_profile;
mod contact_sheet;
mod cull;
mod decode_cache;
mod derive;
mod diff;
mod downscale;
mod events;
mod exif;
mod export;
mod export_metadata;
mod flow;
mod import;
mod keystone;
mod library;
mod lut;
mod mask;
pub mod mask_coverage;
mod mask_overlay;
mod panic_net;
mod pixels;
mod presets;
mod preview;
mod print;
mod provenance;
mod rename;
mod render;
mod reprocess;
mod reproduce;
mod roots;
mod scan;
mod session;
mod source;
mod stages;
mod studio_lock;
mod tether;
mod verify;
mod watch;
mod wb;
mod xmp;

pub use auto_tca::{MIN_SAMPLES as TCA_MIN_SAMPLES, TcaEstimate};
pub use auto_tone::AutoTone;
pub use contact_sheet::{ContactSheetRecipe, ContactSheetReport, ContactSheetRequest};
pub use cull::{
    CullEntry, CullOptions, CullProposal, DEFAULT_BURST_DISTANCE, RejectReason, Verdict,
};
pub use decode_cache::DecodeCache;
pub use diff::changed_settings;
pub use events::{Event, JobResult};
pub use export::{
    ExportRecipe, ExportReport, ExportRequest, ExportedVersion, FailedExport, export_batch,
    export_version,
};
pub use flow::Flow;
pub use import::{ImportOptions, ImportReport, ImportedFile, SkippedFile, import, import_files};
pub use keystone::{GuideLine, KeystoneSolution, MIN_GUIDES as KEYSTONE_MIN_GUIDES};
pub use leyline_catalog::{LIBRARY_ROOT, Root};
pub use leyline_preview::Rgb8;
/// The RAW decoder that produced this process's pixels, e.g. `"0.21.2-Release"`.
///
/// Re-exported here because it is a term of the reproducibility promise
/// (`docs/pipeline.md` §5.1, [ADR 0086]), and the clients that have to show it
/// — the CLI's `--version`, Studio's About dialog — depend on the engine and
/// not on `leyline-raw`.
///
/// [ADR 0086]: https://github.com/leyline-studio/leyline/blob/main/docs/adr/0086-decoder-in-the-promise.md
pub use leyline_raw::decoder_version;
pub use library::{
    CatalogRead, CatalogWrite, ImportedCameraProfile, ImportedLut, Library, RemovalReport,
    RootStatus, SoftProof,
};
pub use panic_net::{install_panic_hook, install_panic_hook_recording};
pub use presets::{FailedApply, PresetApplyReport, apply_batch, capture, overlay};
pub use preview::{Preview, PreviewFile, preview};
pub use print::{FailedPrint, PrintRecipe, PrintReport, PrintRequest, PrintedVersion};
pub use rename::{FailedRename, RenameReport, RenamedAsset};
pub use render::{LensShot, Rendered, SensorShot, lens_shot, render, render_scaled, sensor_shot};
pub use reprocess::{FailedReprocess, ReprocessReport, reprocess_batch};
pub use reproduce::{DifferenceSuspect, ExportMatch, ReproduceVerdict, Reproduction};
pub use scan::{ImportCandidate, ScanOptions, ScanReport, scan};
pub use session::{DEFAULT_AMEND_WINDOW, EditSession, Param, Value, settings_with};
pub use stages::parametric_curve_samples;
pub use stages::{Source, SourceColor, neutral_settings};
pub use studio_lock::{STUDIO_LOCK_FILE, StudioLock};
pub use tether::{DEFAULT_SESSION, TetherOptions, session_folder};
pub use verify::ExportVerdict;
pub use watch::{WatchError, WatchSessionEvent, WatchedFile};
pub use wb::RangeSample;
pub use xmp::{
    XmpSidecar, apply_xmp_sidecar, read_xmp_sidecar, sidecar_candidates, sidecar_path,
    write_xmp_sidecar,
};
