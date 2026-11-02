//! Previews: committed, live and before/after renders, the histogram, soft
//! proofing, and the preview cache's state.

use super::*;

impl Library {
    /// Returns the preview of the asset's current version, rendering it into
    /// the cache first when nothing valid exists (§11).
    ///
    /// Unlike the free `preview::preview` this crate's tests use, this
    /// method deliberately does *not* hold the catalog lock across the
    /// render: the render touches no catalog state, so holding it there
    /// would block every other client's catalog access (grid, search,
    /// metadata edits) for the render's full duration, including under the
    /// bounded render pool's concurrent `preview_async` jobs (ADR 0023).
    /// `preview::record_render`'s settings-match guard is what keeps this
    /// safe against a concurrent amendment landing while the render is in
    /// flight.
    pub fn preview(&self, asset: AssetId, kind: PreviewKind) -> Result<PreviewFile> {
        let plan = {
            let catalog = lock(&self.inner.catalog);
            crate::preview::plan_preview(
                &catalog,
                &self.inner.cache,
                &self.inner.root,
                asset,
                kind,
            )?
        };
        let plan = match plan {
            crate::preview::PreviewPlan::Cached(file) => return Ok(file),
            crate::preview::PreviewPlan::Render(plan) => plan,
            // ADR 0082 §1. Taken with no lock held at all: this decodes no
            // sensor and runs no stage, so it needs neither the decode cache
            // nor the stage cache — the two the render path holds, and the
            // two that made the import pass unparallelizable.
            crate::preview::PreviewPlan::FromFile {
                source_path,
                media_type,
                head,
                fallback,
            } => match crate::preview::file_thumbnail(&source_path, media_type) {
                Some(image) => {
                    let mut catalog = lock(&self.inner.catalog);
                    return crate::preview::record_embedded(
                        &mut catalog,
                        &self.inner.cache,
                        asset,
                        head,
                        &image,
                    );
                }
                // No embedded preview, unreadable, or smaller than the class
                // asked for: develop it after all.
                None => fallback,
            },
        };
        let image = {
            let mut decodes = lock(&self.inner.decodes);
            let mut stage_cache = lock(&self.inner.stage_cache);
            crate::preview::render_preview(&mut decodes, &mut stage_cache, asset, &plan)?
        };
        let mut catalog = lock(&self.inner.catalog);
        crate::preview::record_render(&mut catalog, &self.inner.cache, asset, kind, &plan, &image)
    }

    /// Per-channel 8-bit histogram (`[R, G, B]`, 256 bins each) of the
    /// asset's current develop preview at `kind` — reads back whatever
    /// [`Library::preview`] already rendered/cached rather than re-running
    /// the develop pipeline a second time.
    pub fn histogram(&self, asset: AssetId, kind: PreviewKind) -> Result<[[u32; 256]; 3]> {
        let file = self.preview(asset, kind)?;
        let image = leyline_preview::Rgb8::load_png(&file.path).map_err(|e| {
            LeylineError::DecodeFailed {
                asset,
                reason: e.to_string(),
            }
        })?;
        Ok(image.histogram())
    }

    /// Renders the asset's image at neutral (as-shot) settings, scaled like
    /// `kind` — the "before" half of develop's before/after comparison. A
    /// one-off render: unlike [`Library::preview`], this never touches the
    /// preview cache and is never recorded as any revision's valid preview,
    /// since it deliberately isn't the head's settings.
    pub fn preview_before(
        &self,
        asset: AssetId,
        kind: PreviewKind,
    ) -> Result<leyline_preview::Rgb8> {
        let plan = {
            let catalog = lock(&self.inner.catalog);
            crate::preview::plan_settings_render(&catalog, &self.inner.root, asset, kind)?
        };
        let image = {
            let mut decodes = lock(&self.inner.decodes);
            crate::preview::render_with_settings(
                &mut decodes,
                None,
                asset,
                &plan,
                &Settings::default(),
            )?
        };
        Ok(match leyline_preview::max_edge(kind) {
            Some(edge) => image.scaled_to_fit(edge),
            None => image,
        })
    }

    /// Renders `settings` at `kind`, live: what the photo would look like if
    /// these values were committed (ADR 0074).
    ///
    /// The third *view* of the engine, next to [`Library::preview_before`]
    /// and the mask overlay, and it follows the same rule as both: **nothing
    /// is written**. No preview file, no cache row, no revision — so a value
    /// a user was merely trying out can never be mistaken later for what the
    /// photo is, and a drag across a slider costs no disk at all.
    ///
    /// Unlike those two it renders through the *stage cache* (ADR 0041 §3):
    /// this is the path that cache exists for, and it is what keeps moving a
    /// late-pipeline slider in the tens of milliseconds rather than replaying
    /// the whole pipeline per frame.
    ///
    /// The caller owns the throttling: this renders every time it is asked.
    pub fn preview_live(
        &self,
        asset: AssetId,
        kind: PreviewKind,
        settings: &Settings,
    ) -> Result<leyline_preview::Rgb8> {
        let plan = {
            let catalog = lock(&self.inner.catalog);
            crate::preview::plan_settings_render(&catalog, &self.inner.root, asset, kind)?
        };
        let image = {
            let mut decodes = lock(&self.inner.decodes);
            let mut stage_cache = lock(&self.inner.stage_cache);
            crate::preview::render_with_settings(
                &mut decodes,
                Some((&mut stage_cache, asset)),
                asset,
                &plan,
                settings,
            )?
        };
        Ok(match leyline_preview::max_edge(kind) {
            Some(edge) => image.scaled_to_fit(edge),
            None => image,
        })
    }

