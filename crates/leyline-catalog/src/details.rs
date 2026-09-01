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
    /// Companions attached to this asset — the camera's own renderings of
    /// the same shot (ADR 0079 §6). Empty for the ordinary photo.
    pub companions: Vec<AssetId>,
    /// The master this asset is a companion of, when it is one.
    pub companion_of: Option<AssetId>,
    /// The photograph this asset was derived from, when a pixel processor
    /// made it (ADR 0107 §5). Unlike [`AssetDetails::companion_of`] this
    /// hides nothing: it is lineage, not subordination.
    pub derived_from: Option<AssetId>,
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
                        row.get::<_, String>("filename")?,
                        row.get::<_, i64>("media_type")?,
                        row.get::<_, u64>("file_size")?,
                        row.get::<_, Option<u32>>("width")?,
                        row.get::<_, Option<u32>>("height")?,
                        row.get::<_, Option<i64>>("capture_date")?,
                        row.get::<_, i64>("imported_at")?,
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
            companions: self.companions_of(asset)?,
            companion_of: self.master_of(asset)?,
            derived_from: self.derived_from(asset)?,
        })
    }
}
