//! Collection commands (ADR 0169): list the tree, create a manual or smart
//! collection, fill it, rename, move and delete it. What a collection holds
//! is listed by `ls --collection`.

use leyline_sdk::{
    CollectionId, CollectionNode, CollectionType, KeywordNode, RatingRule, SmartRules,
};

use super::*;

/// The valued flags that make `collection-create` build a smart collection;
/// with the `--pick` switch they are the four criteria `SmartRules`
/// evaluates (ADR 0169 §2).
const RULE_FLAGS: &[&str] = &["rating", "camera", "keyword"];

pub(crate) fn collection_id(text: &str) -> Result<CollectionId, String> {
    text.parse()
        .map(CollectionId::new)
        .map_err(|_| format!("bad collection id {text:?}"))
}

pub(crate) fn collections(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root] = positional.as_slice() else {
        return Err("usage: leyline collections <library>".to_owned());
    };
    let library = open(root)?;
    let tree = library.collections().map_err(|e| e.to_string())?;
    if tree.is_empty() {
        println!("no collection");
        return Ok(());
    }
    print_tree(&library, &tree, 0)
}

fn print_tree(library: &Library, nodes: &[CollectionNode], depth: usize) -> Result<(), String> {
    for node in nodes {
        let query = GridQuery {
            collection: Some(node.collection),
            ..GridQuery::default()
        };
        let members = library.catalog().count(&query).map_err(|e| e.to_string())?;
        let kind = match node.collection_type {
            CollectionType::Manual => "manual",
            CollectionType::Smart => "smart",
        };
        let indent = "  ".repeat(depth);
        println!(
            "c{:<5} {kind:<6} {members:>6}  {indent}{}",
            node.collection.get(),
            node.name
        );
        if let Some(rules) = library
            .smart_rules(node.collection)
            .map_err(|e| e.to_string())?
        {
            println!("{:23}{indent}  {}", "", describe_rules(&rules));
        }
        print_tree(library, &node.children, depth + 1)?;
    }
    Ok(())
}

/// The rules in the words of the flags that write them, so what `collections`
/// prints can be typed back into `collection-create`.
fn describe_rules(rules: &SmartRules) -> String {
    let mut parts = Vec::new();
    if let Some(rating) = rules.rating {
        parts.push(format!("--rating {}", rating.gte));
    }
    if let Some(camera) = &rules.camera {
        parts.push(format!("--camera {camera:?}"));
    }
    for keyword in &rules.keywords {
        parts.push(format!("--keyword {keyword:?}"));
    }
    match rules.pick {
        Some(true) => parts.push("--pick".to_owned()),
        // Written by no client today (ADR 0169 §2), but a catalogue may hold it.
        Some(false) => parts.push("(not a pick)".to_owned()),
        None => {}
    }
    parts.join(" ")
}

pub(crate) fn collection_create(args: &[String]) -> Result<(), String> {
    let mut flags = vec!["parent"];
    flags.extend_from_slice(RULE_FLAGS);
    let (positional, options) = parse(args, &flags)?;
    let [root, name] = positional.as_slice() else {
        return Err(
            "usage: leyline collection-create <library> <name> [--parent <id>]\n\
                    \x20      [--rating <1-5>] [--camera <name>] [--keyword <path>]... [--pick]"
                .to_owned(),
        );
    };
    let library = open(root)?;
    let parent = options.value("parent").map(collection_id).transpose()?;
    let id = match smart_rules(&library, &options)? {
        Some(rules) => library.create_smart_collection(parent, name, &rules),
        None => library.create_collection(parent, name),
    }
    .map_err(|e| e.to_string())?;
    println!("c{}", id.get());
    Ok(())
}

/// The smart rules the flags spell, or `None` when no rule flag was given.
fn smart_rules(library: &Library, options: &Options) -> Result<Option<SmartRules>, String> {
    let rating = options
        .value("rating")
        .map(|r| {
            r.parse()
                .map(|gte| RatingRule { gte })
                .map_err(|_| format!("bad rating {r:?}"))
        })
        .transpose()?;
    let camera = options.value("camera").map(str::to_owned);
    let keywords: Vec<String> = options
        .all("keyword")
        .into_iter()
        .map(str::to_owned)
        .collect();
    let pick = options.switch("pick").then_some(true);
    if rating.is_none() && camera.is_none() && keywords.is_empty() && pick.is_none() {
        return Ok(None);
    }
    // A rule naming a keyword the library does not hold matches nothing, and
    // an empty collection looks like a correct one (ADR 0169 §2).
    if !keywords.is_empty() {
        let tree = library
            .catalog()
            .keyword_tree()
            .map_err(|e| e.to_string())?;
        for keyword in &keywords {
            if !holds_keyword_path(&tree, keyword) {
                return Err(format!("no keyword {keyword:?} in this library"));
            }
        }
    }
    Ok(Some(SmartRules {
        rating,
        camera,
        keywords,
        pick,
        ..SmartRules::default()
    }))
}

fn holds_keyword_path(nodes: &[KeywordNode], path: &str) -> bool {
    nodes
        .iter()
        .any(|node| node.path == path || holds_keyword_path(&node.children, path))
}

pub(crate) fn collection_add(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, collection, versions @ ..] = positional.as_slice() else {
        return Err(
            "usage: leyline collection-add <library> <collection-id> <version-id>...".to_owned(),
        );
    };
    let (collection, versions) = (collection_id(collection)?, version_ids(versions)?);
    open(root)?
        .add_to_collection(collection, &versions)
        .map_err(|e| e.to_string())?;
    println!("{} version(s) in c{}", versions.len(), collection.get());
    Ok(())
}

pub(crate) fn collection_remove(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, collection, versions @ ..] = positional.as_slice() else {
        return Err(
            "usage: leyline collection-remove <library> <collection-id> <version-id>...".to_owned(),
        );
    };
    let (collection, versions) = (collection_id(collection)?, version_ids(versions)?);
    open(root)?
        .remove_from_collection(collection, &versions)
        .map_err(|e| e.to_string())?;
    println!("{} version(s) out of c{}", versions.len(), collection.get());
    Ok(())
}

pub(crate) fn collection_rename(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, collection, name] = positional.as_slice() else {
        return Err("usage: leyline collection-rename <library> <collection-id> <name>".to_owned());
    };
    open(root)?
        .rename_collection(collection_id(collection)?, name)
        .map_err(|e| e.to_string())
}

pub(crate) fn collection_move(args: &[String]) -> Result<(), String> {
    let (positional, options) = parse(args, &["parent"])?;
    let usage = "usage: leyline collection-move <library> <collection-id> (--parent <id> | --root)";
    let [root, collection] = positional.as_slice() else {
        return Err(usage.to_owned());
    };
    // A bare move is refused rather than read as "to the root" (ADR 0169 §1).
    let parent = match (options.value("parent"), options.switch("root")) {
        (Some(parent), false) => Some(collection_id(parent)?),
        (None, true) => None,
        _ => return Err(usage.to_owned()),
    };
    open(root)?
        .move_collection(collection_id(collection)?, parent)
        .map_err(|e| e.to_string())
}

pub(crate) fn collection_delete(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, collection] = positional.as_slice() else {
        return Err("usage: leyline collection-delete <library> <collection-id>".to_owned());
    };
    let gone = open(root)?
        .delete_collection(collection_id(collection)?)
        .map_err(|e| e.to_string())?;
    println!("{gone} collection(s) deleted; no photo was touched");
    Ok(())
}
