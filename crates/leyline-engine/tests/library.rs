//! Integration tests: the library facade (`docs/engine-api.md` §5).

use leyline_catalog::GridQuery;
use leyline_core::LeylineError;
use leyline_engine::{ImportOptions, Library, Param, Value};

#[test]
fn create_open_and_work_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("MyLibrary");

    // Create: the §3 skeleton appears.
    let library = Library::create(&root, "My Library").unwrap();
    for sub in ["Photos", "Cache", "Exports", "Backups"] {
        assert!(root.join(sub).is_dir(), "{sub} should exist");
    }
    assert_eq!(library.catalog().library().unwrap().name, "My Library");
    // Creating again over the same catalog is refused.
    assert!(matches!(
        Library::create(&root, "Again"),
        Err(LeylineError::Io(_))
    ));
    drop(library);

    // Reopen and drive a full flow through the one handle.
    let library = Library::open(&root).unwrap();
    std::fs::write(dir.path().join("photo.png"), b"pixels").unwrap();
    let report = library
        .import(
            &dir.path().join("photo.png"),
            &ImportOptions {
                copy_files: true,
                recursive: false,
            },
            |_, _| {},
        )
        .unwrap();
    let registered = report.imported[0].registered;
    assert_eq!(library.catalog().count(&GridQuery::default()).unwrap(), 1);

    // Edit through the facade.
    {
        let mut session = library.edit(registered.version).unwrap();
        session.set(Param::Exposure, Value::Float(0.7)).unwrap();
        session.commit().unwrap();
    }
    assert_ne!(
        library.catalog().version_head(registered.version).unwrap(),
        registered.revision
    );

    // Classement passes through undecorated.
    library
        .catalog_mut()
        .set_rating(&[registered.version], Some(5))
        .unwrap();
}

#[test]
fn open_reports_missing_libraries() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        Library::open(&dir.path().join("nowhere")),
        Err(LeylineError::LibraryNotFound(_))
    ));
}

#[test]
fn read_only_handles_refuse_writes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("RO");
    drop(Library::create(&root, "RO").unwrap());

    let library = Library::open_read_only(&root).unwrap();
    assert!(library.catalog().is_read_only());
    assert!(matches!(
        library.catalog_mut().ensure_folder("Photos/New"),
        Err(LeylineError::Db(_))
    ));
}
