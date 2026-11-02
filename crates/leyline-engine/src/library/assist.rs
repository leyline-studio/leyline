//! What measures or proposes: auto tone, auto white balance, TCA, keystone,
//! culling, derived assets and detector coverage.

use super::*;

impl Library {
    /// The tone Auto proposes for `asset` (ADR 0088 §1–§2).
    ///
    /// **Writes nothing.** It returns five numbers; feeding them through an
    /// `EditSession` is the caller's business, which is what makes an
    /// automatic tone one ordinary, undoable revision rather than a
    /// second way of developing a photo. No stage, no stage version,
    /// nothing new in `settings_json`: `docs/pipeline.md` §5.1 is untouched
    /// by construction.
    ///
    /// Measured on the photo at its **current** settings, not at neutral:
    /// pressing Auto after moving the white balance should answer for the
    /// photo as it now is. Costs a handful of proxy renders — the exposure
    /// is a logarithm, the recovery a pair of percentiles, and the two ends
    /// a short search against the real pipeline rather than a transfer
    /// function nobody wrote down.
    pub fn auto_tone(&self, asset: AssetId) -> Result<AutoTone> {
        let base = {
            let catalog = lock(&self.inner.catalog);
            let version = catalog.current_version(asset)?;
            let head = catalog.version_head(version)?;
            Settings::parse(&catalog.revision(head)?.settings_json)?
        };
        let mut tone = AutoTone {
            exposure: base.exposure,
            highlights: base.highlights,
            shadows: base.shadows,
            whites: base.whites,
            blacks: base.blacks,
        };

        // 1. Exposure, from the photo as it stands.
        let bins = self.tone_histogram(asset, &base)?;
        tone.exposure = crate::auto_tone::exposure_for(&bins, base.exposure);

        // 2. Recovery, measured *after* that correction: whether the
        //    highlights are crowded depends on where the exposure put them.
        let bins = self.tone_histogram(asset, &crate::auto_tone::with(&base, &tone))?;
        let (highlights, shadows) = crate::auto_tone::recovery_for(&bins);
        tone.highlights = highlights;
        tone.shadows = shadows;

        // 3. The two ends, searched. They are the two sliders whose effect
        //    is exactly "where does this end land", so they are measured
        //    against the real pipeline instead of guessed.
        for _ in 0..crate::auto_tone::SEARCH_STEPS {
            let bins = self.tone_histogram(asset, &crate::auto_tone::with(&base, &tone))?;
            let (white, black) = crate::auto_tone::ends_error(&bins);
            if white.abs() < crate::auto_tone::SEARCH_TOLERANCE
                && black.abs() < crate::auto_tone::SEARCH_TOLERANCE
            {
                break;
            }
            let stepped_whites = crate::auto_tone::stepped(tone.whites, white);
            let stepped_blacks = crate::auto_tone::stepped(tone.blacks, black);
            // Both ends already at the edge of their range: the photo asks
            // for more than a slider can give, and another render would
            // measure the same thing again.
            if stepped_whites == tone.whites && stepped_blacks == tone.blacks {
                break;
            }
            tone.whites = stepped_whites;
            tone.blacks = stepped_blacks;
        }
        Ok(tone)
    }

    /// The luma histogram of one proxy render of `asset` under `settings`.
    pub(super) fn tone_histogram(&self, asset: AssetId, settings: &Settings) -> Result<[u64; 256]> {
        let image = self.preview_live(asset, PreviewKind::Small, settings)?;
        Ok(crate::auto_tone::histogram_of(&image))
    }

    /// The white balance that makes the 5×5 neighbourhood around `(x, y)`
    /// — unit coordinates of the rendered image — neutral (ADR 0091 §1).
    ///
    /// A proposal, exactly like [`Library::auto_tone`]: it writes nothing,
    /// the client feeds the answer through an ordinary session, and
    /// `docs/pipeline.md` §5.1 is untouched by construction. Solved against
    /// the real pipeline (ADR 0091 §2): render at the candidate, sample,
    /// divide out the candidate's own gains, re-solve, until neutral. A
    /// clipped or near-black sample is refused, never guessed.
    pub fn neutralize_wb(&self, asset: AssetId, x: f64, y: f64) -> Result<WhiteBalance> {
        self.solve_wb(asset, |image| crate::wb::sample_mean(image, x, y), true)
    }

