//! EXIF metadata, cameras and lenses (`docs/catalog.md` §13, §14, §15).
//!
//! Metadata lives in its own table to keep `assets` compact. Shutter,
//! aperture and focal length are stored as the exact rationals EXIF
//! provides; SQLite's generated columns derive the indexable decimal values
//! for range searches, so the rationals stay the single reference.

use leyline_core::{AssetId, LeylineError, Result};
use rusqlite::types::Value as SqlValue;

use crate::{Catalog, db_err};

/// An exact EXIF rational (`1/3200`, `56/10`, `70/1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rational {
    /// Numerator.
    pub numerator: i64,
    /// Denominator, strictly positive.
    pub denominator: i64,
}

impl Rational {
    /// The decimal value, as the engine converts it when needed.
    pub fn as_f64(self) -> f64 {
        self.numerator as f64 / self.denominator as f64
    }
}

/// A camera body, deduplicated by `(manufacturer, model)` (§14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraInfo {
    /// Maker name as reported by EXIF.
    pub manufacturer: String,
    /// Model name as reported by EXIF.
    pub model: String,
}

/// A lens, deduplicated by `(manufacturer, model)` (§15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LensInfo {
    /// Maker name as reported by EXIF.
    pub manufacturer: String,
    /// Model name as reported by EXIF.
    pub model: String,
    /// Mount, when known.
    pub mount: Option<String>,
}

/// Complete EXIF metadata of one asset (§13). Every field optional: EXIF is
/// best-effort by nature.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Metadata {
    /// Camera body.
    pub camera: Option<CameraInfo>,
    /// Lens.
    pub lens: Option<LensInfo>,
    /// EXIF orientation tag (1-8).
    pub orientation: Option<u16>,
    /// ISO sensitivity.
    pub iso: Option<u32>,
    /// Shutter speed in seconds, as a rational (`1/3200`).
    pub shutter: Option<Rational>,
    /// Aperture f-number, as a rational (`56/10` = f/5.6).
    pub aperture: Option<Rational>,
    /// Focal length in millimeters, as a rational (`70/1`).
    pub focal_length: Option<Rational>,
    /// Exposure bias in EV.
    pub exposure_bias: Option<f64>,
    /// Whether the flash fired.
    pub flash: Option<bool>,
    /// EXIF white balance mode (0 auto, 1 manual).
    pub white_balance_mode: Option<u16>,
    /// Color space name (`sRGB`, `Adobe RGB`...).
    pub color_space: Option<String>,
    /// GPS latitude in decimal degrees, [-90, 90].
    pub gps_latitude: Option<f64>,
    /// GPS longitude in decimal degrees, [-180, 180].
    pub gps_longitude: Option<f64>,
    /// GPS altitude in meters.
    pub gps_altitude: Option<f64>,
    /// Artist tag.
    pub artist: Option<String>,
    /// Copyright tag.
    pub copyright: Option<String>,
}

