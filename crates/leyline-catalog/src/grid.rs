//! The grid query: the read model of the library (`docs/engine-api.md` §7).
//!
//! The grid enumerates **versions**, not files (`docs/catalog.md` §16).
//! Without a collection filter it shows each asset's current version; with
//! one, the collection's member versions in user order. Every filter
//! translates to SQL executed directly by SQLite (§30) and `range` gives
//! windowed pagination: the client only ever loads the visible window.

use std::ops::Range;

use leyline_core::{
    AssetId, CollectionId, ColorLabel, FolderId, KeywordId, LeylineError, PickState, Result,
    VersionId,
};
use rusqlite::types::Value as SqlValue;

use crate::collections::SmartRules;
use crate::{Catalog, db_err};

/// Sort order of the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    /// By capture date, undated assets last.
    CaptureDate {
        /// Oldest first when true.
        ascending: bool,
    },
    /// By file name, case-insensitive.
    Filename {
        /// A before Z when true.
        ascending: bool,
    },
    /// By import time.
    ImportedAt {
        /// Oldest first when true.
        ascending: bool,
    },
    /// By star rating, unrated last.
    Rating {
        /// Fewest stars first when true.
        ascending: bool,
    },
    /// By the user-defined collection order; requires a collection filter.
    CollectionOrder,
}

/// A grid request: filters, sort, and the visible window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridQuery {
    /// Only assets directly in this folder.
    pub folder: Option<FolderId>,
    /// Only member versions of this collection, instead of current versions.
    pub collection: Option<CollectionId>,
    /// Only versions rated at least this many stars.
    pub rating_at_least: Option<u8>,
    /// Only versions carrying this color label.
    pub color_label: Option<ColorLabel>,
    /// Only versions with this pick state.
    pub pick: Option<PickState>,
    /// Only assets tagged with all of these keywords or their descendants.
    pub keywords: Vec<KeywordId>,
    /// Free-text filter (FTS5, §30); blank text filters nothing.
    pub text: Option<String>,
    /// Only assets captured in `[start, end]`, UTC epoch milliseconds.
    pub capture_range: Option<(i64, i64)>,
    /// Sort order.
    pub sort: Sort,
    /// Window of rows to return (virtual scrolling).
    pub range: Range<u32>,
}

impl Default for GridQuery {
    /// Everything, newest capture first, first thousand rows.
    fn default() -> Self {
        GridQuery {
            folder: None,
            collection: None,
            rating_at_least: None,
            color_label: None,
            pick: None,
            keywords: Vec::new(),
            text: None,
            capture_range: None,
            sort: Sort::CaptureDate { ascending: false },
            range: 0..1000,
        }
    }
}

/// One grid cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridItem {
    /// The version displayed by this cell.
    pub version_id: VersionId,
    /// The asset behind it.
    pub asset_id: AssetId,
    /// File name, including extension.
    pub filename: String,
    /// Capture instant, UTC epoch milliseconds, when known.
    pub capture_date: Option<i64>,
    /// Star rating of the version.
    pub rating: Option<u8>,
    /// Color label of the version.
    pub color_label: Option<ColorLabel>,
    /// Pick / reject flag of the version.
    pub pick: PickState,
    /// Pixel width of the asset, when known.
    pub width: Option<u32>,
    /// Pixel height of the asset, when known.
    pub height: Option<u32>,
    /// Whether this version carries more than the revision it was born with
    /// — what a grid cell's "already developed" badge shows (ADR 0055 §5).
    /// True as soon as one adjustment has been committed, whatever it was.
    pub edited: bool,
}

impl Catalog {
    /// Counts the versions matching the query, ignoring `range` and `sort`.
    pub fn count(&self, query: &GridQuery) -> Result<u64> {
        let filter = self.resolve_collection(query)?;
        let (sql, params) = build(query, &filter, "COUNT(*)", false)?;
        self.conn
            .query_row(&sql, rusqlite::params_from_iter(params), |row| {
                row.get::<_, i64>(0)
            })
            .map(|n| n as u64)
            .map_err(db_err)
    }

