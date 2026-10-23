-- A library as Leyline 0.1.0 left it: schema v13, the version tagged
-- `v0.1.0` (2026-09-23). The schema is dumped verbatim from a catalog that
-- release created; the rows are invented, so nothing private is committed.
--
-- Never regenerate this from the current schema: its whole value is being
-- the *old* one. Used by `tests/upgrade.rs`.

CREATE TABLE library (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
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
    is_missing INTEGER NOT NULL DEFAULT 0, companion_of INTEGER
    REFERENCES assets(id) ON DELETE CASCADE, derived_from INTEGER
    REFERENCES assets(id) ON DELETE SET NULL,
    UNIQUE(folder_id, filename),
    FOREIGN KEY(folder_id)
        REFERENCES folders(id)
        ON DELETE RESTRICT
);
CREATE TABLE cameras (
    id INTEGER PRIMARY KEY,
    manufacturer TEXT NOT NULL,
    model TEXT NOT NULL,
    UNIQUE(manufacturer, model)
);
CREATE TABLE lenses (
    id INTEGER PRIMARY KEY,
    manufacturer TEXT NOT NULL,
    model TEXT NOT NULL,
    mount TEXT,
    UNIQUE(manufacturer, model)
);
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
CREATE TABLE develop_revisions (
    id INTEGER PRIMARY KEY,
    asset_id INTEGER NOT NULL,
    parent_revision_id INTEGER,
    settings_json TEXT NOT NULL,
    author TEXT,
    created_at INTEGER NOT NULL, from_preset_id INTEGER
    REFERENCES develop_presets(id) ON DELETE SET NULL, from_preset_revision INTEGER,
    FOREIGN KEY(asset_id)
        REFERENCES assets(id)
        ON DELETE CASCADE,
    FOREIGN KEY(parent_revision_id)
        REFERENCES develop_revisions(id)
        ON DELETE RESTRICT
);
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
CREATE TABLE previews (
    id INTEGER PRIMARY KEY,
    asset_id INTEGER NOT NULL,
    revision_id INTEGER NOT NULL,
    kind INTEGER NOT NULL,
    width INTEGER NOT NULL,
    height INTEGER NOT NULL,
    relative_path TEXT NOT NULL,
    generated_at INTEGER NOT NULL, origin INTEGER NOT NULL DEFAULT 0,
    UNIQUE(asset_id, revision_id, kind),
    FOREIGN KEY(asset_id)
        REFERENCES assets(id)
        ON DELETE CASCADE,
    FOREIGN KEY(revision_id)
        REFERENCES develop_revisions(id)
        ON DELETE CASCADE
);
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
CREATE TABLE export_presets (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    settings_json TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE print_presets (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    settings_json TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE develop_presets (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    preset_json TEXT NOT NULL,
    created_at INTEGER NOT NULL
, folder_id INTEGER
    REFERENCES preset_folders(id) ON DELETE SET NULL, favourite INTEGER NOT NULL DEFAULT 0, preset_revision INTEGER NOT NULL DEFAULT 1, updated_at INTEGER);
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
CREATE TABLE preset_folders (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE roots (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE "folders" (
    id INTEGER PRIMARY KEY,
    parent_id INTEGER NULL,
    root_id INTEGER NOT NULL DEFAULT 1,
    relative_path TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE(root_id, relative_path),
    FOREIGN KEY(parent_id)
        REFERENCES folders(id)
        ON DELETE RESTRICT,
    FOREIGN KEY(root_id)
        REFERENCES roots(id)
        ON DELETE RESTRICT
);
CREATE TABLE asset_descriptions (
    asset_id INTEGER PRIMARY KEY,
    title TEXT,
    caption TEXT,
    creator TEXT,
    copyright TEXT,
    credit TEXT,
    city TEXT,
    state TEXT,
    country TEXT,

    FOREIGN KEY(asset_id)
        REFERENCES assets(id)
        ON DELETE CASCADE
);
CREATE TABLE contact_sheet_presets (
    id INTEGER PRIMARY KEY,
    uuid TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    settings_json TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
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
CREATE INDEX idx_presets_folder ON develop_presets(folder_id);
CREATE INDEX idx_develop_from_preset ON develop_revisions(from_preset_id);
CREATE INDEX idx_assets_companion ON assets(companion_of);
CREATE INDEX idx_assets_checksum ON assets(checksum);
CREATE INDEX idx_assets_grid ON assets(companion_of, capture_date, id);
CREATE INDEX idx_develop_current_version ON develop_current(version_id);
CREATE INDEX idx_export_history_asset ON export_history(asset_id);
CREATE INDEX idx_previews_revision ON previews(revision_id);
CREATE INDEX idx_collection_versions_version ON collection_versions(version_id);
CREATE INDEX idx_assets_capture_wall
ON assets(capture_date + COALESCE(capture_offset_minutes, 0) * 60000);
CREATE INDEX idx_folders_root ON folders(root_id);
CREATE INDEX idx_assets_derived ON assets(derived_from);
CREATE VIRTUAL TABLE search_index USING fts5(
    asset_id UNINDEXED,
    filename,
    keywords,
    artist,
    copyright,
    title,
    caption,
    camera,
    lens,
    captured,
    tokenize = "unicode61 remove_diacritics 2"
);

INSERT INTO roots VALUES (1, '00000000-0000-4000-8000-000000000002', 'Upgrade fixture', 1727000000);
INSERT INTO library VALUES (1, '00000000-0000-4000-8000-000000000001', 'Upgrade fixture', 1727000000, 1727000000);
INSERT INTO folders (id, parent_id, root_id, relative_path, created_at) VALUES (1, NULL, 1, '2026/09', 1727000000);
INSERT INTO assets (id, uuid, folder_id, filename, extension, media_type, file_size, checksum, width, height,
                    imported_at, modified_at, is_missing, companion_of, derived_from)
VALUES (1, '00000000-0000-4000-8000-000000000011', 1, 'IMG_0001.CR2', 'cr2', 0, 25000000,
        x'0101010101010101010101010101010101010101010101010101010101010101', 6000, 4000,
        1727000100, 1727000100, 0, NULL, NULL),
       (2, '00000000-0000-4000-8000-000000000012', 1, 'IMG_0001.JPG', 'jpg', 1, 8000000,
        x'0202020202020202020202020202020202020202020202020202020202020202', 6000, 4000,
        1727000100, 1727000100, 0, 1, NULL);
INSERT INTO develop_revisions (id, asset_id, parent_revision_id, settings_json, author, created_at)
VALUES (1, 1, NULL, '{}', NULL, 1727000200),
       (2, 1, 1, '{}', NULL, 1727000300),
       (3, 2, NULL, '{}', NULL, 1727000200);
INSERT INTO develop_versions (id, uuid, asset_id, name, head_revision_id, rating, color_label, pick_state, created_at)
VALUES (1, '00000000-0000-4000-8000-000000000021', 1, 'Master', 2, 4, NULL, 0, 1727000200),
       (2, '00000000-0000-4000-8000-000000000022', 1, 'Copy', 1, NULL, NULL, 0, 1727000250),
       (3, '00000000-0000-4000-8000-000000000023', 2, 'Master', 3, NULL, NULL, 0, 1727000200);
INSERT INTO develop_current VALUES (1, 1), (2, 3);
INSERT INTO collections (id, uuid, parent_collection_id, name, description, collection_type, rules_json, created_at)
VALUES (1, '00000000-0000-4000-8000-000000000031', NULL, 'Picks', NULL, 0, NULL, 1727000400);
INSERT INTO collection_versions VALUES (1, 1, 0);
INSERT INTO export_history (id, asset_id, preset_id, format, destination, exported_at)
VALUES (1, 1, NULL, 'jpeg', '/exports/IMG_0001.jpg', 1727000500);
INSERT INTO search_index (asset_id, filename) VALUES (1, 'IMG_0001.CR2'), (2, 'IMG_0001.JPG');

PRAGMA user_version = 13;
