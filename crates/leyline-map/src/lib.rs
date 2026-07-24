//! Offline MBTiles reader (`docs/adr/0040-gps-map-view.md`).
//!
//! MBTiles is a single-file SQLite container for pre-rendered map tiles
//! (<https://github.com/mapbox/mbtiles-spec>). This crate only reads: it
//! opens a `.mbtiles` file the user supplies, serves one tile's raw bytes
//! by `(zoom, x, y)` in XYZ tile-coordinate convention (origin top-left,
//! the one OSM/Leaflet/MapLibre all use), and reads the pack's declared
//! coverage (bounds, zoom range, attribution). It knows nothing about the
//! catalog, GPS points, or how tiles get composed into a viewport image —
//! that composition is `leyline-engine`'s job; this crate is purely a
//! tile-bytes-in, tile-bytes-out data access layer, the same shape as
//! `leyline-catalog` for the SQLite catalog or `leyline-preview` for the
//! thumbnail cache.

use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension};

/// What can go wrong reading a tile pack.
///
/// Deliberately just one variant: `TilePack` never touches the filesystem
/// directly (SQLite does, inside `Connection::open_with_flags`), so every
/// failure — a missing file, a file that isn't a database, a database
/// that isn't a valid MBTiles archive — surfaces as a `rusqlite::Error`
/// and lands here.
#[derive(Debug, thiserror::Error)]
pub enum MapError {
    /// An underlying database operation failed, or the file isn't a valid
    /// MBTiles archive (missing `tiles`/`metadata` tables).
    #[error("database error: {0}")]
    Db(String),
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, MapError>;

/// A tile pack's declared coverage, read from its `metadata` table
/// (MBTiles spec §Metadata) — every key is optional, packs vary in how
/// much they declare.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TilePackInfo {
    /// Human-readable pack name.
    pub name: Option<String>,
    /// Attribution text to display alongside the map (MBTiles spec, and
    /// legally required for OpenStreetMap-derived data — ADR 0040).
    pub attribution: Option<String>,
    /// Lowest zoom level the pack provides tiles for.
    pub min_zoom: Option<u8>,
    /// Highest zoom level the pack provides tiles for.
    pub max_zoom: Option<u8>,
    /// Coverage as `(min_lon, min_lat, max_lon, max_lat)`, decimal degrees.
    pub bounds: Option<(f64, f64, f64, f64)>,
    /// Tile image format (`png`, `jpg`, `webp`, `pbf`...), when declared.
    pub format: Option<String>,
}

/// A live handle on one `.mbtiles` file, opened read-only.
#[derive(Debug)]
pub struct TilePack {
    conn: Connection,
}

impl TilePack {
    /// Opens an existing MBTiles archive read-only. Leyline never writes to
    /// a tile pack — it's a file the user supplies (ADR 0040), not
    /// something this crate manages the lifecycle of.
    pub fn open(path: &Path) -> Result<TilePack> {
        let conn =
            Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(db_err)?;
        Ok(TilePack { conn })
    }

    /// Raw tile bytes (PNG/JPEG/WebP — whatever the pack stores, see
    /// [`TilePackInfo::format`]) for one tile in XYZ convention, or `None`
    /// outside the pack's coverage.
    ///
    /// MBTiles itself stores rows TMS-style (origin bottom-left, spec
    /// §Content) while every consumer of tile coordinates in this codebase
    /// — and every other map library — uses XYZ (origin top-left): the
    /// flip happens here so nothing outside this crate has to know MBTiles
    /// stores rows upside down from everyone else's convention.
    pub fn tile(&self, zoom: u8, x: u32, y: u32) -> Result<Option<Vec<u8>>> {
        // `1u32 << zoom` is only meaningful for zoom < 32 (the shift
        // overflows the width otherwise — panics in debug, wraps in
        // release); no real tile source goes anywhere near that deep
        // (typical max is ~19-24), so treat it the same as "outside the
        // pack's coverage" rather than as a caller error.
        if zoom >= 32 {
            return Ok(None);
        }
        let side = 1u32 << zoom;
        if x >= side || y >= side {
            return Ok(None);
        }
        let tms_row = tms_row(zoom, y);
        self.conn
            .query_row(
                "SELECT tile_data FROM tiles
                 WHERE zoom_level = ?1 AND tile_column = ?2 AND tile_row = ?3",
                rusqlite::params![zoom, x, tms_row],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_err)
    }

    /// The pack's declared coverage (MBTiles spec `metadata` table).
    pub fn info(&self) -> Result<TilePackInfo> {
        let mut stmt = self
            .conn
            .prepare("SELECT name, value FROM metadata")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(db_err)?;

        let mut info = TilePackInfo::default();
        for row in rows {
            let (key, value) = row.map_err(db_err)?;
            match key.as_str() {
                "name" => info.name = Some(value),
                "attribution" => info.attribution = Some(value),
                "format" => info.format = Some(value),
                "minzoom" => info.min_zoom = value.parse().ok(),
                "maxzoom" => info.max_zoom = value.parse().ok(),
                "bounds" => info.bounds = parse_bounds(&value),
                _ => {}
            }
        }
        Ok(info)
    }
}

