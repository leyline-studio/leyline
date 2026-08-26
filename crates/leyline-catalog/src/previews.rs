//! Preview metadata (`docs/catalog.md` §19, §20).
//!
//! The catalog stores only metadata about cached previews: the files
//! themselves live under `Cache/` and belong to `leyline-preview`. Validity
//! needs no timestamp and no hash — a preview is valid exactly when its
//! revision is the head of the asset's current version (§20).

use rusqlite::OptionalExtension;

use leyline_core::{AssetId, LeylineError, PreviewKind, PreviewOrigin, Result, RevisionId};

use crate::{Catalog, db_err, now_ms};

/// Facts about one cached preview file, as recorded by the generator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPreview {
    /// Asset the preview renders.
    pub asset: AssetId,
    /// Develop revision the preview was rendered from.
    pub revision: RevisionId,
    /// Size class of the file.
    pub kind: PreviewKind,
    /// Pixel width of the file.
    pub width: u32,
    /// Pixel height of the file.
    pub height: u32,
    /// Path of the file, relative to the cache root, forward-slashed.
    pub relative_path: String,
    /// Where the pixels came from (ADR 0082 §2).
    pub origin: PreviewOrigin,
}

/// One `previews` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewRow {
    /// Asset the preview renders.
    pub asset: AssetId,
    /// Develop revision the preview was rendered from.
    pub revision: RevisionId,
    /// Size class of the file.
    pub kind: PreviewKind,
    /// Pixel width of the file.
    pub width: u32,
    /// Pixel height of the file.
    pub height: u32,
    /// Path of the file, relative to the cache root, forward-slashed.
    pub relative_path: String,
    /// Generation time, UTC Unix epoch milliseconds.
    pub generated_at: i64,
    /// Where the pixels came from (ADR 0082 §2).
    pub origin: PreviewOrigin,
}

impl Catalog {
    /// Records a generated preview file.
    ///
    /// Regenerating the same `(asset, revision, kind)` replaces the previous
    /// row: the cache holds at most one file per slot.
    pub fn record_preview(&mut self, new: &NewPreview) -> Result<()> {
        self.ensure_writable()?;
        self.conn
            .execute(
                "INSERT INTO previews (asset_id, revision_id, kind, width, height,
                                       relative_path, generated_at, origin)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(asset_id, revision_id, kind) DO UPDATE SET
                     width = excluded.width,
                     height = excluded.height,
                     relative_path = excluded.relative_path,
                     generated_at = excluded.generated_at,
                     origin = excluded.origin",
                rusqlite::params![
                    new.asset.get(),
                    new.revision.get(),
                    new.kind.as_i64(),
                    new.width,
                    new.height,
                    new.relative_path,
                    now_ms(),
                    new.origin.as_i64(),
                ],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Records a generated preview file, but only if `revision` still
    /// carries exactly `expected_settings_json` — returns `false` and writes
    /// nothing otherwise.
    ///
    /// A render started under a held catalog lock is guaranteed current
    /// throughout, but the engine's `Library::preview` (`docs/adr/0023-*.md`)
    /// releases the lock across the render itself, so a
    /// concurrent amendment (§17) can rewrite `revision`'s `settings_json`
    /// in place — same id, different meaning — while the render is in
    /// flight. Comparing the exact settings string inside this same
    /// transaction, atomically with the write, is what stops that race from
    /// recording a stale render as the valid preview of the (now different)
    /// revision it targets. `commit_revision`/`undo`/`redo` never rewrite an
    /// existing revision's `settings_json`, so they never trip this guard —
    /// only an in-place amendment can.
    pub fn record_preview_if_current(
        &mut self,
        new: &NewPreview,
        expected_settings_json: &str,
    ) -> Result<bool> {
        self.ensure_writable()?;
        let tx = self.conn.transaction().map_err(db_err)?;
        let current: Option<String> = tx
            .query_row(
                "SELECT settings_json FROM develop_revisions WHERE id = ?1",
                [new.revision.get()],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_err)?;
        if current.as_deref() != Some(expected_settings_json) {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO previews (asset_id, revision_id, kind, width, height,
                                   relative_path, generated_at, origin)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(asset_id, revision_id, kind) DO UPDATE SET
                 width = excluded.width,
                 height = excluded.height,
                 relative_path = excluded.relative_path,
                 generated_at = excluded.generated_at,
                 origin = excluded.origin",
            rusqlite::params![
                new.asset.get(),
                new.revision.get(),
                new.kind.as_i64(),
                new.width,
                new.height,
                new.relative_path,
                now_ms(),
                new.origin.as_i64(),
            ],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(true)
    }

    /// Returns the head revision of the asset's current version — the only
    /// revision whose previews are valid (§20).
    pub fn current_head_revision(&self, asset: AssetId) -> Result<RevisionId> {
        // Once per grid cell, like `valid_preview` just below it.
        self.conn
            .prepare_cached(
                "SELECT v.head_revision_id
                 FROM develop_current c
                 JOIN develop_versions v ON v.id = c.version_id
                 WHERE c.asset_id = ?1",
            )
            .and_then(|mut stmt| stmt.query_row([asset.get()], |row| row.get::<_, i64>(0)))
            .map(RevisionId::new)
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => LeylineError::AssetMissing(asset),
                other => db_err(other),
            })
    }

