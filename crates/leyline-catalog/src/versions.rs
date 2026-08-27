//! Virtual versions and their classement (`docs/catalog.md` §16, §18,
//! `docs/engine-api.md` §8, §10.2).
//!
//! A version is a branch: a name plus a head pointer into the revision
//! graph. Creating a virtual version duplicates no file and no revision.
//! The classement (rating, color label, pick) lives on the version: every
//! virtual copy is judged independently. Keywords stay on the asset.

use leyline_core::{AssetId, ColorLabel, LeylineError, PickState, Result, RevisionId, VersionId};

use crate::{Catalog, db_err, now_ms};

/// One `develop_versions` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    /// The version itself.
    pub version: VersionId,
    /// Asset the version develops.
    pub asset: AssetId,
    /// Branch name, unique per asset.
    pub name: String,
    /// Head revision the version points at.
    pub head: RevisionId,
    /// Star rating; `None` = unrated.
    pub rating: Option<u8>,
    /// Color label; `None` = no label, or a label this engine does not know.
    pub color_label: Option<ColorLabel>,
    /// Pick / reject flag.
    pub pick: PickState,
    /// Creation time, UTC Unix epoch milliseconds.
    pub created_at: i64,
}

impl Catalog {
    /// Creates a virtual version: a new branch of `from`'s asset, pointing
    /// at `from`'s head — or at `at`, any revision of the same asset.
    ///
    /// No file and no revision is duplicated (§16). The new version starts
    /// unclassed: rating, label and pick are judgments on a rendering, not
    /// properties to inherit.
    pub fn create_version(
        &mut self,
        from: VersionId,
        name: &str,
        at: Option<RevisionId>,
    ) -> Result<VersionId> {
        self.ensure_writable()?;
        let now = now_ms();

        let tx = self.conn.transaction().map_err(db_err)?;
        let (asset, head): (i64, i64) = tx
            .query_row(
                "SELECT asset_id, head_revision_id FROM develop_versions WHERE id = ?1",
                [from.get()],
                |row| Ok((row.get("asset_id")?, row.get("head_revision_id")?)),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => LeylineError::VersionMissing(from),
                other => db_err(other),
            })?;

        let head = match at {
            None => head,
            Some(revision) => tx
                .query_row(
                    "SELECT id FROM develop_revisions WHERE id = ?1 AND asset_id = ?2",
                    [revision.get(), asset],
                    |row| row.get(0),
                )
                .map_err(|e| match e {
                    // Also covers a revision of another asset: branching a
                    // version onto foreign settings would be nonsense.
                    rusqlite::Error::QueryReturnedNoRows => LeylineError::RevisionMissing(revision),
                    other => db_err(other),
                })?,
        };

