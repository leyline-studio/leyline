//! Events and job outcomes (`docs/engine-api.md` §3.2).
//!
//! Events are **notifications, never complete data**: a client re-queries
//! what it needs, so the stream and the catalog can never disagree. Each
//! subscriber owns a standard mpsc channel; a dropped receiver silently
//! unsubscribes at the next emission.

use std::path::PathBuf;

use leyline_core::{AssetId, JobId, PreviewKind, VersionId};

use crate::export::ExportReport;
use crate::import::ImportReport;
use crate::presets::PresetApplyReport;
use crate::preview::PreviewFile;
use crate::print::PrintReport;
use crate::reprocess::ReprocessReport;
use crate::scan::ImportCandidate;

/// A notification from the engine (`docs/engine-api.md` §3.2).
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// New assets entered the catalog (import).
    AssetsAdded {
        /// The assets, in import order.
        asset_ids: Vec<AssetId>,
    },
    /// Existing assets changed (metadata, keywords...).
    AssetsChanged {
        /// The assets touched.
        asset_ids: Vec<AssetId>,
    },
    /// Assets left the catalog (ADR 0060), whether or not their files were
    /// also sent to the trash. Any view still showing them — grid,
    /// filmstrip, map — holds dead rows until it acts on this.
    AssetsRemoved {
        /// The assets that actually existed and were removed.
        asset_ids: Vec<AssetId>,
    },
    /// A version's develop state moved (commit, undo, redo).
    VersionChanged {
        /// The version touched.
        version_id: VersionId,
    },
    /// A preview file is ready in the cache for the asset's current version.
    PreviewReady {
        /// The asset rendered.
        asset_id: AssetId,
        /// The size class rendered.
        kind: PreviewKind,
    },
    /// A job advanced: `done` out of `total` units.
    JobProgress {
        /// The job reporting.
        job_id: JobId,
        /// Units completed so far.
        done: u64,
        /// Total units of the job.
        total: u64,
    },
    /// A job ended, successfully or not.
    JobFinished {
        /// The job that ended.
        job_id: JobId,
        /// What it produced.
        result: JobResult,
    },
    /// The library handle was closed by its owner.
    LibraryClosed,
    /// A tether session (`docs/adr/0038`) connected to a USB camera.
    /// Captured shots arrive as ordinary `AssetsAdded` — a tethered import
    /// is not a distinct kind of event, just a distinct source.
    TetherConnected,
    /// The tether session ended: a clean `stop` (`reason: None`) or an
    /// unplug/transport error (`reason: Some`).
    TetherDisconnected {
        /// Human-readable cause, absent for a caller-initiated stop.
        reason: Option<String>,
    },
    /// A watched-folder session (`docs/adr/0039`) started on `folder`.
    /// Settled files arrive as ordinary `AssetsAdded` — a watched-folder
    /// import is not a distinct kind of event, just a distinct source.
    WatchStarted {
        /// The folder now being watched.
        folder: PathBuf,
    },
    /// The watched-folder session ended: a clean `watch_stop`
    /// (`reason: None`) or the OS watcher failing unexpectedly (`Some`).
    WatchStopped {
        /// Human-readable cause, absent for a caller-initiated stop.
        reason: Option<String>,
    },
}

/// What a finished job produced (`docs/engine-api.md` §3.2).
#[derive(Debug, Clone, PartialEq)]
pub enum JobResult {
    /// An import batch completed (skips included: they never fail a batch).
    Import(ImportReport),
    /// An export batch completed (per-version failures included).
    Export(ExportReport),
    /// A preset application batch completed (per-version failures included).
    Preset(PresetApplyReport),
    /// A reprocess batch completed (per-version failures included).
    Reprocess(ReprocessReport),
    /// A preview render completed.
    Preview(PreviewFile),
    /// A print batch completed (per-version failures included).
    Print(PrintReport),
    /// An import scan completed: what the source holds, nothing written
    /// (ADR 0065 §1).
    Scan(Vec<ImportCandidate>),
    /// The job failed before producing anything.
    Failed(String),
}
