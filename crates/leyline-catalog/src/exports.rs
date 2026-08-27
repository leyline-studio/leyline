//! Export presets and history (`docs/catalog.md` §27, §28).
//!
//! Presets are named, stored export recipes, independent of the exports
//! made with them. The history lets a client reproduce an export or find
//! the last destination used; the recipe content is opaque here — the
//! engine validates it (`leyline-export`), the catalog stores it.

use leyline_core::{AssetId, ExportPresetId, LeylineError, Result};

use crate::{Catalog, db_err, now_ms};

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

/// One line of the export history, newest first.
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
    ) -> Result<()> {
        self.ensure_writable()?;
        let inserted = self
            .conn
            .execute(
                "INSERT INTO export_history (asset_id, preset_id, format, destination, exported_at)
                 SELECT id, ?2, ?3, ?4, ?5 FROM assets WHERE id = ?1",
                rusqlite::params![
                    asset.get(),
                    preset.map(ExportPresetId::get),
                    format,
                    destination,
                    now_ms()
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
                "SELECT preset_id, format, destination, exported_at
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
                })
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }
}
