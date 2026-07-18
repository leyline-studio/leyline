//! The shared error type of the platform.
//!
//! Every error crossing the API boundary is a value (`docs/engine-api.md` §2):
//! the engine never panics across the boundary.

use std::path::PathBuf;

use crate::id::AssetId;

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

    /// An underlying I/O operation failed.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// An underlying database operation failed.
    #[error("database error: {0}")]
    Db(String),
}

/// Convenience alias used across the whole platform.
pub type Result<T> = std::result::Result<T, LeylineError>;
