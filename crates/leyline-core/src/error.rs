//! The shared error type of the platform.
//!
//! Every error crossing the API boundary is a value (`docs/engine-api.md` §2):
//! the engine never panics across the boundary.

use std::path::PathBuf;

use crate::id::{
    AssetId, CollectionId, ContactSheetPresetId, ExportPresetId, KeywordId, PresetId,
    PrintPresetId, RevisionId, VersionId,
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

    /// The referenced contact-sheet preset does not exist in the catalog.
    #[error("contact sheet preset {0} does not exist")]
    ContactSheetPresetMissing(ContactSheetPresetId),

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

    /// The revision cites stage versions that work in two different working
    /// spaces (ADR 0044 §4). They do not compose — an operator written for
    /// linear light handed a gamma-encoded buffer produces plausible, wrong
    /// pixels — so the render is refused, exactly as an unknown stage is.
    /// Migrating the revision to one space is a reprocessing
    /// (`docs/pipeline.md` §4.5), which writes a new revision.
    #[error(
        "settings mix working spaces: {stage} v{version} renders in {space}, \
         {other_stage} v{other_version} in {other_space}"
    )]
    MixedWorkingSpaces {
        /// One stage's name.
        stage: String,
        /// That stage's version.
        version: u16,
        /// The working space that version renders in.
        space: String,
        /// The name of a stage disagreeing with it.
        other_stage: String,
        /// That stage's version.
        other_version: u16,
        /// The working space it renders in.
        other_space: String,
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

    /// A revision references a creative LUT (`.cube`, ADR 0053) that is
    /// missing, unreadable, fails to parse, or whose BLAKE3 checksum no
    /// longer matches what the revision recorded. Same fail-closed contract
    /// as [`LeylineError::CameraProfileFailed`], and for the same reason: a
    /// look that quietly stopped applying is worse than an error.
    #[error("LUT at {path} failed: {reason}")]
    LutFailed {
        /// Library-relative path of the referenced `.cube` file.
        path: String,
        /// Human-readable diagnostic (missing file, checksum mismatch,
        /// parse failure).
        reason: String,
    },

    /// A revision references a stored mask coverage (ADR 0070) that is
    /// missing, unreadable, not a 16-bit grayscale PNG, or whose BLAKE3
    /// checksum no longer matches what the revision recorded. Same
    /// fail-closed contract as [`LeylineError::LutFailed`]: a local
    /// adjustment silently covering the whole image — or nothing — is worse
    /// than an error.
    #[error("mask coverage at {path} failed: {reason}")]
    MaskCoverageFailed {
        /// Library-relative path of the referenced `.png` file.
        path: String,
        /// Human-readable diagnostic (missing file, checksum mismatch,
        /// wrong format).
        reason: String,
    },

    /// The root holding this photograph is not reachable right now
    /// (`docs/adr/0085-named-roots.md` §5).
    ///
    /// **Offline is not missing.** An unplugged disk is not an edit: nothing
    /// about the assets in this root is rewritten, `assets.is_missing` is not
    /// set, and browsing, filtering, rating, keywording and searching go on
    /// working from the library's own preview cache. What cannot be done is
    /// anything that needs the original pixels — develop, export, print,
    /// reprocess, a new preview kind — and that is what this error reports.
    ///
    /// It names the root rather than the file on purpose: the actionable fact
    /// is "plug in Archive 2019", not "this path does not exist".
    #[error("the root {name:?} is offline: reconnect it, or point the library at it again")]
    RootOffline {
        /// What the user calls the root.
        name: String,
        /// Its identity, as its `.leyline-root` marker states it.
        uuid: String,
    },

    /// Another edit session is already open on this version (ADR 0120 §2).
    ///
    /// Refused by name rather than blocked: two sessions on one version is a
    /// real mistake — the second's commit would overwrite the first's
    /// authoritative state — and a mistake that *blocks* is one the user
    /// watches as a freeze. An interface can say "this photograph is open
    /// somewhere else"; it can say nothing at all about a mutex.
    #[error("version {version} is already being edited")]
    VersionBusy {
        /// The version a session already holds.
        version: i64,
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
