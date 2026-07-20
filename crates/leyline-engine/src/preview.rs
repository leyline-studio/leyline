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
use leyline_raw::DecodeParams;

use crate::decode_cache::DecodeCache;
use crate::render;

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

/// Returns the preview of the asset's current version at `kind`, rendering
/// it into the cache first when nothing valid exists.
pub fn preview(
    catalog: &mut Catalog,
    cache: &PreviewCache,
    decodes: &mut DecodeCache,
    library_root: &Path,
    asset: AssetId,
    kind: PreviewKind,
) -> Result<PreviewFile> {
    if let Some(row) = catalog.valid_preview(asset, kind)? {
        return Ok(PreviewFile {
            path: cache.absolute_path(&row.relative_path),
            width: row.width,
            height: row.height,
            freshly_generated: false,
        });
    }

    let head = catalog.current_head_revision(asset)?;
    let settings = Settings::parse(&catalog.revision(head)?.settings_json)?;

    let relative = catalog.asset_relative_path(asset)?;
    let file = library_root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
    let params = DecodeParams {
        // Small size classes never need full resolution: half-size
        // decoding is much faster and still ≥ 2× the target edge.
        half_size: matches!(kind, PreviewKind::Thumbnail | PreviewKind::Small),
        ..DecodeParams::default()
    };
    let decoded = decodes
        .get_or_insert_with(asset, &params, || crate::source::decode(&file, &params))
        .map_err(|e| LeylineError::DecodeFailed {
            asset,
            reason: e.to_string(),
        })?;

    let meta = catalog.metadata(asset)?;
    let shot = meta.as_ref().and_then(render::lens_shot);
    let rendered = render(&decoded, &settings, shot.as_ref())?;
    let image = Rgb8::new(rendered.width, rendered.height, rendered.data).map_err(preview_err)?;
    let stored = cache
        .store(asset, head, kind, &image)
        .map_err(preview_err)?;

    catalog.record_preview(&NewPreview {
        asset,
        revision: head,
        kind,
        width: stored.width,
        height: stored.height,
        relative_path: stored.relative_path.clone(),
    })?;
    Ok(PreviewFile {
        path: cache.absolute_path(&stored.relative_path),
        width: stored.width,
        height: stored.height,
        freshly_generated: true,
    })
}

/// Maps cache errors onto the platform error type.
fn preview_err(error: PreviewError) -> LeylineError {
    match error {
        PreviewError::Io(e) => LeylineError::Io(e),
        other => LeylineError::InvalidImage(other.to_string()),
    }
}
