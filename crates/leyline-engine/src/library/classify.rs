//! Classifying and organising: ratings, labels, picks, keywords, collections,
//! descriptions, removal, pairing and renaming files on disk.

use super::*;

impl Library {
    /// Rates a batch of versions (`docs/engine-api.md` §8); `None` clears.
    /// Emits `VersionChanged` per version.
    pub fn set_rating(&self, versions: &[VersionId], rating: Option<u8>) -> Result<()> {
        self.catalog_mut().set_rating(versions, rating)?;
        self.emit_versions_changed(versions);
        Ok(())
    }

    /// Labels a batch of versions (§8); `None` clears. Emits
    /// `VersionChanged` per version.
    pub fn set_color_label(&self, versions: &[VersionId], label: Option<ColorLabel>) -> Result<()> {
        self.catalog_mut().set_color_label(versions, label)?;
        self.emit_versions_changed(versions);
        Ok(())
    }

    /// Flags a batch of versions (§8). Emits `VersionChanged` per version.
    pub fn set_pick(&self, versions: &[VersionId], pick: PickState) -> Result<()> {
        self.catalog_mut().set_pick(versions, pick)?;
        self.emit_versions_changed(versions);
        Ok(())
    }

    /// Tags a batch of assets (§8, idempotent). Emits `AssetsChanged`.
    pub fn add_keyword(&self, assets: &[AssetId], keyword: KeywordId) -> Result<()> {
        self.catalog_mut().add_keyword(assets, keyword)?;
        self.emit(Event::AssetsChanged {
            asset_ids: assets.to_vec(),
        });
        Ok(())
    }

    /// Untags a batch of assets (§8; removing an absent tag is a no-op).
    /// Emits `AssetsChanged`.
    pub fn remove_keyword(&self, assets: &[AssetId], keyword: KeywordId) -> Result<()> {
        self.catalog_mut().remove_keyword(assets, keyword)?;
        self.emit(Event::AssetsChanged {
            asset_ids: assets.to_vec(),
        });
        Ok(())
    }

    /// Removes assets from the catalog, leaving their files untouched
    /// (ADR 0060 §1). Emits `AssetsRemoved`.
    ///
    /// This is the operation that makes ADR 0043 §5 applicable again: with
    /// the asset gone its checksum is unknown, so the file re-imports
    /// instead of being skipped as a duplicate.
    pub fn remove_assets(&self, assets: &[AssetId]) -> Result<RemovalReport> {
        self.remove_inner(assets, false)
    }

    /// Removes assets from the catalog *and* sends their files to the
    /// system trash (ADR 0060 §2). Emits `AssetsRemoved`.
    ///
    /// The XMP sidecar goes with the file. Nothing outside the library root
    /// is ever touched: even a referenced import lives under it, since the
    /// catalog stores root-relative paths only (ADR 0010).
    ///
    /// A file already missing is not an error — the catalog must be able to
    /// clean up after a photo the user moved away behind Leyline's back.
    /// A file that exists and resists is reported in
    /// [`RemovalReport::failed`], never swallowed.
    pub fn delete_assets(&self, assets: &[AssetId]) -> Result<RemovalReport> {
        self.remove_inner(assets, true)
    }

    /// Shared body of [`Library::remove_assets`] and
    /// [`Library::delete_assets`] — the catalog side is identical, only the
    /// fate of the source files differs.
    pub(super) fn remove_inner(
        &self,
        assets: &[AssetId],
        trash_files: bool,
    ) -> Result<RemovalReport> {
        // A companion goes with its master (ADR 0079 §6). The `ON DELETE
        // CASCADE` would drop its row either way — expanding the list first
        // is what makes its *file* reach the trash and its id appear in the
        // report, instead of a JPEG left on disk that nothing points to.
        let assets = self.catalog().with_companions(assets)?;
        let deleted = self.catalog_mut().delete_assets(&assets)?;
        if deleted.assets.is_empty() {
            return Ok(RemovalReport::default());
        }

        // Preview files are outside SQLite: the cascade dropped their rows
        // and would leave their bytes behind. A cache file that refuses to
        // go is not worth failing a removal over — the cache is derived and
        // can be cleared wholesale.
        for path in &deleted.preview_paths {
            let _ = self.inner.cache.remove(path);
        }

        let mut report = RemovalReport {
            removed: deleted.assets,
            trashed: Vec::new(),
            failed: Vec::new(),
        };

        if trash_files {
            for relative in &deleted.file_paths {
                let file = self.inner.root.join(relative);
                // Both naming conventions: leaving behind the sidecar of a
                // photo that no longer exists is how a later import of the
                // same folder resurrects metadata nothing points to.
                let sidecars = crate::xmp::sidecar_candidates(&file);
                for target in std::iter::once(file).chain(sidecars) {
                    if !target.exists() {
                        continue;
                    }
                    match trash::delete(&target) {
                        Ok(()) => report.trashed.push(target),
                        Err(error) => report.failed.push((target, error.to_string())),
                    }
                }
            }
        }

        self.emit(Event::AssetsRemoved {
            asset_ids: report.removed.clone(),
        });
        Ok(report)
    }

