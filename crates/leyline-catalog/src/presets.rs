//! Develop presets (`docs/presets.md` §4).
//!
//! Named, partial jeux of develop settings, independent of the revisions
//! they produce: no foreign key from `develop_revisions`, so renaming or
//! deleting a preset never touches history already written with it — a
//! revision is a state, never a trace of its origin (`docs/catalog.md` §17).
//! `preset_json` is opaque here, exactly like `export_presets.settings_json`
//! (§27): the engine interprets its structure, the catalog only stores it.

use leyline_core::{LeylineError, PresetId, Result};

use crate::{Catalog, db_err, now_ms};

/// One stored develop preset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preset {
    /// The preset itself.
    pub preset: PresetId,
    /// Display name.
    pub name: String,
    /// The partial settings (`PresetSettings`, `leyline-core`), as JSON.
    pub preset_json: String,
    /// Creation time, UTC Unix epoch milliseconds.
    pub created_at: i64,
}

impl Catalog {
    /// Stores a named develop preset and returns its id.
    pub fn create_preset(&mut self, name: &str, preset_json: &str) -> Result<PresetId> {
        self.ensure_writable()?;
        self.conn
            .execute(
                "INSERT INTO develop_presets (uuid, name, preset_json, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    uuid::Uuid::new_v4().to_string(),
                    name,
                    preset_json,
                    now_ms()
                ],
            )
            .map_err(db_err)?;
        Ok(PresetId::new(self.conn.last_insert_rowid()))
    }

    /// Reads one stored develop preset.
    pub fn preset(&self, preset: PresetId) -> Result<Preset> {
        self.conn
            .query_row(
                "SELECT name, preset_json, created_at
                 FROM develop_presets WHERE id = ?1",
                [preset.get()],
                |row| {
                    Ok(Preset {
                        preset,
                        name: row.get(0)?,
                        preset_json: row.get(1)?,
                        created_at: row.get(2)?,
                    })
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => LeylineError::PresetMissing(preset),
                other => db_err(other),
            })
    }

    /// Lists every develop preset, ordered by name.
    pub fn presets(&self) -> Result<Vec<Preset>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, preset_json, created_at
                 FROM develop_presets ORDER BY name, id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Preset {
                    preset: PresetId::new(row.get(0)?),
                    name: row.get(1)?,
                    preset_json: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    /// Renames a stored develop preset.
    pub fn rename_preset(&mut self, preset: PresetId, name: &str) -> Result<()> {
        self.ensure_writable()?;
        let updated = self
            .conn
            .execute(
                "UPDATE develop_presets SET name = ?2 WHERE id = ?1",
                rusqlite::params![preset.get(), name],
            )
            .map_err(db_err)?;
        if updated == 0 {
            return Err(LeylineError::PresetMissing(preset));
        }
        Ok(())
    }

    /// Deletes a stored develop preset. Revisions it already produced are
    /// untouched (`docs/presets.md` §4).
    pub fn delete_preset(&mut self, preset: PresetId) -> Result<()> {
        self.ensure_writable()?;
        let deleted = self
            .conn
            .execute("DELETE FROM develop_presets WHERE id = ?1", [preset.get()])
            .map_err(db_err)?;
        if deleted == 0 {
            return Err(LeylineError::PresetMissing(preset));
        }
        Ok(())
    }
}
