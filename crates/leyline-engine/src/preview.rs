//! Preview orchestration (`docs/engine-api.md` §11, `docs/catalog.md` §20).
//!
//! `preview` is the synchronous get-or-generate: a valid cached file comes
//! back untouched (validity is the head-revision comparison of §20 — an
//! undo revalidates old files for free); otherwise the asset is decoded,
//! developed at its head settings, scaled into the cache and recorded. The
//! future `Library` facade runs this on its render pool and turns the
//! outcome into `PreviewReady` events.

use std::path::{Path, PathBuf};

use leyline_catalog::{Catalog, NewPreview};
use leyline_core::{AssetId, LeylineError, PreviewKind, Result, Settings};
use leyline_preview::{PreviewCache, PreviewError, Rgb8};
use leyline_raw::RawImage;

use crate::decode_cache::DecodeCache;
use crate::render::{self, render_scaled, render_scaled_cached};

/// Fuses `preview`, `cached_preview` and `preview_async` (§11) into one
/// call: the client always gets something to show immediately (`Ready` or
/// `Stale`), or knows a render is already under way (`Generating`), and
/// either way follows up on the `PreviewReady`/`JobFinished` events (§3.2)
/// of the returned [`leyline_core::JobId`] when there is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preview {
    /// A valid, up-to-date cached file: the head revision's preview.
    Ready(PathBuf),
    /// A cached file exists but is no longer the head revision's: usable
    /// for immediate display while a fresh render is already queued.
    Stale {
        /// The outdated cached file, safe to display right away.
        path: PathBuf,
        /// The regeneration job already started; watch for its
        /// `PreviewReady`/`JobFinished` events.
        job: leyline_core::JobId,
    },
    /// Nothing cached at all: a render was started, nothing to show yet.
    Generating(leyline_core::JobId),
}

/// A preview file ready to display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewFile {
    /// Absolute path of the PNG in the cache.
    pub path: PathBuf,
    /// Pixel width of the file.
    pub width: u32,
    /// Pixel height of the file.
    pub height: u32,
    /// Whether this call rendered it (false: served from cache).
    pub freshly_generated: bool,
}

/// The outcome of [`plan_preview`]: either something already displayable, or
/// everything a render needs, gathered from the catalog up front so the
/// catalog itself doesn't need to stay locked for the render (ADR 0023).
pub(crate) enum PreviewPlan {
    /// A valid cached file: nothing to render.
    Cached(PreviewFile),
    /// Nothing valid cached: nothing left to do that needs the catalog.
    Render(Box<RenderPlan>),
}

/// Everything [`render_preview`] needs to decode, develop and encode a
/// preview without touching the catalog again until [`record_render`].
pub(crate) struct RenderPlan {
    head: leyline_core::RevisionId,
    settings: Settings,
    /// The exact `settings_json` the plan was read from — [`record_render`]
    /// compares against this, not a re-serialization, so the guard can only
    /// ever be tripped by an actual concurrent rewrite (ADR 0023).
    settings_json: String,
    source_path: PathBuf,
    half_size: bool,
    /// Longest edge of the size class being rendered, `None` for full
    /// resolution — the proxy target (ADR 0041).
    max_edge: Option<u32>,
    shot: Option<crate::render::LensShot>,
    /// What the sensor was, and what it was set to — the profiled denoising
    /// stages' input (ADR 0072). Resolved here, from the same metadata as
    /// `shot`, because the render is a pure function of what it is handed.
    sensor: Option<crate::render::SensorShot>,
    /// Library root `settings.camera_profile`'s path (if any) is relative
    /// to — resolved in [`render_preview`], mirroring
    /// [`crate::export::ExportPlan`].
    library_root: PathBuf,
}

