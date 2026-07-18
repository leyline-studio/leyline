//! Collections of develop versions (`docs/catalog.md` §24, §25).
//!
//! Collections are independent of the physical folder tree and contain
//! *versions*: placing the black-and-white version in an album is exactly
//! what displays and exports. Manual collections hold an explicit ordered
//! list; smart collections (§26) are rule-driven and get their members from
//! queries, never from this table.

use leyline_core::{CollectionId, CollectionType, LeylineError, Result, VersionId};

use crate::{Catalog, db_err, now_ms};

/// One node of the collection tree, children ordered by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionNode {
    /// The collection itself.
    pub collection: CollectionId,
    /// Display name.
    pub name: String,
    /// Optional user description.
    pub description: Option<String>,
    /// Manual or smart.
    pub collection_type: CollectionType,
    /// Child collections, recursively.
    pub children: Vec<CollectionNode>,
}

impl Catalog {
    /// Creates a manual collection under `parent` (or at the root).
    pub fn create_collection(
        &mut self,
        parent: Option<CollectionId>,
        name: &str,
    ) -> Result<CollectionId> {
        self.ensure_writable()?;
        let tx = self.conn.transaction().map_err(db_err)?;
        if let Some(parent) = parent {
            collection_type(&tx, parent)?;
        }
        tx.execute(
            "INSERT INTO collections (uuid, parent_collection_id, name, collection_type, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                uuid::Uuid::new_v4().to_string(),
                parent.map(CollectionId::get),
                name,
                CollectionType::Manual.as_i64(),
                now_ms(),
            ],
        )
        .map_err(db_err)?;
        let collection = CollectionId::new(tx.last_insert_rowid());
        tx.commit().map_err(db_err)?;
        Ok(collection)
    }

    /// Returns the complete collection tree, siblings ordered by name.
    pub fn collections(&self) -> Result<Vec<CollectionNode>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, parent_collection_id, name, description, collection_type
                 FROM collections",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(db_err)?;
        let rows = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;

        let mut nodes: std::collections::BTreeMap<i64, CollectionNode> = rows
            .iter()
            .map(|(id, _, name, description, kind)| {
                (
                    *id,
                    CollectionNode {
                        collection: CollectionId::new(*id),
                        name: name.clone(),
                        description: description.clone(),
                        collection_type: CollectionType::from_i64(*kind)
                            .unwrap_or(CollectionType::Manual),
                        children: Vec::new(),
                    },
                )
            })
            .collect();

        // Move children under their parents, highest ids first: a child is
        // always created after its parent, so it is complete when it moves.
        let mut children: Vec<(i64, i64)> = rows
            .iter()
            .filter_map(|(id, parent, ..)| parent.map(|p| (*id, p)))
            .collect();
        children.sort_by_key(|&(id, _)| std::cmp::Reverse(id));
        for (id, parent) in children {
            let node = nodes.remove(&id).expect("node moved once");
            let parent = nodes
                .get_mut(&parent)
                .expect("parent outlives its children (ON DELETE RESTRICT)");
            let at = parent
                .children
                .binary_search_by(|c| c.name.cmp(&node.name))
                .unwrap_or_else(|e| e);
            parent.children.insert(at, node);
        }

        let mut roots: Vec<CollectionNode> = nodes.into_values().collect();
        roots.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(roots)
    }

    /// Appends versions to a manual collection, in the given order, after
    /// the existing members. Versions already present keep their position:
    /// adding is idempotent.
    pub fn add_to_collection(
        &mut self,
        collection: CollectionId,
        versions: &[VersionId],
    ) -> Result<()> {
        self.ensure_writable()?;
        let tx = self.conn.transaction().map_err(db_err)?;
        ensure_manual(&tx, collection)?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT OR IGNORE INTO collection_versions (collection_id, version_id, position)
                     SELECT ?1, id, 1 + COALESCE((SELECT MAX(position) FROM collection_versions
                                                  WHERE collection_id = ?1), -1)
                     FROM develop_versions WHERE id = ?2",
                )
                .map_err(db_err)?;
            for &version in versions {
                let inserted = stmt
                    .execute([collection.get(), version.get()])
                    .map_err(db_err)?;
                let already = inserted == 0
                    && tx
                        .query_row(
                            "SELECT 1 FROM collection_versions
                             WHERE collection_id = ?1 AND version_id = ?2",
                            [collection.get(), version.get()],
                            |_| Ok(()),
                        )
                        .is_ok();
                if inserted == 0 && !already {
                    return Err(LeylineError::VersionMissing(version));
                }
            }
        }
        tx.commit().map_err(db_err)?;
        Ok(())
    }

    /// Removes versions from a manual collection. Absent versions are a
    /// no-op; remaining positions keep their relative order.
    pub fn remove_from_collection(
        &mut self,
        collection: CollectionId,
        versions: &[VersionId],
    ) -> Result<()> {
        self.ensure_writable()?;
        let tx = self.conn.transaction().map_err(db_err)?;
        ensure_manual(&tx, collection)?;
        {
            let mut stmt = tx
                .prepare(
                    "DELETE FROM collection_versions
                     WHERE collection_id = ?1 AND version_id = ?2",
                )
                .map_err(db_err)?;
            for &version in versions {
                stmt.execute([collection.get(), version.get()])
                    .map_err(db_err)?;
            }
        }
        tx.commit().map_err(db_err)?;
        Ok(())
    }

    /// Rewrites the user-defined order of a manual collection.
    ///
    /// `order` must be exactly the current membership — every member once,
    /// nothing else — so a stale drag-and-drop can never silently drop or
    /// invent members.
    pub fn reorder_collection(
        &mut self,
        collection: CollectionId,
        order: &[VersionId],
    ) -> Result<()> {
        self.ensure_writable()?;
        let tx = self.conn.transaction().map_err(db_err)?;
        ensure_manual(&tx, collection)?;

        let members: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM collection_versions WHERE collection_id = ?1",
                [collection.get()],
                |row| row.get(0),
            )
            .map_err(db_err)?;
        let distinct: std::collections::BTreeSet<i64> = order.iter().map(|v| v.get()).collect();
        if members != order.len() as i64 || distinct.len() != order.len() {
            return Err(LeylineError::Db(format!(
                "reorder of collection {collection} must list each of its {members} members exactly once"
            )));
        }
        {
            let mut stmt = tx
                .prepare(
                    "UPDATE collection_versions SET position = ?1
                     WHERE collection_id = ?2 AND version_id = ?3",
                )
                .map_err(db_err)?;
            for (position, &version) in order.iter().enumerate() {
                let updated = stmt
                    .execute(rusqlite::params![
                        position as i64,
                        collection.get(),
                        version.get()
                    ])
                    .map_err(db_err)?;
                if updated == 0 {
                    return Err(LeylineError::VersionMissing(version));
                }
            }
        }
        tx.commit().map_err(db_err)?;
        Ok(())
    }

    /// Lists the versions of a manual collection in user order.
    pub fn collection_versions(&self, collection: CollectionId) -> Result<Vec<VersionId>> {
        collection_type(&self.conn, collection)?;
        let mut stmt = self
            .conn
            .prepare(
                "SELECT version_id FROM collection_versions
                 WHERE collection_id = ?1 ORDER BY position",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([collection.get()], |row| row.get::<_, i64>(0))
            .map_err(db_err)?;
        rows.map(|r| r.map(VersionId::new))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }
}

/// Returns the collection's type, failing with `CollectionMissing`.
fn collection_type(
    conn: &rusqlite::Connection,
    collection: CollectionId,
) -> Result<CollectionType> {
    conn.query_row(
        "SELECT collection_type FROM collections WHERE id = ?1",
        [collection.get()],
        |row| row.get::<_, i64>(0),
    )
    .map(|kind| CollectionType::from_i64(kind).unwrap_or(CollectionType::Manual))
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => LeylineError::CollectionMissing(collection),
        other => db_err(other),
    })
}

/// Refuses membership writes on smart collections: their members come from
/// rules, never from explicit lists (§24).
fn ensure_manual(conn: &rusqlite::Connection, collection: CollectionId) -> Result<()> {
    match collection_type(conn, collection)? {
        CollectionType::Manual => Ok(()),
        CollectionType::Smart => Err(LeylineError::Db(format!(
            "collection {collection} is smart; its members come from rules, not explicit lists"
        ))),
    }
}
