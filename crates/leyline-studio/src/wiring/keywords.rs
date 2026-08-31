//! Wires the keyword half of `DetailState` (ADR 0045 §4).
//!
//! Kept apart from `wiring::grid` even though both feed the detail panel:
//! keywords are a hierarchy with its own resolution rules, the metadata rows
//! are a flat formatting job.

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::{App, item_at, report_error};
use crate::ui::{DetailState, FilterState, GridState, StudioWindow};
use crate::wiring::grid::reload;
use leyline_sdk::{AssetId, KeywordId, KeywordNode};
use slint::{ComponentHandle, Global, Model, SharedString};

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
            let Some(asset) =
                item_at(&app, GridState::get(&window).get_selected()).map(|item| item.asset_id)
            else {
                return;
            };
            let tagged = ensure_keyword_path(&mut app, &path).and_then(|keyword| {
                app.library
                    .add_keyword(&[asset], keyword)
                    .map_err(|e| e.to_string())
            });
            // Reload rather than refresh the panel alone: a text search may
            // now match (or no longer match) the tagged photo.
            if let Err(error) = tagged.and_then(|()| reload(&mut app, &window)) {
                report_error(&window, &error);
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
            let written = (|| {
                let mut description = app
                    .library
                    .catalog()
                    .description(asset)?
                    .unwrap_or_default();
                // An emptied field clears that field, and only it.
                let value = value.trim();
                let value = (!value.is_empty()).then(|| value.to_owned());
                match field.as_str() {
                    "title" => description.title = value,
                    "caption" => description.caption = value,
                    "creator" => description.creator = value,
                    "copyright" => description.copyright = value,
                    _ => return Ok(()),
                }
                app.library.set_description(asset, &description)
            })();
            // Reload rather than refresh the panel alone: an authored
            // creator feeds the search index (ADR 0099 §2), so a text
            // search may now match this photo.
            if let Err(error) = written
                .map_err(|e| e.to_string())
                .and_then(|()| reload(&mut app, &window))
            {
                report_error(&window, &error);
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
            if let Err(error) = untagged.and_then(|()| reload(&mut app, &window)) {
                report_error(&window, &error);
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