/// Reads everything needed to serve or render a preview, without decoding
/// or rendering anything itself — the read-only, catalog-bound half of
/// [`preview`], split out so a caller can drop the catalog lock before the
/// slow half (ADR 0023).
pub(crate) fn plan_preview(
    catalog: &Catalog,
    cache: &PreviewCache,
    library_root: &Path,
    asset: AssetId,
    kind: PreviewKind,
) -> Result<PreviewPlan> {
    if let Some(row) = catalog.valid_preview(asset, kind)? {
        return Ok(PreviewPlan::Cached(PreviewFile {
            path: cache.absolute_path(&row.relative_path),
            width: row.width,
            height: row.height,
            freshly_generated: false,
        }));
    }

    let head = catalog.current_head_revision(asset)?;
    let settings_json = catalog.revision(head)?.settings_json;
    let settings = Settings::parse(&settings_json)?;

    let relative = catalog.asset_relative_path(asset)?;
    let source_path = library_root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));

    let meta = catalog.metadata(asset)?;
    let shot = meta.as_ref().and_then(render::lens_shot);
    let sensor = meta.as_ref().and_then(render::sensor_shot);

    Ok(PreviewPlan::Render(Box::new(RenderPlan {
        head,
        settings,
        settings_json,
        source_path,
        // Small size classes never need full resolution: half-size
        // decoding is much faster and still ≥ 2× the target edge.
        half_size: matches!(kind, PreviewKind::Thumbnail | PreviewKind::Small),
        max_edge: leyline_preview::max_edge(kind),
        shot,
        sensor,
        library_root: library_root.to_path_buf(),
    })))
}

/// Decodes and develops a [`RenderPlan`] into an encodable image — the slow
/// half of [`preview`], deliberately taking no catalog reference so it can
/// run with no catalog lock held (ADR 0023).
pub(crate) fn render_preview(
    decodes: &mut DecodeCache,
    stage_cache: &mut crate::stages::StageCache,
    asset: AssetId,
    plan: &RenderPlan,
) -> Result<Rgb8> {
    let camera_profile = crate::camera_profile::resolve_from_settings(
        &plan.library_root,
        &plan.settings,
        &plan.source_path,
    )?;
    let params = crate::stages::decode_params(&plan.settings, plan.half_size);
    let decoded = decodes
        .get_or_insert_with(asset, &params, || {
            crate::source::decode(&plan.source_path, &params)
        })
        .map_err(|e| LeylineError::DecodeFailed {
            asset,
            reason: e.to_string(),
        })?;
    let lut = crate::lut::resolve_from_settings(&plan.library_root, &plan.settings)?;
    let coverages =
        crate::mask_coverage::resolve_from_settings(&plan.library_root, &plan.settings)?;
    let (decoded, scale) = proxy(&decoded, plan.max_edge);
    let rendered = render_scaled_cached(
        &decoded,
        &plan.settings,
        plan.shot.as_ref(),
        plan.sensor.as_ref(),
        camera_profile.as_ref(),
        lut.as_ref(),
        &coverages,
        crate::source::color(&plan.source_path),
        scale,
        asset,
        stage_cache,
    )?;
    Rgb8::new(rendered.width, rendered.height, rendered.data).map_err(preview_err)
}

/// Reduces a decoded image to the size class actually being displayed
/// before it enters the pipeline, returning it with the scale factor the
/// render must apply to its pixel-denominated radii (ADR 0041).
///
/// `None` — `PreviewKind::Full` — develops at full resolution, as does an
/// image already small enough.
fn proxy(decoded: &RawImage, max_edge: Option<u32>) -> (std::borrow::Cow<'_, RawImage>, f32) {
    match max_edge {
        Some(edge) => {
            let (scaled, scale) = crate::downscale::downscale_to_fit(decoded, edge);
            if scale == 1.0 {
                (std::borrow::Cow::Borrowed(decoded), 1.0)
            } else {
                (std::borrow::Cow::Owned(scaled), scale)
            }
        }
        None => (std::borrow::Cow::Borrowed(decoded), 1.0),
    }
}

