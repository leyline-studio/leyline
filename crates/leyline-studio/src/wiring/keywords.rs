//! Wires the keyword half of `DetailState` (ADR 0045 §4).
//!
//! Kept apart from `wiring::grid` even though both feed the detail panel:
//! keywords are a hierarchy with its own resolution rules, the metadata rows
//! are a flat formatting job.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use crate::app::{App, item_at, report_error, selected_assets};
use crate::ui::{
    DetailState, DialogState, FilterState, GridState, KeywordRow, KeywordState, StudioWindow, Tr,
};
use crate::undo::{Edit, Snapshot};
use crate::wiring::grid::reload;
use crate::wiring::library::refresh_undo;
use leyline_sdk::{AssetDescription, AssetId, KeywordId, KeywordNode};
use slint::{ComponentHandle, Global, Model, ModelRc, SharedString, VecModel};

/// Connects the keyword panel: tagging by path, untagging by row.
pub(crate) fn wire_keywords(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DetailState::get(window).on_add_keyword(move |path| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let path = path.trim().to_owned();
            if path.is_empty() {
                return;
            }
            // The whole selection (ADR 0134 §1) — the multi-selection when
            // there is one, the focused photograph otherwise, which is the
            // rule every batch action follows. Until that ADR this was
            // `&[asset]`, so selecting two hundred frames and typing a
            // keyword tagged one of them.
            let assets = selected_assets(&app, GridState::get(&window).get_selected());
            if assets.is_empty() {
                return;
            }
            let tagged = ensure_keyword_path(&mut app, &path).and_then(|keyword| {
                app.library
                    .add_keyword(&assets, keyword)
                    .map_err(|e| e.to_string())
                    .map(|()| keyword)
            });
            // Reload rather than refresh the panel alone: a text search may
            // now match (or no longer match) the tagged photo.
            match tagged.and_then(|keyword| reload(&mut app, &window).map(|()| keyword)) {
                Ok(keyword) => {
                    // Undoable (ADR 0129): the inverse of tagging one photo
                    // is untagging it. The keyword the path resolved to may
                    // have been created on the way, and is deliberately not
                    // deleted by an undo — a keyword is a name in a
                    // hierarchy, not a property of this photograph.
                    app.undo.push(Edit {
                        kind: "keyword",
                        before: Snapshot::Keyword {
                            assets: assets.clone(),
                            keyword,
                            tagged: false,
                        },
                        after: Snapshot::Keyword {
                            assets: assets.clone(),
                            keyword,
                            tagged: true,
                        },
                    });
                    refresh_undo(&app, &window);
                    refresh_keyword_panel(&mut app, &window);
                }
                Err(error) => report_error(&window, &error),
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        // What someone writes about the photograph (ADR 0099). Committed
        // on Enter, one field at a time, onto the description already
        // stored so editing the title does not clear the caption.
        DetailState::get(window).on_write_description(move |field, value| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some(asset) =
                item_at(&app, GridState::get(&window).get_selected()).map(|item| item.asset_id)
            else {
                return;
            };
            // The description as it stands, which is both what the field
            // is written onto and the half an undo puts back (ADR 0129 §1).
            let before = app
                .library
                .catalog()
                .description(asset)
                .ok()
                .flatten()
                .unwrap_or_default();
            // Annotated because the closure now yields the description it
            // wrote — with `?` alone, nothing here says which error type
            // the two arms share.
            let written: Result<AssetDescription, String> = (|| {
                let mut description = app
                    .library
                    .catalog()
                    .description(asset)
                    .map_err(|e| e.to_string())?
                    .unwrap_or_default();
                // An emptied field clears that field, and only it.
                let value = value.trim();
                let value = (!value.is_empty()).then(|| value.to_owned());
                match field.as_str() {
                    "title" => description.title = value,
                    "caption" => description.caption = value,
                    "creator" => description.creator = value,
                    "copyright" => description.copyright = value,
                    // A field this panel does not write: nothing changed,
                    // and the description travels back unmodified so the
                    // caller compares it with itself and records nothing.
                    _ => return Ok(description),
                }
                app.library
                    .set_description(asset, &description)
                    .map_err(|e| e.to_string())?;
                Ok(description)
            })();
            // Reload rather than refresh the panel alone: an authored
            // creator feeds the search index (ADR 0099 §2), so a text
            // search may now match this photo.
            match written.and_then(|after| reload(&mut app, &window).map(|()| after)) {
                Ok(after) => {
                    if after != before {
                        app.undo.push(Edit {
                            kind: "description",
                            before: Snapshot::Description {
                                asset,
                                description: Box::new(before),
                            },
                            after: Snapshot::Description {
                                asset,
                                description: Box::new(after),
                            },
                        });
                        refresh_undo(&app, &window);
                    }
                }
                Err(error) => report_error(&window, &error),
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DetailState::get(window).on_remove_keyword(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some(asset) =
                item_at(&app, GridState::get(&window).get_selected()).map(|item| item.asset_id)
            else {
                return;
            };
            let Some(keyword) = usize::try_from(index)
                .ok()
                .and_then(|i| app.keywords.get(i))
                .copied()
            else {
                return;
            };
            if app.keyword_filter == Some(keyword) {
                app.keyword_filter = None;
                app.query.keywords.clear();
            }
            let untagged = app
                .library
                .remove_keyword(&[asset], keyword)
                .map_err(|e| e.to_string());
            match untagged.and_then(|()| reload(&mut app, &window)) {
                Ok(()) => {
                    app.undo.push(Edit {
                        kind: "keyword",
                        before: Snapshot::Keyword {
                            assets: vec![asset],
                            keyword,
                            tagged: true,
                        },
                        after: Snapshot::Keyword {
                            assets: vec![asset],
                            keyword,
                            tagged: false,
                        },
                    });
                    refresh_undo(&app, &window);
                    refresh_keyword_panel(&mut app, &window);
                }
                Err(error) => report_error(&window, &error),
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        FilterState::get(window).on_keyword_filter(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some(keyword) = usize::try_from(index)
                .ok()
                .and_then(|i| app.keywords.get(i))
                .copied()
            else {
                return;
            };
            app.keyword_filter = if app.keyword_filter == Some(keyword) {
                None
            } else {
                Some(keyword)
            };
            app.query.keywords = app.keyword_filter.into_iter().collect();
            FilterState::get(&window).set_filter_keyword_label(
                app.keyword_filter
                    .and_then(|k| app.keywords.iter().position(|&x| x == k))
                    .and_then(|i| DetailState::get(&window).get_detail_keywords().row_data(i))
                    .unwrap_or_default(),
            );
            if let Err(error) = reload(&mut app, &window) {
                report_error(&window, &error);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        FilterState::get(window).on_clear_keyword_filter(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            app.keyword_filter = None;
            app.query.keywords.clear();
            FilterState::get(&window).set_filter_keyword_label(SharedString::default());
            if let Err(error) = reload(&mut app, &window) {
                report_error(&window, &error);
            }
        });
    }
}

/// Rebuilds the sidebar's keyword tree: the visible rows, their subtree
/// counts, and which of them the grid is filtered on (ADR 0134 §2).
///
/// Called after anything that can change the tree or the tags on it. Cheap:
/// two queries and a walk of a structure that has as many nodes as the
/// library has keywords.
pub(crate) fn refresh_keyword_panel(app: &mut App, window: &StudioWindow) {
    let (tree, counts) = match (app.library.keyword_tree(), app.library.keyword_counts()) {
        (Ok(tree), Ok(counts)) => (tree, counts.into_iter().collect::<HashMap<_, _>>()),
        (Err(error), _) | (_, Err(error)) => {
            eprintln!("error: {error}");
            return;
        }
    };
    let tagged: HashSet<KeywordId> = app.keywords.iter().copied().collect();
    let mut ids = Vec::new();
    let mut rows = Vec::new();
    flatten_visible(
        &tree,
        0,
        &counts,
        &app.keyword_expanded,
        &tagged,
        app.keyword_filter,
        &mut ids,
        &mut rows,
    );
    app.keyword_panel = ids;
    KeywordState::get(window).set_any(!tree.is_empty());
    KeywordState::get(window).set_keywords(ModelRc::from(Rc::new(VecModel::from(rows))));
}

/// Walks the tree in display order, emitting a row per visible node.
///
/// A node's count is its **subtree** total, because that is the set clicking
/// it filters to (`docs/catalog.md` §22 matches descendants) — the number on
/// the row and the gesture on it have to agree. Computed on the way back up,
/// so each node is visited once.
#[allow(clippy::too_many_arguments)]
fn flatten_visible(
    nodes: &[KeywordNode],
    depth: i32,
    counts: &HashMap<KeywordId, u32>,
    expanded: &BTreeSet<KeywordId>,
    tagged: &HashSet<KeywordId>,
    filtered: Option<KeywordId>,
    ids: &mut Vec<KeywordId>,
    rows: &mut Vec<KeywordRow>,
) -> u32 {
    let mut total = 0;
    for node in nodes {
        let here = counts.get(&node.keyword).copied().unwrap_or(0);
        let open = expanded.contains(&node.keyword);
        // The row is pushed before its children so display order is
        // preserved, and its count is filled in afterwards — the subtree
        // total is not known until the children have been walked.
        let at = rows.len();
        ids.push(node.keyword);
        rows.push(KeywordRow {
            name: SharedString::from(node.name.as_str()),
            path: SharedString::from(node.path.as_str()),
            depth,
            count: 0,
            has_children: !node.children.is_empty(),
            expanded: open,
            active: filtered == Some(node.keyword),
            tagged: tagged.contains(&node.keyword),
        });
        // Hidden children still count: a closed `Nature` shows the herons it
        // holds, which is what makes the closed row worth reading.
        let below = if open {
            flatten_visible(
                &node.children,
                depth + 1,
                counts,
                expanded,
                tagged,
                filtered,
                ids,
                rows,
            )
        } else {
            subtree_count(&node.children, counts)
        };
        rows[at].count = i32::try_from(here + below).unwrap_or(i32::MAX);
        total += here + below;
    }
    total
}

/// The subtree total of nodes nobody is going to draw.
fn subtree_count(nodes: &[KeywordNode], counts: &HashMap<KeywordId, u32>) -> u32 {
    nodes
        .iter()
        .map(|node| {
            counts.get(&node.keyword).copied().unwrap_or(0) + subtree_count(&node.children, counts)
        })
        .sum()
}

/// Connects the sidebar's keyword tree (ADR 0134 §2, §4).
pub(crate) fn wire_keyword_panel(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    /// The keyword a row index names, or nothing when the panel has moved
    /// under the click — which it can, since a rebuild replaces every row.
    fn at(app: &App, row: i32) -> Option<KeywordId> {
        usize::try_from(row)
            .ok()
            .and_then(|i| app.keyword_panel.get(i))
            .copied()
    }

    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        KeywordState::get(window).on_toggle_expand(move |row| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some(keyword) = at(&app, row) else {
                return;
            };
            if !app.keyword_expanded.remove(&keyword) {
                app.keyword_expanded.insert(keyword);
            }
            refresh_keyword_panel(&mut app, &window);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        KeywordState::get(window).on_filter(move |row| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some(keyword) = at(&app, row) else {
                return;
            };
            app.keyword_filter = (app.keyword_filter != Some(keyword)).then_some(keyword);
            app.query.keywords = app.keyword_filter.into_iter().collect();
            // The chip in the filter bar says which keyword, by path: the
            // leaf name alone would be ambiguous the moment two branches
            // hold a `Heron`.
            let label = app
                .keyword_filter
                .and_then(|_| {
                    usize::try_from(row)
                        .ok()
                        .and_then(|i| KeywordState::get(&window).get_keywords().row_data(i))
                })
                .map(|row| row.path)
                .unwrap_or_default();
            FilterState::get(&window).set_filter_keyword_label(label);
            if let Err(error) = reload(&mut app, &window) {
                report_error(&window, &error);
                return;
            }
            refresh_keyword_panel(&mut app, &window);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        KeywordState::get(window).on_tag_selection(move |row| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let focused = GridState::get(&window).get_selected();
            let assets = selected_assets(&app, focused);
            tag(&mut app, &window, row, assets);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        KeywordState::get(window).on_drop_on_keyword(move |cell, row| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            // ADR 0130 §1's rule, the same one the collection drop applies:
            // the whole selection when the dragged cell belongs to it, that
            // one photograph when it does not.
            let dragged = usize::try_from(cell).ok();
            let assets = if dragged.is_some_and(|c| app.multi_selected.contains(&c)) {
                selected_assets(&app, cell)
            } else {
                item_at(&app, cell)
                    .map(|item| vec![item.asset_id])
                    .unwrap_or_default()
            };
            tag(&mut app, &window, row, assets);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        KeywordState::get(window).on_open_rename(move |row| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some(keyword) = at(&app, row) else {
                return;
            };
            app.keyword_target = Some(keyword);
            let Some(shown) = usize::try_from(row)
                .ok()
                .and_then(|i| KeywordState::get(&window).get_keywords().row_data(i))
            else {
                return;
            };
            KeywordState::get(&window).set_target_name(shown.name);
            KeywordState::get(&window).set_target_path(shown.path);
            DialogState::get(&window).set_dialog_result(SharedString::new());
            DialogState::get(&window).set_dialog(SharedString::from("keyword-rename"));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        KeywordState::get(window).on_open_delete(move |row| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some(keyword) = at(&app, row) else {
                return;
            };
            app.keyword_target = Some(keyword);
            let Some(shown) = usize::try_from(row)
                .ok()
                .and_then(|i| KeywordState::get(&window).get_keywords().row_data(i))
            else {
                return;
            };
            // How many photographs actually carry *this* keyword — the
            // direct count, not the subtree's: deleting a leaf untags only
            // what the leaf holds.
            let carried = app
                .library
                .keyword_counts()
                .unwrap_or_default()
                .into_iter()
                .find(|&(id, _)| id == keyword)
                .map_or(0, |(_, count)| count);
            KeywordState::get(&window).set_target_name(shown.name);
            KeywordState::get(&window).set_target_path(shown.path.clone());
            KeywordState::get(&window).set_delete_warning(if shown.has_children {
                Tr::get(&window).invoke_keyword_has_children(shown.path)
            } else {
                Tr::get(&window).invoke_delete_keyword_from(
                    shown.path,
                    i32::try_from(carried).unwrap_or(i32::MAX),
                )
            });
            DialogState::get(&window).set_dialog_result(SharedString::new());
            DialogState::get(&window).set_dialog(SharedString::from("keyword-delete"));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        KeywordState::get(window).on_run_rename(move |name| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some(keyword) = app.keyword_target else {
                return;
            };
            match app.library.rename_keyword(keyword, name.trim()) {
                Ok(()) => {
                    if let Err(error) = reload(&mut app, &window) {
                        report_error(&window, &error);
                        return;
                    }
                    refresh_keyword_panel(&mut app, &window);
                    refresh_focused_keywords(&mut app, &window);
                    DialogState::get(&window).set_dialog(SharedString::new());
                }
                // Shown in the dialog rather than the status line: the user
                // is looking at the field that caused it, and a name already
                // taken is answered by typing another.
                Err(error) => {
                    DialogState::get(&window)
                        .set_dialog_result(SharedString::from(error.to_string()));
                }
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        KeywordState::get(window).on_run_delete(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some(keyword) = app.keyword_target else {
                return;
            };
            match app.library.delete_keyword(keyword) {
                Ok(()) => {
                    // A grid filtered on the keyword just deleted would show
                    // nothing, for a criterion no longer in the tree.
                    if app.keyword_filter == Some(keyword) {
                        app.keyword_filter = None;
                        app.query.keywords.clear();
                        FilterState::get(&window).set_filter_keyword_label(SharedString::new());
                    }
                    app.keyword_expanded.remove(&keyword);
                    if let Err(error) = reload(&mut app, &window) {
                        report_error(&window, &error);
                        return;
                    }
                    refresh_keyword_panel(&mut app, &window);
                    refresh_focused_keywords(&mut app, &window);
                    DialogState::get(&window).set_dialog(SharedString::new());
                }
                Err(error) => {
                    DialogState::get(&window)
                        .set_dialog_result(SharedString::from(error.to_string()));
                }
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DetailState::get(window).on_suggest_keywords(move |typed| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let app = app.borrow();
            let paths = match app.library.keyword_tree() {
                Ok(tree) => suggestions(&tree, typed.trim()),
                Err(error) => {
                    eprintln!("error: {error}");
                    Vec::new()
                }
            };
            DetailState::get(&window)
                .set_keyword_suggestions(ModelRc::from(Rc::new(VecModel::from(paths))));
        });
    }
}

/// The `+` and the drop, which are one intention: tag these photographs with
/// that keyword (ADR 0134 §2).
fn tag(app: &mut App, window: &StudioWindow, row: i32, assets: Vec<AssetId>) {
    let Some(keyword) = usize::try_from(row)
        .ok()
        .and_then(|i| app.keyword_panel.get(i))
        .copied()
    else {
        return;
    };
    if assets.is_empty() {
        return;
    }
    let tagged = app
        .library
        .add_keyword(&assets, keyword)
        .map_err(|e| e.to_string())
        .and_then(|()| reload(app, window));
    match tagged {
        Ok(()) => {
            app.undo.push(Edit {
                kind: "keyword",
                before: Snapshot::Keyword {
                    assets: assets.clone(),
                    keyword,
                    tagged: false,
                },
                after: Snapshot::Keyword {
                    assets,
                    keyword,
                    tagged: true,
                },
            });
            refresh_undo(app, window);
            refresh_focused_keywords(app, window);
            refresh_keyword_panel(app, window);
        }
        Err(error) => report_error(window, &error),
    }
}

/// Re-reads the focused photograph's own tags into the metadata panel.
///
/// Needed wherever the tags change without the selection moving: `show_details`
/// does it on every selection change, and these paths change the tags in place.
fn refresh_focused_keywords(app: &mut App, window: &StudioWindow) {
    let Some(asset) = item_at(app, GridState::get(window).get_selected()).map(|item| item.asset_id)
    else {
        return;
    };
    match keyword_rows(app, asset) {
        Ok((ids, paths)) => {
            app.keywords = ids;
            DetailState::get(window)
                .set_detail_keywords(ModelRc::from(Rc::new(VecModel::from(paths))));
        }
        Err(error) => eprintln!("error: {error}"),
    }
}

/// Paths of the tree matching `typed` on **any level** (ADR 0134 §3), so
/// `her` finds `Nature/Birds/Heron` without typing the branch.
///
/// Case-insensitive, and capped: a suggestion list longer than the panel is
/// a list nobody reads, and the answer to "too many matches" is to type more.
fn suggestions(tree: &[KeywordNode], typed: &str) -> Vec<SharedString> {
    /// How many suggestions the field offers at once.
    const MOST: usize = 6;

    if typed.is_empty() {
        return Vec::new();
    }
    // The last level being typed is what is matched: someone typing
    // `Nature/Bi` means to find `Birds`, not to search for the whole string.
    let needle = typed
        .rsplit('/')
        .next()
        .unwrap_or(typed)
        .trim()
        .to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut paths = Vec::new();
    collect_paths(tree, &mut paths);
    paths.sort();
    paths
        .into_iter()
        .filter(|path| {
            path.rsplit('/')
                .next()
                .unwrap_or(path)
                .to_lowercase()
                .contains(&needle)
        })
        // A path already typed in full is not a suggestion, it is what the
        // field says.
        .filter(|path| !path.eq_ignore_ascii_case(typed))
        .take(MOST)
        .map(|path| SharedString::from(path.as_str()))
        .collect()
}

/// Every path in the tree, depth-first.
fn collect_paths(nodes: &[KeywordNode], out: &mut Vec<String>) {
    for node in nodes {
        out.push(node.path.clone());
        collect_paths(&node.children, out);
    }
}

/// Finds or creates the keyword at a slash-separated path (`Nature/Birds`),
/// creating the missing levels, and returns the leaf keyword.
pub(crate) fn ensure_keyword_path(app: &mut App, path: &str) -> Result<KeywordId, String> {
    let tree = app.library.keyword_tree().map_err(|e| e.to_string())?;
    let (mut parent, missing) = resolve_keyword_path(&tree, path)?;
    for level in missing {
        parent = Some(
            app.library
                .create_keyword(parent, &level)
                .map_err(|e| e.to_string())?,
        );
    }
    parent.ok_or_else(|| "enter a keyword".to_owned())
}

/// Walks the keyword tree along a slash-separated path and returns the
/// deepest existing keyword plus the levels still to create beneath it.
pub(crate) fn resolve_keyword_path(
    tree: &[KeywordNode],
    path: &str,
) -> Result<(Option<KeywordId>, Vec<String>), String> {
    let mut parent = None;
    let mut siblings = tree;
    let mut missing = Vec::new();
    for level in path.split('/') {
        let level = level.trim();
        if level.is_empty() {
            return Err("keyword levels cannot be empty".to_owned());
        }
        if !missing.is_empty() {
            missing.push(level.to_owned());
            continue;
        }
        match siblings.iter().find(|node| node.name == level) {
            Some(node) => {
                parent = Some(node.keyword);
                siblings = &node.children;
            }
            None => missing.push(level.to_owned()),
        }
    }
    Ok((parent, missing))
}

/// The keywords of an asset as parallel `(ids, full paths)` panel rows.
pub(crate) fn keyword_rows(
    app: &App,
    asset: AssetId,
) -> Result<(Vec<KeywordId>, Vec<SharedString>), String> {
    let ids = app
        .library
        .catalog()
        .asset_keywords(asset)
        .map_err(|e| e.to_string())?;
    let tree = app.library.keyword_tree().map_err(|e| e.to_string())?;
    let mut paths = std::collections::HashMap::new();
    collect_keyword_paths(&tree, &mut paths);
    let names = ids
        .iter()
        .map(|id| SharedString::from(paths.get(id).map_or("?", String::as_str)))
        .collect();
    Ok((ids, names))
}

/// Flattens the keyword tree into an id → full path map.
pub(crate) fn collect_keyword_paths(
    nodes: &[KeywordNode],
    out: &mut std::collections::HashMap<KeywordId, String>,
) {
    for node in nodes {
        out.insert(node.keyword, node.path.clone());
        collect_keyword_paths(&node.children, out);
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn keyword_paths_resolve_existing_levels_and_list_missing_ones() {
        let node = |id: i64, name: &str, path: &str, children: Vec<KeywordNode>| KeywordNode {
            keyword: KeywordId::new(id),
            name: name.to_owned(),
            path: path.to_owned(),
            children,
        };
        let tree = vec![node(
            1,
            "Nature",
            "Nature",
            vec![node(2, "Birds", "Nature/Birds", vec![])],
        )];

        // A fully existing path resolves to its leaf, nothing to create.
        let (parent, missing) = resolve_keyword_path(&tree, "Nature/Birds").unwrap();
        assert_eq!((parent, missing), (Some(KeywordId::new(2)), vec![]));

        // A partly existing path stops at the deepest known level; spaces
        // around levels are trimmed.
        let (parent, missing) = resolve_keyword_path(&tree, "Nature / Birds / Heron").unwrap();
        assert_eq!(parent, Some(KeywordId::new(2)));
        assert_eq!(missing, vec!["Heron".to_owned()]);

        // A brand-new root creates every level.
        let (parent, missing) = resolve_keyword_path(&tree, "Travel/Iceland").unwrap();
        assert_eq!(parent, None);
        assert_eq!(missing, vec!["Travel".to_owned(), "Iceland".to_owned()]);

        // Empty levels are refused.
        assert!(resolve_keyword_path(&tree, "Nature//Heron").is_err());
        assert!(resolve_keyword_path(&tree, "/").is_err());

        // The id → path map covers nested nodes.
        let mut paths = std::collections::HashMap::new();
        collect_keyword_paths(&tree, &mut paths);
        assert_eq!(paths[&KeywordId::new(2)], "Nature/Birds");
    }
}
