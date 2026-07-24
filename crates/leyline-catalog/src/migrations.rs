//! Incremental schema migrations (`docs/catalog.md` §34).
//!
//! The schema version lives exclusively in `PRAGMA user_version` — never in a
//! table. Each migration runs inside a single transaction that also bumps
//! `user_version`, so a migration is applied exactly once or not at all.

use rusqlite::Connection;

use crate::db_err;
use leyline_core::Result;

/// Migration scripts: index `n` migrates the database to `user_version` `n + 1`.
const MIGRATIONS: &[&str] = &[SCHEMA_V1];

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
