//! Incremental schema migrations (`docs/catalog.md` §34).
//!
//! The schema version lives exclusively in `PRAGMA user_version` — never in a
//! table. Each migration runs inside a single transaction that also bumps
//! `user_version`, so a migration is applied exactly once or not at all.

use rusqlite::Connection;

use crate::db_err;
use leyline_core::Result;

/// Migration scripts: index `n` migrates the database to `user_version` `n + 1`.
const MIGRATIONS: &[&str] = &[
    SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, SCHEMA_V4, SCHEMA_V5, SCHEMA_V6, SCHEMA_V7,
];

/// The schema version produced by the newest migration.
pub(crate) const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

/// Reads `PRAGMA user_version`.
pub(crate) fn user_version(conn: &Connection) -> Result<u32> {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(db_err)
}

/// Applies every pending migration, one transaction per migration.
pub(crate) fn migrate(conn: &mut Connection) -> Result<()> {
    loop {
        let version = user_version(conn)?;
        if version >= SCHEMA_VERSION {
            return Ok(());
        }
        let script = MIGRATIONS[version as usize];
        let tx = conn.transaction().map_err(db_err)?;
        tx.execute_batch(script).map_err(db_err)?;
        tx.pragma_update(None, "user_version", version + 1)
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
    }
}

/// Version 1: the complete initial schema of `docs/catalog.md` (v2.3).
const SCHEMA_V1: &str = "
-- §7 Library (single row)
CREATE TABLE library (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- §8 Folders
CREATE TABLE folders (
    id INTEGER PRIMARY KEY,
    parent_id INTEGER NULL,
    relative_path TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(parent_id)
        REFERENCES folders(id)
        ON DELETE RESTRICT
);

-- §9 Assets
CREATE TABLE assets (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE,
    folder_id INTEGER NOT NULL,
    filename TEXT NOT NULL,
    extension TEXT NOT NULL,
    media_type INTEGER NOT NULL,
    file_size INTEGER NOT NULL,
    checksum BLOB NOT NULL,
    width INTEGER,
    height INTEGER,
    capture_date INTEGER,
    capture_offset_minutes INTEGER,
    imported_at INTEGER NOT NULL,
    modified_at INTEGER NOT NULL,
    is_missing INTEGER NOT NULL DEFAULT 0,
    UNIQUE(folder_id, filename),
    FOREIGN KEY(folder_id)
        REFERENCES folders(id)
        ON DELETE RESTRICT
);

-- §14 Cameras
CREATE TABLE cameras (
    id INTEGER PRIMARY KEY,
    manufacturer TEXT NOT NULL,
    model TEXT NOT NULL,
    UNIQUE(manufacturer, model)
);

-- §15 Lenses
CREATE TABLE lenses (
    id INTEGER PRIMARY KEY,
    manufacturer TEXT NOT NULL,
    model TEXT NOT NULL,
    mount TEXT,
    UNIQUE(manufacturer, model)
);

-- §13 Metadata
CREATE TABLE metadata (
    asset_id INTEGER PRIMARY KEY,
    camera_id INTEGER,
    lens_id INTEGER,
    orientation INTEGER,
    iso INTEGER,
    shutter_numerator INTEGER,
    shutter_denominator INTEGER,
    aperture_numerator INTEGER,
    aperture_denominator INTEGER,
    focal_length_numerator INTEGER,
    focal_length_denominator INTEGER,
    shutter_speed_s REAL GENERATED ALWAYS AS
        (CAST(shutter_numerator AS REAL) / shutter_denominator) STORED,
    aperture_f REAL GENERATED ALWAYS AS
        (CAST(aperture_numerator AS REAL) / aperture_denominator) STORED,
    focal_length_mm REAL GENERATED ALWAYS AS
        (CAST(focal_length_numerator AS REAL) / focal_length_denominator) STORED,
    exposure_bias REAL,
    flash INTEGER,
    white_balance_mode INTEGER,
    color_space TEXT,
    gps_latitude REAL,
    gps_longitude REAL,
    gps_altitude REAL,
    artist TEXT,
    copyright TEXT,
    FOREIGN KEY(asset_id)
        REFERENCES assets(id)
        ON DELETE CASCADE,
    FOREIGN KEY(camera_id)
        REFERENCES cameras(id)
        ON DELETE RESTRICT,
    FOREIGN KEY(lens_id)
        REFERENCES lenses(id)
        ON DELETE RESTRICT
);

-- §17 Develop revisions
CREATE TABLE develop_revisions (
    id INTEGER PRIMARY KEY,
    asset_id INTEGER NOT NULL,
    parent_revision_id INTEGER,
    settings_json TEXT NOT NULL,
    author TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(asset_id)
        REFERENCES assets(id)
        ON DELETE CASCADE,
    FOREIGN KEY(parent_revision_id)
        REFERENCES develop_revisions(id)
        ON DELETE RESTRICT
);

-- §18 Develop versions (branches)
CREATE TABLE develop_versions (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE,
    asset_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    head_revision_id INTEGER NOT NULL,
    rating INTEGER NULL
        CHECK(rating BETWEEN 1 AND 5),
    color_label INTEGER NULL,
    pick_state INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    UNIQUE(asset_id, name),
    FOREIGN KEY(asset_id)
        REFERENCES assets(id)
        ON DELETE CASCADE,
    FOREIGN KEY(head_revision_id)
        REFERENCES develop_revisions(id)
        ON DELETE RESTRICT
);

-- §18 Current version per asset
CREATE TABLE develop_current (
    asset_id INTEGER PRIMARY KEY,
    version_id INTEGER NOT NULL,
    FOREIGN KEY(asset_id)
        REFERENCES assets(id)
        ON DELETE CASCADE,
    FOREIGN KEY(version_id)
        REFERENCES develop_versions(id)
        ON DELETE CASCADE
);

-- §19 Previews (metadata only; files live in Cache/)
CREATE TABLE previews (
    id INTEGER PRIMARY KEY,
    asset_id INTEGER NOT NULL,
    revision_id INTEGER NOT NULL,
    kind INTEGER NOT NULL,
    width INTEGER NOT NULL,
    height INTEGER NOT NULL,
    relative_path TEXT NOT NULL,
    generated_at INTEGER NOT NULL,
    UNIQUE(asset_id, revision_id, kind),
    FOREIGN KEY(asset_id)
        REFERENCES assets(id)
        ON DELETE CASCADE,
    FOREIGN KEY(revision_id)
        REFERENCES develop_revisions(id)
        ON DELETE CASCADE
);

-- §22 Keywords (hierarchical)
CREATE TABLE keywords (
    id INTEGER PRIMARY KEY,
    parent_id INTEGER,
    name TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(parent_id)
        REFERENCES keywords(id)
        ON DELETE RESTRICT
);

-- §23 Asset keywords (N:N)
CREATE TABLE asset_keywords (
    asset_id INTEGER NOT NULL,
    keyword_id INTEGER NOT NULL,
    PRIMARY KEY(asset_id, keyword_id),
    FOREIGN KEY(asset_id)
        REFERENCES assets(id)
        ON DELETE CASCADE,
    FOREIGN KEY(keyword_id)
        REFERENCES keywords(id)
        ON DELETE RESTRICT
);

-- §24 Collections
CREATE TABLE collections (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE,
    parent_collection_id INTEGER,
    name TEXT NOT NULL,
    description TEXT,
    collection_type INTEGER NOT NULL,
    rules_json TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(parent_collection_id)
        REFERENCES collections(id)
        ON DELETE RESTRICT
);

-- §25 Collection versions
CREATE TABLE collection_versions (
    collection_id INTEGER NOT NULL,
    version_id INTEGER NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY(collection_id, version_id),
    FOREIGN KEY(collection_id)
        REFERENCES collections(id)
        ON DELETE CASCADE,
    FOREIGN KEY(version_id)
        REFERENCES develop_versions(id)
        ON DELETE CASCADE
);

-- §27 Export presets
CREATE TABLE export_presets (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    settings_json TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

-- ADR 0036 Print presets, parallel to export_presets
CREATE TABLE print_presets (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    settings_json TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

-- docs/presets.md §4 Develop presets
CREATE TABLE develop_presets (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    preset_json TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

-- §28 Export history
CREATE TABLE export_history (
    id INTEGER PRIMARY KEY,
    asset_id INTEGER NOT NULL,
    preset_id INTEGER,
    format TEXT NOT NULL,
    destination TEXT NOT NULL,
    exported_at INTEGER NOT NULL,
    FOREIGN KEY(asset_id)
        REFERENCES assets(id)
        ON DELETE CASCADE,
    FOREIGN KEY(preset_id)
        REFERENCES export_presets(id)
        ON DELETE SET NULL
);

-- §30 Full-text search
CREATE VIRTUAL TABLE search_index USING fts5(
    asset_id UNINDEXED,
    filename,
    keywords,
    artist,
    copyright,
    tokenize = \"unicode61 remove_diacritics 2\"
);

-- §32 Index strategy
CREATE INDEX idx_assets_capture_date ON assets(capture_date);
CREATE INDEX idx_assets_folder ON assets(folder_id);

CREATE INDEX idx_metadata_camera ON metadata(camera_id);
CREATE INDEX idx_metadata_lens ON metadata(lens_id);
CREATE INDEX idx_metadata_iso ON metadata(iso);
CREATE INDEX idx_metadata_focal ON metadata(focal_length_mm);
CREATE INDEX idx_metadata_aperture ON metadata(aperture_f);
CREATE INDEX idx_metadata_shutter ON metadata(shutter_speed_s);

CREATE INDEX idx_keywords_parent ON keywords(parent_id);
CREATE INDEX idx_keywords_path ON keywords(path);

CREATE INDEX idx_collection_parent ON collections(parent_collection_id);
CREATE INDEX idx_collection_versions_position ON collection_versions(collection_id, position);

CREATE INDEX idx_develop_asset ON develop_revisions(asset_id);
CREATE INDEX idx_develop_parent ON develop_revisions(parent_revision_id);
CREATE INDEX idx_develop_versions_asset ON develop_versions(asset_id);
CREATE INDEX idx_develop_versions_rating ON develop_versions(rating);
CREATE INDEX idx_develop_versions_color ON develop_versions(color_label);
CREATE INDEX idx_develop_versions_pick ON develop_versions(pick_state);
";

/// Version 2: preset shelving and provenance (ADR 0058).
///
/// Purely additive — two tables' worth of columns and one new table — so an
/// existing library opens and carries on. Nothing here is read by the render
/// pipeline: a preset's identity is catalog metadata, never a pixel input,
/// which is exactly why it is not in `settings_json` (ADR 0058 §5).
const SCHEMA_V2: &str = "
-- §27 Preset folders: one level, renamable, may be empty (ADR 0058 §2).
CREATE TABLE preset_folders (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

-- A preset can sit in a folder, be a favourite, and carries a revision
-- counter bumped by every update (ADR 0058 §6).
ALTER TABLE develop_presets ADD COLUMN folder_id INTEGER
    REFERENCES preset_folders(id) ON DELETE SET NULL;
ALTER TABLE develop_presets ADD COLUMN favourite INTEGER NOT NULL DEFAULT 0;
ALTER TABLE develop_presets ADD COLUMN preset_revision INTEGER NOT NULL DEFAULT 1;
ALTER TABLE develop_presets ADD COLUMN updated_at INTEGER;

-- Which preset produced a revision, and which version of it (ADR 0058 §5).
-- Both NULL for every revision made by hand, which is most of them.
ALTER TABLE develop_revisions ADD COLUMN from_preset_id INTEGER
    REFERENCES develop_presets(id) ON DELETE SET NULL;
ALTER TABLE develop_revisions ADD COLUMN from_preset_revision INTEGER;

CREATE INDEX idx_presets_folder ON develop_presets(folder_id);
CREATE INDEX idx_develop_from_preset ON develop_revisions(from_preset_id);
";

/// Version 3: RAW+JPEG pairing (ADR 0079).
///
/// One nullable column, and it pairs nothing: a migration that paired would
/// make half of an existing library's thumbnails vanish at the first launch
/// after an update (ADR 0079 §7). Existing catalogs carry on showing every
/// file until the explicit pass is run.
const SCHEMA_V3: &str = "
-- §9 A non-RAW file can be the companion of the RAW it was shot with
-- (ADR 0079 §1). NULL — the overwhelming majority — means the asset is
-- itself. A companion never survives its master.
ALTER TABLE assets ADD COLUMN companion_of INTEGER
    REFERENCES assets(id) ON DELETE CASCADE;

CREATE INDEX idx_assets_companion ON assets(companion_of);
";

/// Version 4: the duplicate index the import path was missing.
///
/// `find_asset_by_checksum` (§12) runs once per file offered to the import,
/// and without an index it is a full scan of `assets`: measured at 2,1 ms on
/// a 50 000-asset library against 1,9 µs indexed, and it grows with the
/// library, so the cost of importing a batch grows with everything imported
/// before it. Purely additive, like the two migrations before it.
///
/// Deliberately not `UNIQUE`: two identical files are a legitimate library
/// state (§12 detects the duplicate, it never forbids it), and the import
/// decides what to do about it.
const SCHEMA_V4: &str = "
-- §32 The import's duplicate check, once per candidate file.
CREATE INDEX idx_assets_checksum ON assets(checksum);
";

/// Version 5: the index the grid page walks (ADR 0081 §3).
///
/// `companion_of` first because every grid query carries
/// `a.companion_of IS NULL` (ADR 0079 §5), `capture_date` next because it is
/// the default sort and by far the most used, `id` last because it is the
/// tiebreak. Together with the deferred page of ADR 0081 §1, a window of a
/// hundred cells falls from 10,4 ms to 0,29 ms on a 50 000-asset library.
///
/// `idx_assets_companion` stays, though this index has it as a strict prefix,
/// and the measurement is the whole argument: `count()` scans that index
/// end to end to size the grid's scrollbar, and scanning the wide one costs
/// 28,7 ms against 3,3 ms — the narrow entries are what make it cheap. The
/// same covering index answers the companion test of ADR 0079 §6. One more
/// b-tree per registered asset is the price, on a write path that is not
/// where imports spend their time.
const SCHEMA_V5: &str = "
-- §32 The grid page: the pairing clause, then the default sort.
CREATE INDEX idx_assets_grid ON assets(companion_of, capture_date, id);
";

/// Version 6: where a cached preview's pixels came from (ADR 0082 §2).
///
/// A preview the camera embedded never went through the develop pipeline.
/// Recording it as the render of a revision would be a lie the rest of the
/// engine believes: `valid_preview` answers "here is the head's render", so
/// nothing would ever replace it, and an undo landing back on that revision
/// would show the camera's JPEG while claiming to show a development.
///
/// One nullable-free column with a default, so every existing row keeps the
/// only meaning it could have had: everything written before this migration
/// came out of the pipeline.
const SCHEMA_V6: &str = "
-- §19 0 = rendered by the pipeline, 1 = the preview the file carried.
ALTER TABLE previews ADD COLUMN origin INTEGER NOT NULL DEFAULT 0;
";

/// Version 7: the four `ON DELETE CASCADE` keys no index covered (§32).
///
/// SQLite enforces a cascade by looking for the child rows that reference the
/// deleted parent. Without an index on the referencing column that lookup is a
/// **full scan of the child table, once per deleted row** — quadratic on a
/// batch. Nothing fails and nothing warns; the delete simply gets slower with
/// the library.
///
/// The audit of 2026-08-26 had named two of these by hand. The other two came
/// out of `every_cascading_foreign_key_is_indexed`, which asks the schema
/// itself rather than a reader's memory:
///
/// * `develop_current(version_id)` — one row per asset. Its *other* key needs
///   nothing: `asset_id` is the table's primary key, so that cascade already
///   had a b-tree.
/// * `export_history(asset_id)` — grows with every export the user ever ran.
/// * `previews(revision_id)` — grows with every render kept in cache.
/// * `collection_versions(version_id)` — `idx_collection_versions_position`
///   covers `(collection_id, position)`, and `version_id` does not lead it.
///
/// Deleting 20 000 assets from a library of 20 000, in batches of 500, with an
/// export history of 26 667 rows and every version filed in a collection:
/// **3 714 µs/asset without, 1 864 µs/asset with** — the cost roughly halved
/// (−50 %, and −53 % with the two cases run in the opposite order). What
/// remains is the cascade chain itself over eight tables plus the FTS5 row,
/// which no index removes.
///
/// Purely additive. The price is four b-trees on write paths that are not
/// where imports spend their time.
const SCHEMA_V7: &str = "
-- §32 The cascades that scanned: a delete looks up children by these.
CREATE INDEX idx_develop_current_version ON develop_current(version_id);
CREATE INDEX idx_export_history_asset ON export_history(asset_id);
CREATE INDEX idx_previews_revision ON previews(revision_id);
CREATE INDEX idx_collection_versions_version ON collection_versions(version_id);
";
