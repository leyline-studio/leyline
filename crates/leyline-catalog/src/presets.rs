//! Develop presets (`docs/presets.md` §4).
//!
//! Named, partial jeux of develop settings. `preset_json` is opaque here,
//! exactly like `export_presets.settings_json` (§27): the engine interprets
//! its structure, the catalog only stores it.
//!
//! Since ADR 0058 a preset also carries its shelf — a folder, a favourite
//! flag — and a revision counter bumped by every update, and the revisions it
//! produces record where they came from (`develop_revisions.from_preset_id`).
//! What has not changed, and is the point: **modifying or deleting a preset
//! never touches a revision already written**. The settings of a revision
//! stay a state, the provenance lives in columns no renderer reads, and a
//! deleted preset merely leaves a `NULL` behind (`docs/catalog.md` §41).

use leyline_core::{LeylineError, PresetFolderId, PresetId, Result, VersionId};

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
    /// The folder it is filed under, if any (ADR 0058 §2).
    pub folder: Option<PresetFolderId>,
    /// Shown at the top of the shelf.
    pub favourite: bool,
    /// Bumped by every update; a revision records which one produced it, so
    /// "developed with an older version of this preset" is a question with an
    /// answer (ADR 0058 §6).
    pub revision: u32,
}

/// One folder of the preset shelf (ADR 0058 §2). One level only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetFolder {
    /// The folder itself.
    pub folder: PresetFolderId,
    /// Display name.
    pub name: String,
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
                "SELECT name, preset_json, created_at, folder_id, favourite, preset_revision
                 FROM develop_presets WHERE id = ?1",
                [preset.get()],
                |row| {
                    Ok(Preset {
                        preset,
                        name: row.get("name")?,
                        preset_json: row.get("preset_json")?,
                        created_at: row.get("created_at")?,
                        folder: row
                            .get::<_, Option<i64>>("folder_id")?
                            .map(PresetFolderId::new),
                        favourite: row.get::<_, i64>("favourite")? != 0,
                        revision: row
                            .get::<_, i64>("preset_revision")?
                            .try_into()
                            .unwrap_or(1),
                    })
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => LeylineError::PresetMissing(preset),
                other => db_err(other),
            })
    }

    /// Lists every develop preset: favourites first, then by name — the
    /// order the shelf shows them in (ADR 0058 §2).
    pub fn presets(&self) -> Result<Vec<Preset>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT id, name, preset_json, created_at, folder_id, favourite, preset_revision
                 FROM develop_presets ORDER BY favourite DESC, name, id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Preset {
                    preset: PresetId::new(row.get("id")?),
                    name: row.get("name")?,
                    preset_json: row.get("preset_json")?,
                    created_at: row.get("created_at")?,
                    folder: row
                        .get::<_, Option<i64>>("folder_id")?
                        .map(PresetFolderId::new),
                    favourite: row.get::<_, i64>("favourite")? != 0,
                    revision: row
                        .get::<_, i64>("preset_revision")?
                        .try_into()
                        .unwrap_or(1),
                })
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    /// Replaces a preset's settings and bumps its revision (ADR 0058 §6).
    ///
    /// Revisions already produced by it keep their own settings, and
    /// therefore their pixels: this changes what the *next* application will
    /// write, never what a past one wrote (`docs/presets.md` §2).
    pub fn update_preset(&mut self, preset: PresetId, preset_json: &str) -> Result<u32> {
        self.ensure_writable()?;
        let updated = self
            .conn
            .execute(
                "UPDATE develop_presets
                 SET preset_json = ?2, preset_revision = preset_revision + 1, updated_at = ?3
                 WHERE id = ?1",
                rusqlite::params![preset.get(), preset_json, now_ms()],
            )
            .map_err(db_err)?;
        if updated == 0 {
            return Err(LeylineError::PresetMissing(preset));
        }
        Ok(self.preset(preset)?.revision)
    }

    /// Files a preset in a folder, or at the root with `None`.
    pub fn file_preset(&mut self, preset: PresetId, folder: Option<PresetFolderId>) -> Result<()> {
        self.ensure_writable()?;
        if let Some(folder) = folder {
            require_preset_folder(&self.conn, folder)?;
        }
        let updated = self
            .conn
            .execute(
                "UPDATE develop_presets SET folder_id = ?2 WHERE id = ?1",
                rusqlite::params![preset.get(), folder.map(PresetFolderId::get)],
            )
            .map_err(db_err)?;
        if updated == 0 {
            return Err(LeylineError::PresetMissing(preset));
        }
        Ok(())
    }

    /// Marks a preset as a favourite, or stops.
    pub fn favourite_preset(&mut self, preset: PresetId, favourite: bool) -> Result<()> {
        self.ensure_writable()?;
        let updated = self
            .conn
            .execute(
                "UPDATE develop_presets SET favourite = ?2 WHERE id = ?1",
                rusqlite::params![preset.get(), i64::from(favourite)],
            )
            .map_err(db_err)?;
        if updated == 0 {
            return Err(LeylineError::PresetMissing(preset));
        }
        Ok(())
    }

    /// The versions whose **current** revision came from `preset` (ADR 0058
    /// §6), and the version of the preset each one was made with.
    ///
    /// Current head only: a photo developed with the preset and then edited
    /// by hand is no longer "a photo of this preset", and re-applying it
    /// would throw away the hand work.
    pub fn versions_from_preset(&self, preset: PresetId) -> Result<Vec<(VersionId, u32)>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT v.id, r.from_preset_revision
                 FROM develop_versions v
                 JOIN develop_revisions r ON r.id = v.head_revision_id
                 WHERE r.from_preset_id = ?1
                 ORDER BY v.id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([preset.get()], |row| {
                Ok((
                    VersionId::new(row.get("id")?),
                    row.get::<_, Option<i64>>("from_preset_revision")?
                        .and_then(|value| u32::try_from(value).ok())
                        .unwrap_or(1),
                ))
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    /// Creates a preset folder (ADR 0058 §2).
    pub fn create_preset_folder(&mut self, name: &str) -> Result<PresetFolderId> {
        self.ensure_writable()?;
        let name = name.trim();
        if name.is_empty() {
            return Err(LeylineError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "a preset folder needs a name".to_owned(),
            )));
        }
        self.conn
            .execute(
                "INSERT INTO preset_folders (name, created_at) VALUES (?1, ?2)",
                rusqlite::params![name, now_ms()],
            )
            .map_err(db_err)?;
        Ok(PresetFolderId::new(self.conn.last_insert_rowid()))
    }

    /// Lists the preset folders, by name.
    pub fn preset_folders(&self) -> Result<Vec<PresetFolder>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id, name FROM preset_folders ORDER BY name, id")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(PresetFolder {
                    folder: PresetFolderId::new(row.get("id")?),
                    name: row.get("name")?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    /// Renames a preset folder.
    pub fn rename_preset_folder(&mut self, folder: PresetFolderId, name: &str) -> Result<()> {
        self.ensure_writable()?;
        let name = name.trim();
        if name.is_empty() {
            return Err(LeylineError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "a preset folder needs a name".to_owned(),
            )));
        }
        require_preset_folder(&self.conn, folder)?;
        self.conn
            .execute(
                "UPDATE preset_folders SET name = ?2 WHERE id = ?1",
                rusqlite::params![folder.get(), name],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Deletes a preset folder; its presets go back to the root, none is
    /// lost (`ON DELETE SET NULL`, ADR 0058 §2).
    pub fn delete_preset_folder(&mut self, folder: PresetFolderId) -> Result<()> {
        self.ensure_writable()?;
        require_preset_folder(&self.conn, folder)?;
        self.conn
            .execute("DELETE FROM preset_folders WHERE id = ?1", [folder.get()])
            .map_err(db_err)?;
        Ok(())
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

/// Fails with a plain not-found error when the folder does not exist. Preset
/// folders have no dedicated error variant: they are a shelf, and the caller
/// only ever needs to know that the shelf is gone.
fn require_preset_folder(conn: &rusqlite::Connection, folder: PresetFolderId) -> Result<()> {
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM preset_folders WHERE id = ?1",
            [folder.get()],
            |row| row.get(0),
        )
        .map_err(db_err)?;
    if exists == 0 {
        return Err(LeylineError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("preset folder {folder} does not exist"),
        )));
    }
    Ok(())
}
