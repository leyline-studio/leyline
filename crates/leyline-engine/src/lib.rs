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

mod camera_profile;
mod decode_cache;
mod downscale;
mod events;
mod export;
mod import;
mod library;
mod lut;
mod mask;
mod pixels;
mod presets;
mod preview;
mod print;
mod render;
mod reprocess;
mod session;
mod source;
mod stages;
mod watch;
mod xmp;

pub use decode_cache::DecodeCache;
pub use events::{Event, JobResult};
pub use export::{
    ExportRecipe, ExportReport, ExportRequest, ExportedVersion, FailedExport, export_batch,
    export_version,
};
pub use import::{ImportOptions, ImportReport, ImportedFile, SkippedFile, import};
pub use leyline_preview::Rgb8;
pub use library::{
    CatalogRead, CatalogWrite, ImportedCameraProfile, ImportedLut, Library, SoftProof,
};
pub use presets::{FailedApply, PresetApplyReport, apply_batch, capture};
pub use preview::{Preview, PreviewFile, preview};
pub use print::{FailedPrint, PrintRecipe, PrintReport, PrintRequest, PrintedVersion};
pub use render::{LensShot, Rendered, lens_shot, render, render_scaled};
pub use reprocess::{FailedReprocess, ReprocessReport, reprocess_batch};
pub use session::{DEFAULT_AMEND_WINDOW, EditSession, Param, Value};
pub use stages::{SourceColor, neutral_settings};
pub use watch::{WatchError, WatchSessionEvent, WatchedFile};
pub use xmp::{XmpSidecar, apply_xmp_sidecar, read_xmp_sidecar, sidecar_path, write_xmp_sidecar};
