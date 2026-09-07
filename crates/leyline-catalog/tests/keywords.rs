//! Integration tests: hierarchical keywords (`docs/catalog.md` §22, §23).

use leyline_catalog::{Catalog, NewAsset, RegisteredAsset};
use leyline_core::Settings;
use leyline_core::{AssetId, KeywordId, LeylineError, MediaType};

fn new_catalog(dir: &tempfile::TempDir) -> Catalog {
    Catalog::create(&dir.path().join("catalog.db"), "Keywords").unwrap()
}

fn registered_asset(catalog: &mut Catalog, filename: &str) -> RegisteredAsset {
    let new = NewAsset {
        folder: catalog.ensure_folder("Photos").unwrap(),
        filename: filename.to_owned(),
        extension: "CR3".to_owned(),
        media_type: MediaType::Raw,
        file_size: 1,
        checksum: [0xAB; 32],
        width: None,
        height: None,
        capture_date: None,
        capture_offset_minutes: None,
    };
    catalog.add_asset(&new, &Settings::default()).unwrap()
}

#[test]
fn keyword_tree_mirrors_the_spec_example() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);

    // The §22 example: Nature > Birds/Mammals > species.
    let nature = catalog.create_keyword(None, "Nature").unwrap();
    let birds = catalog.create_keyword(Some(nature), "Birds").unwrap();
    let mammals = catalog.create_keyword(Some(nature), "Mammals").unwrap();
    let heron = catalog.create_keyword(Some(birds), "Heron").unwrap();
    catalog.create_keyword(Some(birds), "Eagle").unwrap();
    catalog.create_keyword(Some(birds), "Owl").unwrap();
    catalog.create_keyword(Some(mammals), "Fox").unwrap();
    catalog.create_keyword(Some(mammals), "Deer").unwrap();

    let tree = catalog.keyword_tree().unwrap();
    assert_eq!(tree.len(), 1);
    let nature_node = &tree[0];
    assert_eq!(nature_node.path, "Nature");
    let names: Vec<_> = nature_node
        .children
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, ["Birds", "Mammals"]);

    let birds_node = &nature_node.children[0];
    let species: Vec<_> = birds_node
        .children
        .iter()
        .map(|c| c.path.as_str())
        .collect();
    assert_eq!(
        species,
        [
            "Nature/Birds/Eagle",
            "Nature/Birds/Heron",
            "Nature/Birds/Owl"
        ]
    );
    assert_eq!(birds_node.children[1].keyword, heron);
}

#[test]
fn create_keyword_validates_names_parents_and_unicity() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let nature = catalog.create_keyword(None, "Nature").unwrap();

    for bad in ["", "a/b", " padded", "padded "] {
        assert!(
            matches!(catalog.create_keyword(None, bad), Err(LeylineError::Io(_))),
            "name {bad:?} should be rejected"
        );
    }
    assert!(matches!(
        catalog.create_keyword(Some(KeywordId::new(999)), "Birds"),
        Err(LeylineError::KeywordMissing(id)) if id.get() == 999
    ));
    // Paths are unique.
    assert!(matches!(
        catalog.create_keyword(None, "Nature"),
        Err(LeylineError::Db(_))
    ));
    // The same leaf name under two parents is fine.
    let birds = catalog.create_keyword(Some(nature), "Birds").unwrap();
    catalog.create_keyword(Some(birds), "Nature").unwrap();
}

#[test]
fn tagging_is_batched_and_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let a = registered_asset(&mut catalog, "IMG_0001.CR3");
    let b = registered_asset(&mut catalog, "IMG_0002.CR3");
    let heron = catalog.create_keyword(None, "Heron").unwrap();

    let batch = [a.asset, b.asset];
    catalog.add_keyword(&batch, heron).unwrap();
    catalog.add_keyword(&batch, heron).unwrap(); // idempotent
    assert_eq!(catalog.asset_keywords(a.asset).unwrap(), vec![heron]);
    assert_eq!(catalog.asset_keywords(b.asset).unwrap(), vec![heron]);

    catalog.remove_keyword(&[a.asset], heron).unwrap();
    catalog.remove_keyword(&[a.asset], heron).unwrap(); // no-op
    assert_eq!(catalog.asset_keywords(a.asset).unwrap(), vec![]);
    assert_eq!(catalog.asset_keywords(b.asset).unwrap(), vec![heron]);
}

#[test]
fn tagging_reports_missing_assets_and_keywords() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let a = registered_asset(&mut catalog, "IMG_0001.CR3");
    let heron = catalog.create_keyword(None, "Heron").unwrap();

    assert!(matches!(
        catalog.add_keyword(&[a.asset], KeywordId::new(999)),
        Err(LeylineError::KeywordMissing(_))
    ));
    // A missing asset rolls the whole batch back.
    assert!(matches!(
        catalog.add_keyword(&[a.asset, AssetId::new(999)], heron),
        Err(LeylineError::AssetMissing(id)) if id.get() == 999
    ));
    assert_eq!(catalog.asset_keywords(a.asset).unwrap(), vec![]);
    assert!(matches!(
        catalog.remove_keyword(&[a.asset], KeywordId::new(999)),
        Err(LeylineError::KeywordMissing(_))
    ));
}

