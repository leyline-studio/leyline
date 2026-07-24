//! Print presets (ADR 0036), parallel to export presets (`docs/catalog.md`
//! §27) — a named, stored print recipe (paper, margins, DPI, destination
//! profile, intent), independent of any particular print job. Unlike
//! exports, printing has no history table: a print does not modify a
//! revision or produce a catalog-tracked artifact the way an exported file
//! does (ADR 0036), so there is nothing to journal.

use leyline_core::{LeylineError, PrintPresetId, Result};

use crate::{Catalog, db_err, now_ms};

/// One stored print preset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintPreset {
    /// The preset itself.
    pub preset: PrintPresetId,
    /// Display name.
    pub name: String,
    /// The print recipe, as consumed by the engine.
    pub settings_json: String,
    /// Creation time, UTC Unix epoch milliseconds.
    pub created_at: i64,
}

impl Catalog {
    /// Stores a named print preset and returns its id.
    pub fn create_print_preset(
        &mut self,
        name: &str,
        settings_json: &str,
    ) -> Result<PrintPresetId> {
        self.ensure_writable()?;
        self.conn
            .execute(
                "INSERT INTO print_presets (uuid, name, settings_json, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    uuid::Uuid::new_v4().to_string(),
                    name,
                    settings_json,
                    now_ms()
                ],
            )
            .map_err(db_err)?;
        Ok(PrintPresetId::new(self.conn.last_insert_rowid()))
    }

    /// Reads one stored print preset.
    pub fn print_preset(&self, preset: PrintPresetId) -> Result<PrintPreset> {
        self.conn
            .query_row(
                "SELECT name, settings_json, created_at
                 FROM print_presets WHERE id = ?1",
                [preset.get()],
                |row| {
                    Ok(PrintPreset {
                        preset,
                        name: row.get(0)?,
                        settings_json: row.get(1)?,
                        created_at: row.get(2)?,
                    })
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => LeylineError::PrintPresetMissing(preset),
                other => db_err(other),
            })
    }

    /// Lists every print preset, ordered by name.
    pub fn print_presets(&self) -> Result<Vec<PrintPreset>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, settings_json, created_at
                 FROM print_presets ORDER BY name, id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(PrintPreset {
                    preset: PrintPresetId::new(row.get(0)?),
                    name: row.get(1)?,
                    settings_json: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }
}
