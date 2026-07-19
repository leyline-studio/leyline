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
mod export;
mod import;
mod library;
mod pixels;
mod preview;
mod process1;
mod render;
mod session;
mod xmp;

pub use decode_cache::DecodeCache;
pub use export::{ExportReport, ExportedVersion, FailedExport, export_batch, export_version};
pub use import::{ImportOptions, ImportReport, ImportedFile, SkippedFile, import};
pub use library::Library;
pub use preview::{PreviewFile, preview};
pub use render::{Rendered, render};
pub use session::{DEFAULT_AMEND_WINDOW, EditSession, Param, Value};
pub use xmp::write_xmp_sidecar;