impl Catalog {
    /// Records the metadata of an asset, replacing any previous row.
    ///
    /// Cameras and lenses are deduplicated through their `UNIQUE` pairs: two
    /// assets shot with the same body share one `cameras` row. Artist and
    /// copyright land in the search index (§30).
    pub fn set_metadata(&mut self, asset: AssetId, meta: &Metadata) -> Result<()> {
        self.ensure_writable()?;
        validate(meta)?;

        let tx = self.conn.transaction().map_err(db_err)?;
        tx.query_row("SELECT 1 FROM assets WHERE id = ?1", [asset.get()], |_| {
            Ok(())
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => LeylineError::AssetMissing(asset),
            other => db_err(other),
        })?;

        let camera_id = match &meta.camera {
            None => None,
            Some(camera) => Some(ensure_camera(&tx, camera)?),
        };
        let lens_id = match &meta.lens {
            None => None,
            Some(lens) => Some(ensure_lens(&tx, lens)?),
        };

        tx.execute(
            "INSERT OR REPLACE INTO metadata
                 (asset_id, camera_id, lens_id, orientation, iso,
                  shutter_numerator, shutter_denominator,
                  aperture_numerator, aperture_denominator,
                  focal_length_numerator, focal_length_denominator,
                  exposure_bias, flash, white_balance_mode, color_space,
                  gps_latitude, gps_longitude, gps_altitude, artist, copyright)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                     ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
            rusqlite::params![
                asset.get(),
                camera_id,
                lens_id,
                meta.orientation,
                meta.iso,
                meta.shutter.map(|r| r.numerator),
                meta.shutter.map(|r| r.denominator),
                meta.aperture.map(|r| r.numerator),
                meta.aperture.map(|r| r.denominator),
                meta.focal_length.map(|r| r.numerator),
                meta.focal_length.map(|r| r.denominator),
                meta.exposure_bias,
                meta.flash,
                meta.white_balance_mode,
                meta.color_space,
                meta.gps_latitude,
                meta.gps_longitude,
                meta.gps_altitude,
                meta.artist,
                meta.copyright,
            ],
        )
        .map_err(db_err)?;

        tx.execute(
            // An authored description wins over what the file says
            // (ADR 0099 §2): reading the file again must not put the EXIF
            // artist back over a creator someone typed.
            "UPDATE search_index SET
                 artist = COALESCE(
                     (SELECT creator FROM asset_descriptions WHERE asset_id = ?1), ?2),
                 copyright = COALESCE(
                     (SELECT copyright FROM asset_descriptions WHERE asset_id = ?1), ?3)
             WHERE asset_id = ?1",
            rusqlite::params![
                asset.get(),
                meta.artist.as_deref().unwrap_or(""),
                meta.copyright.as_deref().unwrap_or(""),
            ],
        )
        .map_err(db_err)?;
        // The body and the lens the file names, in the vocabulary the shot
        // filters already use (ADR 0144 §2).
        crate::search::refresh_asset_gear(&tx, asset)?;
        tx.commit().map_err(db_err)?;
        Ok(())
    }

