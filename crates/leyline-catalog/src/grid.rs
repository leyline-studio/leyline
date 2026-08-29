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

/// An inclusive interval on a shot quantity, both bounds optional
/// (ADR 0064 §1): `ISO ≥ 3200` is asked far more often than `ISO in
/// [3200, 6400]`, so neither bound is required. Unbounded on both sides
/// filters nothing.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ShotRange {
    /// Lower bound, inclusive.
    pub min: Option<f64>,
    /// Upper bound, inclusive.
    pub max: Option<f64>,
}

impl ShotRange {
    /// A range bounded below only.
    pub fn at_least(min: f64) -> ShotRange {
        ShotRange {
            min: Some(min),
            max: None,
        }
    }

    /// A range bounded above only.
    pub fn at_most(max: f64) -> ShotRange {
        ShotRange {
            min: None,
            max: Some(max),
        }
    }

    /// A range bounded on both sides.
    pub fn between(min: f64, max: f64) -> ShotRange {
        ShotRange {
            min: Some(min),
            max: Some(max),
        }
    }

    /// Whether this range constrains nothing.
    pub fn is_unbounded(&self) -> bool {
        self.min.is_none() && self.max.is_none()
    }

    /// Reads the written form of an interval — `min-max`, `min-`, `-max`, or
    /// a single value standing for both bounds — as the CLI and Studio both
    /// take it (ADR 0064 §5). Blank text is the absent filter.
    ///
    /// Bounds may be fractions: `1/200` is how a shutter speed is read
    /// everywhere else, and making the filter the one place it is not would
    /// be a small cruelty.
    pub fn parse(text: &str) -> Result<ShotRange> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(ShotRange::default());
        }
        let number = |part: &str| -> Result<f64> {
            let refused = || {
                LeylineError::InvalidSettings(format!(
                    "cannot read {text:?} as a range: expected <min>-<max>, <min>- or -<max>"
                ))
            };
            match part.split_once('/') {
                Some((numerator, denominator)) => {
                    let numerator: f64 = numerator.trim().parse().map_err(|_| refused())?;
                    let denominator: f64 = denominator.trim().parse().map_err(|_| refused())?;
                    if denominator == 0.0 {
                        return Err(refused());
                    }
                    Ok(numerator / denominator)
                }
                None => part.trim().parse().map_err(|_| refused()),
            }
        };
        if let Some(max) = text.strip_prefix('-') {
            Ok(ShotRange::at_most(number(max)?))
        } else if let Some(min) = text.strip_suffix('-') {
            Ok(ShotRange::at_least(number(min)?))
        } else if let Some((min, max)) = text.split_once('-') {
            Ok(ShotRange::between(number(min)?, number(max)?))
        } else {
            let exact = number(text)?;
            Ok(ShotRange::between(exact, exact))
        }
    }
}

/// A grid request: filters, sort, and the visible window.
#[derive(Debug, Clone, PartialEq)]
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
    /// Only assets shot with this body, named as [`Catalog::shot_facets`]
    /// names it — or by model alone (ADR 0064 §2).
    pub camera: Option<String>,
    /// Only assets shot with this lens, named the same way.
    pub lens: Option<String>,
    /// Only assets whose ISO falls in this range.
    pub iso: ShotRange,
    /// Only assets whose f-number falls in this range.
    pub aperture: ShotRange,
    /// Only assets whose focal length, in millimeters, falls in this range.
    pub focal_length: ShotRange,
    /// Only assets whose shutter speed, in seconds, falls in this range.
    pub shutter_speed: ShotRange,
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
            camera: None,
            lens: None,
            iso: ShotRange::default(),
            aperture: ShotRange::default(),
            focal_length: ShotRange::default(),
            shutter_speed: ShotRange::default(),
            sort: Sort::CaptureDate { ascending: false },
            range: 0..1000,
        }
    }
}

/// The grid select list, in order.
///
/// [`Catalog::grid`] reads its rows **by position**, which is only safe as
/// long as the SQL built by `grid_sql` selects these names in this order —
/// and that SQL varies with the sort. This array is the declared order, and
/// [`Catalog::grid_columns`] is how a test compares it with the one SQLite
/// actually prepares. Reordering the select list without reordering this
/// array fails that test rather than swapping two integer columns in silence.
pub const GRID_COLUMNS: [&str; 12] = [
    "version_id",
    "asset_id",
    "filename",
    "capture_date",
    "rating",
    "color_label",
    "pick_state",
    "width",
    "height",
    "edited",
    "paired",
    "root_id",
];

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
    /// Whether a camera rendering of the same shot is attached to this
    /// asset — what a cell's `RAW+J` badge shows (ADR 0079 §6).
    pub paired: bool,
    /// Which root holds the asset (ADR 0085 §5).
    ///
    /// Carried by the grid rather than looked up per cell because the client
    /// needs it on every visible thumbnail — to mark the ones whose volume is
    /// unplugged — and a query per cell is a query per scroll step.
    pub root_id: i64,
}

