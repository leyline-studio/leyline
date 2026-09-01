//! Integration tests: where a derived asset came from (ADR 0107 §5).
//!
//! Two properties carry the design, and both are about what `derived_from`
//! is *not*: it does not hide the row from the grid, the way `companion_of`
//! does, and it does not take the derived asset down with its parent.

use leyline_catalog::{Catalog, GridQuery, NewAsset};
use leyline_core::{AssetId, MediaType, Settings};

fn new_catalog(dir: &tempfile::TempDir) -> Catalog {
    Catalog::create(&dir.path().join("catalog.db"), "Derived").unwrap()
}

fn add(catalog: &mut Catalog, filename: &str) -> AssetId {
    let (stem, extension) = filename.rsplit_once('.').unwrap();
    let new = NewAsset {
        folder: catalog.ensure_folder("Photos/2026").unwrap(),
        filename: filename.to_owned(),
        extension: extension.to_ascii_lowercase(),
        media_type: if extension.eq_ignore_ascii_case("tif") {
            MediaType::Tiff
        } else {
            MediaType::Raw
        },
        file_size: 1,
        checksum: std::array::from_fn(|i| (stem.len() + i + extension.len()) as u8),
        width: Some(6000),
        height: Some(4000),
        capture_date: Some(1_700_000_000_000),
        capture_offset_minutes: None,
    };
    catalog.add_asset(&new, &Settings::default()).unwrap().asset
}

#[test]
fn an_ordinary_asset_is_derived_from_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let asset = add(&mut catalog, "IMG_0001.CR2");
    assert_eq!(catalog.derived_from(asset).unwrap(), None);
    assert!(catalog.derivatives_of(asset).unwrap().is_empty());
}

#[test]
fn a_derivation_is_readable_from_both_ends() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let parent = add(&mut catalog, "IMG_0001.CR2");
    let child = add(&mut catalog, "IMG_0001-denoise.tif");
    catalog.set_derived_from(child, parent).unwrap();
    assert_eq!(catalog.derived_from(child).unwrap(), Some(parent));
    assert_eq!(catalog.derivatives_of(parent).unwrap(), vec![child]);
    assert_eq!(
        catalog.asset_details(child).unwrap().derived_from,
        Some(parent)
    );
}

#[test]
fn an_asset_cannot_be_derived_from_itself() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let asset = add(&mut catalog, "IMG_0001.CR2");
    assert!(catalog.set_derived_from(asset, asset).is_err());
}

/// Unlike a companion, which the grid hides behind its master (ADR 0079 §5),
/// a derived asset is a photograph the user chooses between: it stays a cell.
#[test]
fn a_derived_asset_stays_in_the_grid() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let parent = add(&mut catalog, "IMG_0001.CR2");
    let child = add(&mut catalog, "IMG_0001-denoise.tif");
    catalog.set_derived_from(child, parent).unwrap();
    assert_eq!(
        catalog.count(&GridQuery::default()).unwrap(),
        2,
        "a derived asset is a cell of its own, where a companion is not"
    );
}

/// The whole reason the column is `SET NULL` and not `CASCADE`: a denoised
/// frame is what the user kept, and deleting what it was made from must not
/// delete it too (ADR 0107 §5).
#[test]
fn deleting_the_parent_keeps_the_derived_asset() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = new_catalog(&dir);
    let parent = add(&mut catalog, "IMG_0001.CR2");
    let child = add(&mut catalog, "IMG_0001-denoise.tif");
    catalog.set_derived_from(child, parent).unwrap();
    catalog.delete_assets(&[parent]).unwrap();
    assert_eq!(catalog.derived_from(child).unwrap(), None);
    assert!(catalog.asset_details(child).is_ok());
}
