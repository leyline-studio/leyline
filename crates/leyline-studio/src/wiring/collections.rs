//! Wires `CollectionState` (ADR 0045 §4).

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::{App, item_at, report_error, selected_versions};
use crate::ui::{
    CollectionState, DialogState, FolderState, GridState, LibraryState, StudioWindow, Tr,
};
use crate::undo::{Edit, Snapshot};
use crate::wiring::grid::reload;
use crate::wiring::library::refresh_undo;
use leyline_sdk::{
    CollectionId, CollectionNode, CollectionType, KeywordId, KeywordNode, PickState, RatingRule,
    SmartRules,
};
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
                return;
            }
            // A dynamic collection says what it holds, and it is the only
            // place it says it (ADR 0143 §3): the rules were shown once, in
            // the dialog that made it, and a collection whose contents come
            // from a rule nobody can read afterwards is a black box.
            if let Some(id) = picked
                && let Ok(Some(rules)) = app.library.smart_rules(id)
            {
                let line = LibraryState::get(&window).get_status_line();
                if let Some(parts) = rule_parts(&window, &rules) {
                    LibraryState::get(&window)
                        .set_status_line(SharedString::from(format!("{line}  ·  {parts}")));
                }
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        CollectionState::get(window).on_prepare_new_collection(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let app = app.borrow();
            let state = CollectionState::get(&window);
            // Every dialog opens on « Manual »: a dynamic collection is the
            // deliberate answer, never the one a distracted Enter produces.
            state.set_new_collection_smart(false);
            match smart_rules_from_query(&app) {
                Ok(rules) => {
                    // No criterion at all is refused too: a dynamic
                    // collection holding the whole library is a second name
                    // for « Toutes les photos ».
                    let empty = rules.rating.is_none()
                        && rules.camera.is_none()
                        && rules.keywords.is_empty()
                        && rules.pick.is_none();
                    state.set_smart_summary(describe_rules(&window, &rules));
                    state.set_smart_refused(empty);
                }
                Err(criterion) => {
                    state.set_smart_summary(Tr::get(&window).invoke_smart_refused(
                        Tr::get(&window).invoke_smart_criterion(SharedString::from(criterion)),
                    ));
                    state.set_smart_refused(true);
                }
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        CollectionState::get(window).on_run_new_collection(move |name, smart| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let name = name.trim();
            if name.is_empty() {
                DialogState::get(&window).set_dialog_result(Tr::get(&window).invoke_enter_a_name());
                return;
            }
            let mut app = app.borrow_mut();
            let made = if smart {
                match smart_rules_from_query(&app) {
                    Ok(rules) => app.library.create_smart_collection(None, name, &rules),
                    // The button is disabled in that case, so this is the
                    // race where the filter changed under an open dialog.
                    Err(_) => {
                        DialogState::get(&window)
                            .set_dialog_result(CollectionState::get(&window).get_smart_summary());
                        return;
                    }
                }
            } else {
                app.library.create_collection(None, name)
            };
            let created = made
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
        CollectionState::get(window).on_drop_on_collection(move |cell, target| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some(collection) = usize::try_from(target)
                .ok()
                .and_then(|i| app.collections.get(i))
                .copied()
            else {
                return;
            };
            // What was dragged (ADR 0130 §1): the whole selection when the
            // dragged cell belongs to it, otherwise that one photograph.
            // Dragging a cell outside the selection is a statement about
            // that cell, and it must not silently file fifty others.
            let dragged = usize::try_from(cell).ok();
            let versions = if dragged.is_some_and(|c| app.multi_selected.contains(&c)) {
                selected_versions(&app, cell)
            } else {
                item_at(&app, cell)
                    .map(|item| vec![item.version_id])
                    .unwrap_or_default()
            };
            if versions.is_empty() {
                return;
            }
            if let Err(error) = app
                .library
                .add_to_collection(collection, &versions)
                .map_err(|e| e.to_string())
                .and_then(|()| reload(&mut app, &window))
            {
                report_error(&window, &error);
                return;
            }
            let membership = |member| Snapshot::Collection {
                collection,
                versions: versions.clone(),
                member,
            };
            app.undo.push(Edit {
                kind: "collection",
                before: membership(false),
                after: membership(true),
            });
            refresh_undo(&app, &window);
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
        return;
    }
    // Undoable (ADR 0129). The membership is a fact about one version and
    // one collection, so both halves of the edit are the same shape with
    // the boolean flipped.
    let membership = |member| Snapshot::Collection {
        collection,
        versions: vec![version],
        member,
    };
    app.undo.push(Edit {
        kind: "collection",
        before: membership(!add),
        after: membership(add),
    });
    refresh_undo(app, window);
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

/// What a dynamic collection built from the grid's current filter would
/// remember — or the first criterion that cannot be kept (ADR 0143).
///
/// Refusal by name rather than a silent drop: a collection that means
/// something *wider* than the filter it was saved from would answer with
/// photographs the photographer never asked for, and would look right while
/// doing it. The same rule the pipeline applies to a setting a pinned stage
/// version cannot express.
fn smart_rules_from_query(app: &App) -> Result<SmartRules, &'static str> {
    let query = &app.query;
    if query.color_label.is_some() {
        return Err("color");
    }
    if query.lens.is_some() {
        return Err("lens");
    }
    if query.iso.min.is_some() || query.iso.max.is_some() {
        return Err("iso");
    }
    if query.aperture.min.is_some() || query.aperture.max.is_some() {
        return Err("aperture");
    }
    if query.focal_length.min.is_some() || query.focal_length.max.is_some() {
        return Err("focal");
    }
    if query.shutter_speed.min.is_some() || query.shutter_speed.max.is_some() {
        return Err("shutter");
    }
    if query.text.is_some() {
        return Err("text");
    }
    if query.capture_range.is_some() {
        return Err("date");
    }
    if query.folder.is_some() {
        return Err("folder");
    }
    if query.collection.is_some() {
        return Err("collection");
    }
    // « Not flagged as a pick » covers rejected *and* unflagged, so neither
    // of those two filters has an equivalent here — where `Pick` has one
    // exactly.
    let pick = match query.pick {
        None => None,
        Some(PickState::Pick) => Some(true),
        Some(PickState::Reject) => return Err("rejected"),
        Some(PickState::None) => return Err("unflagged"),
    };
    // The rules name keywords by **path**, not by id: a rule outlives a
    // rename of the row it points at only if it says what it means
    // (`docs/catalog.md` §26).
    let mut keywords = Vec::new();
    if !query.keywords.is_empty() {
        let Ok(tree) = app.library.catalog().keyword_tree() else {
            return Err("keyword");
        };
        for id in &query.keywords {
            match keyword_path(&tree, *id) {
                Some(path) => keywords.push(path),
                None => return Err("keyword"),
            }
        }
    }
    Ok(SmartRules {
        rating: query.rating_at_least.map(|gte| RatingRule { gte }),
        camera: query.camera.clone(),
        keywords,
        pick,
        extra: serde_json::Map::new(),
    })
}

/// A keyword's full path, found in the tree the panel already reads.
fn keyword_path(nodes: &[KeywordNode], id: KeywordId) -> Option<String> {
    for node in nodes {
        if node.keyword == id {
            return Some(node.path.clone());
        }
        if let Some(found) = keyword_path(&node.children, id) {
            return Some(found);
        }
    }
    None
}

/// The criteria in words, or `None` when the rules hold nothing at all.
///
/// Two callers, two framings: the dialog wraps this in a sentence about what
/// the collection *will* hold, the status line shows it beside the count of
/// what it *does* hold (ADR 0143 §1, §3).
fn rule_parts(window: &StudioWindow, rules: &SmartRules) -> Option<SharedString> {
    let tr = Tr::get(window);
    let mut parts: Vec<SharedString> = Vec::new();
    if let Some(rating) = rules.rating {
        parts.push(tr.invoke_smart_rule_rating(i32::from(rating.gte)));
    }
    if let Some(camera) = &rules.camera {
        parts.push(tr.invoke_smart_rule_camera(SharedString::from(camera.as_str())));
    }
    for keyword in &rules.keywords {
        parts.push(tr.invoke_smart_rule_keyword(SharedString::from(keyword.as_str())));
    }
    if rules.pick == Some(true) {
        parts.push(tr.invoke_smart_rule_pick());
    }
    (!parts.is_empty()).then(|| {
        SharedString::from(
            parts
                .iter()
                .map(SharedString::as_str)
                .collect::<Vec<_>>()
                .join(", "),
        )
    })
}

/// The sentence the dialog shows under the « Dynamic » chip.
fn describe_rules(window: &StudioWindow, rules: &SmartRules) -> SharedString {
    let tr = Tr::get(window);
    match rule_parts(window, rules) {
        Some(parts) => tr.invoke_smart_rules_are(parts),
        None => tr.invoke_smart_rules_empty(),
    }
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
