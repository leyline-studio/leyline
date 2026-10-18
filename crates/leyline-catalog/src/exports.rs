//! Export presets and history (`docs/catalog.md` §27, §28).
//!
//! Presets are named, stored export recipes, independent of the exports
//! made with them. The history lets a client reproduce an export or find
//! the last destination used; the recipe content is opaque here — the
//! engine validates it (`leyline-export`), the catalog stores it.

use leyline_core::{AssetId, ExportPresetId, LeylineError, Result, RevisionId, VersionId};

use crate::{CHECKSUM_LEN, Catalog, db_err, now_ms};

/// One stored export preset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportPreset {
    /// The preset itself.
    pub preset: ExportPresetId,
    /// Display name.
    pub name: String,
    /// The export recipe, as consumed by the engine.
    pub settings_json: String,
    /// Creation time, UTC Unix epoch milliseconds.
    pub created_at: i64,
}

/// What an export was made from, and what it wrote (ADR 0153 §2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportProvenance {
    /// The version exported.
    pub version: VersionId,
    /// Its head revision at export time.
    pub revision: RevisionId,
    /// BLAKE3 of the asset checksum and the revision's stored settings
    /// (ADR 0153 §1) — computed once, never recomputed to match.
    pub revision_fingerprint: [u8; CHECKSUM_LEN],
    /// The resolved recipe that was applied, whatever preset it came from.
    pub export_settings_json: String,
    /// The decoder version loaded at export time (ADR 0086).
    pub decoder: String,
    /// BLAKE3 of the file written.
    pub output_checksum: [u8; CHECKSUM_LEN],
}

/// One line of the export history, newest first.
///
/// Every field after `exported_at` is `None` for a row written before
/// migration v14, and the version and revision also once deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRecord {
    /// The exported asset.
    pub asset: AssetId,
    /// Preset used, when the export came from one still existing.
    pub preset: Option<ExportPresetId>,
    /// Output format name (`jpg`, `png`...).
    pub format: String,
    /// Destination path as given by the user.
    pub destination: String,
    /// Export time, UTC Unix epoch milliseconds.
    pub exported_at: i64,
    /// The version exported.
    pub version: Option<VersionId>,
    /// Its head revision at export time.
    pub revision: Option<RevisionId>,
    /// See [`ExportProvenance::revision_fingerprint`].
    pub revision_fingerprint: Option<[u8; CHECKSUM_LEN]>,
    /// The resolved recipe.
    pub export_settings_json: Option<String>,
    /// The decoder version at export time.
    pub decoder: Option<String>,
    /// BLAKE3 of the file written.
    pub output_checksum: Option<[u8; CHECKSUM_LEN]>,
}

impl Catalog {
    /// Stores a named export preset and returns its id.
    pub fn create_export_preset(
        &mut self,
        name: &str,
        settings_json: &str,
    ) -> Result<ExportPresetId> {
        self.ensure_writable()?;
        self.conn
            .execute(
                "INSERT INTO export_presets (uuid, name, settings_json, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    uuid::Uuid::new_v4().to_string(),
                    name,
                    settings_json,
                    now_ms()
                ],
            )
            .map_err(db_err)?;
        Ok(ExportPresetId::new(self.conn.last_insert_rowid()))
    }

    /// Reads one stored export preset.
    pub fn export_preset(&self, preset: ExportPresetId) -> Result<ExportPreset> {
        self.conn
            .query_row(
                "SELECT name, settings_json, created_at
                 FROM export_presets WHERE id = ?1",
                [preset.get()],
                |row| {
                    Ok(ExportPreset {
                        preset,
                        name: row.get("name")?,
                        settings_json: row.get("settings_json")?,
                        created_at: row.get("created_at")?,
                    })
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => LeylineError::ExportPresetMissing(preset),
                other => db_err(other),
            })
    }

    /// Lists every export preset, ordered by name.
    pub fn export_presets(&self) -> Result<Vec<ExportPreset>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT id, name, settings_json, created_at
                 FROM export_presets ORDER BY name, id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ExportPreset {
                    preset: ExportPresetId::new(row.get("id")?),
                    name: row.get("name")?,
                    settings_json: row.get("settings_json")?,
                    created_at: row.get("created_at")?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    /// Journals one finished export (§28).
    pub fn record_export(
        &mut self,
        asset: AssetId,
        preset: Option<ExportPresetId>,
        format: &str,
        destination: &str,
        provenance: &ExportProvenance,
    ) -> Result<()> {
        self.ensure_writable()?;
        let inserted = self
            .conn
            .execute(
                "INSERT INTO export_history (asset_id, preset_id, format, destination, exported_at,
                                             version_id, revision_id, revision_fingerprint,
                                             export_settings_json, decoder, output_checksum)
                 SELECT id, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11 FROM assets WHERE id = ?1",
                rusqlite::params![
                    asset.get(),
                    preset.map(ExportPresetId::get),
                    format,
                    destination,
                    now_ms(),
                    provenance.version.get(),
                    provenance.revision.get(),
                    provenance.revision_fingerprint.as_slice(),
                    provenance.export_settings_json,
                    provenance.decoder,
                    provenance.output_checksum.as_slice(),
                ],
            )
            .map_err(db_err)?;
        if inserted == 0 {
            return Err(LeylineError::AssetMissing(asset));
        }
        Ok(())
    }

    /// Returns an asset's export history, newest first.
    pub fn export_history(&self, asset: AssetId) -> Result<Vec<ExportRecord>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT preset_id, format, destination, exported_at, version_id, revision_id,
                        revision_fingerprint, export_settings_json, decoder, output_checksum
                 FROM export_history WHERE asset_id = ?1
                 ORDER BY exported_at DESC, id DESC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([asset.get()], |row| {
                Ok(ExportRecord {
                    asset,
                    preset: row
                        .get::<_, Option<i64>>("preset_id")?
                        .map(ExportPresetId::new),
                    format: row.get("format")?,
                    destination: row.get("destination")?,
                    exported_at: row.get("exported_at")?,
                    version: row.get::<_, Option<i64>>("version_id")?.map(VersionId::new),
                    revision: row
                        .get::<_, Option<i64>>("revision_id")?
                        .map(RevisionId::new),
                    revision_fingerprint: checksum_column(row, "revision_fingerprint")?,
                    export_settings_json: row.get("export_settings_json")?,
                    decoder: row.get("decoder")?,
                    output_checksum: checksum_column(row, "output_checksum")?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }
}

/// A nullable 32-byte checksum column. A blob of another length is read as
/// absent rather than failing the whole history: a damaged row must not hide
/// the others.
fn checksum_column(
    row: &rusqlite::Row<'_>,
    column: &str,
) -> rusqlite::Result<Option<[u8; CHECKSUM_LEN]>> {
    Ok(row
        .get::<_, Option<Vec<u8>>>(column)?
        .and_then(|bytes| bytes.try_into().ok()))
}
