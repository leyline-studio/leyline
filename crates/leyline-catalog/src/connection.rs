//! Per-connection SQLite configuration (`docs/catalog.md` §6).

use rusqlite::Connection;

use crate::db_err;
use leyline_core::Result;

/// Applies the mandatory pragmas to a freshly opened connection.
///
/// Every pragma here is per-connection, except `journal_mode = WAL` which is
/// persistent; re-issuing it on an already-WAL database is a no-op, so the
/// same configuration path serves read-only connections too.
pub(crate) fn configure(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA temp_store = MEMORY;
         PRAGMA cache_size = -65536;",
    )
    .map_err(db_err)?;

    // Statements whose SQL is a **literal** are prepared through
    // `prepare_cached`: the same text runs again and again — once per grid
    // cell for the preview lookup, once per file for the import — and
    // re-parsing it costs about as much as running it (measured: 3,2 µs
    // against 1,6 µs for a point read). Statements assembled at runtime are
    // deliberately left uncached, the cache being keyed by the SQL text: a
    // grid query carries its filters in that text, so caching it would fill
    // the cache with entries no second call ever matches.
    //
    // The default capacity is 16 and the crate holds rather more literal
    // statements than that; too small a cache silently evicts the hot ones
    // and gives back exactly what caching was meant to save.
    conn.set_prepared_statement_cache_capacity(64);
    Ok(())
}
