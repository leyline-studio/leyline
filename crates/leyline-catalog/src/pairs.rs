//! RAW+JPEG pairing (`docs/catalog.md` §9, ADR 0079).
//!
//! A camera set to RAW+JPEG writes two files per shot, and both are photos
//! as far as the file system is concerned. The catalog records which one is
//! the rendering of which: the RAW is the master, the other file carries
//! `companion_of` pointing at it, and the grid shows masters only.
//!
//! The criterion is the conjunction of ADR 0079 §2 — same filename stem,
//! same capture instant, same body — at any depth of the library. Each of
//! the three taken alone produces false pairs on a real corpus: names repeat
//! across bodies, and a burst puts several frames in one second.

use leyline_core::{AssetId, Result};
use rusqlite::OptionalExtension;

use crate::{Catalog, db_err};

/// What [`Catalog::pair_asset`] did with one asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pairing {
    /// The asset became the companion of this master.
    Companion(AssetId),
    /// The asset is a master, and adopted these already-imported companions.
    Master(Vec<AssetId>),
    /// Nothing in the library matches it.
    None,
}

/// The pair criterion of ADR 0079 §2, as a join.
///
/// `m` is the master candidate (a RAW), `c` the companion candidate (any
/// other image). Both must be unengaged: a companion never becomes a master
/// in turn, so no chains can form. `IS` rather than `=` on `camera_id` so
/// that two files whose body is equally unknown still match — `=` would be
/// false for two NULLs and would silently refuse the pair.
const PAIR_JOIN: &str = "
    FROM assets m
    JOIN assets c
      ON c.id <> m.id
     AND c.capture_date = m.capture_date
     AND lower(substr(c.filename, 1, length(c.filename) - length(c.extension) - 1))
       = lower(substr(m.filename, 1, length(m.filename) - length(m.extension) - 1))
    LEFT JOIN metadata mm ON mm.asset_id = m.id
    LEFT JOIN metadata mc ON mc.asset_id = c.id
    WHERE m.media_type IN (0, 4)
      AND c.media_type NOT IN (0, 4)
      AND m.capture_date IS NOT NULL
      AND m.companion_of IS NULL
      AND c.companion_of IS NULL
      AND mm.camera_id IS mc.camera_id
      AND NOT EXISTS (SELECT 1 FROM assets x WHERE x.companion_of = c.id)
";

