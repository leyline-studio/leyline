//! The on-disk preview cache (`docs/catalog.md` §21, §36).
//!
//! Layout, relative to the cache root:
//!
//! ```text
//! thumbnails/{asset}/{revision}.png          kind 0
//! previews/{kind}/{asset}/{revision}.png     kinds 1-4
//! ```
//!
//! The cache is never business data: every file is regenerable, the whole
//! tree can be deleted at any time, and the catalog's `relative_path` column
//! is the only link between a `previews` row and its file.

use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use leyline_core::{AssetId, PreviewKind, RevisionId};

use crate::{PreviewError, Rgb8, max_edge};

/// A preview file written by [`PreviewCache::store`] — the facts the caller
/// records in the catalog (`docs/catalog.md` §19).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPreview {
    /// Path of the file, relative to the cache root, forward-slashed.
    pub relative_path: String,
    /// Pixel width of the file.
    pub width: u32,
    /// Pixel height of the file.
    pub height: u32,
}

/// The preview cache rooted at a library's `Cache/` directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewCache {
    root: PathBuf,
}

impl PreviewCache {
    /// Opens the cache rooted at `root` (the `Cache/` directory itself).
    /// Nothing is created until the first [`store`](PreviewCache::store).
    pub fn new(root: impl Into<PathBuf>) -> PreviewCache {
        PreviewCache { root: root.into() }
    }

    /// The cache root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The cache path of a preview slot, relative to the root,
    /// forward-slashed. Deterministic: same slot, same path.
    pub fn relative_path(asset: AssetId, revision: RevisionId, kind: PreviewKind) -> String {
        match kind {
            PreviewKind::Thumbnail => format!("thumbnails/{asset}/{revision}.png"),
            other => format!("previews/{}/{asset}/{revision}.png", other.as_i64()),
        }
    }

    /// Resolves a catalog `relative_path` to the file on disk.
    pub fn absolute_path(&self, relative_path: &str) -> PathBuf {
        self.root.join(relative_path)
    }

    /// Scales `image` to the size class of `kind` and writes it as a PNG in
    /// the cache, replacing any previous file of the same slot atomically.
    pub fn store(
        &self,
        asset: AssetId,
        revision: RevisionId,
        kind: PreviewKind,
        image: &Rgb8,
    ) -> Result<StoredPreview, PreviewError> {
        let scaled;
        let output = match max_edge(kind) {
            Some(edge) => {
                scaled = image.scaled_to_fit(edge);
                &scaled
            }
            None => image,
        };

        let relative_path = Self::relative_path(asset, revision, kind);
        let path = self.absolute_path(&relative_path);
        let parent = path.parent().expect("cache paths always have a parent");
        fs::create_dir_all(parent)?;

        // Write next to the final name, then rename: a crash never leaves a
        // half-written PNG under a path the catalog could reference.
        let staging = path.with_extension("png.tmp");
        write_png(&staging, output)?;
        fs::rename(&staging, &path)?;

        Ok(StoredPreview {
            relative_path,
            width: output.width(),
            height: output.height(),
        })
    }

    /// Deletes one cached file. Missing files are fine: the catalog row may
    /// outlive a manually cleaned cache.
    pub fn remove(&self, relative_path: &str) -> Result<(), PreviewError> {
        match fs::remove_file(self.absolute_path(relative_path)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Deletes the entire cache tree (`docs/catalog.md` §36: the cache is
    /// disposable). A missing root is fine.
    pub fn clear(&self) -> Result<(), PreviewError> {
        match fs::remove_dir_all(&self.root) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

/// Writes an image as an 8-bit RGB PNG.
fn write_png(path: &Path, image: &Rgb8) -> Result<(), PreviewError> {
    let file = fs::File::create(path)?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), image.width(), image.height());
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(image.data())?;
    writer.finish()?;
    Ok(())
}