/// Everything a settings-scoped render needs from the catalog: the asset's
/// source path and lens shot — the same two `plan_preview` reads, but
/// without its head-revision lookup, since the caller supplies its own
/// `Settings` instead of "whatever the head currently is."
pub(crate) struct SettingsRenderPlan {
    source_path: PathBuf,
    half_size: bool,
    /// Longest edge of the size class being rendered, `None` for full
    /// resolution — the proxy target (ADR 0041).
    max_edge: Option<u32>,
    shot: Option<crate::render::LensShot>,
    /// What the sensor was, and what it was set to — the profiled denoising
    /// stages' input (ADR 0072). Resolved here, from the same metadata as
    /// `shot`, because the render is a pure function of what it is handed.
    sensor: Option<crate::render::SensorShot>,
    /// Library root the render's own `settings.camera_profile` path (if
    /// any) is relative to — resolved in [`render_with_settings`].
    library_root: PathBuf,
}

/// Reads what [`render_with_settings`] needs for an ad-hoc render — used for
/// the develop view's before/after comparison, which renders a fixed
/// `Settings` (e.g. neutral) rather than the head revision. Deliberately
/// separate from [`plan_preview`]: this never touches (or is recorded into)
/// the preview cache, so it also never needs `plan_preview`'s
/// `settings`/`settings_json` guard pair.
pub(crate) fn plan_settings_render(
    catalog: &Catalog,
    library_root: &Path,
    asset: AssetId,
    kind: PreviewKind,
) -> Result<SettingsRenderPlan> {
    let relative = catalog.asset_relative_path(asset)?;
    let source_path = library_root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
    let meta = catalog.metadata(asset)?;
    let shot = meta.as_ref().and_then(render::lens_shot);
    let sensor = meta.as_ref().and_then(render::sensor_shot);
    Ok(SettingsRenderPlan {
        source_path,
        half_size: matches!(kind, PreviewKind::Thumbnail | PreviewKind::Small),
        max_edge: leyline_preview::max_edge(kind),
        shot,
        sensor,
        library_root: library_root.to_path_buf(),
    })
}

/// Renders the effective coverage of one local adjustment at preview size
/// (ADR 0071), as 8-bit grey: 0 = the adjustment does not apply, 255 = fully.
///
/// Never cached and never recorded — it is a view, not a render, so nothing
/// about it belongs in the preview cache a revision keys.
pub(crate) fn render_mask_coverage(
    decodes: &mut DecodeCache,
    asset: AssetId,
    plan: &SettingsRenderPlan,
    settings: &Settings,
    index: usize,
) -> Result<Rgb8> {
    let camera_profile = crate::camera_profile::resolve_from_settings(
        &plan.library_root,
        settings,
        &plan.source_path,
    )?;
    let params = crate::stages::decode_params(settings, plan.half_size);
    let decoded = decodes
        .get_or_insert_with(asset, &params, || {
            crate::source::decode(&plan.source_path, &params)
        })
        .map_err(|e| LeylineError::DecodeFailed {
            asset,
            reason: e.to_string(),
        })?;
    let (decoded, scale) = proxy(&decoded, plan.max_edge);
    let lut = crate::lut::resolve_from_settings(&plan.library_root, settings)?;
    let coverages = crate::mask_coverage::resolve_from_settings(&plan.library_root, settings)?;
    let (width, height, samples) = crate::stages::develop_mask_coverage(
        &decoded,
        settings,
        plan.shot.as_ref(),
        plan.sensor.as_ref(),
        camera_profile.as_ref(),
        lut.as_ref(),
        &coverages,
        crate::source::color(&plan.source_path),
        scale,
        index,
    )?;
    // Grey, never coloured: the tint is the interface's decision (ADR 0071
    // §Conséquences).
    let data = samples
        .iter()
        .flat_map(|c| {
            let level = (c * 255.0).round().clamp(0.0, 255.0) as u8;
            [level, level, level]
        })
        .collect();
    Rgb8::new(width, height, data).map_err(preview_err)
}

