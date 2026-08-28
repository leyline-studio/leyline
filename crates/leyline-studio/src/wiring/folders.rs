//! Wires `FolderState`: the sidebar's folder tree (ADR 0045 §4, ADR 0055 §2).
//!
//! Read-only on purpose — there is no rename, move or delete here, because
//! moving a folder is file management rather than cataloguing.

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::{App, report_error};
use crate::ui::{CollectionState, FolderState, StudioWindow};
use crate::wiring::grid::reload;
use slint::{ComponentHandle, Global, ModelRc, SharedString, VecModel};

/// Connects the folder tree of the sidebar.
pub(crate) fn wire_folders(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    let app = Rc::clone(app);
    let handle = window.as_weak();
    FolderState::get(window).on_select_folder(move |index| {
        let Some(window) = handle.upgrade() else {
            return;
        };
        let mut app = app.borrow_mut();
        let picked = usize::try_from(index)
            .ok()
            .and_then(|i| app.folders.get(i))
            .copied();
        FolderState::get(&window).set_active_folder(if picked.is_some() { index } else { -1 });
        // The sidebar carries one selection across catalog, folders and
        // collections (ADR 0055 §2): picking a folder — or the "All photos"
        // row, which arrives here as -1 — drops the collection filter, so
        // the grid never shows an intersection nobody asked for.
        CollectionState::get(&window).set_active_collection(-1);
        app.query.folder = picked;
        app.query.collection = None;
        if let Err(error) = reload(&mut app, &window) {
            report_error(&window, &error);
        }
    });
}

/// Reloads the sidebar from the catalog's folder tree.
///
/// The rows arrive already in display order, so the only thing computed here
/// is what a row *shows*: its last path segment, and how deep it sits.
pub(crate) fn refresh_folders(app: &mut App, window: &StudioWindow) -> Result<(), String> {
    let tree = app.library.folders().map_err(|e| e.to_string())?;
    app.folders = tree.iter().map(|node| node.folder).collect();
    // The library root gets a row of its own as soon as a photograph sits
    // directly under it (catalogue §8). It has no last segment to show, so it
    // shows the library's name; and since it is everything else's parent, its
    // presence shifts the whole tree one indent to the right.
    let has_root = tree.iter().any(|node| node.relative_path.is_empty());
    let library_name = if has_root {
        app.library
            .info()
            .map(|info| info.name)
            .unwrap_or_else(|_| String::new())
    } else {
        String::new()
    };
    let rows: Vec<crate::ui::FolderRow> = tree
        .iter()
        .map(|node| crate::ui::FolderRow {
            name: SharedString::from(if node.relative_path.is_empty() {
                library_name.as_str()
            } else {
                folder_name(&node.relative_path)
            }),
            depth: folder_depth(&node.relative_path, has_root),
            count: i32::try_from(node.photo_count).unwrap_or(i32::MAX),
        })
        .collect();
    FolderState::get(window).set_folders(ModelRc::from(Rc::new(VecModel::from(rows))));
    Ok(())
}

/// The last segment of a library-relative folder path — what the sidebar
/// shows, the ancestors being expressed by the indent instead.
fn folder_name(relative_path: &str) -> &str {
    relative_path.rsplit('/').next().unwrap_or(relative_path)
}

/// How deep a folder sits, counting from zero.
///
/// `has_root` says whether the tree carries the library root's own row. When
/// it does, that row is depth zero and everything else is one level deeper,
/// because the root really is their parent; when it does not — the ordinary
/// case, no photograph directly under the root — nothing moves.
fn folder_depth(relative_path: &str, has_root: bool) -> i32 {
    if relative_path.is_empty() {
        return 0;
    }
    let own = i32::try_from(relative_path.matches('/').count()).unwrap_or(i32::MAX);
    own.saturating_add(i32::from(has_root))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_shows_its_last_segment_at_the_depth_of_its_path() {
        assert_eq!(folder_name("Photos"), "Photos");
        assert_eq!(folder_depth("Photos", false), 0);
        assert_eq!(folder_name("Photos/Wildlife/Birds"), "Birds");
        assert_eq!(folder_depth("Photos/Wildlife/Birds", false), 2);
    }

    /// The root's own row (catalogue §8) is the tree's first row and every
    /// other row's parent, so it sits at zero and pushes the rest right.
    #[test]
    fn the_library_root_row_sits_above_everything_else() {
        assert_eq!(folder_depth("", true), 0);
        assert_eq!(folder_depth("Photos", true), 1);
        assert_eq!(folder_depth("Photos/Wildlife/Birds", true), 3);
    }
}
