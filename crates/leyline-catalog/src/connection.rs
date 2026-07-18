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
    .map_err(db_err)
}
