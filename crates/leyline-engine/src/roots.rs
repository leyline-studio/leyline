//! Where a root currently sits, and how that is verified
//! ([ADR 0085](../../../docs/adr/0085-named-roots.md) §1–2).
//!
//! The catalog stores a root's **identity** and never its location. This
//! module holds the other half: a marker file that makes the identity
//! *verifiable*, and a hint file that remembers where the identity was last
//! seen on **this** machine.
//!
//! The split is what keeps [ADR 0010](../../../docs/adr/0010-relative-paths.md)
//! literally intact. `catalog.db` still contains no absolute path, so a
//! library folder copied to another OS still opens and still shows every
//! photograph it has a preview for — it just asks, once per external root,
//! where that root went.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use leyline_catalog::{Catalog, LIBRARY_ROOT};
use leyline_core::{AssetId, LeylineError, Result};

/// The marker a root folder carries, holding that root's UUID.
///
/// A folder **is** a given root if and only if its marker says so — not
/// because it sits at a remembered path, and not because a disk carries a
/// remembered label. The marker belongs to the folder rather than to a
/// library: two libraries may reference the same root, and neither owns it.
pub const MARKER: &str = ".leyline-root";

/// Where the hints live, beside `catalog.db`.
const HINTS: &str = "roots.json";

/// Writes `folder`'s marker. Idempotent, and overwrites a marker that
/// disagrees — the catalog is the authority on what root 1 is.
pub(crate) fn write_marker(folder: &Path, uuid: &str) -> Result<()> {
    std::fs::write(folder.join(MARKER), format!("{uuid}\n"))?;
    Ok(())
}

/// Reads a folder's marker.
///
/// `None` covers every way of not being a root — no marker, an unreadable
/// one, an empty one — because they are the same fact to every caller: this
/// folder does not identify itself, so nothing may be resolved through it.
pub(crate) fn read_marker(folder: &Path) -> Option<String> {
    let text = std::fs::read_to_string(folder.join(MARKER)).ok()?;
    let uuid = text.trim().to_owned();
    (!uuid.is_empty()).then_some(uuid)
}

/// Whether `folder` is the root with this identity, read from disk.
///
/// This is the verification every hint goes through before it is used. A
/// hint is never trusted on its own: a path can be reused by another folder,
/// a drive letter can be handed to a different volume, and a backup can be
/// restored somewhere unexpected — in all three cases the marker is what
/// tells the truth.
pub(crate) fn verifies(folder: &Path, uuid: &str) -> bool {
    read_marker(folder).is_some_and(|found| found == uuid)
}

/// Reads the hints, `uuid -> location on this machine`.
///
/// Advisory in the strict sense: a missing, unreadable or malformed file is
/// not an error, it is simply no hints. The file is rebuildable state, like
/// `Cache/`, and deleting it costs one re-location per external root.
pub(crate) fn read_hints(library_root: &Path) -> BTreeMap<String, PathBuf> {
    let Ok(text) = std::fs::read_to_string(library_root.join(HINTS)) else {
        return BTreeMap::new();
    };
    serde_json::from_str::<BTreeMap<String, PathBuf>>(&text).unwrap_or_default()
}

/// Records where a root was found, replacing any previous hint for it.
pub(crate) fn remember(library_root: &Path, uuid: &str, location: &Path) -> Result<()> {
    let mut hints = read_hints(library_root);
    hints.insert(uuid.to_owned(), location.to_owned());
    write_hints(library_root, &hints)
}

/// Drops a root's hint, for a root being forgotten.
pub(crate) fn forget(library_root: &Path, uuid: &str) -> Result<()> {
    let mut hints = read_hints(library_root);
    if hints.remove(uuid).is_none() {
        return Ok(());
    }
    write_hints(library_root, &hints)
}

fn write_hints(library_root: &Path, hints: &BTreeMap<String, PathBuf>) -> Result<()> {
    let json = serde_json::to_string_pretty(hints).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, format!("roots.json: {e}"))
    })?;
    std::fs::write(library_root.join(HINTS), format!("{json}\n"))?;
    Ok(())
}

/// Where an asset's file is, right now, on this machine (ADR 0085 §4).
///
/// **The one place a stored path becomes a real one.** It lives here rather
/// than on `Library` because the callers that open a photograph — export,
/// preview, print — are free functions taking a catalog and the library root,
/// and giving them a second, parallel resolution would defeat the point of
/// having one.
///
/// Three steps, and there is no fourth (ADR 0085 §2): the library's own root
/// needs no hint and is always root 1; otherwise the hint from `roots.json`,
/// **verified by reading the marker**; otherwise the root is offline. Mounted
/// volumes are never scanned looking for markers — it is slow, and it guesses.
pub(crate) fn locate(catalog: &Catalog, library_root: &Path, asset: AssetId) -> Result<PathBuf> {
    let (root_id, relative) = catalog.asset_location(asset)?;
    let base = location_of(catalog, library_root, root_id)?;
    Ok(base.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR)))
}

/// Resolves a root to a folder on this machine, or reports it offline.
pub(crate) fn location_of(catalog: &Catalog, library_root: &Path, root_id: i64) -> Result<PathBuf> {
    if root_id == LIBRARY_ROOT {
        return Ok(library_root.to_owned());
    }
    let root = catalog
        .roots()?
        .into_iter()
        .find(|r| r.id == root_id)
        .ok_or_else(|| {
            LeylineError::Db(format!("folder references a root {root_id} that is gone"))
        })?;

    match read_hints(library_root).get(&root.uuid) {
        Some(path) if verifies(path, &root.uuid) => Ok(path.clone()),
        _ => Err(LeylineError::RootOffline {
            name: root.name,
            uuid: root.uuid,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_is_a_root_only_if_its_marker_says_so() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_marker(dir.path()).is_none());
        assert!(!verifies(dir.path(), "abc"));

        write_marker(dir.path(), "abc").unwrap();
        assert_eq!(read_marker(dir.path()).as_deref(), Some("abc"));
        assert!(verifies(dir.path(), "abc"));
        // The identity is what is compared, never the location.
        assert!(!verifies(dir.path(), "def"));
    }

    #[test]
    fn an_empty_marker_identifies_nothing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MARKER), "   \n").unwrap();
        assert!(read_marker(dir.path()).is_none());
    }

    #[test]
    fn hints_are_advisory_and_survive_being_absent_or_broken() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_hints(dir.path()).is_empty());

        std::fs::write(dir.path().join(HINTS), "{ not json").unwrap();
        assert!(read_hints(dir.path()).is_empty(), "malformed is not fatal");

        remember(dir.path(), "abc", Path::new("/mnt/archive")).unwrap();
        assert_eq!(
            read_hints(dir.path()).get("abc").map(PathBuf::as_path),
            Some(Path::new("/mnt/archive"))
        );

        forget(dir.path(), "abc").unwrap();
        assert!(read_hints(dir.path()).is_empty());
    }
}