    /// The white balance that makes the frame's mean color neutral —
    /// grey-world, clipped pixels excluded (ADR 0091 §3). The same solver
    /// as [`Library::neutralize_wb`], fed the whole frame.
    pub fn auto_wb(&self, asset: AssetId) -> Result<WhiteBalance> {
        self.solve_wb(asset, crate::wb::frame_mean, false)
    }

    /// The shared render/sample/solve loop of the two pickers. `strict`
    /// refuses a sample that cannot answer (the pointed picker); the
    /// whole-frame mean is always readable.
    pub(super) fn solve_wb(
        &self,
        asset: AssetId,
        sample: impl Fn(&leyline_preview::Rgb8) -> [f64; 3],
        strict: bool,
    ) -> Result<WhiteBalance> {
        let base = {
            let catalog = lock(&self.inner.catalog);
            let version = catalog.current_version(asset)?;
            let head = catalog.version_head(version)?;
            Settings::parse(&catalog.revision(head)?.settings_json)?
        };
        let mut wb = base.white_balance.clone().unwrap_or_default();
        for step in 0..crate::wb::SEARCH_STEPS {
            let mut settings = base.clone();
            settings.white_balance = Some(wb.clone());
            let image = self.preview_live(asset, PreviewKind::Small, &settings)?;
            let sampled = sample(&image);
            if step == 0
                && strict
                && let Some(reason) = crate::wb::refuse(sampled)
            {
                return Err(LeylineError::InvalidImage(reason.to_owned()));
            }
            if crate::wb::neutral_enough(sampled) {
                break;
            }
            let gains = crate::wb::wb_gains(&wb);
            wb = crate::wb::solve([
                sampled[0] / gains[0],
                sampled[1] / gains[1],
                sampled[2] / gains[2],
            ]);
        }
        Ok(wb)
    }

    /// Measures the transverse chromatic aberration of a version's
    /// photograph (ADR 0111 §3): two numbers, in percent of the radius,
    /// ready to be written into `LensCorrection::tca_red`/`tca_blue`.
    ///
    /// **Nothing is written here.** The measurement is a proposal, like
    /// [`Library::auto_tone`]'s: the caller decides, and an ordinary
    /// `EditSession` records the decision (ADR 0111 §2). Below
    /// [`crate::auto_tca::MIN_SAMPLES`] usable edges the answer is zero,
    /// with the sample count saying why.
    ///
    /// The photograph is decoded **at full size**, not at the half size a
    /// thumbnail would use: LibRaw's half-size path takes red and blue from
    /// their own sites in the Bayer cell, which displaces the two channels
    /// against each other by construction — the very quantity being
    /// measured here.
    pub fn estimate_tca(&self, version: VersionId) -> Result<TcaEstimate> {
        let (asset, source, develop) = {
            let catalog = lock(&self.inner.catalog);
            let asset = catalog.version_asset(version)?;
            let head = catalog.version_head(version)?;
            let develop = Settings::parse(&catalog.revision(head)?.settings_json)?;
            let source = crate::roots::locate(&catalog, &self.inner.root, asset)?;
            (asset, source, develop)
        };
        let params = crate::stages::decode_params(&develop, false);
        let native_depth = crate::stages::native_bit_depth(&develop);
        let decoded = crate::source::decode(&source, &params, native_depth).map_err(|e| {
            LeylineError::DecodeFailed {
                asset,
                reason: e.to_string(),
            }
        })?;
        Ok(crate::auto_tca::estimate(&crate::pixels::Pixels::from_raw(
            &decoded,
        )?))
    }