    /// Whether [`Library::preview_live`] at `kind` would render `settings`
    /// on the GPU **now** (ADR 0166 §3): a device, a checkpoint of that
    /// class still valid for these settings, and a kernel for every stage
    /// after it. What a client asks before choosing a loupe-sized frame
    /// over a `Small` one; it renders nothing and drops nothing. Always
    /// `false` in a build without the `gpu` feature.
    pub fn live_on_gpu(&self, asset: AssetId, kind: PreviewKind, settings: &Settings) -> bool {
        #[cfg(feature = "gpu")]
        {
            lock(&self.inner.stage_cache).would_render_on_gpu(
                asset,
                leyline_preview::max_edge(kind),
                settings,
            )
        }
        #[cfg(not(feature = "gpu"))]
        {
            let _ = (asset, kind, settings);
            false
        }
    }

    /// What the preview renders on (ADR 0156 §2): the GPU adapter, or
    /// `None` for the CPU — including in a build without the `gpu` feature.
    pub fn preview_backend(&self) -> Option<String> {
        #[cfg(feature = "gpu")]
        {
            crate::gpu_preview::preview_backend()
        }
        #[cfg(not(feature = "gpu"))]
        {
            None
        }
    }

    /// What one click reads for a range mask (ADR 0093): the display-axis
    /// luminance and the hue of the 5×5 mean around `(x, y)` — unit
    /// coordinates of the rendered image.
    ///
    /// A measurement, never a write: the client decides what band to make
    /// of it. Read from the same proxy render the white-balance picker
    /// reads, which is the image the user is pointing at; the frozen
    /// `local_adjustments` modules read their terms at rank 160, and the
    /// gap is accepted and stated (ADR 0093 §1) — the eyedropper proposes
    /// a starting point, the sliders own the truth.
    pub fn sample_range(&self, asset: AssetId, x: f64, y: f64) -> Result<RangeSample> {
        let base = {
            let catalog = lock(&self.inner.catalog);
            let version = catalog.current_version(asset)?;
            let head = catalog.version_head(version)?;
            Settings::parse(&catalog.revision(head)?.settings_json)?
        };
        let image = self.preview_live(asset, PreviewKind::Small, &base)?;
        let rgb = crate::wb::sample_mean_display(&image, x, y);
        let luminance = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
        #[allow(clippy::cast_possible_truncation)]
        let (hue, _, _) =
            crate::stages::kernel::v1::rgb_to_hsl(&[rgb[0] as f32, rgb[1] as f32, rgb[2] as f32]);
        Ok(RangeSample {
            luminance,
            hue: f64::from(hue),
        })
    }

    /// Renders `asset` as it would look **with** `preset` applied, without
    /// applying it (ADR 0058 §4): the trial a client shows while a preset is
    /// merely hovered.
    ///
    /// A *view*, like [`Library::preview_before`] and the soft proof: no
    /// revision, no preset provenance, no cache entry — nothing that could
    /// later be mistaken for a development the photographer asked for.
    /// Leaving the hover shows the photo as it really is because nothing
    /// ever changed.
    ///
    /// It goes through the same `param_values` mapping the real application
    /// uses (`presets::overlay`), so what is shown is what would be written.
    pub fn preset_preview(
        &self,
        asset: AssetId,
        kind: PreviewKind,
        preset: PresetId,
    ) -> Result<leyline_preview::Rgb8> {
        let settings = {
            let catalog = lock(&self.inner.catalog);
            let stored = catalog.preset(preset)?;
            let fields = PresetSettings::parse(&stored.preset_json)?;
            let version = catalog.current_version(asset)?;
            let head = catalog.version_head(version)?;
            let base = Settings::parse(&catalog.revision(head)?.settings_json)?;
            crate::presets::overlay(&base, &fields)?
        };
        self.preview_live(asset, kind, &settings)
    }

