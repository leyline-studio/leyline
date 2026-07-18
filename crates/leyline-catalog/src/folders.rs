//! Folder tree operations (`docs/catalog.md` §8).
//!
//! Folders mirror the physical tree under the library root and carry no
//! business information. Paths are always relative, always forward-slashed
//! (§2.3): portability across Windows, Linux and macOS is a schema rule, not
//! a display concern.

use leyline_core::{FolderId, LeylineError, Result};

use crate::{Catalog, db_err, now_ms};

/// Checks that a folder path is relative, normalized and portable.
fn validate_relative_path(path: &str) -> Result<()> {
    let invalid = |reason: &str| {
        Err(LeylineError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("invalid folder path {path:?}: {reason}"),
        )))
    };
    if path.is_empty() {
        return invalid("empty path");
    }
    if path.contains('\\') {
        return invalid("use forward slashes");
    }
    if path.starts_with('/') || path.contains(':') {
        return invalid("path must be relative to the library root");
    }
    if path.ends_with('/') {
        return invalid("no trailing slash");
    }
    for segment in path.split('/') {
        match segment {
            "" => return invalid("empty segment"),
            "." | ".." => return invalid("no dot segments"),
            _ => {}
        }
    }
    Ok(())
}

impl Catalog {
    /// Returns the folder at `relative_path`, creating it — and every missing
    /// ancestor — if needed. Idempotent.
    ///
    /// `relative_path` is relative to the library root, forward-slashed, e.g.
    /// `"Photos/Wildlife"`.
    pub fn ensure_folder(&mut self, relative_path: &str) -> Result<FolderId> {
        self.ensure_writable()?;
        validate_relative_path(relative_path)?;

        let mut parent: Option<FolderId> = None;
        let mut current = String::new();
        for segment in relative_path.split('/') {
            if !current.is_empty() {
                current.push('/');
            }
            current.push_str(segment);
            parent = Some(self.ensure_single_folder(parent, &current)?);
        }
        Ok(parent.expect("path has at least one segment"))
    }

    /// Finds or inserts one folder row whose full path is `relative_path`.
    fn ensure_single_folder(
        &self,
        parent: Option<FolderId>,
        relative_path: &str,
    ) -> Result<FolderId> {
        let existing = self
            .conn
            .query_row(
                "SELECT id FROM folders WHERE relative_path = ?1",
                [relative_path],
                |row| row.get::<_, i64>(0),
            )
            .map(FolderId::new);
        match existing {
            Ok(id) => Ok(id),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                self.conn
                    .execute(
                        "INSERT INTO folders (parent_id, relative_path, created_at)
                         VALUES (?1, ?2, ?3)",
                        rusqlite::params![parent.map(FolderId::get), relative_path, now_ms()],
                    )
                    .map_err(db_err)?;
                Ok(FolderId::new(self.conn.last_insert_rowid()))
            }
            Err(e) => Err(db_err(e)),
        }
    }
}
