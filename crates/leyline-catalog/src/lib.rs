//! SQLite catalog: libraries, assets, versions and revisions.
//!
//! This crate owns `catalog.db`: opening and creating it, applying the
//! per-connection configuration (`docs/catalog.md` §6) and the incremental
//! `user_version` migrations (§34). The catalog is an implementation detail of
//! the engine, never a public interface (`docs/engine-api.md` §14): clients go
//! through `leyline-sdk`.

mod connection;
mod migrations;

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, ErrorCode, OpenFlags};

use leyline_core::{LeylineError, Result};

/// Identity row of a library (`docs/catalog.md` §7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryInfo {
    /// Stable unique identifier of the library.
    pub uuid: String,
    /// Human-readable library name.
    pub name: String,
    /// Creation time, UTC Unix epoch milliseconds.
    pub created_at: i64,
    /// Last update time, UTC Unix epoch milliseconds.
    pub updated_at: i64,
}

/// An open `catalog.db` connection, configured and migrated.
#[derive(Debug)]
pub struct Catalog {
    conn: Connection,
    read_only: bool,
}

impl Catalog {
    /// The schema version this crate produces and supports.
    pub const SCHEMA_VERSION: u32 = migrations::SCHEMA_VERSION;

    /// Creates a new catalog at `path` (the `catalog.db` file itself) and
    /// inserts the single `library` row. Fails if the file already exists.
    pub fn create(path: &Path, name: &str) -> Result<Catalog> {
        if path.exists() {
            return Err(LeylineError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("a catalog already exists at {}", path.display()),
            )));
        }
        let mut conn = Connection::open(path).map_err(db_err)?;
        connection::configure(&conn)?;
        migrations::migrate(&mut conn)?;

        let now = now_ms();
        conn.execute(
            "INSERT INTO library (uuid, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
            rusqlite::params![uuid::Uuid::new_v4().to_string(), name, now],
        )
        .map_err(db_err)?;

        Ok(Catalog {
            conn,
            read_only: false,
        })
    }

    /// Opens an existing catalog and applies any pending migration.
    ///
    /// A catalog written by a newer engine is refused with
    /// [`LeylineError::NewerCatalog`]: use [`Catalog::open_read_only`] instead.
    pub fn open(path: &Path) -> Result<Catalog> {
        if !path.is_file() {
            return Err(LeylineError::LibraryNotFound(path.to_owned()));
        }
        let mut conn = Connection::open(path).map_err(db_err)?;
        connection::configure(&conn)?;

        let found = migrations::user_version(&conn)?;
        if found > Self::SCHEMA_VERSION {
            return Err(LeylineError::NewerCatalog {
                found,
                supported: Self::SCHEMA_VERSION,
            });
        }
        migrations::migrate(&mut conn)?;

        Ok(Catalog {
            conn,
            read_only: false,
        })
    }

    /// Opens an existing catalog without any write access.
    ///
    /// Accepts catalogs newer than [`Catalog::SCHEMA_VERSION`]: a newer schema
    /// is readable but must never be modified (`docs/pipeline.md` §3.4).
    pub fn open_read_only(path: &Path) -> Result<Catalog> {
        if !path.is_file() {
            return Err(LeylineError::LibraryNotFound(path.to_owned()));
        }
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(db_err)?;
        connection::configure(&conn)?;

        Ok(Catalog {
            conn,
            read_only: true,
        })
    }

    /// Returns the schema version (`PRAGMA user_version`) of the open catalog.
    pub fn user_version(&self) -> Result<u32> {
        migrations::user_version(&self.conn)
    }

    /// Whether this handle was opened without write access.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Reads the single `library` identity row.
    pub fn library(&self) -> Result<LibraryInfo> {
        self.conn
            .query_row(
                "SELECT uuid, name, created_at, updated_at FROM library",
                [],
                |row| {
                    Ok(LibraryInfo {
                        uuid: row.get(0)?,
                        name: row.get(1)?,
                        created_at: row.get(2)?,
                        updated_at: row.get(3)?,
                    })
                },
            )
            .map_err(db_err)
    }

    /// The underlying SQLite connection.
    ///
    /// For use by the engine crates only: the catalog schema is not a public
    /// interface (`docs/engine-api.md` §14).
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

/// Maps a SQLite error to the platform error type.
fn db_err(error: rusqlite::Error) -> LeylineError {
    match &error {
        rusqlite::Error::SqliteFailure(e, _)
            if matches!(e.code, ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) =>
        {
            LeylineError::LibraryLocked
        }
        _ => LeylineError::Db(error.to_string()),
    }
}

/// Current time as UTC Unix epoch milliseconds (`docs/catalog.md` §7).
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_millis() as i64
}
