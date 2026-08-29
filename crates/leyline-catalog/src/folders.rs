//! Folder tree operations (`docs/catalog.md` §8).
//!
//! Folders mirror the physical tree under the library root and carry no
//! business information. Paths are always relative, always forward-slashed
//! (§2.3): portability across Windows, Linux and macOS is a schema rule, not
//! a display concern.

use leyline_core::{FolderId, LeylineError, Result};

use crate::{Catalog, db_err, now_ms};

/// Checks that a folder path is relative, normalized and portable.
///
/// The empty path never reaches here: it is the library root's own folder
/// row (`docs/catalog.md` §8), handled by [`Catalog::ensure_folder`] before
/// any validation, and it is the one path that has no segment to check.
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
    /// Library-relative, forward-slashed path (§2.3). **Empty for the
    /// library root's own row**, which exists as soon as a photograph sits
    /// directly under the root rather than in a subfolder (§8).
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
            .prepare_cached(
                "SELECT f.id, f.parent_id, f.relative_path, COUNT(a.id) AS photo_count
                 FROM folders f
                 LEFT JOIN assets a ON a.folder_id = f.id AND a.is_missing = 0
                 GROUP BY f.id
                 ORDER BY f.relative_path",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(FolderNode {
                    folder: FolderId::new(row.get::<_, i64>("id")?),
                    parent: row.get::<_, Option<i64>>("parent_id")?.map(FolderId::new),
                    relative_path: row.get::<_, String>("relative_path")?,
                    photo_count: row
                        .get::<_, i64>("photo_count")?
                        .try_into()
                        .unwrap_or(u32::MAX),
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
    ///
    /// **The empty path is the library root itself** (§8): a photograph may
    /// sit directly under the root, and it needs a folder row like any
    /// other. That row is created on demand — a library whose photographs
    /// all live in subfolders never has one — carries no parent, and is the
    /// only path allowed to have no segment.
    pub fn ensure_folder(&mut self, relative_path: &str) -> Result<FolderId> {
        self.ensure_folder_in(crate::LIBRARY_ROOT, relative_path)
    }

    /// Same, for a path relative to `root` rather than to the library
    /// (ADR 0085 §3).
    ///
    /// A path is relative to **its own root**, so the same `"2019/Iceland"`
    /// may exist under two roots and mean two folders — which is why the
    /// lookup below is keyed by both, matching `UNIQUE(root_id,
    /// relative_path)`. Reading `relative_path` alone was correct only for
    /// as long as a library had one root, and stops being correct silently.
    pub fn ensure_folder_in(&mut self, root: i64, relative_path: &str) -> Result<FolderId> {
        self.ensure_writable()?;
        if relative_path.is_empty() {
            return self.ensure_single_folder(root, None, "");
        }
        validate_relative_path(relative_path)?;

        let mut parent: Option<FolderId> = None;
        let mut current = String::new();
        for segment in relative_path.split('/') {
            if !current.is_empty() {
                current.push('/');
            }
            current.push_str(segment);
            parent = Some(self.ensure_single_folder(root, parent, &current)?);
        }
        Ok(parent.expect("path has at least one segment"))
    }

    /// Finds or inserts one folder row whose full path is `relative_path`
    /// within `root`.
    fn ensure_single_folder(
        &self,
        root: i64,
        parent: Option<FolderId>,
        relative_path: &str,
    ) -> Result<FolderId> {
        let existing = self
            .conn
            .query_row(
                "SELECT id FROM folders WHERE root_id = ?1 AND relative_path = ?2",
                rusqlite::params![root, relative_path],
                |row| row.get::<_, i64>(0),
            )
            .map(FolderId::new);
        match existing {
            Ok(id) => Ok(id),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                self.conn
                    .execute(
                        "INSERT INTO folders (parent_id, root_id, relative_path, created_at)
                         VALUES (?1, ?2, ?3, ?4)",
                        rusqlite::params![parent.map(FolderId::get), root, relative_path, now_ms()],
                    )
                    .map_err(db_err)?;
                Ok(FolderId::new(self.conn.last_insert_rowid()))
            }
            Err(e) => Err(db_err(e)),
        }
    }
}
