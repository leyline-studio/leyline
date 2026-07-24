//! Leyline orchestration engine: jobs, events and rendering.
//!
//! The develop renderer lives here: [`render`] turns a decoded RAW image and
//! a revision's settings into pixels, honoring the process version contract
//! of `docs/pipeline.md` §3.3 — every past process version stays rendable
//! forever, each one frozen in its own module (`process1` being the first).
//!
//! Editing goes through [`EditSession`], which owns the coalescence policy
//! of `docs/engine-api.md` §10.1 on top of the catalog's revision mechanics.

mod decode_cache;
mod events;
mod export;
mod import;
mod library;
mod mask;
mod pixels;
mod presets;
mod preview;
mod print;
mod process1;
mod process2;
mod process3;
mod process4;
mod process5;
mod process6;
mod process7;
mod process8;
mod render;
mod reprocess;
mod session;
mod source;
mod xmp;

pub use decode_cache::DecodeCache;
pub use events::{Event, JobResult};
pub use export::{
    ExportRecipe, ExportReport, ExportRequest, ExportedVersion, FailedExport, export_batch,
    export_version,
};
pub use import::{ImportOptions, ImportReport, ImportedFile, SkippedFile, import};
pub use leyline_preview::Rgb8;
pub use library::{CatalogRead, CatalogWrite, Library};
pub use presets::{FailedApply, PresetApplyReport, apply_batch, capture};
pub use preview::{Preview, PreviewFile, preview};
pub use print::{FailedPrint, PrintRecipe, PrintReport, PrintRequest, PrintedVersion};
pub use render::{LensShot, Rendered, lens_shot, render};
pub use reprocess::{FailedReprocess, ReprocessReport, reprocess_batch};
pub use session::{DEFAULT_AMEND_WINDOW, EditSession, Param, Value};
pub use xmp::write_xmp_sidecar;
