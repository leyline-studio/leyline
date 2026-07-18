//! Hierarchical keywords (`docs/catalog.md` §22, §23).
//!
//! Keywords describe the *content* of the image and therefore live on the
//! asset, identical for all its versions (§16: a heron in black and white is
//! still a heron). The hierarchy is materialized twice: `parent_id` for the
//! tree, `path` for fast subtree queries and XMP export.

use leyline_core::{AssetId, KeywordId, LeylineError, Result};

use crate::{Catalog, db_err, now_ms};

/// One node of the keyword tree, children ordered by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeywordNode {
    /// The keyword itself.
    pub keyword: KeywordId,
    /// Leaf name (`Heron`).
    pub name: String,
    /// Full slash-separated path (`Nature/Birds/Heron`).
    pub path: String,
    /// Child keywords, recursively.
    pub children: Vec<KeywordNode>,
}

impl Catalog {
    /// Creates a keyword under `parent` (or at the root) and returns its id.
    ///
    /// The name is a single hierarchy level: it cannot be empty or contain
    /// `/`. Paths are unique — recreating an existing keyword is an error,
    /// not a lookup.
    pub fn create_keyword(&mut self, parent: Option<KeywordId>, name: &str) -> Result<KeywordId> {
        self.ensure_writable()?;
        if name.is_empty() || name.contains('/') || name.trim() != name {
            return Err(LeylineError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("keyword name {name:?} must be one non-empty trimmed hierarchy level"),
            )));
        }

        let tx = self.conn.transaction().map_err(db_err)?;
        let path = match parent {
            None => name.to_owned(),
            Some(parent) => {
                let parent_path: String = tx
                    .query_row(
                        "SELECT path FROM keywords WHERE id = ?1",
                        [parent.get()],
                        |row| row.get(0),
                    )
                    .map_err(|e| match e {
                        rusqlite::Error::QueryReturnedNoRows => {
                            LeylineError::KeywordMissing(parent)
                        }
                        other => db_err(other),
                    })?;
                format!("{parent_path}/{name}")
            }
        };

        tx.execute(
            "INSERT INTO keywords (parent_id, name, path, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![parent.map(KeywordId::get), name, path, now_ms()],
        )
        .map_err(db_err)?;
        let keyword = KeywordId::new(tx.last_insert_rowid());
        tx.commit().map_err(db_err)?;
        Ok(keyword)
    }

    /// Returns the complete keyword tree, siblings ordered by name.
    pub fn keyword_tree(&self) -> Result<Vec<KeywordNode>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, parent_id, name, path FROM keywords ORDER BY name")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(db_err)?;
        let rows = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)?;

        // Assemble children under their parents, deepest paths first, so a
        // node is complete before it moves under its parent.
        let mut nodes: std::collections::BTreeMap<i64, KeywordNode> = rows
            .iter()
            .map(|(id, _, name, path)| {
                (
                    *id,
                    KeywordNode {
                        keyword: KeywordId::new(*id),
                        name: name.clone(),
                        path: path.clone(),
                        children: Vec::new(),
                    },
                )
            })
            .collect();

        let mut children: Vec<(i64, i64)> = rows
            .iter()
            .filter_map(|(id, parent, _, _)| parent.map(|p| (*id, p)))
            .collect();
        children.sort_by_key(|&(id, _)| std::cmp::Reverse(nodes[&id].path.matches('/').count()));
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

        let mut roots: Vec<KeywordNode> = nodes.into_values().collect();
        roots.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(roots)
    }

    /// Tags a batch of assets with a keyword. Already-tagged assets are
    /// untouched: tagging is idempotent.
    pub fn add_keyword(&mut self, assets: &[AssetId], keyword: KeywordId) -> Result<()> {
        self.ensure_writable()?;
        let tx = self.conn.transaction().map_err(db_err)?;
        keyword_exists(&tx, keyword)?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT OR IGNORE INTO asset_keywords (asset_id, keyword_id)
                     SELECT id, ?2 FROM assets WHERE id = ?1",
                )
                .map_err(db_err)?;
            for &asset in assets {
                let inserted = stmt.execute([asset.get(), keyword.get()]).map_err(db_err)?;
                let already = inserted == 0
                    && tx
                        .query_row(
                            "SELECT 1 FROM asset_keywords WHERE asset_id = ?1 AND keyword_id = ?2",
                            [asset.get(), keyword.get()],
                            |_| Ok(()),
                        )
                        .is_ok();
                if inserted == 0 && !already {
                    return Err(LeylineError::AssetMissing(asset));
                }
            }
        }
        tx.commit().map_err(db_err)?;
        Ok(())
    }

    /// Untags a batch of assets. Removing an absent tag is a no-op.
    pub fn remove_keyword(&mut self, assets: &[AssetId], keyword: KeywordId) -> Result<()> {
        self.ensure_writable()?;
        let tx = self.conn.transaction().map_err(db_err)?;
        keyword_exists(&tx, keyword)?;
        {
            let mut stmt = tx
                .prepare("DELETE FROM asset_keywords WHERE asset_id = ?1 AND keyword_id = ?2")
                .map_err(db_err)?;
            for &asset in assets {
                stmt.execute([asset.get(), keyword.get()]).map_err(db_err)?;
            }
        }
        tx.commit().map_err(db_err)?;
        Ok(())
    }

    /// Returns the keywords of an asset, ordered by path.
    pub fn asset_keywords(&self, asset: AssetId) -> Result<Vec<KeywordId>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT k.id FROM asset_keywords ak
                 JOIN keywords k ON k.id = ak.keyword_id
                 WHERE ak.asset_id = ?1 ORDER BY k.path",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([asset.get()], |row| row.get::<_, i64>(0))
            .map_err(db_err)?;
        rows.map(|r| r.map(KeywordId::new))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }
}

/// Fails with `KeywordMissing` when the keyword does not exist.
fn keyword_exists(conn: &rusqlite::Connection, keyword: KeywordId) -> Result<()> {
    conn.query_row(
        "SELECT 1 FROM keywords WHERE id = ?1",
        [keyword.get()],
        |_| Ok(()),
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => LeylineError::KeywordMissing(keyword),
        other => db_err(other),
    })
}
