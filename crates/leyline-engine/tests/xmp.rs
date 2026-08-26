//! Integration tests: XMP sidecars, written (`docs/catalog.md` §29) and read
//! back as a seed (ADR 0047).

use leyline_core::{AssetId, ColorLabel, LeylineError};
use leyline_engine::{ImportOptions, Library};

/// A library with one imported PNG, plus the source directory it came from.
fn library_with_one_photo(dir: &std::path::Path) -> (Library, AssetId, leyline_core::VersionId) {
    let library = Library::create(&dir.join("Lib"), "XMP").unwrap();
    std::fs::write(dir.join("heron.png"), b"pixels").unwrap();
    let report = library
        .import(
            &dir.join("heron.png"),
            &ImportOptions {
                copy_files: true,
                recursive: false,
                pair_companions: true,
                thumbnails: false,
            },
            |_, _| {},
        )
        .unwrap();
    let registered = report.imported[0].registered;
    (library, registered.asset, registered.version)
}

/// Where an explicit `read_xmp` looks: beside the *catalogued* file, which is
/// the copy under `Photos/`, since that is the only path the catalog knows.
fn sidecar_beside_the_copy(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("Lib").join("Photos").join("heron.xmp")
}

/// A sidecar as another program would leave it: element form for the rating,
/// attribute form for the label, both keyword spellings, a namespace prefix
/// that is *not* the one we write, and whitespace throughout.
const FOREIGN_SIDECAR: &str = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Some Other Program 9.2">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
   xmlns:ns9="http://ns.adobe.com/xap/1.0/"
   xmlns:dc="http://purl.org/dc/elements/1.1/"
   xmlns:lr="http://ns.adobe.com/lightroom/1.0/"
   ns9:Label="green">
   <ns9:Rating> 3 </ns9:Rating>
   <dc:subject><rdf:Bag>
    <rdf:li>Kyoto</rdf:li>
   </rdf:Bag></dc:subject>
   <lr:hierarchicalSubject><rdf:Bag>
    <rdf:li>Voyage|Japon|Kyoto</rdf:li>
   </rdf:Bag></lr:hierarchicalSubject>
   <dc:creator><rdf:Seq><rdf:li>Ada</rdf:li></rdf:Seq></dc:creator>
   <dc:rights><rdf:Alt><rdf:li xml:lang="x-default">© Ada</rdf:li></rdf:Alt></dc:rights>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>
"#;

#[test]
fn sidecar_reflects_the_catalog_truth() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Lib");
    let library = Library::create(&root, "XMP").unwrap();

    std::fs::write(dir.path().join("heron.png"), b"pixels").unwrap();
    let report = library
        .import(
            &dir.path().join("heron.png"),
            &ImportOptions {
                copy_files: true,
                recursive: false,
                pair_companions: true,
                thumbnails: false,
            },
            |_, _| {},
        )
        .unwrap();
    let registered = report.imported[0].registered;

    // Scoped: the catalog guard must be released before write_xmp locks.
    {
        let mut catalog = library.catalog_mut();
        catalog.set_rating(&[registered.version], Some(4)).unwrap();
        catalog
            .set_color_label(&[registered.version], Some(ColorLabel::Blue))
            .unwrap();
        let nature = catalog.create_keyword(None, "Nature").unwrap();
        let heron = catalog.create_keyword(Some(nature), "Héron & co").unwrap();
        catalog.add_keyword(&[registered.asset], heron).unwrap();
    }

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

/// darktable's naming: the sidecar keeps the photo's whole name and appends
/// `.xmp` (`kyoto.png.xmp`), where Lightroom replaces the extension
/// (`kyoto.xmp`). Reading only the second form is a silent loss of the first
/// program's work — ADR 0047 §2.1, found on a real darktable sidecar.
#[test]
fn an_import_seeds_the_catalog_from_a_sidecar_named_the_darktable_way() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Lib"), "XMP").unwrap();
    std::fs::write(dir.path().join("kyoto.png"), b"pixels").unwrap();
    std::fs::write(dir.path().join("kyoto.png.xmp"), FOREIGN_SIDECAR).unwrap();

    let report = library
        .import(
            &dir.path().join("kyoto.png"),
            &ImportOptions {
                copy_files: true,
                recursive: false,
                pair_companions: true,
                thumbnails: false,
            },
            |_, _| {},
        )
        .unwrap();
    let registered = report.imported[0].registered;

    let catalog = library.catalog();
    let details = catalog.asset_details(registered.asset).unwrap();
    let current = details
        .versions
        .iter()
        .find(|v| v.version == details.current_version)
        .unwrap();
    assert_eq!(current.rating, Some(3));
    assert_eq!(details.keywords.len(), 1);
}