impl Catalog {
    /// Pairs one freshly imported asset with what the library already holds,
    /// in both directions (ADR 0079 §4).
    ///
    /// Both directions are needed, and the second is the common one: an
    /// import enumerates by sorted path, so `Photos/5D4_2326.JPG` is
    /// registered before `Photos/raw/5D4_2326.CR2` and it is the RAW that
    /// finds its companion, not the other way round.
    pub fn pair_asset(&mut self, asset: AssetId) -> Result<Pairing> {
        self.ensure_writable()?;

        // As a companion: exactly one master can claim it, and the lowest
        // asset id wins if a library somehow holds two (a CR2 and a DNG of
        // the same shot) — an arbitrary but stable choice, and `pair_all`
        // resolves the same collision the same way.
        let master: Option<i64> = self
            .conn
            .query_row(
                &format!("SELECT m.id {PAIR_JOIN} AND c.id = ?1 ORDER BY m.id LIMIT 1"),
                [asset.get()],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_err)?;
        if let Some(master) = master {
            let master = AssetId::new(master);
            self.attach(master, asset)?;
            return Ok(Pairing::Companion(master));
        }

        // As a master: every unengaged non-RAW of the same shot attaches,
        // so a body writing RAW+JPEG+HEIF yields one photo and not three.
        let companions: Vec<i64> = self
            .conn
            .prepare(&format!(
                "SELECT c.id {PAIR_JOIN} AND m.id = ?1 ORDER BY c.id"
            ))
            .map_err(db_err)?
            .query_map([asset.get()], |row| row.get(0))
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)?;
        if companions.is_empty() {
            return Ok(Pairing::None);
        }
        let companions: Vec<AssetId> = companions.into_iter().map(AssetId::new).collect();
        for companion in &companions {
            self.attach(asset, *companion)?;
        }
        Ok(Pairing::Master(companions))
    }

    /// Pairs everything pairable in the library, and reports each pair as
    /// `(master, companion)` (ADR 0079 §7).
    ///
    /// This is the retroactive pass: the v3 migration adds the column
    /// without pairing anything, so a library imported before ADR 0079 keeps
    /// showing every file until this is asked for explicitly.
    pub fn pair_all(&mut self) -> Result<Vec<(AssetId, AssetId)>> {
        self.ensure_writable()?;

        let rows: Vec<(i64, i64)> = self
            .conn
            .prepare(&format!(
                "SELECT m.id, c.id {PAIR_JOIN} ORDER BY m.id, c.id"
            ))
            .map_err(db_err)?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)?;

        // One candidate row per (master, companion) couple: a companion
        // claimed by two masters appears twice, and only the first claim
        // may be applied. The rows are ordered, so "first" is stable.
        let mut paired = Vec::new();
        let mut taken = std::collections::HashSet::new();
        for (master, companion) in rows {
            if !taken.insert(companion) {
                continue;
            }
            let (master, companion) = (AssetId::new(master), AssetId::new(companion));
            self.attach(master, companion)?;
            paired.push((master, companion));
        }
        Ok(paired)
    }

    /// How many companions `pair_all` would attach, without attaching any.
    ///
    /// `DISTINCT` on the companion and not on the couple: a file claimed by
    /// two masters is one photo leaving the grid, not two, and the count has
    /// to agree with what the pass then does.
    pub fn pairable_count(&self) -> Result<u64> {
        self.conn
            .query_row(
                &format!("SELECT COUNT(DISTINCT c.id) {PAIR_JOIN}"),
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|n| n as u64)
            .map_err(db_err)
    }

    /// Detaches `assets`, whether each is a master or a companion, and
    /// returns how many rows stopped being companions (ADR 0079 §6).
    ///
    /// Nothing is lost: a detached companion returns to the grid with the
    /// rating, keywords and revisions it always had.
    pub fn unpair_assets(&mut self, assets: &[AssetId]) -> Result<u32> {
        self.ensure_writable()?;
        let mut detached = 0;
        let tx = self.conn.transaction().map_err(db_err)?;
        for asset in assets {
            detached += tx
                .execute(
                    // `companion_of IS NOT NULL` is not redundant with the
                    // two branches: without it, detaching a master counts
                    // its own already-null row and reports one photo more
                    // than came back to the grid.
                    "UPDATE assets SET companion_of = NULL
                     WHERE (id = ?1 OR companion_of = ?1)
                       AND companion_of IS NOT NULL",
                    [asset.get()],
                )
                .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        Ok(u32::try_from(detached).unwrap_or(u32::MAX))
    }

    /// Writes one pair. Private: the criterion of §2 is what may create a
    /// pair, never a caller's choice of two ids.
    fn attach(&mut self, master: AssetId, companion: AssetId) -> Result<()> {
        self.conn
            .execute(
                "UPDATE assets SET companion_of = ?1 WHERE id = ?2",
                [master.get(), companion.get()],
            )
            .map_err(db_err)?;
        Ok(())
    }

    /// The companions attached to `asset`, empty for the ordinary photo.
    pub fn companions_of(&self, asset: AssetId) -> Result<Vec<AssetId>> {
        let ids: Vec<i64> = self
            .conn
            .prepare_cached("SELECT id FROM assets WHERE companion_of = ?1 ORDER BY id")
            .map_err(db_err)?
            .query_map([asset.get()], |row| row.get(0))
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)?;
        Ok(ids.into_iter().map(AssetId::new).collect())
    }

    /// The master `asset` is a companion of, `None` when it is its own photo.
    pub fn master_of(&self, asset: AssetId) -> Result<Option<AssetId>> {
        let master: Option<Option<i64>> = self
            .conn
            .query_row(
                "SELECT companion_of FROM assets WHERE id = ?1",
                [asset.get()],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_err)?;
        Ok(master.flatten().map(AssetId::new))
    }

    /// `assets` plus every companion attached to them, each id once,
    /// **every companion before its master** (ADR 0079 §6).
    ///
    /// Removal and deletion go through this: without it the `ON DELETE
    /// CASCADE` would drop a companion's row while leaving its file on disk,
    /// and the removal report would be short by one file.
    ///
    /// The order is the load-bearing part, and it is not cosmetic.
    /// `delete_assets` reads a row's path and deletes it one id at a time,
    /// so a master listed first takes its companion's row down with it
    /// before that row's turn comes — the id then matches nothing, its file
    /// is never trashed, and nothing anywhere reports a problem. Listing the
    /// companion first is what makes the expansion do its job.
    pub fn with_companions(&self, assets: &[AssetId]) -> Result<Vec<AssetId>> {
        let mut all = Vec::with_capacity(assets.len());
        let mut seen = std::collections::HashSet::new();
        for asset in assets {
            for companion in self.companions_of(*asset)? {
                if seen.insert(companion.get()) {
                    all.push(companion);
                }
            }
            if seen.insert(asset.get()) {
                all.push(*asset);
            }
        }
        Ok(all)
    }
}