    /// Solves the two perspective sliders that best bring a set of drawn
    /// guide lines upright (ADR 0119).
    ///
    /// **Nothing is written here**, and nothing is decoded either: the lines
    /// are the whole input, and the aspect ratio of the buffer they were
    /// drawn on is computed from what the catalog already holds — the
    /// sensor's dimensions, the EXIF orientation and the revision's own
    /// rotation. The caller decides what to do with the answer, and an
    /// ordinary `EditSession` records the decision, exactly as for
    /// [`Library::auto_tone`] and [`Library::estimate_tca`].
    ///
    /// `lines` are in ADR 0026's frame: normalized, post-rotation,
    /// **pre-crop**. A client showing a cropped photograph maps its own
    /// coordinates back through the crop before calling.
    pub fn keystone(&self, version: VersionId, lines: &[GuideLine]) -> Result<KeystoneSolution> {
        let aspect = {
            let catalog = lock(&self.inner.catalog);
            let asset = catalog.version_asset(version)?;
            let head = catalog.version_head(version)?;
            let develop = Settings::parse(&catalog.revision(head)?.settings_json)?;
            let details = catalog.asset_details(asset)?;
            let (Some(width), Some(height)) = (details.width, details.height) else {
                return Err(LeylineError::InvalidSettings(
                    "this photograph's dimensions were never recorded, so a guided \
                     keystone has no frame to solve in"
                        .to_owned(),
                ));
            };
            let orientation = details.metadata.as_ref().and_then(|m| m.orientation);
            crate::keystone::frame_aspect(width, height, orientation, develop.rotation)
        };
        crate::keystone::solve(lines, aspect)
    }

    /// Runs an external pixel processor over a version and files what comes
    /// back as a **new asset** (ADR 0107).
    ///
    /// The whole gesture: the photograph is developed up to rank 20 — after
    /// the decode and the camera profile, before everything else — handed to
    /// `processor` as a 16-bit linear TIFF, and the answer is written beside
    /// the original, imported, given the parent's metadata and the parent's
    /// development, and recorded as derived from it.
    ///
    /// **The engine renders and imports; the processor is a separate
    /// process.** It never opens the catalog, takes no lock, and learns no
    /// identifier — which is why a library that has never seen one still
    /// opens, renders and exports what one produced. Nothing here is a
    /// pipeline stage: a purchase can never become the condition of
    /// rendering a revision the library already holds
    /// ([ADR 0102](../../../docs/adr/0102-paid-extensions-and-the-pixel-boundary.md)).
    ///
    /// The catalog is locked twice, briefly, with the slow half between —
    /// the split ADR 0024 made for exports, for the same reason: a denoise
    /// is minutes of work and the library must stay usable throughout.
    ///
    /// Refuses when the destination name is taken: a derivation never
    /// overwrites.
    pub fn derive(
        &self,
        version: VersionId,
        processor: &leyline_derive::ProcessorSource,
        operation: &str,
    ) -> Result<AssetId> {
        if self.catalog().is_read_only() {
            return Err(LeylineError::Db(
                "library opened read-only; writes are refused".to_owned(),
            ));
        }
        let plan = {
            let catalog = lock(&self.inner.catalog);
            crate::derive::plan_derive(&catalog, &self.inner.root, version)?
        };
        let answer = crate::derive::exchange(&plan, processor, operation)?;
        let asset = {
            let mut catalog = lock(&self.inner.catalog);
            crate::derive::register(&mut catalog, &plan, operation, &answer)?
        };
        // The same event an ordinary import fires: a derived asset is a
        // photograph in the library, and a grid has no reason to learn a
        // second word for a new row appearing.
        self.emit(Event::AssetsAdded {
            asset_ids: vec![asset],
        });
        Ok(asset)
    }

