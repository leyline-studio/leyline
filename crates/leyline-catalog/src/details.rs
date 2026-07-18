//! Full details of one asset (`docs/engine-api.md` §7).
//!
//! The grid shows the visible window; `asset_details` is the deep read a
//! detail pane needs — file facts, complete EXIF, keywords, every version
//! and the current one — assembled in a single call.

use leyline_core::{AssetId, KeywordId, LeylineError, MediaType, Result, VersionId};

use crate::metadata::Metadata;
use crate::versions::VersionInfo;
use crate::{Catalog, db_err};

/// Everything the catalog knows about one asset.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetDetails {
    /// The asset itself.
    pub asset: AssetId,
    /// Library-relative path, forward-slashed.
    pub relative_path: String,
    /// File name, including extension.
    pub filename: String,
    /// Kind of file.
    pub media_type: MediaType,
    /// File size in bytes.
    pub file_size: u64,
    /// Pixel width, when known.
    pub width: Option<u32>,
    /// Pixel height, when known.
    pub height: Option<u32>,
    /// Capture instant, UTC epoch milliseconds, when known.
    pub capture_date: Option<i64>,
    /// Import time, UTC epoch milliseconds.
    pub imported_at: i64,
    /// Complete EXIF metadata, when recorded.
    pub metadata: Option<Metadata>,
    /// Keywords of the asset, ordered by path.
    pub keywords: Vec<KeywordId>,
    /// Every develop version, oldest first.
    pub versions: Vec<VersionInfo>,
    /// The active version.
    pub current_version: VersionId,
}

impl Catalog {
    /// Reads the complete details of one asset.
    pub fn asset_details(&self, asset: AssetId) -> Result<AssetDetails> {
        let (filename, media_type, file_size, width, height, capture_date, imported_at) = self
            .conn
            .query_row(
                "SELECT filename, media_type, file_size, width, height,
                        capture_date, imported_at
                 FROM assets WHERE id = ?1",
                [asset.get()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, u64>(2)?,
                        row.get::<_, Option<u32>>(3)?,
                        row.get::<_, Option<u32>>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                        row.get::<_, i64>(6)?,
                    ))
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => LeylineError::AssetMissing(asset),
                other => db_err(other),
            })?;

        Ok(AssetDetails {
            asset,
            relative_path: self.asset_relative_path(asset)?,
            filename,
            media_type: MediaType::from_i64(media_type).unwrap_or(MediaType::Other),
            file_size,
            width,
            height,
            capture_date,
            imported_at,
            metadata: self.metadata(asset)?,
            keywords: self.asset_keywords(asset)?,
            versions: self.versions(asset)?,
            current_version: self.current_version(asset)?,
        })
    }
}
