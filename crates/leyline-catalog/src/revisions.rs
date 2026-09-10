//! Develop revision mechanics (`docs/catalog.md` §16, §17, §18).
//!
//! The model is Git's: a revision is an immutable complete settings state, a
//! version is a branch whose `head_revision_id` pointer moves. Editing commits
//! a child revision and advances the head; undo and redo only move the
//! pointer; nothing is ever deleted.
//!
//! The catalog provides the *mechanics* and enforces the §17 guards. The
//! coalescence *policy* — when a slider drag becomes a commit, when the
//! amendment window applies — belongs to the engine's edit session
//! (`docs/engine-api.md` §10.1).

use leyline_core::{AssetId, LeylineError, PresetId, Result, RevisionId, Settings, VersionId};

use crate::{Catalog, db_err, now_ms};

/// One `develop_revisions` row.
#[derive(Debug, Clone, PartialEq)]
pub struct RevisionRow {
    /// The revision itself.
    pub revision: RevisionId,
    /// Asset the revision develops.
    pub asset: AssetId,
    /// Parent in the revision graph; `None` for the initial revision.
    pub parent: Option<RevisionId>,
    /// Complete, self-contained develop state (`docs/pipeline.md` §3.2).
    pub settings_json: String,
    /// Creation time, UTC Unix epoch milliseconds. Never touched by an
    /// amendment: amending coalesces into the original intention.
    pub created_at: i64,
    /// The preset that produced this revision, when one did (ADR 0058 §5).
    /// Stored since that ADR and read since [ADR 0142](../../docs/adr/0142-a-history-that-says-what-changed.md):
    /// « this photograph took the Portrait preset » is the one thing about a
    /// revision that a list of changed settings cannot say.
    pub from_preset: Option<PresetId>,
}

/// Result of a successful head amendment.
#[derive(Debug, Clone, PartialEq)]
pub struct Amendment {
    /// The amended revision (unchanged id: the row was rewritten in place).
    pub revision: RevisionId,
    /// Cache paths of the preview files invalidated by the amendment (§17):
    /// their rows are already deleted, the caller removes the files.
    pub removed_previews: Vec<String>,
}

impl Catalog {
    /// Commits `settings` as a new revision of the version and advances its
    /// head. The parent of the new revision is the previous head.
    ///
    /// After an undo, committing branches the graph: the undone revisions
    /// stay reachable, exactly like committing on a moved Git branch.
    pub fn commit_revision(
        &mut self,
        version: VersionId,
        settings: &Settings,
    ) -> Result<RevisionId> {
        self.commit_revision_from(version, settings, None)
    }

    /// [`Catalog::commit_revision`], recording that a preset produced it
    /// (ADR 0058 §5): the preset and the version of it that was applied.
    ///
    /// The pair is catalog metadata and nothing else — no renderer reads it,
    /// and `settings_json` is untouched, which is what keeps two identical
    /// sets of settings rendering identically whatever their origin.
    pub fn commit_revision_from(
        &mut self,
        version: VersionId,
        settings: &Settings,
        from_preset: Option<(PresetId, u32)>,
    ) -> Result<RevisionId> {
        self.ensure_writable()?;
        settings.validate()?;
        let now = now_ms();

        let tx = self.conn.transaction().map_err(db_err)?;
        let (asset, head) = version_row(&tx, version)?;
        tx.execute(
            "INSERT INTO develop_revisions
                 (asset_id, parent_revision_id, settings_json, created_at,
                  from_preset_id, from_preset_revision)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                asset.get(),
                head.get(),
                settings.to_json(),
                now,
                from_preset.map(|(preset, _)| preset.get()),
                from_preset.map(|(_, revision)| i64::from(revision)),
            ],
        )
        .map_err(db_err)?;
        let revision = RevisionId::new(tx.last_insert_rowid());

        tx.execute(
            "UPDATE develop_versions SET head_revision_id = ?1 WHERE id = ?2",
            rusqlite::params![revision.get(), version.get()],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(revision)
    }