    /// Reads the metadata of an asset; `None` when none was recorded yet.
    pub fn metadata(&self, asset: AssetId) -> Result<Option<Metadata>> {
        self.conn
            .query_row("SELECT 1 FROM assets WHERE id = ?1", [asset.get()], |_| {
                Ok(())
            })
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => LeylineError::AssetMissing(asset),
                other => db_err(other),
            })?;

        let found = self.conn.query_row(
            "SELECT c.manufacturer AS camera_manufacturer, c.model AS camera_model,
                    l.manufacturer AS lens_manufacturer, l.model AS lens_model, l.mount,
                    m.orientation, m.iso,
                    m.shutter_numerator, m.shutter_denominator,
                    m.aperture_numerator, m.aperture_denominator,
                    m.focal_length_numerator, m.focal_length_denominator,
                    m.exposure_bias, m.flash, m.white_balance_mode, m.color_space,
                    m.gps_latitude, m.gps_longitude, m.gps_altitude,
                    m.artist, m.copyright
             FROM metadata m
             LEFT JOIN cameras c ON c.id = m.camera_id
             LEFT JOIN lenses l ON l.id = m.lens_id
             WHERE m.asset_id = ?1",
            [asset.get()],
            |row| {
                let camera = match (
                    row.get::<_, Option<String>>("camera_manufacturer")?,
                    row.get("camera_model")?,
                ) {
                    (Some(manufacturer), Some(model)) => Some(CameraInfo {
                        manufacturer,
                        model,
                    }),
                    _ => None,
                };
                let lens = match (
                    row.get::<_, Option<String>>("lens_manufacturer")?,
                    row.get("lens_model")?,
                ) {
                    (Some(manufacturer), Some(model)) => Some(LensInfo {
                        manufacturer,
                        model,
                        mount: row.get("mount")?,
                    }),
                    _ => None,
                };
                Ok(Metadata {
                    camera,
                    lens,
                    orientation: row.get("orientation")?,
                    iso: row.get("iso")?,
                    shutter: rational(
                        row.get("shutter_numerator")?,
                        row.get("shutter_denominator")?,
                    ),
                    aperture: rational(
                        row.get("aperture_numerator")?,
                        row.get("aperture_denominator")?,
                    ),
                    focal_length: rational(
                        row.get("focal_length_numerator")?,
                        row.get("focal_length_denominator")?,
                    ),
                    exposure_bias: row.get("exposure_bias")?,
                    flash: row.get("flash")?,
                    white_balance_mode: row.get("white_balance_mode")?,
                    color_space: row.get("color_space")?,
                    gps_latitude: row.get("gps_latitude")?,
                    gps_longitude: row.get("gps_longitude")?,
                    gps_altitude: row.get("gps_altitude")?,
                    artist: row.get("artist")?,
                    copyright: row.get("copyright")?,
                })
            },
        );
        match found {
            Ok(meta) => Ok(Some(meta)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(db_err(e)),
        }
    }

    /// The shot values present in the library: the bodies and lenses actually
    /// used, and the observed bounds of the four continuous quantities
    /// (ADR 0064 §3).
    ///
    /// Computed over the **whole** library, not over the filtered selection
    /// in progress: a list that shrank as filters were added would cost a
    /// recount per keystroke and be harder to predict. Bodies and lenses come
    /// back in the exact form the camera and lens filters expect.
    pub fn shot_facets(&self) -> Result<ShotFacets> {
        let names = |table: &str, column: &str| -> Result<Vec<String>> {
            let mut stmt = self
                .conn
                .prepare(&format!(
                    "SELECT DISTINCT {FULL_NAME} AS name
                     FROM metadata m JOIN {table} t ON t.id = m.{column}
                     ORDER BY name COLLATE NOCASE"
                ))
                .map_err(db_err)?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(db_err)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(db_err)
        };

        let bounds = self
            .conn
            .query_row(
                "SELECT MIN(iso) AS iso_min, MAX(iso) AS iso_max,
                        MIN(aperture_f) AS aperture_min, MAX(aperture_f) AS aperture_max,
                        MIN(focal_length_mm) AS focal_min, MAX(focal_length_mm) AS focal_max,
                        MIN(shutter_speed_s) AS shutter_min, MAX(shutter_speed_s) AS shutter_max
                 FROM metadata",
                [],
                |row| {
                    let pair = |facet: &str| -> rusqlite::Result<Option<(f64, f64)>> {
                        Ok(
                            match (
                                row.get(format!("{facet}_min").as_str())?,
                                row.get(format!("{facet}_max").as_str())?,
                            ) {
                                (Some(min), Some(max)) => Some((min, max)),
                                _ => None,
                            },
                        )
                    };
                    Ok((
                        pair("iso")?,
                        pair("aperture")?,
                        pair("focal")?,
                        pair("shutter")?,
                    ))
                },
            )
            .map_err(db_err)?;

        Ok(ShotFacets {
            cameras: names("cameras", "camera_id")?,
            lenses: names("lenses", "lens_id")?,
            iso: bounds.0,
            aperture: bounds.1,
            focal_length: bounds.2,
            shutter_speed: bounds.3,
        })
    }
}

/// The values available to the shot filters (ADR 0064 §3): what the library
/// actually contains, so a filter list never offers a body nobody shot with.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShotFacets {
    /// Bodies used, as `manufacturer model` (or the model alone when the
    /// maker is unknown), sorted.
    pub cameras: Vec<String>,
    /// Lenses used, same form, sorted.
    pub lenses: Vec<String>,
    /// Lowest and highest ISO recorded, when any asset carries one.
    pub iso: Option<(f64, f64)>,
    /// Widest and narrowest aperture recorded, as f-numbers.
    pub aperture: Option<(f64, f64)>,
    /// Shortest and longest focal length recorded, in millimeters.
    pub focal_length: Option<(f64, f64)>,
    /// Fastest and slowest shutter speed recorded, in seconds.
    pub shutter_speed: Option<(f64, f64)>,
}

/// How a body or a lens is named once it leaves the catalog: the maker and
/// the model, or the model alone when the maker is blank.
const FULL_NAME: &str = "CASE WHEN TRIM(t.manufacturer) = '' THEN TRIM(t.model)
                             ELSE TRIM(t.manufacturer) || ' ' || TRIM(t.model) END";