    /// Returns the visible window of the grid.
    pub fn grid(&self, query: &GridQuery) -> Result<Vec<GridItem>> {
        if query.range.is_empty() {
            return Ok(Vec::new());
        }
        let filter = self.resolve_collection(query)?;
        let (mut sql, mut params) = build(
            query,
            &filter,
            "v.id, a.id, a.filename, a.capture_date, v.rating, v.color_label,
             v.pick_state, a.width, a.height,
             (SELECT r.parent_revision_id IS NOT NULL FROM develop_revisions r
               WHERE r.id = v.head_revision_id)",
            true,
        )?;
        sql.push_str(" LIMIT ?");
        params.push(SqlValue::Integer(i64::from(
            query.range.end - query.range.start,
        )));
        sql.push_str(" OFFSET ?");
        params.push(SqlValue::Integer(i64::from(query.range.start)));

        let mut stmt = self.conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params), |row| {
                Ok(GridItem {
                    version_id: VersionId::new(row.get(0)?),
                    asset_id: AssetId::new(row.get(1)?),
                    filename: row.get(2)?,
                    capture_date: row.get(3)?,
                    rating: row.get(4)?,
                    color_label: row.get::<_, Option<i64>>(5)?.and_then(ColorLabel::from_i64),
                    pick: PickState::from_i64(row.get(6)?).unwrap_or(PickState::None),
                    width: row.get(7)?,
                    height: row.get(8)?,
                    // A head revision with no parent is the one `add_asset`
                    // wrote: the photo is still exactly as it came in.
                    edited: row.get::<_, Option<bool>>(9)?.unwrap_or(false),
                })
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    /// Resolves the query's collection filter: manual collections enumerate
    /// their explicit members, smart collections apply their rules to the
    /// current versions — the caller sees no difference (§26).
    fn resolve_collection(&self, query: &GridQuery) -> Result<CollectionFilter> {
        let Some(collection) = query.collection else {
            return Ok(CollectionFilter::None);
        };
        let rules: Option<String> = self
            .conn
            .query_row(
                "SELECT rules_json FROM collections WHERE id = ?1 AND collection_type = 1",
                [collection.get()],
                |row| row.get(0),
            )
            .or_else(|e| match e {
                // Not smart: confirm the collection exists as manual.
                rusqlite::Error::QueryReturnedNoRows => {
                    crate::collections::require_collection(&self.conn, collection).map(|()| None)
                }
                other => Err(db_err(other)),
            })?;
        match rules {
            None => Ok(CollectionFilter::Manual(collection)),
            Some(json) => Ok(CollectionFilter::Smart(SmartRules::parse(&json)?)),
        }
    }
}

/// The resolved collection filter of one query.
enum CollectionFilter {
    /// No collection: the grid shows current versions.
    None,
    /// Manual: the grid shows the explicit members.
    Manual(CollectionId),
    /// Smart: the grid shows current versions matching the rules.
    Smart(SmartRules),
}

/// Assembles the SQL and its parameters for `select`, with or without the
/// ORDER BY clause.
fn build(
    query: &GridQuery,
    filter: &CollectionFilter,
    select: &str,
    ordered: bool,
) -> Result<(String, Vec<SqlValue>)> {
    let mut params: Vec<SqlValue> = Vec::new();
    let mut sql = format!("SELECT {select} ");

    match filter {
        CollectionFilter::None | CollectionFilter::Smart(_) => {
            sql.push_str(
                "FROM develop_current c
                 JOIN develop_versions v ON v.id = c.version_id
                 JOIN assets a ON a.id = c.asset_id
                 WHERE 1=1",
            );
        }
        CollectionFilter::Manual(collection) => {
            sql.push_str(
                "FROM collection_versions cv
                 JOIN develop_versions v ON v.id = cv.version_id
                 JOIN assets a ON a.id = v.asset_id
                 WHERE cv.collection_id = ?",
            );
            params.push(SqlValue::Integer(collection.get()));
        }
    }

    if let CollectionFilter::Smart(rules) = filter {
        if let Some(rating) = rules.rating {
            sql.push_str(" AND v.rating >= ?");
            params.push(SqlValue::Integer(i64::from(rating.gte)));
        }
        if let Some(camera) = &rules.camera {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM metadata m JOIN cameras cam ON cam.id = m.camera_id
                              WHERE m.asset_id = a.id
                                AND (cam.model = ?
                                     OR cam.manufacturer || ' ' || cam.model = ?))",
            );
            params.push(SqlValue::Text(camera.clone()));
            params.push(SqlValue::Text(camera.clone()));
        }
        for path in &rules.keywords {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM asset_keywords ak
                              JOIN keywords k ON k.id = ak.keyword_id
                              WHERE ak.asset_id = a.id
                                AND (k.path = ? OR k.path LIKE ? || '/%'))",
            );
            params.push(SqlValue::Text(path.clone()));
            params.push(SqlValue::Text(path.clone()));
        }
        match rules.pick {
            Some(true) => sql.push_str(" AND v.pick_state = 1"),
            Some(false) => sql.push_str(" AND v.pick_state <> 1"),
            None => {}
        }
    }

    if let Some(folder) = query.folder {
        sql.push_str(" AND a.folder_id = ?");
        params.push(SqlValue::Integer(folder.get()));
    }
    if let Some(rating) = query.rating_at_least {
        sql.push_str(" AND v.rating >= ?");
        params.push(SqlValue::Integer(i64::from(rating)));
    }
    if let Some(label) = query.color_label {
        sql.push_str(" AND v.color_label = ?");
        params.push(SqlValue::Integer(label.as_i64()));
    }
    if let Some(pick) = query.pick {
        sql.push_str(" AND v.pick_state = ?");
        params.push(SqlValue::Integer(pick.as_i64()));
    }
    for &keyword in &query.keywords {
        // Hierarchical: the keyword or any descendant (path prefix, §22).
        sql.push_str(
            " AND EXISTS (SELECT 1 FROM asset_keywords ak
                          JOIN keywords k ON k.id = ak.keyword_id
                          JOIN keywords root ON root.id = ?
                          WHERE ak.asset_id = a.id
                            AND (k.id = root.id OR k.path LIKE root.path || '/%'))",
        );
        params.push(SqlValue::Integer(keyword.get()));
    }
    if let Some(text) = query.text.as_deref()
        && let Some(fts) = crate::search::fts_query(text)
    {
        sql.push_str(" AND a.id IN (SELECT asset_id FROM search_index WHERE search_index MATCH ?)");
        params.push(SqlValue::Text(fts));
    }
    if let Some((start, end)) = query.capture_range {
        sql.push_str(" AND a.capture_date BETWEEN ? AND ?");
        params.push(SqlValue::Integer(start));
        params.push(SqlValue::Integer(end));
    }

    if ordered {
        let direction = |ascending| if ascending { "ASC" } else { "DESC" };
        match query.sort {
            Sort::CaptureDate { ascending } => {
                sql.push_str(&format!(
                    " ORDER BY a.capture_date IS NULL, a.capture_date {}, a.id",
                    direction(ascending)
                ));
            }
            Sort::Filename { ascending } => {
                sql.push_str(&format!(
                    " ORDER BY a.filename COLLATE NOCASE {}, a.id",
                    direction(ascending)
                ));
            }
            Sort::ImportedAt { ascending } => {
                sql.push_str(&format!(
                    " ORDER BY a.imported_at {}, a.id",
                    direction(ascending)
                ));
            }
            Sort::Rating { ascending } => {
                sql.push_str(&format!(
                    " ORDER BY v.rating IS NULL, v.rating {}, a.id",
                    direction(ascending)
                ));
            }
            Sort::CollectionOrder => {
                if !matches!(filter, CollectionFilter::Manual(_)) {
                    return Err(LeylineError::Db(
                        "collection order requires a manual collection filter".to_owned(),
                    ));
                }
                sql.push_str(" ORDER BY cv.position");
            }
        }
    }
    Ok((sql, params))
}