    /// Returns the valid preview of `kind` for the asset, or `None` when the
    /// cache holds nothing for the current head revision.
    ///
    /// An undo that moves the head back onto an already-previewed revision
    /// revalidates the old file automatically: validity is the identifier
    /// comparison of §20, nothing else.
    ///
    /// A preview the file itself carried is **not** an answer to this
    /// question (ADR 0082 §2): it never went through the pipeline, so it is
    /// not the head's render however fresh it is. [`Catalog::displayable_preview`]
    /// is the call that accepts it.
    pub fn valid_preview(&self, asset: AssetId, kind: PreviewKind) -> Result<Option<PreviewRow>> {
        let head = self.current_head_revision(asset)?;
        // Studio asks this for every cell of every window it loads, so the
        // statement is prepared once and re-run, never re-parsed.
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT width, height, relative_path, generated_at
                 FROM previews
                 WHERE asset_id = ?1 AND revision_id = ?2 AND kind = ?3
                   AND origin = 0",
            )
            .map_err(db_err)?;
        let found = stmt.query_row(
            rusqlite::params![asset.get(), head.get(), kind.as_i64()],
            |row| {
                Ok(PreviewRow {
                    asset,
                    revision: head,
                    kind,
                    width: row.get(0)?,
                    height: row.get(1)?,
                    relative_path: row.get(2)?,
                    generated_at: row.get(3)?,
                    origin: PreviewOrigin::Rendered,
                })
            },
        );
        match found {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(db_err(e)),
        }
    }

    /// Returns the most recently generated preview of `kind` for the asset,
    /// whatever revision it was rendered from — `None` only when the cache
    /// holds nothing at all for this asset and kind.
    ///
    /// Unlike [`Catalog::valid_preview`], this does not check freshness: a
    /// plain commit (`docs/catalog.md` §17) leaves the previous revision's
    /// preview rows in place, so a stale-but-displayable file can still be
    /// found here after the head has moved on (`docs/engine-api.md` §11).
    pub fn latest_preview(&self, asset: AssetId, kind: PreviewKind) -> Result<Option<PreviewRow>> {
        let found = self.conn.query_row(
            "SELECT revision_id, width, height, relative_path, generated_at, origin
             FROM previews
             WHERE asset_id = ?1 AND kind = ?2
             ORDER BY generated_at DESC
             LIMIT 1",
            rusqlite::params![asset.get(), kind.as_i64()],
            |row| {
                Ok(PreviewRow {
                    asset,
                    revision: RevisionId::new(row.get(0)?),
                    kind,
                    width: row.get(1)?,
                    height: row.get(2)?,
                    relative_path: row.get(3)?,
                    generated_at: row.get(4)?,
                    origin: PreviewOrigin::from_i64(row.get(5)?).unwrap_or(PreviewOrigin::Rendered),
                })
            },
        );
        match found {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(db_err(e)),
        }
    }

    /// Whether the asset's current version still sits on the revision
    /// `add_asset` wrote — that is, whether it has never been developed.
    ///
    /// The initial revision is the only one with no parent (§18), which is
    /// also what the grid's "already developed" badge reads. ADR 0082 §1
    /// turns on it: a photo nobody has touched is shown as the camera
    /// rendered it, and the pipeline only takes over once there is an edit
    /// to show.
    pub fn head_is_initial(&self, asset: AssetId) -> Result<bool> {
        let head = self.current_head_revision(asset)?;
        let parent: Option<i64> = self
            .conn
            .prepare_cached("SELECT parent_revision_id FROM develop_revisions WHERE id = ?1")
            .and_then(|mut stmt| stmt.query_row([head.get()], |row| row.get(0)))
            .map_err(db_err)?;
        Ok(parent.is_none())
    }

    /// Returns the best image the cache can show for the head revision,
    /// whatever produced it — and the row says which (ADR 0082 §3).
    ///
    /// This is the grid's question, and it is not
    /// [`Catalog::valid_preview`]'s: a cell wants something to draw now, and
    /// a preview the camera embedded draws perfectly well. Its `origin` is
    /// what tells the client the cell is not finished, so that it queues the
    /// render that will replace it — a caller that ignores the field shows a
    /// camera rendering forever without knowing it.
    pub fn displayable_preview(
        &self,
        asset: AssetId,
        kind: PreviewKind,
    ) -> Result<Option<PreviewRow>> {
        let head = self.current_head_revision(asset)?;
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT width, height, relative_path, generated_at, origin
                 FROM previews
                 WHERE asset_id = ?1 AND revision_id = ?2 AND kind = ?3",
            )
            .map_err(db_err)?;
        let found = stmt.query_row(
            rusqlite::params![asset.get(), head.get(), kind.as_i64()],
            |row| {
                Ok(PreviewRow {
                    asset,
                    revision: head,
                    kind,
                    width: row.get(0)?,
                    height: row.get(1)?,
                    relative_path: row.get(2)?,
                    generated_at: row.get(3)?,
                    origin: PreviewOrigin::from_i64(row.get(4)?).unwrap_or(PreviewOrigin::Rendered),
                })
            },
        );
        match found {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(db_err(e)),
        }
    }

    /// Keeps the asset's previews inside the window of
    /// [ADR 0075](../../../docs/adr/0075-preview-cache-retention.md) and
    /// returns the cache paths of everything it dropped, so the caller can
    /// unlink the files.
    ///
    /// Survive: the **head of every version** of the asset — a virtual copy
    /// parked on an old revision must keep its preview — and the `keep` most
    /// recent revisions of that asset. Revision ids increase with time, so
    /// "most recent" needs no timestamp; and keeping the newest is what
    /// leaves both undo *and* redo instant around the point of work, without
    /// reasoning about which way the head last moved.
    ///
    /// Deletes rows, never revisions: the history stays whole and replayable.
    /// What goes is a derived image, and the engine can rebuild it in about a
    /// second.
    pub fn retain_previews(&mut self, asset: AssetId, keep: usize) -> Result<Vec<String>> {
        self.ensure_writable()?;
        // ADR 0082 §2: an embedded preview belongs to the *file*, not to a
        // revision, so it does not age with the history and the window of
        // ADR 0075 does not see it. It goes when the asset does, by cascade.
        const CONDEMNED: &str = "asset_id = ?1
             AND origin = 0
             AND revision_id NOT IN (
                 SELECT head_revision_id FROM develop_versions WHERE asset_id = ?1
             )
             AND revision_id NOT IN (
                 SELECT id FROM develop_revisions WHERE asset_id = ?1
                 ORDER BY id DESC LIMIT ?2
             )";
        let keep = i64::try_from(keep).unwrap_or(i64::MAX);
        let tx = self.conn.transaction().map_err(db_err)?;
        let dropped = {
            let mut stmt = tx
                .prepare(&format!(
                    "SELECT relative_path FROM previews WHERE {CONDEMNED}"
                ))
                .map_err(db_err)?;
            let rows = stmt
                .query_map(rusqlite::params![asset.get(), keep], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(db_err)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(db_err)?
        };
        tx.execute(
            &format!("DELETE FROM previews WHERE {CONDEMNED}"),
            rusqlite::params![asset.get(), keep],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(dropped)
    }

    /// Deletes every preview row of a revision and returns the cache paths of
    /// the deleted files, so the caller can remove them from disk.
    ///
    /// This is the amendment rule of §17: amending the head revision
    /// invalidates its previews.
    pub fn remove_revision_previews(&mut self, revision: RevisionId) -> Result<Vec<String>> {
        self.ensure_writable()?;
        let tx = self.conn.transaction().map_err(db_err)?;
        let paths = {
            let mut stmt = tx
                .prepare_cached(
                    "SELECT relative_path FROM previews
                     WHERE revision_id = ?1 AND origin = 0",
                )
                .map_err(db_err)?;
            let rows = stmt
                .query_map([revision.get()], |row| row.get::<_, String>(0))
                .map_err(db_err)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(db_err)?
        };
        tx.execute(
            "DELETE FROM previews WHERE revision_id = ?1 AND origin = 0",
            [revision.get()],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(paths)
    }
}