/// Decodes and develops `plan` under `settings` — never recorded, unlike
/// [`render_preview`]'s revision-scoped counterpart.
///
/// Two callers, and the difference between them is `stages`:
///
/// * the before/after comparison renders neutral settings **once** per
///   develop session and passes `None`: a single render has nothing to reuse,
///   and letting it retarget the shared stage cache would evict what the
///   *edited* side just filled it with;
/// * the live render of a slider drag (ADR 0074) passes the cache and lives
///   on it — that is the whole reason ADR 0041 §3 built it.
pub(crate) fn render_with_settings(
    decodes: &mut DecodeCache,
    stages: Option<(&mut crate::stages::StageCache, AssetId)>,
    asset: AssetId,
    plan: &SettingsRenderPlan,
    settings: &Settings,
) -> Result<Rgb8> {
    let camera_profile = crate::camera_profile::resolve_from_settings(
        &plan.library_root,
        settings,
        &plan.source_path,
    )?;
    let params = crate::stages::decode_params(settings, plan.half_size);
    let decoded = decodes
        .get_or_insert_with(asset, &params, || {
            crate::source::decode(&plan.source_path, &params)
        })
        .map_err(|e| LeylineError::DecodeFailed {
            asset,
            reason: e.to_string(),
        })?;
    let (decoded, scale) = proxy(&decoded, plan.max_edge);
    let lut = crate::lut::resolve_from_settings(&plan.library_root, settings)?;
    let coverages = crate::mask_coverage::resolve_from_settings(&plan.library_root, settings)?;
    let rendered = match stages {
        Some((cache, cached_asset)) => render_scaled_cached(
            &decoded,
            settings,
            plan.shot.as_ref(),
            plan.sensor.as_ref(),
            camera_profile.as_ref(),
            lut.as_ref(),
            &coverages,
            crate::source::color(&plan.source_path),
            scale,
            cached_asset,
            cache,
        )?,
        None => render_scaled(
            &decoded,
            settings,
            plan.shot.as_ref(),
            plan.sensor.as_ref(),
            camera_profile.as_ref(),
            lut.as_ref(),
            &coverages,
            crate::source::color(&plan.source_path),
            scale,
        )?,
    };
    Rgb8::new(rendered.width, rendered.height, rendered.data).map_err(preview_err)
}

/// Stores a rendered image and records it in the catalog, but only if
/// `plan`'s revision still carries the exact settings it was rendered from
/// — the write half of [`preview`], and the guard that makes narrowing the
/// catalog lock around the render safe against a concurrent amendment
/// (`docs/catalog.md` §17, ADR 0023). A tripped guard still returns the
/// rendered file for this call's immediate display; it's just not recorded
/// as valid, so the next `preview` call regenerates.
/// How many recent revisions of a photo keep their previews
/// ([ADR 0075](../../../docs/adr/0075-preview-cache-retention.md) §1).
///
/// Enough for the back-and-forth of a single setting — undo, look, redo —
/// without remembering a whole session. Beyond it the render is rebuilt on
/// demand, which costs about a second and happens from the fourth consecutive
/// undo. Every version's head survives whatever its age, so a parked virtual
/// copy never loses its thumbnail.
const RETENTION: usize = 3;

pub(crate) fn record_render(
    catalog: &mut Catalog,
    cache: &PreviewCache,
    asset: AssetId,
    kind: PreviewKind,
    plan: &RenderPlan,
    image: &Rgb8,
) -> Result<PreviewFile> {
    let stored = cache
        .store(asset, plan.head, kind, image)
        .map_err(preview_err)?;
    catalog.record_preview_if_current(
        &NewPreview {
            asset,
            revision: plan.head,
            kind,
            width: stored.width,
            height: stored.height,
            relative_path: stored.relative_path.clone(),
        },
        &plan.settings_json,
    )?;
    // A cache can only exceed its window just after something was added to
    // it, so this is the one place that has to bring it back — no sweeper, no
    // background task, no "empty the cache" left to the user (ADR 0075 §3).
    //
    // A file that cannot be unlinked is left where it is: the row is gone, so
    // nothing will serve it again, and failing a render over a stale byte in
    // a cache would trade a real result for a derived one.
    for stale in catalog.retain_previews(asset, RETENTION)? {
        let _ = cache.remove(&stale);
    }
    Ok(PreviewFile {
        path: cache.absolute_path(&stored.relative_path),
        width: stored.width,
        height: stored.height,
        freshly_generated: true,
    })
}