/// Converts an XYZ row to MBTiles' TMS row for the same zoom level.
/// Callers must have already checked `xyz_row < 2^zoom` (`tile` does) — an
/// out-of-range row would underflow the subtraction.
fn tms_row(zoom: u8, xyz_row: u32) -> u32 {
    (1u32 << zoom) - 1 - xyz_row
}

/// Parses the MBTiles `bounds` metadata value: `"min_lon,min_lat,max_lon,max_lat"`.
fn parse_bounds(value: &str) -> Option<(f64, f64, f64, f64)> {
    let mut parts = value.split(',').map(str::trim).map(str::parse::<f64>);
    let min_lon = parts.next()?.ok()?;
    let min_lat = parts.next()?.ok()?;
    let max_lon = parts.next()?.ok()?;
    let max_lat = parts.next()?.ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((min_lon, min_lat, max_lon, max_lat))
}

fn db_err(error: rusqlite::Error) -> MapError {
    MapError::Db(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal but spec-compliant MBTiles archive: `metadata` + `tiles`,
    /// one tile at `z=1` written in TMS row order.
    fn sample_pack() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pack.mbtiles");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE metadata (name TEXT, value TEXT);
             CREATE TABLE tiles (
                 zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER,
                 tile_data BLOB
             );
             INSERT INTO metadata VALUES ('name', 'Sample');
             INSERT INTO metadata VALUES ('attribution', '© OpenStreetMap contributors');
             INSERT INTO metadata VALUES ('minzoom', '0');
             INSERT INTO metadata VALUES ('maxzoom', '3');
             INSERT INTO metadata VALUES ('format', 'png');
             INSERT INTO metadata VALUES ('bounds', '-1.5,-2.5,3.5,4.5');",
        )
        .unwrap();
        // XYZ (z=1, x=0, y=0) is the northwest quadrant, TMS row = 2^1-1-0 = 1.
        conn.execute(
            "INSERT INTO tiles (zoom_level, tile_column, tile_row, tile_data)
             VALUES (1, 0, 1, ?1)",
            rusqlite::params![vec![1u8, 2, 3]],
        )
        .unwrap();
        drop(conn);
        (dir, path)
    }

    #[test]
    fn a_tile_written_at_its_tms_row_is_read_back_at_its_xyz_row() {
        let (_dir, path) = sample_pack();
        let pack = TilePack::open(&path).unwrap();
        assert_eq!(pack.tile(1, 0, 0).unwrap(), Some(vec![1, 2, 3]));
    }

    #[test]
    fn a_tile_beyond_the_zoom_levels_grid_is_none_not_a_panic() {
        // z=1 only has coordinates 0..2; well past that must not underflow
        // the TMS row subtraction.
        let (_dir, path) = sample_pack();
        let pack = TilePack::open(&path).unwrap();
        assert_eq!(pack.tile(1, 5, 5).unwrap(), None);
    }

    #[test]
    fn a_zoom_of_32_or_more_is_none_not_a_shift_overflow_panic() {
        // `1u32 << zoom` is only defined for zoom < 32; a naive caller (or
        // hostile input crossing the `Library::map_tile` boundary) passing
        // 32+ must get `None`, not a panic.
        let (_dir, path) = sample_pack();
        let pack = TilePack::open(&path).unwrap();
        assert_eq!(pack.tile(32, 0, 0).unwrap(), None);
        assert_eq!(pack.tile(255, 0, 0).unwrap(), None);
    }

    #[test]
    fn a_tile_in_range_but_absent_from_the_table_is_none() {
        let (_dir, path) = sample_pack();
        let pack = TilePack::open(&path).unwrap();
        assert_eq!(pack.tile(1, 1, 1).unwrap(), None);
    }

    #[test]
    fn info_reads_every_declared_metadata_key() {
        let (_dir, path) = sample_pack();
        let pack = TilePack::open(&path).unwrap();
        let info = pack.info().unwrap();
        assert_eq!(info.name.as_deref(), Some("Sample"));
        assert_eq!(
            info.attribution.as_deref(),
            Some("© OpenStreetMap contributors")
        );
        assert_eq!(info.min_zoom, Some(0));
        assert_eq!(info.max_zoom, Some(3));
        assert_eq!(info.format.as_deref(), Some("png"));
        assert_eq!(info.bounds, Some((-1.5, -2.5, 3.5, 4.5)));
    }

    #[test]
    fn missing_metadata_keys_stay_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bare.mbtiles");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE metadata (name TEXT, value TEXT);
             CREATE TABLE tiles (
                 zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER,
                 tile_data BLOB
             );",
        )
        .unwrap();
        drop(conn);

        let pack = TilePack::open(&path).unwrap();
        assert_eq!(pack.info().unwrap(), TilePackInfo::default());
    }

    #[test]
    fn tms_row_flip_is_its_own_inverse() {
        for y in 0..8u32 {
            assert_eq!(tms_row(3, tms_row(3, y)), y);
        }
    }
}
