//! GPS map pins (`docs/adr/0040-gps-map-view.md`).
//!
//! One pin per asset with recorded GPS coordinates — the asset's *current*
//! version, same scoping as the grid's uncollected view (`grid.rs`). No
//! filtering beyond "has coordinates": the map view is a separate surface
//! from the grid, V1 keeps it simple (`docs/adr/0040` alternatives).

use leyline_core::{AssetId, Result, VersionId};

use crate::{Catalog, db_err};

/// One asset placed on the map.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapPin {
    /// The current version this pin represents (what a click navigates to).
    pub version_id: VersionId,
    /// The asset behind it.
    pub asset_id: AssetId,
    /// GPS latitude in decimal degrees.
    pub latitude: f64,
    /// GPS longitude in decimal degrees.
    pub longitude: f64,
}

impl Catalog {
    /// Every current version with recorded GPS coordinates.
    pub fn map_pins(&self) -> Result<Vec<MapPin>> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT v.id, a.id, m.gps_latitude, m.gps_longitude
                 FROM develop_current c
                 JOIN develop_versions v ON v.id = c.version_id
                 JOIN assets a ON a.id = c.asset_id
                 JOIN metadata m ON m.asset_id = a.id
                 WHERE m.gps_latitude IS NOT NULL AND m.gps_longitude IS NOT NULL",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(MapPin {
                    version_id: VersionId::new(row.get(0)?),
                    asset_id: AssetId::new(row.get(1)?),
                    latitude: row.get(2)?,
                    longitude: row.get(3)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_err)
    }
}
