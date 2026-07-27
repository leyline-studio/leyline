//! Wires `CollectionState` (ADR 0045 §4).

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::{App, item_at, report_error};
use crate::ui::{CollectionState, DialogState, GridState, StudioWindow, Tr};
use crate::wiring::grid::reload;
use leyline_sdk::{CollectionId, CollectionNode, CollectionType};
use slint::{ComponentHandle, Global, ModelRc, SharedString, VecModel};

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
