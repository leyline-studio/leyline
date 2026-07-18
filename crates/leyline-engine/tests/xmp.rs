//! Integration tests: XMP sidecar export (`docs/catalog.md` §29).

use leyline_core::{ColorLabel, LeylineError};
use leyline_engine::{ImportOptions, Library};

#[test]
fn sidecar_reflects_the_catalog_truth() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Lib");
    let mut library = Library::create(&root, "XMP").unwrap();

    std::fs::write(dir.path().join("heron.png"), b"pixels").unwrap();
    let report = library
        .import(
            &dir.path().join("heron.png"),
            &ImportOptions {
                copy_files: true,
                recursive: false,
            },
            |_, _| {},
        )
        .unwrap();
    let registered = report.imported[0].registered;

    let catalog = library.catalog_mut();
    catalog.set_rating(&[registered.version], Some(4)).unwrap();
    catalog
        .set_color_label(&[registered.version], Some(ColorLabel::Blue))
        .unwrap();
    let nature = catalog.create_keyword(None, "Nature").unwrap();
    let heron = catalog.create_keyword(Some(nature), "Héron & co").unwrap();
    catalog.add_keyword(&[registered.asset], heron).unwrap();

    let sidecar = library.write_xmp(registered.asset).unwrap();
    assert_eq!(sidecar, root.join("Photos").join("heron.xmp"));
    let xml = std::fs::read_to_string(&sidecar).unwrap();

    assert!(xml.contains("xmp:Rating=\"4\""));
    assert!(xml.contains("xmp:Label=\"Blue\""));
    // Flat subject uses the leaf name, hierarchical uses | separators;
    // XML-special characters are escaped.
    assert!(xml.contains("<rdf:li>Héron &amp; co</rdf:li>"));
    assert!(xml.contains("<rdf:li>Nature|Héron &amp; co</rdf:li>"));

    // Synchronizing again after a change rewrites the same file.
    library
        .catalog_mut()
        .set_rating(&[registered.version], None)
        .unwrap();
    library.write_xmp(registered.asset).unwrap();
    let xml = std::fs::read_to_string(&sidecar).unwrap();
    assert!(!xml.contains("xmp:Rating"));
    assert!(xml.contains("xmp:Label=\"Blue\""));
}

#[test]
fn missing_assets_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Lib"), "XMP").unwrap();
    assert!(matches!(
        library.write_xmp(leyline_core::AssetId::new(999)),
        Err(LeylineError::AssetMissing(_))
    ));
}
