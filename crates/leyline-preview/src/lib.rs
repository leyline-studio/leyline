//! Preview and thumbnail cache management.
//!
//! Previews are plain PNG files under the library's `Cache/` directory,
//! organised per `docs/catalog.md` §21. The catalog stores only their
//! metadata (§19): this crate never touches SQLite, and the whole cache can
//! be deleted at any time without losing data — the engine regenerates it.
//!
//! This crate is internal to the engine, which maps [`PreviewError`] onto
//! `leyline_core::LeylineError` once it knows the asset involved.

mod cache;
mod image;

pub use cache::{PreviewCache, StoredPreview};
pub use image::Rgb8;

use leyline_core::PreviewKind;

/// Errors produced while scaling or caching previews.
#[derive(Debug, thiserror::Error)]
pub enum PreviewError {
    /// A cache file could not be read or written.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    /// The pixel buffer does not describe a usable image.
    #[error("invalid image: {0}")]
    InvalidImage(String),
}

impl From<png::EncodingError> for PreviewError {
    fn from(error: png::EncodingError) -> Self {
        match error {
            png::EncodingError::IoError(e) => PreviewError::Io(e),
            other => PreviewError::InvalidImage(other.to_string()),
        }
    }
}

impl From<png::DecodingError> for PreviewError {
    fn from(error: png::DecodingError) -> Self {
        match error {
            png::DecodingError::IoError(e) => PreviewError::Io(e),
            other => PreviewError::InvalidImage(other.to_string()),
        }
    }
}

/// Longest edge, in pixels, of each preview size class (`docs/catalog.md`
/// §19). `None` means native resolution.
pub const fn max_edge(kind: PreviewKind) -> Option<u32> {
    match kind {
        PreviewKind::Thumbnail => Some(256),
        PreviewKind::Small => Some(1024),
        PreviewKind::Medium => Some(2048),
        PreviewKind::Large => Some(4096),
        PreviewKind::Full => None,
    }
}