/// When both conventions sit beside the same photo, the whole-name form wins:
/// it names one photo, where the stem form is shared by every file of that
/// stem in the directory (ADR 0047 §2.1).
#[test]
fn the_unambiguous_sidecar_name_wins_over_the_shared_one() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("kyoto.png");
    std::fs::write(&file, b"pixels").unwrap();
    std::fs::write(dir.path().join("kyoto.png.xmp"), FOREIGN_SIDECAR).unwrap();
    std::fs::write(
        dir.path().join("kyoto.xmp"),
        FOREIGN_SIDECAR.replace("<ns9:Rating> 3 </ns9:Rating>", "<ns9:Rating>1</ns9:Rating>"),
    )
    .unwrap();

    let sidecar = leyline_engine::read_xmp_sidecar(&file).unwrap();
    assert_eq!(sidecar.rating, Some(3));

    // And the stem form still answers on its own, which is what Lightroom
    // and Leyline itself write.
    std::fs::remove_file(dir.path().join("kyoto.png.xmp")).unwrap();
    assert_eq!(
        leyline_engine::read_xmp_sidecar(&file).unwrap().rating,
        Some(1)
    );
}

/// The migration path itself: the sidecar is next to the file *before* the
/// import, and the import seeds the catalog from it without the user asking
/// for anything (ADR 0047 §2). Also the ADR §6 case: foreign prefixes,
/// element/attribute duality, whitespace.
#[test]
fn an_import_seeds_the_catalog_from_a_sidecar_left_by_another_program() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Lib"), "XMP").unwrap();
    std::fs::write(dir.path().join("kyoto.png"), b"pixels").unwrap();
    std::fs::write(dir.path().join("kyoto.xmp"), FOREIGN_SIDECAR).unwrap();

    let report = library
        .import(
            &dir.path().join("kyoto.png"),
            &ImportOptions {
                copy_files: true,
                recursive: false,
                pair_companions: true,
                thumbnails: false,
            },
            |_, _| {},
        )
        .unwrap();
    let registered = report.imported[0].registered;

    let catalog = library.catalog();
    let details = catalog.asset_details(registered.asset).unwrap();
    let current = details
        .versions
        .iter()
        .find(|v| v.version == details.current_version)
        .unwrap();
    assert_eq!(current.rating, Some(3));
    assert_eq!(current.color_label, Some(ColorLabel::Green));
    assert_eq!(
        details.metadata.as_ref().unwrap().artist.as_deref(),
        Some("Ada")
    );
    assert_eq!(
        details.metadata.as_ref().unwrap().copyright.as_deref(),
        Some("© Ada")
    );

    // The hierarchy won over the flat `dc:subject`: three levels created,
    // and the leaf is what the asset carries.
    let paths: Vec<String> = catalog
        .keyword_tree()
        .unwrap()
        .iter()
        .flat_map(|root| {
            let mut out = vec![root.path.clone()];
            let mut stack: Vec<_> = root.children.iter().collect();
            while let Some(node) = stack.pop() {
                out.push(node.path.clone());
                stack.extend(node.children.iter());
            }
            out
        })
        .collect();
    assert!(paths.contains(&"Voyage".to_owned()), "{paths:?}");
    assert!(paths.contains(&"Voyage/Japon".to_owned()), "{paths:?}");
    assert!(
        paths.contains(&"Voyage/Japon/Kyoto".to_owned()),
        "{paths:?}"
    );
    assert_eq!(details.keywords.len(), 1);
}