    /// Pairs every RAW+JPEG couple the library already holds, and reports
    /// each as `(master, companion)` (ADR 0079 §7).
    ///
    /// The v3 migration adds the column without pairing anything, so this is
    /// how a library imported before ADR 0079 stops listing every shot
    /// twice. Idempotent: a second run finds nothing left to pair.
    pub fn pair_assets(&self) -> Result<Vec<(AssetId, AssetId)>> {
        let paired = self.catalog_mut().pair_all()?;
        if !paired.is_empty() {
            // Both sides changed for a view: the companion left the grid,
            // and the master now carries one.
            let mut touched = Vec::with_capacity(paired.len() * 2);
            for (master, companion) in &paired {
                touched.push(*master);
                touched.push(*companion);
            }
            self.emit(Event::AssetsChanged { asset_ids: touched });
        }
        Ok(paired)
    }

    /// Detaches `assets`, master or companion, and returns how many rows
    /// stopped being companions (ADR 0079 §6).
    ///
    /// Nothing is lost or moved: a detached companion returns to the grid
    /// with the rating, keywords and revisions it always had.
    pub fn unpair_assets(&self, assets: &[AssetId]) -> Result<u32> {
        let detached = self.catalog_mut().unpair_assets(assets)?;
        if detached > 0 {
            self.emit(Event::AssetsChanged {
                asset_ids: assets.to_vec(),
            });
        }
        Ok(detached)
    }

    /// Creates a keyword under `parent`, or at the root (§8).
    pub fn create_keyword(&self, parent: Option<KeywordId>, name: &str) -> Result<KeywordId> {
        self.catalog_mut().create_keyword(parent, name)
    }

    /// The complete keyword tree, siblings ordered by name (§8).
    pub fn keyword_tree(&self) -> Result<Vec<KeywordNode>> {
        self.catalog().keyword_tree()
    }

    /// Renames one level of the keyword hierarchy (ADR 0134 §4).
    pub fn rename_keyword(&self, keyword: KeywordId, name: &str) -> Result<()> {
        self.catalog_mut().rename_keyword(keyword, name)
    }

    /// Deletes a leaf keyword and untags every photograph carrying it
    /// (ADR 0134 §4). Refused while it has children.
    pub fn delete_keyword(&self, keyword: KeywordId) -> Result<()> {
        self.catalog_mut().delete_keyword(keyword)
    }

    /// How many photographs carry each keyword, directly (ADR 0134 §2).
    pub fn keyword_counts(&self) -> Result<Vec<(KeywordId, u32)>> {
        self.catalog().keyword_counts()
    }

    /// Creates a manual collection under `parent`, or at the root (§9).
    pub fn create_collection(
        &self,
        parent: Option<CollectionId>,
        name: &str,
    ) -> Result<CollectionId> {
        self.catalog_mut().create_collection(parent, name)
    }

    /// Creates a smart collection driven by `rules` (§9, catalogue §26).
    pub fn create_smart_collection(
        &self,
        parent: Option<CollectionId>,
        name: &str,
        rules: &SmartRules,
    ) -> Result<CollectionId> {
        self.catalog_mut()
            .create_smart_collection(parent, name, rules)
    }

    /// The rules of a smart collection, or `None` for a manual one
    /// (ADR 0143 §3).
    pub fn smart_rules(&self, collection: CollectionId) -> Result<Option<SmartRules>> {
        self.catalog().smart_rules(collection)
    }

    /// Adds versions to a manual collection (§9).
    pub fn add_to_collection(&self, id: CollectionId, versions: &[VersionId]) -> Result<()> {
        self.catalog_mut().add_to_collection(id, versions)
    }

    /// Removes versions from a manual collection (§9).
    pub fn remove_from_collection(&self, id: CollectionId, versions: &[VersionId]) -> Result<()> {
        self.catalog_mut().remove_from_collection(id, versions)
    }

    /// The collection tree, children under their parents (§9).
    pub fn collections(&self) -> Result<Vec<CollectionNode>> {
        self.catalog().collections()
    }

    /// Renames a collection (§9, `docs/catalog.md` §24).
    pub fn rename_collection(&self, id: CollectionId, name: &str) -> Result<()> {
        self.catalog_mut().rename_collection(id, name)
    }

    /// Moves a collection under `parent`, or to the root with `None` (§9).
    ///
    /// A move that would make the collection its own descendant is refused.
    pub fn move_collection(&self, id: CollectionId, parent: Option<CollectionId>) -> Result<()> {
        self.catalog_mut().move_collection(id, parent)
    }

    /// Deletes a collection and everything under it, returning how many
    /// collections went (§9).
    ///
    /// A grouping disappears; no version, no revision and no file is touched.
    pub fn delete_collection(&self, id: CollectionId) -> Result<u32> {
        self.catalog_mut().delete_collection(id)
    }

