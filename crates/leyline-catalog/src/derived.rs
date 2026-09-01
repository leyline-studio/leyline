//! Where a derived asset came from (`docs/catalog.md` §9, ADR 0107 §5).
//!
//! A pixel extension produces a new asset rather than a stage
//! ([ADR 0102](../../../docs/adr/0102-paid-extensions-and-the-pixel-boundary.md)),
//! so a denoised frame is an ordinary photograph in the library, indexed,
//! developed and exported like any other. `derived_from` is the one thing
//! that remembers which photograph it was made from.
//!
//! Deliberately unlike `companion_of` ([`crate::pairs`]) in both directions:
//! a derived asset is **not** hidden from the grid — it is a picture the
//! user will look at and choose between — and it **outlives** its parent,
//! because deleting the original is not a reason to lose the frame that was
//! kept instead of it.

use leyline_core::{AssetId, LeylineError, Result};

use crate::{Catalog, db_err};

impl Catalog {
    /// Records that `asset` was derived from `parent`.
    ///
    /// Refuses an asset that is its own parent — a one-row cycle is the only
    /// one this column can make, since a chain of derivations is a chain of
    /// distinct rows.
    pub fn set_derived_from(&mut self, asset: AssetId, parent: AssetId) -> Result<()> {
        self.ensure_writable()?;
        if asset == parent {
            return Err(LeylineError::InvalidSettings(
                "an asset cannot be derived from itself".to_owned(),
            ));
        }
        let changed = self
            .conn
            .execute(
                "UPDATE assets SET derived_from = ?1 WHERE id = ?2",
                rusqlite::params![parent.get(), asset.get()],
            )
            .map_err(db_err)?;
        if changed == 0 {
            return Err(LeylineError::AssetMissing(asset));
        }
        Ok(())
    }

    /// The asset `asset` was derived from, when it is a derived one.
    pub fn derived_from(&self, asset: AssetId) -> Result<Option<AssetId>> {
        let parent: Option<i64> = self
            .conn
            .query_row(
                "SELECT derived_from FROM assets WHERE id = ?1",
                [asset.get()],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => LeylineError::AssetMissing(asset),
                other => db_err(other),
            })?;
        Ok(parent.map(AssetId::new))
    }

    /// Every asset derived from `asset`, oldest first. Empty for the
    /// photograph nobody has run a processor on, which is most of them.
    pub fn derivatives_of(&self, asset: AssetId) -> Result<Vec<AssetId>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id FROM assets WHERE derived_from = ?1 ORDER BY id")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([asset.get()], |row| row.get::<_, i64>(0))
            .map_err(db_err)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(AssetId::new(row.map_err(db_err)?));
        }
        Ok(out)
    }
}
