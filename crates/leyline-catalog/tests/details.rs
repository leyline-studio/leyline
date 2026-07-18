//! Integration tests: full asset details (`docs/engine-api.md` §7).

use leyline_catalog::{CameraInfo, Catalog, Metadata, NewAsset};
use leyline_core::{AssetId, LeylineError, MediaType};

#[test]
fn details_assemble_every_facet_of_an_asset() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = Catalog::create(&dir.path().join("catalog.db"), "Details").unwrap();

    let folder = catalog.ensure_folder("Photos/Wildlife").unwrap();
    let reg = catalog
        .add_asset(&NewAsset {
            folder,
            filename: "heron.CR3".to_owned(),
            extension: "CR3".to_owned(),
            media_type: MediaType::Raw,
            file_size: 32_000_000,
            checksum: [0xAB; 32],
            width: Some(6000),
            height: Some(4000),
            capture_date: Some(1_784_000_000_000),
            capture_offset_minutes: None,
        })
        .unwrap();

    catalog
        .set_metadata(
            reg.asset,
            &Metadata {
                camera: Some(CameraInfo {
                    manufacturer: "Canon".to_owned(),
                    model: "EOS R5".to_owned(),
                }),
                iso: Some(800),
                ..Metadata::default()
            },
        )
        .unwrap();
    let heron = catalog.create_keyword(None, "Heron").unwrap();
    catalog.add_keyword(&[reg.asset], heron).unwrap();
    let bw = catalog
        .create_version(reg.version, "Noir & Blanc", None)
        .unwrap();
    catalog.set_current_version(reg.asset, bw).unwrap();

    let details = catalog.asset_details(reg.asset).unwrap();
    assert_eq!(details.relative_path, "Photos/Wildlife/heron.CR3");
    assert_eq!(details.filename, "heron.CR3");
    assert_eq!(details.media_type, MediaType::Raw);
    assert_eq!(details.file_size, 32_000_000);
    assert_eq!((details.width, details.height), (Some(6000), Some(4000)));
    assert_eq!(details.capture_date, Some(1_784_000_000_000));
    assert_eq!(details.metadata.unwrap().iso, Some(800));
    assert_eq!(details.keywords, vec![heron]);
    assert_eq!(details.versions.len(), 2);
    assert_eq!(details.current_version, bw);
}

#[test]
fn details_work_without_metadata_and_report_missing_assets() {
    let dir = tempfile::tempdir().unwrap();
    let mut catalog = Catalog::create(&dir.path().join("catalog.db"), "Details").unwrap();
    let folder = catalog.ensure_folder("Photos").unwrap();
    let reg = catalog
        .add_asset(&NewAsset {
            folder,
            filename: "plain.png".to_owned(),
            extension: "png".to_owned(),
            media_type: MediaType::Png,
            file_size: 1,
            checksum: [1; 32],
            width: None,
            height: None,
            capture_date: None,
            capture_offset_minutes: None,
        })
        .unwrap();

    let details = catalog.asset_details(reg.asset).unwrap();
    assert_eq!(details.metadata, None);
    assert_eq!(details.keywords, vec![]);
    assert_eq!(details.current_version, reg.version);

    assert!(matches!(
        catalog.asset_details(AssetId::new(999)),
        Err(LeylineError::AssetMissing(_))
    ));
}