/// Returns the preview of the asset's current version at `kind`, rendering
/// it into the cache first when nothing valid exists.
///
/// This free function drives [`plan_preview`], [`render_preview`] and
/// [`record_render`] over one `&mut Catalog` held throughout — the pattern
/// this crate's tests use directly. [`crate::Library::preview`] instead
/// sequences the same three phases itself, dropping the catalog lock across
/// [`render_preview`] (ADR 0023).
pub fn preview(
    catalog: &mut Catalog,
    cache: &PreviewCache,
    decodes: &mut DecodeCache,
    library_root: &Path,
    asset: AssetId,
    kind: PreviewKind,
) -> Result<PreviewFile> {
    let plan = match plan_preview(catalog, cache, library_root, asset, kind)? {
        PreviewPlan::Cached(file) => return Ok(file),
        PreviewPlan::Render(plan) => plan,
    };
    // This standalone entry point has no long-lived cache to lend, so it
    // renders with a throwaway one: correct by construction, and no slower
    // than before, since a fresh cache simply never hits.
    let image = render_preview(
        decodes,
        &mut crate::stages::StageCache::default(),
        asset,
        &plan,
    )?;
    record_render(catalog, cache, asset, kind, &plan, &image)
}

/// Maps cache errors onto the platform error type.
fn preview_err(error: PreviewError) -> LeylineError {
    match error {
        PreviewError::Io(e) => LeylineError::Io(e),
        other => LeylineError::InvalidImage(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::{ImportOptions, import};
    use crate::session::{EditSession, Param, Value};

    /// A real 8×4 PNG, imported into a fresh library, with everything
    /// `plan_preview`/`render_preview`/`record_render` need to exercise the
    /// split directly — these are `pub(crate)`, so only reachable from a
    /// unit test in this module, not the crate's external `tests/`.
    fn imported(
        dir: &tempfile::TempDir,
    ) -> (
        Catalog,
        PathBuf,
        PreviewCache,
        AssetId,
        leyline_core::VersionId,
    ) {
        let root = dir.path().join("Library");
        std::fs::create_dir(&root).unwrap();
        let mut catalog = Catalog::create(&root.join("catalog.db"), "Preview").unwrap();
        let cache = PreviewCache::new(root.join("Cache"));

        let source = dir.path().join("photo.png");
        image::save_buffer(
            &source,
            &[128u8; 8 * 4 * 3],
            8,
            4,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
        let report = import(
            &mut catalog,
            &root,
            &source,
            &ImportOptions {
                copy_files: true,
                recursive: false,
            },
            |_, _| {},
        )
        .unwrap();
        let registered = report.imported[0].registered;
        (catalog, root, cache, registered.asset, registered.version)
    }

    #[test]
    fn record_render_writes_when_nothing_raced_it() {
        let dir = tempfile::tempdir().unwrap();
        let (mut catalog, root, cache, asset, _version) = imported(&dir);
        let mut decodes = DecodeCache::new(2);

        let plan =
            match plan_preview(&catalog, &cache, &root, asset, PreviewKind::Thumbnail).unwrap() {
                PreviewPlan::Render(plan) => plan,
                PreviewPlan::Cached(_) => panic!("nothing cached yet"),
            };
        let image = render_preview(
            &mut decodes,
            &mut crate::stages::StageCache::default(),
            asset,
            &plan,
        )
        .unwrap();
        let file = record_render(
            &mut catalog,
            &cache,
            asset,
            PreviewKind::Thumbnail,
            &plan,
            &image,
        )
        .unwrap();

        assert!(file.freshly_generated);
        assert!(
            catalog
                .valid_preview(asset, PreviewKind::Thumbnail)
                .unwrap()
                .is_some(),
            "no concurrent write happened, so this render must be recorded as valid"
        );
    }

    /// The scenario ADR 0023 exists for: a plan is read (as `Library::preview`
    /// would, right before dropping the catalog lock across the render), and
    /// while the render is "in flight" a concurrent amendment rewrites the
    /// same revision id in place. `record_render` must not let the stale
    /// render pass as the valid preview of what the revision now means.
    #[test]
    fn record_render_discards_a_render_raced_by_an_amendment() {
        let dir = tempfile::tempdir().unwrap();
        let (mut catalog, root, cache, asset, version) = imported(&dir);
        let mut decodes = DecodeCache::new(2);

        // A first real edit: only a non-initial revision can ever be
        // amended (§17), so the plan below must target this one, not the
        // asset's initial revision.
        let mut session = EditSession::open(&mut catalog, version).unwrap();
        session.set(Param::Exposure, Value::Float(0.2)).unwrap();
        session.commit().unwrap();
        drop(session);

        let plan =
            match plan_preview(&catalog, &cache, &root, asset, PreviewKind::Thumbnail).unwrap() {
                PreviewPlan::Render(plan) => plan,
                PreviewPlan::Cached(_) => panic!("nothing cached yet"),
            };
        // The render itself doesn't touch the catalog, so it can genuinely
        // run here, in between reading the plan and recording it — exactly
        // where `Library::preview` releases the catalog lock.
        let image = render_preview(
            &mut decodes,
            &mut crate::stages::StageCache::default(),
            asset,
            &plan,
        )
        .unwrap();

        // Concurrent amendment of the exact revision the plan targeted —
        // same id, rewritten settings — called directly on the catalog
        // (bypassing `EditSession`'s in-memory amend-window bookkeeping,
        // which only decides *whether* to amend; the catalog-side
        // preconditions checked by `try_amend_head` are what actually
        // matter here, and this revision satisfies them).
        let mut amended = plan.settings.clone();
        amended.exposure = 0.9;
        catalog.try_amend_head(version, &amended).unwrap();

        let file = record_render(
            &mut catalog,
            &cache,
            asset,
            PreviewKind::Thumbnail,
            &plan,
            &image,
        )
        .unwrap();

        // Still displayable for this call...
        assert!(file.freshly_generated);
        // ...but never recorded as the valid preview of the revision, which
        // no longer means what it meant when the render started.
        assert_eq!(
            catalog
                .valid_preview(asset, PreviewKind::Thumbnail)
                .unwrap(),
            None
        );
    }

    /// A plain commit (a *new* revision, not an amendment) between the plan
    /// and the record must never trip the guard: the planned revision's own
    /// `settings_json` is untouched, so the render is still correctly
    /// recorded — just not as the (now different) head, exactly like
    /// `record_preview` behaved before ADR 0023.
    #[test]
    fn record_render_survives_a_commit_to_a_new_revision() {
        let dir = tempfile::tempdir().unwrap();
        let (mut catalog, root, cache, asset, version) = imported(&dir);
        let mut decodes = DecodeCache::new(2);

        let plan =
            match plan_preview(&catalog, &cache, &root, asset, PreviewKind::Thumbnail).unwrap() {
                PreviewPlan::Render(plan) => plan,
                PreviewPlan::Cached(_) => panic!("nothing cached yet"),
            };
        let rendered_revision = plan.head;
        let image = render_preview(
            &mut decodes,
            &mut crate::stages::StageCache::default(),
            asset,
            &plan,
        )
        .unwrap();

        // A plain commit creates a new revision and moves the head — it
        // never rewrites `rendered_revision`'s own settings.
        let mut session = EditSession::open(&mut catalog, version).unwrap();
        session.set(Param::Contrast, Value::Int(10)).unwrap();
        session.commit().unwrap();
        drop(session);
        assert_ne!(
            catalog.current_head_revision(asset).unwrap(),
            rendered_revision
        );

        record_render(
            &mut catalog,
            &cache,
            asset,
            PreviewKind::Thumbnail,
            &plan,
            &image,
        )
        .unwrap();

        // Not head anymore, so not valid — but the row exists: an undo back
        // onto `rendered_revision` revalidates it for free.
        assert_eq!(
            catalog
                .valid_preview(asset, PreviewKind::Thumbnail)
                .unwrap(),
            None
        );
        let mut session = EditSession::open(&mut catalog, version).unwrap();
        session.undo().unwrap();
        drop(session);
        assert!(
            catalog
                .valid_preview(asset, PreviewKind::Thumbnail)
                .unwrap()
                .is_some()
        );
    }
}