    /// Rewrites the head revision of the version in place — the coalescence
    /// amendment of §17, the only exception to revision immutability.
    ///
    /// Returns `None` without writing anything when the head is not
    /// amendable; the caller then falls back to [`Catalog::commit_revision`].
    /// The §17 guards: the head must have no child revision, must not be the
    /// head of any other version, and must not be the initial revision. As a
    /// §3.4 safety net, a head whose *format* was written by a newer engine
    /// (newer `schema`) is refused with [`LeylineError::NewerSettings`]
    /// rather than silently overwritten. The rendering axis — the `stages`
    /// map — is the engine's to check, since only it holds the stage
    /// registry (ADR 0043 §4).
    ///
    /// The previews of the amended revision are invalidated: their rows are
    /// deleted and their cache paths returned for removal from disk.
    pub fn try_amend_head(
        &mut self,
        version: VersionId,
        settings: &Settings,
    ) -> Result<Option<Amendment>> {
        self.ensure_writable()?;
        settings.validate()?;

        let tx = self.conn.transaction().map_err(db_err)?;
        let (_, head) = version_row(&tx, version)?;

        let (parent, stored_json): (Option<i64>, String) = tx
            .query_row(
                "SELECT parent_revision_id, settings_json
                 FROM develop_revisions WHERE id = ?1",
                [head.get()],
                |row| Ok((row.get("parent_revision_id")?, row.get("settings_json")?)),
            )
            .map_err(db_err)?;
        if parent.is_none() {
            return Ok(None); // Initial revision: never amended.
        }

        let stored = Settings::parse(&stored_json)?;
        if stored.schema > leyline_core::CURRENT_SCHEMA {
            return Err(LeylineError::NewerSettings {
                schema: stored.schema,
            });
        }

        let referenced: i64 = tx
            .query_row(
                "SELECT (SELECT COUNT(*) FROM develop_revisions
                         WHERE parent_revision_id = ?1)
                      + (SELECT COUNT(*) FROM develop_versions
                         WHERE head_revision_id = ?1 AND id <> ?2)",
                rusqlite::params![head.get(), version.get()],
                |row| row.get(0),
            )
            .map_err(db_err)?;
        if referenced > 0 {
            return Ok(None); // Referenced elsewhere: immutability holds.
        }

        let removed_previews = {
            let mut stmt = tx
                .prepare_cached("SELECT relative_path FROM previews WHERE revision_id = ?1")
                .map_err(db_err)?;
            let rows = stmt
                .query_map([head.get()], |row| row.get::<_, String>(0))
                .map_err(db_err)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(db_err)?
        };
        tx.execute("DELETE FROM previews WHERE revision_id = ?1", [head.get()])
            .map_err(db_err)?;
        tx.execute(
            "UPDATE develop_revisions SET settings_json = ?1 WHERE id = ?2",
            rusqlite::params![settings.to_json(), head.get()],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(Some(Amendment {
            revision: head,
            removed_previews,
        }))
    }

