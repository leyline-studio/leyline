//! Asset registration (`docs/catalog.md` §9, §18).
//!
//! Registering an asset is atomic and always creates the mandatory develop
//! trio of §18: the initial revision (neutral settings, no parent), the
//! `Default` version pointing at it, and the `develop_current` entry. An asset
//! therefore always has at least one revision and one version.

use leyline_core::{AssetId, FolderId, MediaType, RevisionId, Settings, VersionId};
use leyline_core::{LeylineError, Result};
use rusqlite::OptionalExtension;

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

/// What [`Catalog::delete_assets`] took out of the catalog.
///
/// The three vectors describe the same removal from three angles: which
/// assets actually existed, which files on disk they named, and which
/// preview files in the cache became orphans. `assets` and `file_paths`
/// are index-aligned; `preview_paths` is not, an asset having any number
/// of cached previews.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeletedAssets {
    /// The assets that existed and were removed, in the order given.
    pub assets: Vec<AssetId>,
    /// Their library-relative file paths, aligned with `assets`.
    pub file_paths: Vec<String>,
    /// Cache-relative paths of the preview files left orphaned.
    pub preview_paths: Vec<String>,
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
    /// `initial` is the develop state the first revision records — neutral
    /// values, and the stage versions the caller's engine pins for them
    /// (`docs/pipeline.md` §3.3: a *stored* revision gets its entries when
    /// it is written). This crate cannot derive those itself, which is why
    /// they come in as a parameter: stage versions are engine knowledge and
    /// the catalog sits below the engine.
    ///
    /// Fails if a file with the same name already exists in the folder
    /// (`UNIQUE(folder_id, filename)`).
    pub fn add_asset(&mut self, new: &NewAsset, initial: &Settings) -> Result<RegisteredAsset> {
        self.ensure_writable()?;
        let now = now_ms();
        let neutral = initial.to_json();

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

        crate::search::index_new_asset(&tx, asset, &new.filename)?;

        tx.commit().map_err(db_err)?;
        Ok(RegisteredAsset {
            asset,
            version,
            revision,
        })
    }

    /// Removes assets from the catalog, in one transaction (ADR 0060 §1).
    ///
    /// Everything hanging off an asset — metadata, develop versions and
    /// revisions, keyword links, preview rows, export history — is carried
    /// away by the schema's `ON DELETE CASCADE`. Two things are *not*, and
    /// are this method's whole substance:
    ///
    /// * `search_index` is an FTS5 **virtual** table, and foreign keys do
    ///   not apply to virtual tables. Left alone, it would keep answering
    ///   searches with photos that no longer exist. It is deleted here,
    ///   explicitly.
    /// * Preview files live in the cache **outside** SQLite. The cascade
    ///   drops their rows and would orphan their bytes on disk, so their
    ///   paths are collected before the delete and returned for the caller
    ///   to unlink.
    ///
    /// The returned [`DeletedAssets`] also carries each asset's
    /// library-relative file path, read before the rows vanish: a caller
    /// deleting the files themselves (ADR 0060 §2) cannot look them up
    /// afterwards. Unknown ids are ignored rather than refused — removing
    /// what is already gone is the caller's intent either way.
    pub fn delete_assets(&mut self, assets: &[AssetId]) -> Result<DeletedAssets> {
        self.ensure_writable()?;
        if assets.is_empty() {
            return Ok(DeletedAssets::default());
        }
        let tx = self.conn.transaction().map_err(db_err)?;

        let mut deleted = DeletedAssets::default();
        for asset in assets {
            let id = asset.get();

            let file: Option<String> = tx
                .query_row(
                    "SELECT f.relative_path || '/' || a.filename
                     FROM assets a JOIN folders f ON f.id = a.folder_id
                     WHERE a.id = ?1",
                    [id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(db_err)?;
            // An id that matches nothing contributes nothing, and must not
            // make the rest of the batch fail.
            let Some(file) = file else {
                continue;
            };

            {
                let mut stmt = tx
                    .prepare("SELECT relative_path FROM previews WHERE asset_id = ?1")
                    .map_err(db_err)?;
                let rows = stmt
                    .query_map([id], |row| row.get::<_, String>(0))
                    .map_err(db_err)?;
                for path in rows {
                    deleted.preview_paths.push(path.map_err(db_err)?);
                }
            }

            tx.execute("DELETE FROM search_index WHERE asset_id = ?1", [id])
                .map_err(db_err)?;
            tx.execute("DELETE FROM assets WHERE id = ?1", [id])
                .map_err(db_err)?;

            deleted.assets.push(*asset);
            deleted.file_paths.push(file);
        }

        tx.commit().map_err(db_err)?;
        Ok(deleted)
    }

    /// Returns the asset already carrying this checksum, if any — the
    /// duplicate detection of `docs/catalog.md` §12.
    pub fn find_asset_by_checksum(&self, checksum: &[u8; CHECKSUM_LEN]) -> Result<Option<AssetId>> {
        let found = self.conn.query_row(
            "SELECT id FROM assets WHERE checksum = ?1",
            [checksum.as_slice()],
            |row| row.get::<_, i64>(0),
        );
        match found {
            Ok(id) => Ok(Some(AssetId::new(id))),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(db_err(e)),
        }
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
