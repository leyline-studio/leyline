//! Full-text search index maintenance (`docs/catalog.md` §30).
//!
//! `search_index` is a cache: the engine keeps it in sync on every mutation
//! (registration, keywords, descriptions, metadata), and it can always be
//! rebuilt from the source tables. Keyword paths are indexed whole — the
//! unicode61 tokenizer splits `Nature/Birds/Heron` into its levels, so
//! searching any level matches.
//!
//! Since ADR 0144 the row also carries what the photographer *wrote* (title,
//! caption) and what the file *says* (body, lens, the day it was taken): the
//! four answers a search box is asked for and could not give.

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
    tx.prepare_cached(
        "INSERT INTO search_index (asset_id, filename, keywords, artist, copyright,
                                   title, caption, camera, lens, captured)
         VALUES (?1, ?2, '', '', '', '', '', '', '', '')",
    )
    .and_then(|mut stmt| stmt.execute(rusqlite::params![asset.get(), filename]))
    .map_err(db_err)?;
    // The day it was taken is known at registration — the metadata pass that
    // brings the body and the lens comes later (ADR 0144 §2).
    refresh_asset_capture_date(tx, asset)?;
    Ok(())
}

/// SQL recomputing the `captured` column from the asset's own row.
///
/// The **local** day, offset included: what a photographer types is the date
/// they remember taking the photograph, not the same instant read in UTC —
/// the distinction `docs/catalog.md` §13 makes and that a search must honour
/// the same way the grid does.
const REFRESH_CAPTURED: &str = "UPDATE search_index SET captured =
     COALESCE((SELECT strftime('%Y-%m-%d',
                      (a.capture_date + COALESCE(a.capture_offset_minutes, 0) * 60000) / 1000,
                      'unixepoch')
               FROM assets a WHERE a.id = ?1), '')
 WHERE asset_id = ?1";

/// Recomputes the capture-date column of one asset.
pub(crate) fn refresh_asset_capture_date(
    tx: &rusqlite::Transaction<'_>,
    asset: AssetId,
) -> Result<()> {
    tx.execute(REFRESH_CAPTURED, [asset.get()])
        .map_err(db_err)?;
    Ok(())
}

/// Recomputes the columns fed by what someone **wrote** about a photograph:
/// the authorship, preferring the description over the file (ADR 0099 §2),
/// and — since ADR 0144 — the title and the caption themselves.
pub(crate) fn refresh_asset_authorship(conn: &rusqlite::Connection, asset: AssetId) -> Result<()> {
    conn.execute(
        "UPDATE search_index SET
             artist = COALESCE(
                 (SELECT creator FROM asset_descriptions WHERE asset_id = ?1),
                 (SELECT artist FROM metadata WHERE asset_id = ?1), ''),
             copyright = COALESCE(
                 (SELECT copyright FROM asset_descriptions WHERE asset_id = ?1),
                 (SELECT copyright FROM metadata WHERE asset_id = ?1), ''),
             title = COALESCE(
                 (SELECT title FROM asset_descriptions WHERE asset_id = ?1), ''),
             caption = COALESCE(
                 (SELECT caption FROM asset_descriptions WHERE asset_id = ?1), '')
         WHERE asset_id = ?1",
        [asset.get()],
    )
    .map_err(db_err)?;
    Ok(())
}

/// Recomputes the body and the lens of one asset, named the way the shot
/// filters name them (ADR 0064 §2) so one vocabulary answers both.
pub(crate) fn refresh_asset_gear(tx: &rusqlite::Transaction<'_>, asset: AssetId) -> Result<()> {
    tx.execute(
        "UPDATE search_index SET
             camera = COALESCE((SELECT TRIM(COALESCE(c.manufacturer, '') || ' ' || c.model)
                                FROM metadata m JOIN cameras c ON c.id = m.camera_id
                                WHERE m.asset_id = ?1), ''),
             lens = COALESCE((SELECT TRIM(COALESCE(l.manufacturer, '') || ' ' || l.model)
                              FROM metadata m JOIN lenses l ON l.id = m.lens_id
                              WHERE m.asset_id = ?1), '')
         WHERE asset_id = ?1",
        [asset.get()],
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
            "INSERT INTO search_index (asset_id, filename, keywords, artist, copyright,
                                       title, caption, camera, lens, captured)
             SELECT a.id, a.filename,
                    COALESCE((SELECT group_concat(k.path, ' ')
                              FROM asset_keywords ak JOIN keywords k ON k.id = ak.keyword_id
                              WHERE ak.asset_id = a.id), ''),
                    COALESCE(d.creator, m.artist, ''),
                    COALESCE(d.copyright, m.copyright, ''),
                    COALESCE(d.title, ''),
                    COALESCE(d.caption, ''),
                    COALESCE(TRIM(COALESCE(c.manufacturer, '') || ' ' || c.model), ''),
                    COALESCE(TRIM(COALESCE(l.manufacturer, '') || ' ' || l.model), ''),
                    COALESCE(strftime('%Y-%m-%d',
                             (a.capture_date + COALESCE(a.capture_offset_minutes, 0) * 60000) / 1000,
                             'unixepoch'), '')
             FROM assets a
             LEFT JOIN metadata m ON m.asset_id = a.id
             LEFT JOIN asset_descriptions d ON d.asset_id = a.id
             LEFT JOIN cameras c ON c.id = m.camera_id
             LEFT JOIN lenses l ON l.id = m.lens_id",
            [],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(())
    }

    /// Returns the assets matching a free-text query, unordered.
    ///
    /// Each whitespace-separated term is matched as a prefix against every
    /// indexed column — filename, keyword levels, artist, copyright, title,
    /// caption, body, lens and the day it was taken (ADR 0144) — all terms
    /// required. Diacritics are ignored: « héron » matches « heron ».
    pub fn search_assets(&self, text: &str) -> Result<Vec<AssetId>> {
        let Some(query) = fts_query(text) else {
            return Ok(Vec::new());
        };
        let mut stmt = self
            .conn
            .prepare_cached("SELECT asset_id FROM search_index WHERE search_index MATCH ?1")
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
