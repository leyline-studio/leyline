//! The offline map: installing a map pack, tiles and photo pins.

use super::*;

impl Library {
    /// The world basemap compiled into the binary (ADR 0059), when this
    /// build carries one. Natural Earth I, zoom 0–5, JPEG tiles: enough to
    /// place GPS pins on continents and coastlines from the first launch,
    /// never a substitute for a pack the user imports.
    #[cfg(feature = "bundled-basemap")]
    const BUNDLED_BASEMAP: &'static [u8] =
        include_bytes!("../../../../assets/basemap/world-z0-5.mbtiles");

    /// The library-relative path of the active map pack file (ADR 0040) —
    /// a convention, not a catalog row: presence of the file is what makes
    /// a pack "active", one at a time in V1.
    pub(super) fn map_pack_path(&self) -> PathBuf {
        self.inner.root.join("Map").join("pack.mbtiles")
    }

    /// Imports `source` as the library's active map pack (ADR 0040): copies
    /// it to the conventional `Map/pack.mbtiles` location, overwriting
    /// whatever pack (if any) was active before. Refused on a read-only
    /// handle (`Library::open_read_only`) — same contract as every catalog
    /// write, even though a map pack is filesystem state, not a catalog
    /// row.
    pub fn import_map_pack(&self, source: &Path) -> Result<()> {
        if self.catalog().is_read_only() {
            return Err(LeylineError::Db(
                "library opened read-only; writes are refused".to_owned(),
            ));
        }
        let destination = self.map_pack_path();
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Invalidate the cached handle *before* touching the file: with the
        // old order, a pack already opened once (any prior `map_tile`/
        // `map_pack_info` call) still had a live SQLite connection to
        // `destination` while it got overwritten out from under it — risky
        // on every platform if the copy is interrupted partway, outright
        // liable to fail on Windows where replacing an open file needs
        // share-delete cooperation SQLite doesn't request.
        lock(&self.inner.map_pack).take();
        // Copy to a sibling temp file first, then move it into place: a
        // reader that opens the pack mid-copy must never see a half-written
        // file. The temp name carries the process id so two Leyline
        // instances importing at once cannot collide on it, and so a
        // crashed import leaves an obviously-stale file rather than one the
        // next import might mistake for its own.
        let temp = destination.with_extension(format!("mbtiles.{}.tmp", std::process::id()));
        std::fs::copy(source, &temp)?;
        // `std::fs::rename` replaces the destination on POSIX but *fails*
        // on Windows when it already exists, so a re-import has to unlink
        // first. That leaves a brief window with no active pack; it is the
        // narrowest one available without platform-specific APIs, and the
        // cached handle is already dropped by this point either way.
        if destination.exists() {
            std::fs::remove_file(&destination)?;
        }
        if let Err(error) = std::fs::rename(&temp, &destination) {
            // Don't leave the copy behind to be mistaken for a pack.
            let _ = std::fs::remove_file(&temp);
            return Err(error.into());
        }
        Ok(())
    }

    /// Opens (or returns the cached handle to) the active map pack: the one
    /// the user imported if there is one, the embedded world basemap
    /// otherwise (ADR 0059), and `None` when this build embeds none.
    ///
    /// An imported pack always wins. The two are never composed into one
    /// view — Leyline serves one pack, it does not blend a fine regional
    /// pack over a coarse world one.
    pub(super) fn with_map_pack<T>(
        &self,
        f: impl FnOnce(&leyline_map::TilePack) -> T,
    ) -> Result<Option<T>> {
        let mut slot = lock(&self.inner.map_pack);
        if slot.is_none() {
            let path = self.map_pack_path();
            let pack = if path.is_file() {
                leyline_map::TilePack::open(&path).map_err(map_err)?
            } else {
                #[cfg(feature = "bundled-basemap")]
                {
                    leyline_map::TilePack::from_static(Self::BUNDLED_BASEMAP).map_err(map_err)?
                }
                #[cfg(not(feature = "bundled-basemap"))]
                {
                    return Ok(None);
                }
            };
            *slot = Some(pack);
        }
        Ok(slot.as_ref().map(f))
    }

    /// The active map pack's declared coverage (ADR 0040), or `None` if
    /// none has been imported yet.
    pub fn map_pack_info(&self) -> Result<Option<leyline_map::TilePackInfo>> {
        self.with_map_pack(|pack| pack.info().map_err(map_err))?
            .transpose()
    }

    /// One tile's raw bytes (whatever image format the pack stores), or
    /// `None` when no pack is imported or the tile is outside its
    /// coverage.
    pub fn map_tile(&self, zoom: u8, x: u32, y: u32) -> Result<Option<Vec<u8>>> {
        Ok(self
            .with_map_pack(|pack| pack.tile(zoom, x, y).map_err(map_err))?
            .transpose()?
            .flatten())
    }

    /// Every current version with recorded GPS coordinates (ADR 0040).
    pub fn map_pins(&self) -> Result<Vec<leyline_catalog::MapPin>> {
        self.catalog().map_pins()
    }
}
