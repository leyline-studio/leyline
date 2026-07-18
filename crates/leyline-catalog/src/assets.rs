//! Asset registration (`docs/catalog.md` §9, §18).
//!
//! Registering an asset is atomic and always creates the mandatory develop
//! trio of §18: the initial revision (neutral settings, no parent), the
//! `Default` version pointing at it, and the `develop_current` entry. An asset
//! therefore always has at least one revision and one version.

use leyline_core::{AssetId, FolderId, MediaType, RevisionId, Settings, VersionId};
use leyline_core::{LeylineError, Result};

use crate::{Catalog, db_err, now_ms};

/// Length in bytes of a BLAKE3 checksum (`docs/catalog.md` §12).
pub const CHECKSUM_LEN: usize = 32;

/// Facts describing a file to register (`docs/catalog.md` §9).
///
/// Assets are purely factual: rating, label and pick belong to develop
/// versions, keywords to a separate relation.
#[derive(Debug, Clone)]
pub struct NewAsset {
    /// Folder containing the file.
    pub folder: FolderId,
    /// File name, including its extension.
    pub filename: String,
    /// File extension, without the dot.
    pub extension: String,
    /// Kind of file.
    pub media_type: MediaType,
    /// File size in bytes.
    pub file_size: u64,
    /// BLAKE3 checksum of the complete file.
    pub checksum: [u8; CHECKSUM_LEN],
    /// Pixel width, when known.
    pub width: Option<u32>,
    /// Pixel height, when known.
    pub height: Option<u32>,
    /// Capture instant, UTC Unix epoch milliseconds (semantics of §9).
    pub capture_date: Option<i64>,
    /// Capture UTC offset in minutes, when the EXIF or GPS provides it.
    pub capture_offset_minutes: Option<i32>,
}

/// Identifiers created by [`Catalog::add_asset`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegisteredAsset {
    /// The new asset row.
    pub asset: AssetId,
    /// Its `Default` develop version.
    pub version: VersionId,
    /// The initial (neutral) revision the version points at.
    pub revision: RevisionId,
}

impl Catalog {
    /// Registers a file and its mandatory develop trio, in one transaction.
    ///
    /// Fails if a file with the same name already exists in the folder
    /// (`UNIQUE(folder_id, filename)`).
    pub fn add_asset(&mut self, new: &NewAsset) -> Result<RegisteredAsset> {
        self.ensure_writable()?;
        let now = now_ms();
        let neutral = Settings::default().to_json();

        let tx = self.conn.transaction().map_err(db_err)?;

        tx.execute(
            "INSERT INTO assets (uuid, folder_id, filename, extension, media_type,
                                 file_size, checksum, width, height,
                                 capture_date, capture_offset_minutes,
                                 imported_at, modified_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
            rusqlite::params![
                uuid::Uuid::new_v4().to_string(),
                new.folder.get(),
                new.filename,
                new.extension,
                new.media_type.as_i64(),
                new.file_size,
                new.checksum,
                new.width,
                new.height,
                new.capture_date,
                new.capture_offset_minutes,
                now,
            ],
        )
        .map_err(db_err)?;
        let asset = AssetId::new(tx.last_insert_rowid());

        tx.execute(
            "INSERT INTO develop_revisions (asset_id, parent_revision_id, settings_json, created_at)
             VALUES (?1, NULL, ?2, ?3)",
            rusqlite::params![asset.get(), neutral, now],
        )
        .map_err(db_err)?;
        let revision = RevisionId::new(tx.last_insert_rowid());

        tx.execute(
            "INSERT INTO develop_versions (uuid, asset_id, name, head_revision_id, created_at)
             VALUES (?1, ?2, 'Default', ?3, ?4)",
            rusqlite::params![
                uuid::Uuid::new_v4().to_string(),
                asset.get(),
                revision.get(),
                now
            ],
        )
        .map_err(db_err)?;
        let version = VersionId::new(tx.last_insert_rowid());

        tx.execute(
            "INSERT INTO develop_current (asset_id, version_id) VALUES (?1, ?2)",
            rusqlite::params![asset.get(), version.get()],
        )
        .map_err(db_err)?;

        tx.commit().map_err(db_err)?;
        Ok(RegisteredAsset {
            asset,
            version,
            revision,
        })
    }

    /// Returns the library-relative path of an asset's file, always derived
    /// from its folder (`docs/catalog.md` §9: no stored asset path).
    pub fn asset_relative_path(&self, asset: AssetId) -> Result<String> {
        self.conn
            .query_row(
                "SELECT f.relative_path || '/' || a.filename
                 FROM assets a JOIN folders f ON f.id = a.folder_id
                 WHERE a.id = ?1",
                [asset.get()],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => LeylineError::AssetMissing(asset),
                other => db_err(other),
            })
    }
}