    /// Moves the version's head back to the parent revision.
    ///
    /// Returns the new head, or `None` when the head is the initial revision
    /// (nothing to undo). The stepped-over revision remains in the graph.
    pub fn undo_version(&mut self, version: VersionId) -> Result<Option<RevisionId>> {
        self.ensure_writable()?;
        let tx = self.conn.transaction().map_err(db_err)?;
        let (_, head) = version_row(&tx, version)?;

        let parent: Option<i64> = tx
            .query_row(
                "SELECT parent_revision_id FROM develop_revisions WHERE id = ?1",
                [head.get()],
                |row| row.get(0),
            )
            .map_err(db_err)?;
        let Some(parent) = parent else {
            return Ok(None);
        };

        tx.execute(
            "UPDATE develop_versions SET head_revision_id = ?1 WHERE id = ?2",
            rusqlite::params![parent, version.get()],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(Some(RevisionId::new(parent)))
    }

    /// Moves the version's head forward to a child of the current head.
    ///
    /// Returns the new head, or `None` when the head has no child (nothing to
    /// redo). When the graph branches at the head, redo follows the most
    /// recently created child — after an undo followed by a new commit, redo
    /// re-traces the newest line of work, like every editor's redo.
    pub fn redo_version(&mut self, version: VersionId) -> Result<Option<RevisionId>> {
        self.ensure_writable()?;
        let tx = self.conn.transaction().map_err(db_err)?;
        let (_, head) = version_row(&tx, version)?;

        let child: Option<i64> = tx
            .query_row(
                "SELECT id FROM develop_revisions
                 WHERE parent_revision_id = ?1
                 ORDER BY id DESC LIMIT 1",
                [head.get()],
                |row| row.get(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(db_err(other)),
            })?;
        let Some(child) = child else {
            return Ok(None);
        };

        tx.execute(
            "UPDATE develop_versions SET head_revision_id = ?1 WHERE id = ?2",
            rusqlite::params![child, version.get()],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(Some(RevisionId::new(child)))
    }

    /// Moves the version's head directly to any revision of the same asset
    /// — the "jump to a specific revision" a browsable history panel needs,
    /// beyond `undo_version`/`redo_version`'s one-link-at-a-time movement.
    /// Like those two, this only moves the pointer: no revision is created
    /// or deleted, and the revision jumped away from stays reachable.
    ///
    /// Deliberately validated against the *asset*, not `version_history`
    /// (which only walks backward from the *current* head and would refuse
    /// jumping forward again after a jump back): `create_version`'s own
    /// `at` parameter uses this exact same asset-scoped check, since a
    /// virtual copy's revisions are already reachable across every version
    /// of that asset by design.
    pub fn checkout_revision(&mut self, version: VersionId, revision: RevisionId) -> Result<()> {
        self.ensure_writable()?;
        let (asset, _) = version_row(&self.conn, version)?;
        let target = self.revision(revision)?;
        if target.asset != asset {
            return Err(LeylineError::RevisionMissing(revision));
        }
        self.conn
            .execute(
                "UPDATE develop_versions SET head_revision_id = ?1 WHERE id = ?2",
                rusqlite::params![revision.get(), version.get()],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Returns the head revision of a version.
    pub fn version_head(&self, version: VersionId) -> Result<RevisionId> {
        version_row(&self.conn, version).map(|(_, head)| head)
    }

    /// Returns the asset a version develops.
    pub fn version_asset(&self, version: VersionId) -> Result<AssetId> {
        version_row(&self.conn, version).map(|(asset, _)| asset)
    }

    /// Reads one revision row.
    pub fn revision(&self, revision: RevisionId) -> Result<RevisionRow> {
        self.conn
            .query_row(
                "SELECT asset_id, parent_revision_id, settings_json, created_at, from_preset_id
                 FROM develop_revisions WHERE id = ?1",
                [revision.get()],
                |row| {
                    Ok(RevisionRow {
                        revision,
                        asset: AssetId::new(row.get("asset_id")?),
                        parent: row
                            .get::<_, Option<i64>>("parent_revision_id")?
                            .map(RevisionId::new),
                        settings_json: row.get("settings_json")?,
                        created_at: row.get("created_at")?,
                        from_preset: row
                            .get::<_, Option<i64>>("from_preset_id")?
                            .map(PresetId::new),
                    })
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => LeylineError::RevisionMissing(revision),
                other => db_err(other),
            })
    }

    /// Returns the revision chain of a version, head first, back to the
    /// initial revision. Revisions on other branches are not included.
    pub fn version_history(&self, version: VersionId) -> Result<Vec<RevisionRow>> {
        let (_, head) = version_row(&self.conn, version)?;
        let mut stmt = self
            .conn
            .prepare_cached(
                "WITH RECURSIVE chain(id) AS (
                     SELECT ?1
                     UNION ALL
                     SELECT r.parent_revision_id
                     FROM develop_revisions r JOIN chain ON r.id = chain.id
                     WHERE r.parent_revision_id IS NOT NULL
                 )
                 SELECT r.id, r.asset_id, r.parent_revision_id, r.settings_json, r.created_at,
                        r.from_preset_id
                 FROM chain JOIN develop_revisions r ON r.id = chain.id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([head.get()], |row| {
                Ok(RevisionRow {
                    revision: RevisionId::new(row.get("id")?),
                    asset: AssetId::new(row.get("asset_id")?),
                    parent: row
                        .get::<_, Option<i64>>("parent_revision_id")?
                        .map(RevisionId::new),
                    settings_json: row.get("settings_json")?,
                    created_at: row.get("created_at")?,
                    from_preset: row
                        .get::<_, Option<i64>>("from_preset_id")?
                        .map(PresetId::new),
                })
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }
}

/// Reads a version's asset and head, failing with `VersionMissing`.
fn version_row(conn: &rusqlite::Connection, version: VersionId) -> Result<(AssetId, RevisionId)> {
    conn.query_row(
        "SELECT asset_id, head_revision_id FROM develop_versions WHERE id = ?1",
        [version.get()],
        |row| {
            Ok((
                AssetId::new(row.get("asset_id")?),
                RevisionId::new(row.get("head_revision_id")?),
            ))
        },
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => LeylineError::VersionMissing(version),
        other => db_err(other),
    })
}