        tx.execute(
            "INSERT INTO develop_versions (uuid, asset_id, name, head_revision_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![uuid::Uuid::new_v4().to_string(), asset, name, head, now],
        )
        .map_err(db_err)?;
        let version = VersionId::new(tx.last_insert_rowid());
        tx.commit().map_err(db_err)?;
        Ok(version)
    }

    /// Lists the versions of an asset, oldest first.
    pub fn versions(&self, asset: AssetId) -> Result<Vec<VersionInfo>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT id, name, head_revision_id, rating, color_label, pick_state, created_at
                 FROM develop_versions WHERE asset_id = ?1 ORDER BY id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([asset.get()], |row| {
                Ok(VersionInfo {
                    version: VersionId::new(row.get("id")?),
                    asset,
                    name: row.get("name")?,
                    head: RevisionId::new(row.get("head_revision_id")?),
                    rating: row.get("rating")?,
                    color_label: row
                        .get::<_, Option<i64>>("color_label")?
                        .and_then(ColorLabel::from_i64),
                    pick: PickState::from_i64(row.get("pick_state")?).unwrap_or(PickState::None),
                    created_at: row.get("created_at")?,
                })
            })
            .map_err(db_err)?;
        let versions = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        if versions.is_empty() {
            // §18: an asset always has at least one version.
            return Err(LeylineError::AssetMissing(asset));
        }
        Ok(versions)
    }

    /// Returns the asset's current (active) version.
    pub fn current_version(&self, asset: AssetId) -> Result<VersionId> {
        self.conn
            .query_row(
                "SELECT version_id FROM develop_current WHERE asset_id = ?1",
                [asset.get()],
                |row| row.get::<_, i64>(0),
            )
            .map(VersionId::new)
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => LeylineError::AssetMissing(asset),
                other => db_err(other),
            })
    }

    /// Makes `version` the asset's current version. The version must belong
    /// to the asset.
    pub fn set_current_version(&mut self, asset: AssetId, version: VersionId) -> Result<()> {
        self.ensure_writable()?;
        let updated = self
            .conn
            .execute(
                "UPDATE develop_current SET version_id = ?1
                 WHERE asset_id = ?2
                   AND EXISTS (SELECT 1 FROM develop_versions
                               WHERE id = ?1 AND asset_id = ?2)",
                [version.get(), asset.get()],
            )
            .map_err(db_err)?;
        if updated == 0 {
            return Err(LeylineError::VersionMissing(version));
        }
        Ok(())
    }

    /// Renames a version. Names are unique per asset.
    pub fn rename_version(&mut self, version: VersionId, name: &str) -> Result<()> {
        self.ensure_writable()?;
        let updated = self
            .conn
            .execute(
                "UPDATE develop_versions SET name = ?1 WHERE id = ?2",
                rusqlite::params![name, version.get()],
            )
            .map_err(db_err)?;
        if updated == 0 {
            return Err(LeylineError::VersionMissing(version));
        }
        Ok(())
    }

    /// Deletes a version — never the asset's last one (§18: an asset always
    /// has at least one version). Its revisions stay in the graph.
    ///
    /// When the deleted version was current, the asset's oldest remaining
    /// version becomes current.
    pub fn delete_version(&mut self, version: VersionId) -> Result<()> {
        self.ensure_writable()?;
        let tx = self.conn.transaction().map_err(db_err)?;

        let asset: i64 = tx
            .query_row(
                "SELECT asset_id FROM develop_versions WHERE id = ?1",
                [version.get()],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => LeylineError::VersionMissing(version),
                other => db_err(other),
            })?;

        let survivor: Option<i64> = tx
            .query_row(
                "SELECT MIN(id) FROM develop_versions WHERE asset_id = ?1 AND id <> ?2",
                [asset, version.get()],
                |row| row.get(0),
            )
            .map_err(db_err)?;
        let Some(survivor) = survivor else {
            return Err(LeylineError::Db(format!(
                "version {version} is the last version of its asset and cannot be deleted"
            )));
        };

        // Repoint the current version first: the develop_current row would
        // otherwise vanish through ON DELETE CASCADE, leaving the asset
        // without a current version.
        tx.execute(
            "UPDATE develop_current SET version_id = ?1
             WHERE asset_id = ?2 AND version_id = ?3",
            [survivor, asset, version.get()],
        )
        .map_err(db_err)?;
        tx.execute(
            "DELETE FROM develop_versions WHERE id = ?1",
            [version.get()],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(())
    }

    /// Sets the star rating of a batch of versions; `None` clears it.
    /// Batches are the nominal case (`docs/engine-api.md` §8).
    pub fn set_rating(&mut self, versions: &[VersionId], rating: Option<u8>) -> Result<()> {
        if let Some(stars) = rating
            && !(1..=5).contains(&stars)
        {
            return Err(LeylineError::InvalidSettings(format!(
                "rating must be in [1, 5], got {stars}"
            )));
        }
        self.update_versions(versions, "rating", &rating.map(i64::from))
    }

    /// Sets the color label of a batch of versions; `None` clears it.
    pub fn set_color_label(
        &mut self,
        versions: &[VersionId],
        label: Option<ColorLabel>,
    ) -> Result<()> {
        self.update_versions(versions, "color_label", &label.map(ColorLabel::as_i64))
    }

    /// Sets the pick / reject flag of a batch of versions.
    pub fn set_pick(&mut self, versions: &[VersionId], pick: PickState) -> Result<()> {
        self.update_versions(versions, "pick_state", &Some(pick.as_i64()))
    }

    /// Updates one classement column for every version of the batch, in one
    /// transaction: a missing version rolls the whole batch back.
    fn update_versions(
        &mut self,
        versions: &[VersionId],
        column: &str,
        value: &Option<i64>,
    ) -> Result<()> {
        self.ensure_writable()?;
        let tx = self.conn.transaction().map_err(db_err)?;
        {
            let mut stmt = tx
                .prepare(&format!(
                    "UPDATE develop_versions SET {column} = ?1 WHERE id = ?2"
                ))
                .map_err(db_err)?;
            for &version in versions {
                let updated = stmt
                    .execute(rusqlite::params![value, version.get()])
                    .map_err(db_err)?;
                if updated == 0 {
                    return Err(LeylineError::VersionMissing(version));
                }
            }
        }
        tx.commit().map_err(db_err)?;
        Ok(())
    }
}