impl Catalog {
    /// The subset of `assets` the grid actually shows, in the order it shows
    /// them — the default sort, newest capture first (ADR 0082 §4).
    ///
    /// Two jobs in one query, and deliberately so: a warming pass wants to
    /// start with what the user will see first, and it must not spend a
    /// second on a companion, which no grid ever draws (ADR 0079 §5). Both
    /// facts live in the same clause here, so they cannot drift apart the way
    /// two separate filters would.
    ///
    /// Unknown ids simply do not come back.
    pub fn grid_order(&self, assets: &[AssetId]) -> Result<Vec<AssetId>> {
        if assets.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat_n("?", assets.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT id FROM assets
             WHERE id IN ({placeholders}) AND companion_of IS NULL
             ORDER BY capture_date DESC, id"
        );
        let mut stmt = self.conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(assets.iter().map(|a| a.get())),
                |row| row.get::<_, i64>(0).map(AssetId::new),
            )
            .map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }

    /// Counts the versions matching the query, ignoring `range` and `sort`.
    pub fn count(&self, query: &GridQuery) -> Result<u64> {
        let filter = self.resolve_collection(query)?;
        let (core, params) = core(query, &filter)?;
        // Counting sorts nothing and renders no column, so the deferred page
        // of ADR 0081 §1 has nothing to offer it: one flat query, as before.
        let sql = format!("SELECT COUNT(*) {core}");
        self.conn
            .query_row(&sql, rusqlite::params_from_iter(params), |row| {
                row.get::<_, i64>(0)
            })
            .map(|n| n as u64)
            .map_err(db_err)
    }

    /// How SQLite says it will answer one grid query, step by step.
    ///
    /// A diagnostic, and the only honest way to test the shape of a query
    /// that is built privately: a test that rebuilt the SQL itself would
    /// check its own copy rather than the one that runs (ADR 0081 §1).
    pub fn grid_plan(&self, query: &GridQuery) -> Result<Vec<String>> {
        let (sql, params) = self.grid_sql(query)?;
        let mut stmt = self
            .conn
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params), |row| row.get(3))
            .map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<String>, _>>()
            .map_err(db_err)
    }

    /// The columns one grid query really selects, in the order SQLite
    /// prepares them.
    ///
    /// A diagnostic, for the same reason as [`Catalog::grid_plan`]: the SQL is
    /// built privately and varies with the sort, so a test that rebuilt it
    /// would check its own copy. Compare against [`GRID_COLUMNS`].
    pub fn grid_columns(&self, query: &GridQuery) -> Result<Vec<String>> {
        let (sql, _) = self.grid_sql(query)?;
        let stmt = self.conn.prepare(&sql).map_err(db_err)?;
        Ok(stmt.column_names().into_iter().map(str::to_owned).collect())
    }

    /// Builds the two-level page query of ADR 0081 §1 and its parameters.
    fn grid_sql(&self, query: &GridQuery) -> Result<(String, Vec<SqlValue>)> {
        let filter = self.resolve_collection(query)?;
        let (core, mut params) = core(query, &filter)?;
        let order = order_by(query, &filter)?;

        // ADR 0081 §1: the window is chosen on two integers per row, and only
        // the survivors are decorated. Sorting the eleven columns below —
        // two of them correlated subqueries — would run them once per row in
        // the library to render a hundred cells, since SQLite materialises
        // the output row before it sorts.
        //
        // `v.id`/`a.id` name the same two rows under either shape of trunk;
        // the collection's own order is the one key the outer query cannot
        // recompute, so it travels as a column.
        let mut keys = "v.id AS vid, a.id AS aid".to_owned();
        let outer_order = if matches!(query.sort, Sort::CollectionOrder) {
            keys.push_str(", cv.position AS ord");
            " ORDER BY page.ord".to_owned()
        } else {
            order.clone()
        };
        params.push(SqlValue::Integer(i64::from(
            query.range.end - query.range.start,
        )));
        params.push(SqlValue::Integer(i64::from(query.range.start)));

        let sql = format!(
            "WITH page AS (SELECT {keys} {core}{order} LIMIT ? OFFSET ?)
             SELECT v.id AS version_id, a.id AS asset_id, a.filename, a.capture_date,
                    v.rating, v.color_label, v.pick_state, a.width, a.height,
                    (SELECT r.parent_revision_id IS NOT NULL FROM develop_revisions r
                      WHERE r.id = v.head_revision_id) AS edited,
                    EXISTS (SELECT 1 FROM assets p WHERE p.companion_of = a.id) AS paired,
                    f.root_id
             FROM page
             JOIN develop_versions v ON v.id = page.vid
             JOIN assets a ON a.id = page.aid
             JOIN folders f ON f.id = a.folder_id{outer_order}"
        );

        Ok((sql, params))
    }

    /// Returns the visible window of the grid.
    pub fn grid(&self, query: &GridQuery) -> Result<Vec<GridItem>> {
        if query.range.is_empty() {
            return Ok(Vec::new());
        }
        let (sql, params) = self.grid_sql(query)?;

        let mut stmt = self.conn.prepare(&sql).map_err(db_err)?;
        // Read by position, in the order [`GRID_COLUMNS`] declares. `grid`
        // runs once per scroll step on the interface thread, and reading by
        // name would scan the statement's column names on every column of
        // every row — measured at +35 % on the head page of a 50,000-asset
        // library. `grid_columns_match_the_declared_order` is what makes the
        // positions safe; it costs nothing at runtime.
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
                    paired: row.get(10)?,
                    root_id: row.get(11)?,
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

/// Assembles the `FROM`/`WHERE` trunk shared by the grid and the count, and
/// the parameters it binds — everything but the selected columns and the
/// order (ADR 0081 §4).
fn core(query: &GridQuery, filter: &CollectionFilter) -> Result<(String, Vec<SqlValue>)> {
    let mut params: Vec<SqlValue> = Vec::new();
    let mut sql = String::new();

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

    // A companion is not a photo of its own (ADR 0079 §5). One clause, on
    // both shapes of query, and everything downstream follows without
    // knowing pairs exist: the count, the shot filters, the full-text
    // search and the smart collections all read this same statement.
    sql.push_str(" AND a.companion_of IS NULL");

    if let CollectionFilter::Smart(rules) = filter {
        if let Some(rating) = rules.rating {
            sql.push_str(" AND v.rating >= ?");
            params.push(SqlValue::Integer(i64::from(rating.gte)));
        }
        if let Some(camera) = &rules.camera {
            crate::metadata::camera_clause(&mut sql, &mut params, camera);
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
    if let Some(camera) = query.camera.as_deref() {
        crate::metadata::camera_clause(&mut sql, &mut params, camera);
    }
    if let Some(lens) = query.lens.as_deref() {
        crate::metadata::lens_clause(&mut sql, &mut params, lens);
    }
    for (name, column, range) in [
        ("iso", "iso", query.iso),
        ("aperture", "aperture_f", query.aperture),
        ("focal length", "focal_length_mm", query.focal_length),
        ("shutter speed", "shutter_speed_s", query.shutter_speed),
    ] {
        shot_range_clause(&mut sql, &mut params, name, column, range)?;
    }

    Ok((sql, params))
}

/// The `ORDER BY` clause of one query, leading space included.
///
/// Undated assets come last whichever way the dates run, and that is said
/// without a leading expression (ADR 0081 §2): descending, SQLite already
/// sorts NULLs last, so naming it decided nothing while forbidding every
/// index; ascending, `NULLS LAST` says the same thing and stays indexable.
/// The rating keeps its expression — it sorts a column of another table,
/// which no index of `assets` could serve anyway.
fn order_by(query: &GridQuery, filter: &CollectionFilter) -> Result<String> {
    let direction = |ascending| if ascending { "ASC" } else { "DESC" };
    Ok(match query.sort {
        Sort::CaptureDate { ascending: true } => {
            " ORDER BY a.capture_date ASC NULLS LAST, a.id".to_owned()
        }
        Sort::CaptureDate { ascending: false } => " ORDER BY a.capture_date DESC, a.id".to_owned(),
        Sort::Filename { ascending } => format!(
            " ORDER BY a.filename COLLATE NOCASE {}, a.id",
            direction(ascending)
        ),
        Sort::ImportedAt { ascending } => {
            format!(" ORDER BY a.imported_at {}, a.id", direction(ascending))
        }
        Sort::Rating { ascending } => format!(
            " ORDER BY v.rating IS NULL, v.rating {}, a.id",
            direction(ascending)
        ),
        Sort::CollectionOrder => {
            if !matches!(filter, CollectionFilter::Manual(_)) {
                return Err(LeylineError::Db(
                    "collection order requires a manual collection filter".to_owned(),
                ));
            }
            " ORDER BY cv.position".to_owned()
        }
    })
}

/// Appends one continuous shot filter, on an indexed column of `metadata`
/// (`docs/catalog.md` §32). An asset without that measurement does not
/// satisfy the criterion and drops out, as ADR 0064 §1 requires.
///
/// A reversed range is refused rather than answered with an empty grid: it
/// can only be a mistake, and a silent zero would look like a library that
/// holds nothing of the kind.
fn shot_range_clause(
    sql: &mut String,
    params: &mut Vec<SqlValue>,
    name: &str,
    column: &str,
    range: ShotRange,
) -> Result<()> {
    if range.is_unbounded() {
        return Ok(());
    }
    if let (Some(min), Some(max)) = (range.min, range.max)
        && min > max
    {
        return Err(LeylineError::InvalidSettings(format!(
            "{name} filter: minimum {min} is above maximum {max}"
        )));
    }
    sql.push_str(&format!(
        " AND EXISTS (SELECT 1 FROM metadata m WHERE m.asset_id = a.id AND m.{column} IS NOT NULL"
    ));
    if let Some(min) = range.min {
        sql.push_str(&format!(" AND m.{column} >= ?"));
        params.push(SqlValue::Real(min));
    }
    if let Some(max) = range.max {
        sql.push_str(&format!(" AND m.{column} <= ?"));
        params.push(SqlValue::Real(max));
    }
    sql.push(')');
    Ok(())
}
