//! Contact-sheet presets (ADR 0110 §8), parallel to print presets
//! (`docs/catalog.md` §42) and, through them, to export presets (§27) — a
//! named, stored sheet recipe (the page, plus the grid drawn on it),
//! independent of any particular job.
//!
//! A separate table rather than a row among the print presets: those rows are
//! read back as `leyline_export::PrintSettings`, which refuses unknown fields,
//! so one contact-sheet recipe stored among them would make every listing of
//! print presets fail. Like printing, a contact sheet has no history table —
//! it modifies no revision and is not a state to journal.

use leyline_core::{ContactSheetPresetId, LeylineError, Result};

use crate::{Catalog, db_err, now_ms};

/// One stored contact-sheet preset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactSheetPreset {
    /// The preset itself.
    pub preset: ContactSheetPresetId,
    /// Display name.
    pub name: String,
    /// The sheet recipe, as consumed by the engine.
    pub settings_json: String,
    /// Creation time, UTC Unix epoch milliseconds.
    pub created_at: i64,
}

impl Catalog {
    /// Stores a named contact-sheet preset and returns its id.
    pub fn create_contact_sheet_preset(
        &mut self,
        name: &str,
        settings_json: &str,
    ) -> Result<ContactSheetPresetId> {
        self.ensure_writable()?;
        self.conn
            .execute(
                "INSERT INTO contact_sheet_presets (uuid, name, settings_json, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    uuid::Uuid::new_v4().to_string(),
                    name,
                    settings_json,
                    now_ms()
                ],
            )
            .map_err(db_err)?;
        Ok(ContactSheetPresetId::new(self.conn.last_insert_rowid()))
    }

    /// Reads one stored contact-sheet preset.
    pub fn contact_sheet_preset(&self, preset: ContactSheetPresetId) -> Result<ContactSheetPreset> {
        self.conn
            .query_row(
                "SELECT name, settings_json, created_at
                 FROM contact_sheet_presets WHERE id = ?1",
                [preset.get()],
                |row| {
                    Ok(ContactSheetPreset {
                        preset,
                        name: row.get("name")?,
                        settings_json: row.get("settings_json")?,
                        created_at: row.get("created_at")?,
                    })
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    LeylineError::ContactSheetPresetMissing(preset)
                }
                other => db_err(other),
            })
    }

    /// Lists every contact-sheet preset, ordered by name.
    pub fn contact_sheet_presets(&self) -> Result<Vec<ContactSheetPreset>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT id, name, settings_json, created_at
                 FROM contact_sheet_presets ORDER BY name, id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ContactSheetPreset {
                    preset: ContactSheetPresetId::new(row.get("id")?),
                    name: row.get("name")?,
                    settings_json: row.get("settings_json")?,
                    created_at: row.get("created_at")?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }
}