    /// Goes through `assets` and **proposes** which to reject (ADR 0084).
    ///
    /// Writes nothing. What comes back is a [`CullProposal`] the caller
    /// shows to the photographer; accepting it is an ordinary
    /// `set_pick` — the same call `leyline pick` makes — and that is the
    /// whole of §1's "the assistant proposes the keystrokes the
    /// photographer would have typed".
    ///
    /// Measured on the **thumbnail** the import pass already filled from
    /// the preview the body embedded (ADR 0082), so a shoot that has been
    /// imported costs no decode here. A photograph whose thumbnail cannot
    /// be produced or read lands in [`CullProposal::skipped`] rather than
    /// receiving an invented verdict.
    ///
    /// `progress` is called after each photograph with `(done, total)`.
    pub fn cull(
        &self,
        assets: &[AssetId],
        options: &CullOptions,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<CullProposal> {
        let plan = {
            let catalog = lock(&self.inner.catalog);
            crate::cull::plan_cull(&catalog, assets)?
        };
        let total = plan.frames.len() as u64;
        let mut measures = Vec::with_capacity(plan.frames.len());
        let mut kept = Vec::with_capacity(plan.frames.len());
        let mut skipped = Vec::new();
        for (done, &(asset, version)) in plan.frames.iter().enumerate() {
            // Deliberately sequential where the import pass is parallel:
            // this reads a cached file per photograph and spends its time
            // in a gradient, not in LibRaw, and a run that is already
            // milliseconds per frame gains little from sixteen threads
            // while losing the simple cancellation a caller gets from a
            // job it can drop.
            match self
                .preview(asset, crate::cull::MEASURED_AT)
                .map_err(|error| crate::cull::missing_preview(asset, &error))
                .and_then(|file| crate::cull::measure(&file.path).map_err(|reason| (asset, reason)))
            {
                Ok(measure) => {
                    kept.push((asset, version));
                    measures.push(measure);
                }
                Err(skip) => skipped.push(skip),
            }
            progress(done as u64 + 1, total);
        }
        Ok(CullProposal {
            entries: crate::cull::decide(&kept, &measures, options),
            skipped,
        })
    }

    /// [`Library::cull`] as a job (§3.1): `JobProgress` per photograph,
    /// then `JobFinished` with the proposal.
    ///
    /// A run over a shoot is thousands of files, so an interface uses this
    /// one. Like every other job, nothing it produces is written: the
    /// proposal arrives in `JobResult::Cull` and waits for the
    /// photographer.
    pub fn cull_async(&self, assets: Vec<AssetId>, options: CullOptions) -> JobId {
        let job = self.new_job();
        let library = self.clone();
        self.spawn_job(move || {
            let result = match library.cull(&assets, &options, {
                let library = library.clone();
                move |done, total| {
                    library.emit(Event::JobProgress {
                        job_id: job,
                        done,
                        total,
                    });
                }
            }) {
                Ok(proposal) => JobResult::Cull(Box::new(proposal)),
                Err(error) => JobResult::Failed(error.to_string()),
            };
            library.finish_job(job, result);
        });
        job
    }

    /// [`Library::derive`] as a job (`docs/engine-api.md` §3.1).
    ///
    /// The reason this exists and the detector's equivalent does not is
    /// **measured**: a detection runs a model on a 1024 px preview and
    /// comes back in a second or three, where a derivation runs one on
    /// every pixel of the original — 162 s for a 10 Mpx frame on sixteen
    /// cores, and minutes more on a smaller machine. A client that called
    /// the synchronous form from its interface thread would freeze for
    /// that long, which on Windows is the point at which the window is
    /// declared unresponsive.
    ///
    /// No `JobProgress`: the socket hands a processor one file and gets one
    /// back, so there is nothing to count until it is finished. Saying "1
    /// of 1" for three minutes would be a progress bar that lies.
    pub fn derive_async(
        &self,
        version: VersionId,
        processor: &leyline_derive::ProcessorSource,
        operation: &str,
    ) -> JobId {
        let job = self.new_job();
        let library = self.clone();
        let processor = processor.clone();
        let operation = operation.to_owned();
        self.spawn_job(move || {
            let result = match library.derive(version, &processor, &operation) {
                Ok(asset) => JobResult::Derive(asset),
                Err(error) => JobResult::Failed(error.to_string()),
            };
            library.finish_job(job, result);
        });
        job
    }

    /// Stores a mask coverage in the library and returns the
    /// [`leyline_core::Mask`] that references it (ADR 0070 §5).
    ///
    /// `coverage` is `width * height` samples in row-major order, `0` = the
    /// adjustment does not apply here, `u16::MAX` = it applies fully. The
    /// resolution is the producer's own: it is sampled over the normalized
    /// canvas, so it need not match the photo, and storing a segmentation
    /// model's native output beats upsampling it to the sensor's size.
    ///
    /// The only way to create a stored mask, and the surface a closed
    /// extension uses through the SDK ([ADR 0069](../../../docs/adr/0069-closed-extension-boundary.md)).
    /// Content-addressed, so storing the same coverage twice writes one
    /// file; the caller never learns the layout.
    pub fn store_mask_coverage(
        &self,
        width: u32,
        height: u32,
        coverage: &[u16],
    ) -> Result<leyline_core::Mask> {
        if self.catalog().is_read_only() {
            return Err(LeylineError::Db(
                "library opened read-only; writes are refused".to_owned(),
            ));
        }
        crate::mask_coverage::store_coverage(&self.inner.root, width, height, coverage)
    }
}
