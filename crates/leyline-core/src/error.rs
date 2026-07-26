//! The shared error type of the platform.
//!
//! Every error crossing the API boundary is a value (`docs/engine-api.md` §2):
//! the engine never panics across the boundary.

use std::path::PathBuf;

use crate::id::{
    AssetId, CollectionId, ExportPresetId, KeywordId, PresetId, PrintPresetId, RevisionId,
    VersionId,
};

/// Errors produced by the Leyline engine and its components (`docs/engine-api.md` §4).
#[derive(Debug, thiserror::Error)]
pub enum LeylineError {
    /// No library exists at the given path.
    #[error("library not found at {0}")]
    LibraryNotFound(PathBuf),

    /// Another instance already holds the library open for writing.
    #[error("library is locked by another writer")]
    LibraryLocked,

    /// The catalog was written by a newer engine (`docs/pipeline.md` §3.4):
    /// open read-only or refuse, but never modify.
    #[error("catalog version {found} is newer than the supported version {supported}")]
    NewerCatalog {
        /// Catalog schema version found on disk.
        found: u32,
        /// Most recent schema version this engine supports.
        supported: u32,
    },

    /// The referenced asset does not exist in the catalog.
    #[error("asset {0} does not exist")]
    AssetMissing(AssetId),

    /// The referenced develop version does not exist in the catalog.
    #[error("version {0} does not exist")]
    VersionMissing(VersionId),

    /// The referenced develop revision does not exist in the catalog.
    #[error("revision {0} does not exist")]
    RevisionMissing(RevisionId),

    /// The referenced keyword does not exist in the catalog.
    #[error("keyword {0} does not exist")]
    KeywordMissing(KeywordId),

    /// The referenced collection does not exist in the catalog.
    #[error("collection {0} does not exist")]
    CollectionMissing(CollectionId),

    /// The referenced export preset does not exist in the catalog.
    #[error("export preset {0} does not exist")]
    ExportPresetMissing(ExportPresetId),

    /// The referenced develop preset does not exist in the catalog.
    #[error("develop preset {0} does not exist")]
    PresetMissing(PresetId),

    /// The referenced print preset does not exist in the catalog.
    #[error("print preset {0} does not exist")]
    PrintPresetMissing(PrintPresetId),

    /// The asset's file could not be decoded.
    #[error("failed to decode asset {asset}: {reason}")]
    DecodeFailed {
        /// Asset whose file failed to decode.
        asset: AssetId,
        /// Human-readable decoder diagnostic.
        reason: String,
    },

    /// A settings value or document is invalid.
    #[error("invalid settings: {0}")]
    InvalidSettings(String),

    /// The revision's *format* was written by a newer engine
    /// (`docs/pipeline.md` §3.4): show the best cached preview instead,
    /// never edit, never guess.
    #[error("settings declare schema {schema}, newer than this engine")]
    NewerSettings {
        /// Settings format version the revision declares.
        schema: u32,
    },

    /// The revision's *rendering* was written by a newer engine: it cites a
    /// pipeline stage, or a version of one, that this engine does not
    /// implement (ADR 0043 §4). Same fail-closed contract as
    /// [`LeylineError::NewerSettings`] — the finer granularity only lets the
    /// message name what is missing.
    #[error("settings cite stage {stage} v{version}, which this engine does not implement")]
    UnknownStage {
        /// Stage name the revision cites.
        stage: String,
        /// Version of that stage the revision cites.
        version: u16,
    },

    /// A pixel buffer handed to the engine is malformed.
    #[error("invalid image: {0}")]
    InvalidImage(String),

    /// A revision references a camera profile (`.dcp`, ADR 0035) that is
    /// missing, unreadable, fails to parse, or whose BLAKE3 checksum no
    /// longer matches what the revision recorded. Same fail-closed
    /// contract as [`LeylineError::NewerSettings`]: show the best cached
    /// preview instead, never render with a silently different profile,
    /// never edit the revision.
    #[error("camera profile at {path} failed: {reason}")]
    CameraProfileFailed {
        /// Library-relative path of the referenced `.dcp` file.
        path: String,
        /// Human-readable diagnostic (missing file, checksum mismatch,
        /// parse failure).
        reason: String,
    },

    /// An underlying I/O operation failed.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// An underlying database operation failed.
    #[error("database error: {0}")]
    Db(String),

    /// A tether session (`docs/adr/0038-tethered-capture.md`) failed to
    /// connect, or a session was already open on this library.
    #[error("tether error: {0}")]
    Tether(String),

    /// A watched-folder session (`docs/adr/0039-watched-folder-import.md`)
    /// failed to start, or a session was already running on this library.
    #[error("watch error: {0}")]
    Watch(String),
}

/// Convenience alias used across the whole platform.
pub type Result<T> = std::result::Result<T, LeylineError>;
