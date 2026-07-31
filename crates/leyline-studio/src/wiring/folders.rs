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
    let rows: Vec<crate::ui::FolderRow> = tree
        .iter()
        .map(|node| crate::ui::FolderRow {
            name: SharedString::from(folder_name(&node.relative_path)),
            depth: folder_depth(&node.relative_path),
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

/// How deep a folder sits, counting from zero at the library root.
fn folder_depth(relative_path: &str) -> i32 {
    i32::try_from(relative_path.matches('/').count()).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_shows_its_last_segment_at_the_depth_of_its_path() {
        assert_eq!(folder_name("Photos"), "Photos");
        assert_eq!(folder_depth("Photos"), 0);
        assert_eq!(folder_name("Photos/Wildlife/Birds"), "Birds");
        assert_eq!(folder_depth("Photos/Wildlife/Birds"), 2);
    }
}
