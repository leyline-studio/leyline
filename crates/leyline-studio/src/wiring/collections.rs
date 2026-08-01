//! Wires `CollectionState` (ADR 0045 §4).

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::{App, item_at, report_error};
use crate::ui::{CollectionState, DialogState, FolderState, GridState, StudioWindow, Tr};
use crate::wiring::grid::reload;
use leyline_sdk::{CollectionId, CollectionNode, CollectionType};
use slint::{ComponentHandle, Global, Model, ModelRc, SharedString, VecModel};

/// Connects the collections sidebar and its creation dialog.
pub(crate) fn wire_collections(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        CollectionState::get(window).on_select_collection(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let picked = usize::try_from(index)
                .ok()
                .and_then(|i| app.collections.get(i))
                .copied();
            CollectionState::get(&window).set_active_collection(if picked.is_some() {
                index
            } else {
                -1
            });
            // One selection across the whole sidebar (ADR 0055 §2): a
            // collection replaces the folder filter rather than narrowing it.
            FolderState::get(&window).set_active_folder(-1);
            app.query.folder = None;
            app.query.collection = picked;
            if let Err(error) = reload(&mut app, &window) {
                report_error(&window, &error);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        CollectionState::get(window).on_run_new_collection(move |name| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let name = name.trim();
            if name.is_empty() {
                DialogState::get(&window).set_dialog_result(Tr::get(&window).invoke_enter_a_name());
                return;
            }
            let mut app = app.borrow_mut();
            let created = app
                .library
                .create_collection(None, name)
                .map_err(|e| e.to_string())
                .and_then(|_| refresh_collections(&mut app, &window));
            match created {
                Ok(()) => DialogState::get(&window).set_dialog(SharedString::default()),
                Err(error) => DialogState::get(&window).set_dialog_result(
                    Tr::get(&window).invoke_creation_failed(SharedString::from(error)),
                ),
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        CollectionState::get(window).on_add_to_collection(move || {
            if let Some(window) = handle.upgrade() {
                collection_membership(&mut app.borrow_mut(), &window, true);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        CollectionState::get(window).on_remove_from_collection(move || {
            if let Some(window) = handle.upgrade() {
                collection_membership(&mut app.borrow_mut(), &window, false);
            }
        });
    }
}

/// Connects the three operations that manage the tree itself
/// (`docs/catalog.md` §24): rename, move, delete.
///
/// Each is two steps — an `open_*` that gathers what the dialog has to show,
/// and a `run_*` that performs it — because none of the three can be
/// described without asking the catalog side first.
pub(crate) fn wire_collection_management(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        CollectionState::get(window).on_open_rename(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let app = app.borrow();
            if target(&app, &window, index).is_none() {
                return;
            }
            open_dialog(&window, "collection-rename");
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        CollectionState::get(window).on_open_move(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            if target(&app, &window, index).is_none() {
                return;
            }
            // Everything except the collection itself and its descendants:
            // the catalog would refuse the rest anyway, and an option that
            // is always refused should not be offered.
            let subtree = subtree_range(&app, index);
            let mut labels = vec![Tr::get(&window).invoke_top_level()];
            let mut targets: Vec<Option<CollectionId>> = vec![None];
            let rows = CollectionState::get(&window).get_collections();
            let self_row = usize::try_from(index).unwrap_or(usize::MAX);
            for (position, (row, id)) in rows.iter().zip(app.collections.iter()).enumerate() {
                if position == self_row || subtree.contains(&position) {
                    continue;
                }
                labels.push(SharedString::from(format!(
                    "{}{}",
                    "    ".repeat(usize::try_from(row.depth).unwrap_or(0)),
                    row.name
                )));
                targets.push(Some(*id));
            }
            app.move_targets = targets;
            CollectionState::get(&window)
                .set_move_targets(ModelRc::from(Rc::new(VecModel::from(labels))));
            open_dialog(&window, "collection-move");
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        CollectionState::get(window).on_open_delete(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let app = app.borrow();
            if target(&app, &window, index).is_none() {
                return;
            }
            // The subtree is the run of following rows deeper than this one:
            // the sidebar already holds the shape, so saying how many
            // collections would go costs no query.
            let doomed = subtree_range(&app, index).count() + 1;
            CollectionState::get(&window)
                .set_target_doomed(i32::try_from(doomed).unwrap_or(i32::MAX));
            open_dialog(&window, "collection-delete");
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        CollectionState::get(window).on_run_rename(move |name| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some(collection) = current_target(&app, &window) else {
                return;
            };
            let renamed = app
                .library
                .rename_collection(collection, name.as_str())
                .map_err(|e| e.to_string())
                .and_then(|()| refresh_collections(&mut app, &window));
            finish(&window, renamed);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        CollectionState::get(window).on_run_move(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some(collection) = current_target(&app, &window) else {
                return;
            };
            let Some(parent) = usize::try_from(index)
                .ok()
                .and_then(|i| app.move_targets.get(i))
                .copied()
            else {
                return;
            };
            let moved = app
                .library
                .move_collection(collection, parent)
                .map_err(|e| e.to_string())
                .and_then(|()| refresh_collections(&mut app, &window));
            finish(&window, moved);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        CollectionState::get(window).on_run_delete(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some(collection) = current_target(&app, &window) else {
                return;
            };
            let deleted = app
                .library
                .delete_collection(collection)
                .map_err(|e| e.to_string())
                .and_then(|_| {
                    // The grid may have been filtered to a collection that
                    // no longer exists: fall back to the whole library
                    // rather than to an empty view with no way out.
                    if app.query.collection == Some(collection) {
                        app.query.collection = None;
                        CollectionState::get(&window).set_active_collection(-1);
                        reload(&mut app, &window)?;
                    }
                    refresh_collections(&mut app, &window)
                });
            finish(&window, deleted);
        });
    }
}

/// Records which collection a management dialog is about, and returns it.
fn target(app: &App, window: &StudioWindow, index: i32) -> Option<CollectionId> {
    let collection = usize::try_from(index)
        .ok()
        .and_then(|i| app.collections.get(i))
        .copied()?;
    let name = CollectionState::get(window)
        .get_collections()
        .row_data(usize::try_from(index).ok()?)?
        .name;
    CollectionState::get(window).set_target_collection(index);
    CollectionState::get(window).set_target_name(name);
    Some(collection)
}

/// The collection the open dialog is about.
fn current_target(app: &App, window: &StudioWindow) -> Option<CollectionId> {
    usize::try_from(CollectionState::get(window).get_target_collection())
        .ok()
        .and_then(|i| app.collections.get(i))
        .copied()
}

/// Row indices of the descendants of the row at `index`: the following rows,
/// for as long as they are deeper than it.
fn subtree_range(app: &App, index: i32) -> std::ops::Range<usize> {
    let Ok(index) = usize::try_from(index) else {
        return 0..0;
    };
    let Some(&depth) = app.collection_depths.get(index) else {
        return 0..0;
    };
    let end = app
        .collection_depths
        .iter()
        .enumerate()
        .skip(index + 1)
        .find(|&(_, &row)| row <= depth)
        .map_or(app.collection_depths.len(), |(at, _)| at);
    (index + 1)..end
}

/// Opens `dialog` with a blank result line.
fn open_dialog(window: &StudioWindow, dialog: &str) {
    DialogState::get(window).set_dialog_result(SharedString::default());
    DialogState::get(window).set_dialog(SharedString::from(dialog));
}

/// Closes the dialog on success, or leaves it open showing why not.
fn finish(window: &StudioWindow, outcome: Result<(), String>) {
    match outcome {
        Ok(()) => DialogState::get(window).set_dialog(SharedString::default()),
        Err(error) => {
            DialogState::get(window).set_dialog_result(SharedString::from(error));
        }
    }
}

/// Adds the selected photo's version to the active collection, or removes
/// it, then reloads the grid (membership may change what it shows).
pub(crate) fn collection_membership(app: &mut App, window: &StudioWindow, add: bool) {
    let Some(collection) = usize::try_from(CollectionState::get(window).get_active_collection())
        .ok()
        .and_then(|i| app.collections.get(i))
        .copied()
    else {
        report_error(window, &Tr::get(window).invoke_select_collection_first());
        return;
    };
    let Some(version) =
        item_at(app, GridState::get(window).get_selected()).map(|item| item.version_id)
    else {
        return;
    };
    let changed = if add {
        app.library.add_to_collection(collection, &[version])
    } else {
        app.library.remove_from_collection(collection, &[version])
    };
    if let Err(error) = changed
        .map_err(|e| e.to_string())
        .and_then(|()| reload(app, window))
    {
        report_error(window, &error);
    }
}

/// Reloads the sidebar from the catalog's collection tree.
pub(crate) fn refresh_collections(app: &mut App, window: &StudioWindow) -> Result<(), String> {
    let tree = app.library.collections().map_err(|e| e.to_string())?;
    let mut flat = Vec::new();
    flatten_collections(&tree, 0, &mut flat);
    app.collections = flat.iter().map(|(id, ..)| *id).collect();
    app.collection_depths = flat.iter().map(|(_, _, depth, _)| *depth).collect();
    let rows: Vec<crate::ui::CollectionRow> = flat
        .into_iter()
        .map(|(_, name, depth, smart)| crate::ui::CollectionRow {
            name: SharedString::from(name),
            depth,
            smart,
        })
        .collect();
    CollectionState::get(window).set_collections(ModelRc::from(Rc::new(VecModel::from(rows))));
    Ok(())
}

/// Flattens the collection tree into `(id, name, depth, smart)` sidebar
/// rows, depth first, children under their parent.
pub(crate) fn flatten_collections(
    nodes: &[CollectionNode],
    depth: i32,
    out: &mut Vec<(CollectionId, String, i32, bool)>,
) {
    for node in nodes {
        out.push((
            node.collection,
            node.name.clone(),
            depth,
            node.collection_type == CollectionType::Smart,
        ));
        flatten_collections(&node.children, depth + 1, out);
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn flattening_keeps_children_under_their_parent() {
        let node = |id: i64, name: &str, children: Vec<CollectionNode>| CollectionNode {
            collection: CollectionId::new(id),
            name: name.to_owned(),
            description: None,
            collection_type: CollectionType::Manual,
            children,
        };
        let tree = vec![
            node(1, "Travel", vec![node(2, "Iceland", vec![])]),
            node(3, "Portfolio", vec![]),
        ];
        let mut flat = Vec::new();
        flatten_collections(&tree, 0, &mut flat);
        let shape: Vec<(i64, &str, i32)> = flat
            .iter()
            .map(|(id, name, depth, _)| (id.get(), name.as_str(), *depth))
            .collect();
        assert_eq!(
            shape,
            vec![(1, "Travel", 0), (2, "Iceland", 1), (3, "Portfolio", 0)]
        );
    }
}