/// Appends the clause matching an asset's body against `camera`, either the
/// model alone or `manufacturer model`.
///
/// This is the **single** definition of that correspondence (ADR 0064 §2):
/// smart collections and the grid's camera filter both call it, so the same
/// question cannot get two answers.
pub(crate) fn camera_clause(sql: &mut String, params: &mut Vec<SqlValue>, camera: &str) {
    name_clause(sql, params, "cameras", "camera_id", camera);
}

/// The same, for the lens: a lens is chosen from a list built by
/// [`Catalog::shot_facets`], and matched exactly as a body is.
pub(crate) fn lens_clause(sql: &mut String, params: &mut Vec<SqlValue>, lens: &str) {
    name_clause(sql, params, "lenses", "lens_id", lens);
}

/// Shared body of the two clauses above. An asset with no metadata row, or
/// none for this column, matches nothing — a filtered grid that quietly kept
/// the undocumented photos would make every filter a lie (ADR 0064 §1).
fn name_clause(
    sql: &mut String,
    params: &mut Vec<SqlValue>,
    table: &str,
    column: &str,
    value: &str,
) {
    sql.push_str(&format!(
        " AND EXISTS (SELECT 1 FROM metadata m JOIN {table} t ON t.id = m.{column}
                      WHERE m.asset_id = a.id AND (TRIM(t.model) = ? OR {FULL_NAME} = ?))"
    ));
    params.push(SqlValue::Text(value.to_owned()));
    params.push(SqlValue::Text(value.to_owned()));
}

/// Builds a rational from two optional columns.
fn rational(numerator: Option<i64>, denominator: Option<i64>) -> Option<Rational> {
    match (numerator, denominator) {
        (Some(numerator), Some(denominator)) => Some(Rational {
            numerator,
            denominator,
        }),
        _ => None,
    }
}

/// Returns the deduplicated `cameras` row for the pair.
fn ensure_camera(tx: &rusqlite::Transaction<'_>, camera: &CameraInfo) -> Result<i64> {
    tx.execute(
        "INSERT OR IGNORE INTO cameras (manufacturer, model) VALUES (?1, ?2)",
        rusqlite::params![camera.manufacturer, camera.model],
    )
    .map_err(db_err)?;
    tx.query_row(
        "SELECT id FROM cameras WHERE manufacturer = ?1 AND model = ?2",
        rusqlite::params![camera.manufacturer, camera.model],
        |row| row.get(0),
    )
    .map_err(db_err)
}

/// Returns the deduplicated `lenses` row for the pair, keeping the first
/// non-NULL mount seen.
fn ensure_lens(tx: &rusqlite::Transaction<'_>, lens: &LensInfo) -> Result<i64> {
    tx.execute(
        "INSERT INTO lenses (manufacturer, model, mount) VALUES (?1, ?2, ?3)
         ON CONFLICT(manufacturer, model) DO UPDATE SET
             mount = COALESCE(lenses.mount, excluded.mount)",
        rusqlite::params![lens.manufacturer, lens.model, lens.mount],
    )
    .map_err(db_err)?;
    tx.query_row(
        "SELECT id FROM lenses WHERE manufacturer = ?1 AND model = ?2",
        rusqlite::params![lens.manufacturer, lens.model],
        |row| row.get(0),
    )
    .map_err(db_err)
}

/// Rejects rationals with non-positive denominators and out-of-range GPS.
fn validate(meta: &Metadata) -> Result<()> {
    let invalid = |message: String| {
        LeylineError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            message,
        ))
    };
    for (name, value) in [
        ("shutter", meta.shutter),
        ("aperture", meta.aperture),
        ("focal_length", meta.focal_length),
    ] {
        if let Some(r) = value
            && r.denominator <= 0
        {
            return Err(invalid(format!(
                "{name} denominator must be strictly positive, got {}",
                r.denominator
            )));
        }
    }
    if let Some(lat) = meta.gps_latitude
        && !(-90.0..=90.0).contains(&lat)
    {
        return Err(invalid(format!("gps_latitude out of range: {lat}")));
    }
    if let Some(lon) = meta.gps_longitude
        && !(-180.0..=180.0).contains(&lon)
    {
        return Err(invalid(format!("gps_longitude out of range: {lon}")));
    }
    Ok(())
}
