//! Full-text search index maintenance (`docs/catalog.md` §30).
//!
//! `search_index` is a cache: the engine keeps it in sync on every mutation
//! (registration, keywords; artist and copyright will follow with metadata),
//! and it can always be rebuilt from the source tables. Keyword paths are
//! indexed whole — the unicode61 tokenizer splits `Nature/Birds/Heron` into
//! its levels, so searching any level matches.

use leyline_core::{AssetId, Result};

use crate::{Catalog, db_err};

/// SQL recomputing the keyword column of one asset from `asset_keywords`.
const REFRESH_KEYWORDS: &str = "UPDATE search_index SET keywords =
     (SELECT COALESCE(group_concat(k.path, ' '), '')
      FROM asset_keywords ak JOIN keywords k ON k.id = ak.keyword_id
      WHERE ak.asset_id = ?1)
 WHERE asset_id = ?1";

/// Inserts the search row of a freshly registered asset.
pub(crate) fn index_new_asset(
    tx: &rusqlite::Transaction<'_>,
    asset: AssetId,
    filename: &str,
) -> Result<()> {
    tx.execute(
        "INSERT INTO search_index (asset_id, filename, keywords, artist, copyright)
         VALUES (?1, ?2, '', '', '')",
        rusqlite::params![asset.get(), filename],
    )
    .map_err(db_err)?;
    Ok(())
}

/// Recomputes the keyword column of one asset.
pub(crate) fn refresh_asset_keywords(tx: &rusqlite::Transaction<'_>, asset: AssetId) -> Result<()> {
    tx.execute(REFRESH_KEYWORDS, [asset.get()])
        .map_err(db_err)?;
    Ok(())
}

impl Catalog {
    /// Rebuilds the whole search index from the source tables.
    ///
    /// §30: `search_index` regenerates like a cache — when in doubt, this is
    /// always safe.
    pub fn rebuild_search_index(&mut self) -> Result<()> {
        self.ensure_writable()?;
        let tx = self.conn.transaction().map_err(db_err)?;
        tx.execute("DELETE FROM search_index", []).map_err(db_err)?;
        tx.execute(
            "INSERT INTO search_index (asset_id, filename, keywords, artist, copyright)
             SELECT a.id, a.filename,
                    COALESCE((SELECT group_concat(k.path, ' ')
                              FROM asset_keywords ak JOIN keywords k ON k.id = ak.keyword_id
                              WHERE ak.asset_id = a.id), ''),
                    COALESCE(m.artist, ''), COALESCE(m.copyright, '')
             FROM assets a LEFT JOIN metadata m ON m.asset_id = a.id",
            [],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(())
    }

    /// Returns the assets matching a free-text query, unordered.
    ///
    /// Each whitespace-separated term is matched as a prefix against every
    /// indexed column (filename, keyword levels, artist, copyright), all
    /// terms required. Diacritics are ignored: « héron » matches « heron ».
    pub fn search_assets(&self, text: &str) -> Result<Vec<AssetId>> {
        let Some(query) = fts_query(text) else {
            return Ok(Vec::new());
        };
        let mut stmt = self
            .conn
            .prepare("SELECT asset_id FROM search_index WHERE search_index MATCH ?1")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([query], |row| row.get::<_, i64>(0))
            .map_err(db_err)?;
        rows.map(|r| r.map(AssetId::new))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }
}

/// Turns free text into a safe FTS5 query: every term quoted (no operator
/// injection) and prefixed (`"her"*` matches « heron » while typing).
pub(crate) fn fts_query(text: &str) -> Option<String> {
    let terms: Vec<String> = text
        .split_whitespace()
        .map(|term| format!("\"{}\"*", term.replace('"', "\"\"")))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}