    /// Renders the coverage of one of the head revision's local adjustments,
    /// scaled like `kind` — the mask overlay of ADR 0071.
    ///
    /// Grey, never coloured: 0 where the adjustment does not apply, 255 where
    /// it applies fully, already through the revision's rotation, perspective
    /// and crop so it lands on the preview it will be drawn over. The tint is
    /// the interface's decision, not the engine's.
    ///
    /// A *view*, like [`Library::preview_before`] and the soft proof: nothing
    /// is cached, nothing is recorded, and no stage version exists for it —
    /// `docs/pipeline.md` §5.1 is not in play.
    pub fn mask_coverage_preview(
        &self,
        asset: AssetId,
        kind: PreviewKind,
        index: usize,
    ) -> Result<leyline_preview::Rgb8> {
        let (plan, settings) = {
            let catalog = lock(&self.inner.catalog);
            let plan =
                crate::preview::plan_settings_render(&catalog, &self.inner.root, asset, kind)?;
            let version = catalog.current_version(asset)?;
            let head = catalog.version_head(version)?;
            let settings = Settings::parse(&catalog.revision(head)?.settings_json)?;
            (plan, settings)
        };
        let image = {
            let mut decodes = lock(&self.inner.decodes);
            crate::preview::render_mask_coverage(&mut decodes, asset, &plan, &settings, index)?
        };
        Ok(match leyline_preview::max_edge(kind) {
            Some(edge) => image.scaled_to_fit(edge),
            None => image,
        })
    }

    /// Renders the asset's current preview as it would appear once it had been
    /// through `proof`'s destination profile — screen soft-proofing (ADR 0034,
    /// ADR 0051 §4).
    ///
    /// In memory, like [`Library::preview_before`] and for the same reason: a
    /// proof is a way of *looking* at a photo, not a version of it. Nothing is
    /// written — no preview cache entry, no revision, no preset — so a proofed
    /// look can never be mistaken later for what the photo is.
    ///
    /// The image is the cached preview when there is one, so proofing costs a
    /// color transform rather than a develop pass.
    pub fn preview_soft_proofed(
        &self,
        asset: AssetId,
        kind: PreviewKind,
        proof: &SoftProof,
    ) -> Result<leyline_preview::Rgb8> {
        let transform = leyline_color::SoftProofTransform::load(
            &proof.profile,
            proof.intent,
            proof.gamut_warning,
        )
        .map_err(|e| LeylineError::InvalidSettings(e.to_string()))?;
        let file = self.preview(asset, kind)?;
        let mut image = leyline_preview::Rgb8::load_png(&file.path).map_err(|e| {
            LeylineError::DecodeFailed {
                asset,
                reason: e.to_string(),
            }
        })?;
        transform.apply(image.data_mut());
        Ok(image)
    }

    /// Renders a preview as a job (§3.1, §11): returns immediately, then
    /// `PreviewReady` on success and `JobFinished` either way. A cache hit
    /// still emits both — the client logic stays uniform.
    pub fn preview_async(&self, asset: AssetId, kind: PreviewKind) -> JobId {
        let job = self.new_job();
        let library = self.clone();
        self.spawn_job(move || {
            let result = match library.preview(asset, kind) {
                Ok(file) => {
                    library.emit(Event::PreviewReady {
                        asset_id: asset,
                        kind,
                    });
                    JobResult::Preview(file)
                }
                Err(error) => JobResult::Failed(error.to_string()),
            };
            library.finish_job(job, result);
        });
        job
    }

    /// Returns what the cache can show for the asset's current version,
    /// without ever rendering (§11). Lets a client fill what is already on
    /// disk instantly and schedule the rest.
    ///
    /// "What it can show" and not "the head's render": the thumbnail of a
    /// photo nobody has developed is the picture the file carries, and it is
    /// the finished answer for that state, not a placeholder (ADR 0082 §1).
    /// A client therefore needs no notion of provenance — a cell either has
    /// an image or does not, exactly as before.
    pub fn cached_preview(&self, asset: AssetId, kind: PreviewKind) -> Result<Option<PreviewFile>> {
        Ok(self
            .catalog()
            .displayable_preview(asset, kind)?
            .map(|row| PreviewFile {
                path: self.inner.cache.absolute_path(&row.relative_path),
                width: row.width,
                height: row.height,
                freshly_generated: false,
            }))
    }

    /// Fuses `preview`, `cached_preview` and `preview_async` into one
    /// convenience call (§11): the client always has something to display
    /// immediately, except when nothing has ever been cached.
    ///
    /// - A valid cache hit (head revision) comes back as [`Preview::Ready`],
    ///   no job started.
    /// - A cache entry for an older revision comes back as
    ///   [`Preview::Stale`]: display it right away, a regeneration job is
    ///   already running, watch `PreviewReady`/`JobFinished` (§3.2) for the
    ///   fresh file.
    /// - Nothing cached at all comes back as [`Preview::Generating`]: a job
    ///   was started, nothing to show until it finishes.
    pub fn preview_state(&self, asset: AssetId, kind: PreviewKind) -> Result<Preview> {
        if let Some(row) = self.catalog().valid_preview(asset, kind)? {
            return Ok(Preview::Ready(
                self.inner.cache.absolute_path(&row.relative_path),
            ));
        }
        if let Some(row) = self.catalog().latest_preview(asset, kind)? {
            let path = self.inner.cache.absolute_path(&row.relative_path);
            let job = self.preview_async(asset, kind);
            return Ok(Preview::Stale { path, job });
        }
        Ok(Preview::Generating(self.preview_async(asset, kind)))
    }
}