#[test]
fn asset_keywords_come_back_ordered_by_path() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let a = registered_asset(&mut catalog, "IMG_0001.CR3");

    let nature = catalog.create_keyword(None, "Nature").unwrap();
    let birds = catalog.create_keyword(Some(nature), "Birds").unwrap();
    let heron = catalog.create_keyword(Some(birds), "Heron").unwrap();
    let alpha = catalog.create_keyword(None, "Alpha").unwrap();

    catalog.add_keyword(&[a.asset], heron).unwrap();
    catalog.add_keyword(&[a.asset], alpha).unwrap();
    catalog.add_keyword(&[a.asset], nature).unwrap();

    // Alpha < Nature < Nature/Birds/Heron.
    assert_eq!(
        catalog.asset_keywords(a.asset).unwrap(),
        vec![alpha, nature, heron]
    );
}

/// ADR 0134 §4: renaming rewrites the level and every path under it, and
/// touches nothing a photograph carries.
#[test]
fn renaming_rewrites_the_subtree_and_keeps_the_tags() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let asset = registered_asset(&mut catalog, "IMG_0001.CR3").asset;

    let nature = catalog.create_keyword(None, "Nature").unwrap();
    let birds = catalog.create_keyword(Some(nature), "Birds").unwrap();
    let heron = catalog.create_keyword(Some(birds), "Heron").unwrap();
    // A sibling branch, to prove the prefix match does not reach it.
    let natural = catalog.create_keyword(None, "Naturalism").unwrap();
    catalog.add_keyword(&[asset], heron).unwrap();

    catalog.rename_keyword(nature, "Wildlife").unwrap();

    let paths: Vec<String> = catalog
        .keyword_tree()
        .unwrap()
        .iter()
        .flat_map(flatten)
        .map(|(_, path)| path)
        .collect();
    assert!(paths.contains(&"Wildlife".to_owned()));
    assert!(paths.contains(&"Wildlife/Birds".to_owned()));
    assert!(paths.contains(&"Wildlife/Birds/Heron".to_owned()));
    // `Naturalism` starts with `Nature`'s old name and must be untouched:
    // the rewrite matches `Nature/%`, not `Nature%`.
    assert!(paths.contains(&"Naturalism".to_owned()));
    assert_eq!(catalog.keyword_tree().unwrap().len(), 2);
    let _ = natural;

    // The tag is by id, so the photograph still carries the same keyword.
    assert_eq!(catalog.asset_keywords(asset).unwrap(), vec![heron]);
}

/// ADR 0134 §4: leaves only, and the tags go with it.
#[test]
fn deleting_is_refused_above_a_child_and_untags_below_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let asset = registered_asset(&mut catalog, "IMG_0002.CR3").asset;

    let nature = catalog.create_keyword(None, "Nature").unwrap();
    let birds = catalog.create_keyword(Some(nature), "Birds").unwrap();
    catalog.add_keyword(&[asset], birds).unwrap();

    // A keyword with children is refused: one deletes leaves, upward.
    assert!(catalog.delete_keyword(nature).is_err());
    assert_eq!(catalog.asset_keywords(asset).unwrap(), vec![birds]);

    catalog.delete_keyword(birds).unwrap();
    assert!(catalog.asset_keywords(asset).unwrap().is_empty());
    // And now the parent is a leaf.
    catalog.delete_keyword(nature).unwrap();
    assert!(catalog.keyword_tree().unwrap().is_empty());
}

/// ADR 0134 §2: one query for every count the panel shows.
#[test]
fn counts_are_per_keyword_and_direct() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let one = registered_asset(&mut catalog, "IMG_0003.CR3").asset;
    let two = registered_asset(&mut catalog, "IMG_0004.CR3").asset;

    let nature = catalog.create_keyword(None, "Nature").unwrap();
    let birds = catalog.create_keyword(Some(nature), "Birds").unwrap();
    catalog.add_keyword(&[one, two], birds).unwrap();
    catalog.add_keyword(&[one], nature).unwrap();

    let mut counts = catalog.keyword_counts().unwrap();
    counts.sort_by_key(|&(keyword, _)| keyword.get());
    assert_eq!(counts, vec![(nature, 1), (birds, 2)]);

    // Direct, not rolled up: the subtree total is the caller's to compute,
    // because the caller is the one holding the tree.
    assert_eq!(counts.iter().find(|(k, _)| *k == nature).unwrap().1, 1);
}

/// Every `(id, path)` in a tree, depth-first.
fn flatten(node: &leyline_catalog::KeywordNode) -> Vec<(KeywordId, String)> {
    let mut out = vec![(node.keyword, node.path.clone())];
    for child in &node.children {
        out.extend(flatten(child));
    }
    out
}