    /// Records what someone wrote about a photograph (ADR 0099).
    ///
    /// Passes through to the catalog undecorated, like the other authored
    /// state (classification, keywords): the facade adds orchestration only
    /// where several stores cooperate. Emits `AssetsChanged` so a client
    /// showing the description refreshes.
    pub fn set_description(
        &self,
        asset: AssetId,
        description: &leyline_catalog::AssetDescription,
    ) -> Result<()> {
        self.catalog_mut().set_description(asset, description)?;
        self.emit(Event::AssetsChanged {
            asset_ids: vec![asset],
        });
        Ok(())
    }

    /// Renames files on disk from a template (ADR 0100).
    ///
    /// **The only engine operation that moves a user's original file.**
    /// Per asset, in the order given: the destination name is computed,
    /// refused if it already exists, the file is moved, its sidecars and
    /// its companions follow, and only then does the catalog record it
    /// (ADR 0100 §2). A failure stops that asset and no other — the report
    /// says which, and why.
    pub fn rename(&self, assets: &[AssetId], template: &str) -> Result<RenameReport> {
        let mut report = RenameReport::default();
        for (index, &asset) in assets.iter().enumerate() {
            match self.rename_one(asset, template, index + 1) {
                Ok(Some(renamed)) => report.renamed.push(renamed),
                // Nothing to do: the name it would take is the name it has.
                Ok(None) => {}
                Err(error) => report.failed.push(crate::rename::FailedRename {
                    asset,
                    reason: error.to_string(),
                }),
            }
        }
        if !report.renamed.is_empty() {
            self.emit(Event::AssetsChanged {
                asset_ids: report.renamed.iter().map(|r| r.asset).collect(),
            });
        }
        Ok(report)
    }

    /// One asset's rename, disk first (ADR 0100 §2). `None` when the file
    /// already carries the name the template gives it.
    pub(super) fn rename_one(
        &self,
        asset: AssetId,
        template: &str,
        index: usize,
    ) -> Result<Option<crate::rename::RenamedAsset>> {
        let (current, capture_ms, companions) = {
            let catalog = lock(&self.inner.catalog);
            let details = catalog.asset_details(asset)?;
            (
                details.filename.clone(),
                details.capture_date,
                catalog.companions_of(asset)?,
            )
        };
        let stem = std::path::Path::new(&current)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&current);
        let target = crate::rename::target_name(
            &current,
            template,
            &crate::rename::NameFacts {
                stem,
                capture_ms,
                index,
            },
        )?;
        if target == current {
            return Ok(None);
        }

        let file = self.locate(asset)?;
        let destination = file.with_file_name(&target);
        if destination.exists() {
            return Err(LeylineError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "{} already exists; renaming refuses to overwrite a file",
                    destination.display()
                ),
            )));
        }
        // The disk first: a failure here leaves everything as it was.
        std::fs::rename(&file, &destination)?;
        move_sidecars(&file, &destination);
        lock(&self.inner.catalog).rename_asset(asset, &target)?;

        // A companion is the same shot under another extension, and the
        // pair criterion is the shared stem (ADR 0079 §2): renaming the
        // master alone would unmake the pair, silently.
        let new_stem = std::path::Path::new(&target)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&target)
            .to_owned();
        for companion in companions {
            // The master is already renamed and correct; a companion that
            // could not follow leaves the pair to be remade by `pair`,
            // rather than rolling back a move that succeeded.
            let _ = self.rename_companion(companion, &new_stem);
        }
        Ok(Some(crate::rename::RenamedAsset {
            asset,
            from: current,
            to: target,
        }))
    }

    /// Renames a companion onto its master's new stem, keeping its own
    /// extension — a JPEG stays a JPEG.
    pub(super) fn rename_companion(&self, companion: AssetId, stem: &str) -> Result<()> {
        let current = lock(&self.inner.catalog).asset_details(companion)?.filename;
        let target = match std::path::Path::new(&current)
            .extension()
            .and_then(|e| e.to_str())
        {
            Some(extension) => format!("{stem}.{extension}"),
            None => stem.to_owned(),
        };
        if target == current {
            return Ok(());
        }
        let file = self.locate(companion)?;
        let destination = file.with_file_name(&target);
        if destination.exists() {
            return Err(LeylineError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("{} already exists", destination.display()),
            )));
        }
        std::fs::rename(&file, &destination)?;
        move_sidecars(&file, &destination);
        lock(&self.inner.catalog).rename_asset(companion, &target)
    }

    /// Overlays one description onto many assets (ADR 0099 §4): a template
    /// typed once and stamped on a whole import.
    ///
    /// *Overlays*, never replaces: a template that sets only a copyright
    /// line leaves a title someone already wrote. Emits one
    /// `AssetsChanged` for the batch rather than one per asset — a client
    /// refreshing ten thousand times is a client that stops responding.
    pub fn describe_batch(
        &self,
        assets: &[AssetId],
        template: &leyline_catalog::AssetDescription,
    ) -> Result<()> {
        if assets.is_empty() || template.is_empty() {
            return Ok(());
        }
        {
            let mut catalog = self.catalog_mut();
            for &asset in assets {
                let existing = catalog.description(asset)?.unwrap_or_default();
                catalog.set_description(asset, &existing.overlaid_with(template))?;
            }
        }
        self.emit(Event::AssetsChanged {
            asset_ids: assets.to_vec(),
        });
        Ok(())
    }
}
