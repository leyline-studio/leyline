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

/// One row of the folder tree, as a sidebar lists it (ADR 0055 §2).
///
/// Flat rather than nested, unlike [`crate::CollectionNode`]: folder rows are
/// produced already in display order by the path sort, so a tree would be
/// built only to be flattened again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderNode {
    /// The folder itself.
    pub folder: FolderId,
    /// Its parent, absent for a root folder.
    pub parent: Option<FolderId>,
    /// Library-relative, forward-slashed path (§2.3).
    pub relative_path: String,
    /// Photos **directly** in this folder, missing ones excluded — the count
    /// a folder row shows next to its name, which answers "is there anything
    /// in there" rather than "how big is this subtree".
    pub photo_count: u32,
}

impl Catalog {
    /// Every folder, with its photo count, ordered by path.
    ///
    /// Sorting on the full relative path *is* depth-first order — a child's
    /// path always starts with its parent's — so the caller can indent on
    /// segment count without walking anything.
    pub fn folders(&self) -> Result<Vec<FolderNode>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT f.id, f.parent_id, f.relative_path, COUNT(a.id)
                 FROM folders f
                 LEFT JOIN assets a ON a.folder_id = f.id AND a.is_missing = 0
                 GROUP BY f.id
                 ORDER BY f.relative_path",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(FolderNode {
                    folder: FolderId::new(row.get::<_, i64>(0)?),
                    parent: row.get::<_, Option<i64>>(1)?.map(FolderId::new),
                    relative_path: row.get::<_, String>(2)?,
                    photo_count: row.get::<_, i64>(3)?.try_into().unwrap_or(u32::MAX),
                })
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }

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