/// The whole of ADR 0047 §3: a sidecar fills what is empty and never touches
/// what is not. The rating and label already in the catalog survive a sidecar
/// that disagrees, and its keyword is added beside the existing one.
#[test]
fn a_sidecar_fills_the_gaps_and_overwrites_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (library, asset, version) = library_with_one_photo(dir.path());
    {
        let mut catalog = library.catalog_mut();
        catalog.set_rating(&[version], Some(5)).unwrap();
        let own = catalog.create_keyword(None, "Trié dans Leyline").unwrap();
        catalog.add_keyword(&[asset], own).unwrap();
    }
    // The sidecar disagrees on the rating, says nothing we have on the label.
    std::fs::write(sidecar_beside_the_copy(dir.path()), FOREIGN_SIDECAR).unwrap();

    assert!(library.read_xmp(asset).unwrap());

    let catalog = library.catalog();
    let details = catalog.asset_details(asset).unwrap();
    let current = details
        .versions
        .iter()
        .find(|v| v.version == details.current_version)
        .unwrap();
    assert_eq!(current.rating, Some(5), "the catalog's rating must survive");
    assert_eq!(
        current.color_label,
        Some(ColorLabel::Green),
        "the gap is filled"
    );
    assert_eq!(details.keywords.len(), 2, "keywords are a union");
}

/// Applying twice writes nothing the second time: keyword tagging is
/// idempotent and every other field is now filled.
#[test]
fn reading_the_same_sidecar_twice_is_a_no_op() {
    let dir = tempfile::tempdir().unwrap();
    let (library, asset, _) = library_with_one_photo(dir.path());
    std::fs::write(sidecar_beside_the_copy(dir.path()), FOREIGN_SIDECAR).unwrap();

    assert!(library.read_xmp(asset).unwrap());
    assert!(
        !library.read_xmp(asset).unwrap(),
        "a second read has nothing left to fill"
    );
    assert_eq!(
        library
            .catalog()
            .asset_details(asset)
            .unwrap()
            .keywords
            .len(),
        1
    );
}

/// ADR 0047 §4: what we write, we read. The invariant that keeps both halves
/// of the module honest when either one moves.
#[test]
fn what_the_writer_produces_the_reader_understands() {
    let dir = tempfile::tempdir().unwrap();
    let (library, asset, version) = library_with_one_photo(dir.path());
    {
        let mut catalog = library.catalog_mut();
        catalog.set_rating(&[version], Some(4)).unwrap();
        catalog
            .set_color_label(&[version], Some(ColorLabel::Blue))
            .unwrap();
        let nature = catalog.create_keyword(None, "Nature").unwrap();
        let heron = catalog.create_keyword(Some(nature), "Héron & co").unwrap();
        catalog.add_keyword(&[asset], heron).unwrap();
    }
    let written = library.write_xmp(asset).unwrap();

    let parsed = leyline_engine::read_xmp_sidecar(&written.with_extension("png")).unwrap();
    assert_eq!(parsed.rating, Some(4));
    assert_eq!(parsed.color_label, Some(ColorLabel::Blue));
    // The escaped `&` survives the round trip, and the hierarchy comes back
    // as a path rather than Lightroom's pipes.
    assert_eq!(parsed.keywords, vec!["Nature/Héron & co".to_owned()]);
}

/// ADR 0047 §5: a broken sidecar costs the seeding, never the photo.
#[test]
fn a_malformed_sidecar_never_breaks_the_import() {
    let dir = tempfile::tempdir().unwrap();
    let library = Library::create(&dir.path().join("Lib"), "XMP").unwrap();
    std::fs::write(dir.path().join("torn.png"), b"pixels").unwrap();
    std::fs::write(dir.path().join("torn.xmp"), "<x:xmpmeta><unclosed>").unwrap();

    let report = library
        .import(
            &dir.path().join("torn.png"),
            &ImportOptions {
                copy_files: true,
                recursive: false,
                pair_companions: true,
                thumbnails: false,
            },
            |_, _| {},
        )
        .unwrap();
    assert_eq!(report.imported.len(), 1);
    assert!(report.skipped.is_empty());
    let asset = report.imported[0].registered.asset;
    let details = library.catalog().asset_details(asset).unwrap();
    assert!(details.keywords.is_empty());
}

/// An `xmp:Rating` of `-1` is Adobe's "rejected", not a star count, and `0`
/// is "unrated": neither may land in the catalog as a rating.
#[test]
fn non_star_ratings_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let (library, asset, _) = library_with_one_photo(dir.path());
    for value in ["-1", "0", "6", "later"] {
        std::fs::write(
            sidecar_beside_the_copy(dir.path()),
            FOREIGN_SIDECAR.replace("> 3 <", &format!(">{value}<")),
        )
        .unwrap();
        library.read_xmp(asset).unwrap();
        let details = library.catalog().asset_details(asset).unwrap();
        let current = details
            .versions
            .iter()
            .find(|v| v.version == details.current_version)
            .unwrap();
        assert_eq!(current.rating, None, "{value:?} is not a rating");
    }
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
