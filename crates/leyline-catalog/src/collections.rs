//! Collections of develop versions (`docs/catalog.md` §24, §25).
//!
//! Collections are independent of the physical folder tree and contain
//! *versions*: placing the black-and-white version in an album is exactly
//! what displays and exports. Manual collections hold an explicit ordered
//! list; smart collections (§26) are rule-driven and get their members from
//! queries, never from this table.

use serde::{Deserialize, Serialize};

use leyline_core::{CollectionId, CollectionType, LeylineError, Result, VersionId};

use crate::{Catalog, db_err, now_ms};

/// Rating criterion of a smart collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RatingRule {
    /// At least this many stars.
    pub gte: u8,
}

/// Criteria of a smart collection (`docs/catalog.md` §26), all combined
/// with AND. The JSON format is deliberately versionable: rules written by
/// a newer engine (unknown fields) are refused, never silently truncated.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SmartRules {
    /// Minimum rating.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<RatingRule>,
    /// Camera match: the model, or "manufacturer model".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera: Option<String>,
    /// Keyword paths; each matches the keyword or any descendant.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// `true`: flagged as pick; `false`: not flagged as pick.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pick: Option<bool>,
    /// Fields from newer rule formats, preserved for the refusal check.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl SmartRules {
    /// Parses a `rules_json` document, refusing formats this engine cannot
    /// evaluate completely: showing a wrong subset would be worse than an
    /// error (the philosophy of `docs/pipeline.md` §3.4).
    pub fn parse(json: &str) -> Result<SmartRules> {
        let rules: SmartRules = serde_json::from_str(json)
            .map_err(|e| LeylineError::InvalidSettings(format!("smart rules: {e}")))?;
        if !rules.extra.is_empty() {
            let fields: Vec<_> = rules.extra.keys().map(String::as_str).collect();
            return Err(LeylineError::InvalidSettings(format!(
                "smart rules use criteria this engine does not know: {}",
                fields.join(", ")
            )));
        }
        rules.validate()?;
        Ok(rules)
    }

    /// Serializes the rules to their `rules_json` form.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("rules serialization cannot fail")
    }

    /// Validates criterion ranges.
    fn validate(&self) -> Result<()> {
        if let Some(rating) = self.rating
            && !(1..=5).contains(&rating.gte)
        {
            return Err(LeylineError::InvalidSettings(format!(
                "smart rules rating.gte must be in [1, 5], got {}",
                rating.gte
            )));
        }
        Ok(())
    }
}

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

    /// Renames a collection (§24).
    ///
    /// A blank name is refused; anything else is accepted, duplicates
    /// included — two albums called "Portraits" under different parents are
    /// legitimate, and under the same parent that is the user's business.
    pub fn rename_collection(&mut self, collection: CollectionId, name: &str) -> Result<()> {
        self.ensure_writable()?;
        let name = name.trim();
        if name.is_empty() {
            return Err(invalid("a collection needs a name"));
        }
        require_collection(&self.conn, collection)?;
        self.conn
            .execute(
                "UPDATE collections SET name = ?2 WHERE id = ?1",
                rusqlite::params![collection.get(), name],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Moves a collection under `parent`, or to the root with `None` (§24).
    ///
    /// Refuses a move that would make the collection its own descendant: the
    /// rows would stay valid for SQLite, but the whole subtree would vanish
    /// from every read that starts at the root.
    pub fn move_collection(
        &mut self,
        collection: CollectionId,
        parent: Option<CollectionId>,
    ) -> Result<()> {
        self.ensure_writable()?;
        require_collection(&self.conn, collection)?;
        if let Some(parent) = parent {
            require_collection(&self.conn, parent)?;
            if parent == collection || descendants(&self.conn, collection)?.contains(&parent) {
                return Err(invalid("a collection cannot be moved inside itself"));
            }
        }
        self.conn
            .execute(
                "UPDATE collections SET parent_collection_id = ?2 WHERE id = ?1",
                rusqlite::params![collection.get(), parent.map(CollectionId::get)],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// Deletes a collection and everything under it, returning how many
    /// collections went (§24).
    ///
    /// Bottom-up, because `parent_collection_id` is `ON DELETE RESTRICT`: a
    /// parent cannot leave before its children. Only memberships follow
    /// (`collection_versions`, `ON DELETE CASCADE`) — no version, no
    /// revision and no file is touched, which is the invariant of §29.
    pub fn delete_collection(&mut self, collection: CollectionId) -> Result<u32> {
        self.ensure_writable()?;
        require_collection(&self.conn, collection)?;
        // `descendants` lists parents before children; reversing it takes the
        // deepest first, and the collection itself goes last — a parent
        // cannot leave before its children.
        let mut doomed = descendants(&self.conn, collection)?;
        doomed.reverse();
        doomed.push(collection);
        let tx = self.conn.transaction().map_err(db_err)?;
        for victim in &doomed {
            tx.execute("DELETE FROM collections WHERE id = ?1", [victim.get()])
                .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        Ok(u32::try_from(doomed.len()).unwrap_or(u32::MAX))
    }

    /// The rules of a smart collection, or `None` for a manual one
    /// (ADR 0143 §3).
    ///
    /// Published because a rule nobody can read after the fact leaves a
    /// collection whose contents have no explanation: the interface shows
    /// them beside the count.
    pub fn smart_rules(&self, collection: CollectionId) -> Result<Option<SmartRules>> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT rules_json FROM collections WHERE id = ?1 AND collection_type = 1",
                [collection.get()],
                |row| row.get(0),
            )
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    require_collection(&self.conn, collection).map(|()| None)
                }
                other => Err(db_err(other)),
            })?;
        json.map(|json| SmartRules::parse(&json)).transpose()
    }

    /// Creates a smart collection under `parent` (or at the root): its
    /// members come from the rules, evaluated by the grid query.
    pub fn create_smart_collection(
        &mut self,
        parent: Option<CollectionId>,
        name: &str,
        rules: &SmartRules,
    ) -> Result<CollectionId> {
        self.ensure_writable()?;
        rules.validate()?;
        let tx = self.conn.transaction().map_err(db_err)?;
        if let Some(parent) = parent {
            collection_type(&tx, parent)?;
        }
        tx.execute(
            "INSERT INTO collections (uuid, parent_collection_id, name, collection_type,
                                      rules_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                uuid::Uuid::new_v4().to_string(),
                parent.map(CollectionId::get),
                name,
                CollectionType::Smart.as_i64(),
                rules.to_json(),
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
            .prepare_cached(
                "SELECT id, parent_collection_id, name, description, collection_type
                 FROM collections",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>("id")?,
                    row.get::<_, Option<i64>>("parent_collection_id")?,
                    row.get::<_, String>("name")?,
                    row.get::<_, Option<String>>("description")?,
                    row.get::<_, i64>("collection_type")?,
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
                .prepare_cached(
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
                .prepare_cached(
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
                .prepare_cached(
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
            .prepare_cached(
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

/// Fails with `CollectionMissing` when the collection does not exist.
pub(crate) fn require_collection(
    conn: &rusqlite::Connection,
    collection: CollectionId,
) -> Result<()> {
    collection_type(conn, collection).map(|_| ())
}

/// An invalid-argument error, in the shape the rest of the catalog uses for
/// them (see `folders::validate_relative_path`).
fn invalid(reason: &str) -> LeylineError {
    LeylineError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        reason.to_owned(),
    ))
}

/// Every collection under `root`, parents before their children.
///
/// Walked level by level rather than by a recursive CTE so the order is the
/// one both callers need: a deletion goes through it backwards to reach the
/// deepest first, and a cycle check only asks whether an id is in it.
fn descendants(conn: &rusqlite::Connection, root: CollectionId) -> Result<Vec<CollectionId>> {
    let mut found: Vec<CollectionId> = Vec::new();
    let mut frontier = vec![root];
    while let Some(parent) = frontier.pop() {
        let mut stmt = conn
            .prepare_cached(
                "SELECT id FROM collections WHERE parent_collection_id = ?1 ORDER BY id",
            )
            .map_err(db_err)?;
        let children = stmt
            .query_map([parent.get()], |row| {
                row.get::<_, i64>(0).map(CollectionId::new)
            })
            .map_err(db_err)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;
        for child in children {
            found.push(child);
            frontier.push(child);
        }
    }
    Ok(found)
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
