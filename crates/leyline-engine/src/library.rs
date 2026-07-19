//! The library facade: one handle over a library on disk
//! (`docs/engine-api.md` §5, `docs/catalog.md` §3).
//!
//! A `Library` owns the physical layout — `catalog.db`, `Photos/`,
//! `Cache/`, `Exports/`, `Backups/` — and orchestrates the engine's
//! synchronous cores (import, preview, export, edit sessions) over it.
//! Threading, jobs and events (§3 of the API) will wrap this type; clients
//! outside the engine reach it through `leyline-sdk`.

use std::path::{Path, PathBuf};

use leyline_catalog::{Catalog, ExportPreset};
use leyline_core::{AssetId, ExportPresetId, PreviewKind, Result, VersionId};
use leyline_export::ExportSettings;
use leyline_preview::PreviewCache;

use crate::export::ExportReport;
use crate::import::{ImportOptions, ImportReport};
use crate::preview::PreviewFile;
use crate::session::EditSession;

/// An open Leyline library.
#[derive(Debug)]
pub struct Library {
    root: PathBuf,
    catalog: Catalog,
    cache: PreviewCache,
}

impl Library {
    /// Creates a new library: the §3 directory skeleton and its catalog.
    /// The root may exist (empty or not); the catalog must not.
    pub fn create(root: &Path, name: &str) -> Result<Library> {
        for dir in ["Photos", "Cache", "Exports", "Backups"] {
            std::fs::create_dir_all(root.join(dir))?;
        }
        let catalog = Catalog::create(&root.join("catalog.db"), name)?;
        Ok(Library {
            root: root.to_owned(),
            catalog,
            cache: PreviewCache::new(root.join("Cache")),
        })
    }

    /// Opens an existing library, applying pending catalog migrations.
    pub fn open(root: &Path) -> Result<Library> {
        let catalog = Catalog::open(&root.join("catalog.db"))?;
        Ok(Library {
            root: root.to_owned(),
            catalog,
            cache: PreviewCache::new(root.join("Cache")),
        })
    }

    /// Opens an existing library without write access — the §5 fallback for
    /// catalogs newer than this engine.
    pub fn open_read_only(root: &Path) -> Result<Library> {
        let catalog = Catalog::open_read_only(&root.join("catalog.db"))?;
        Ok(Library {
            root: root.to_owned(),
            catalog,
            cache: PreviewCache::new(root.join("Cache")),
        })
    }

    /// The library root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Read access to the catalog: grid queries, trees, histories.
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// Write access to the catalog: classement, keywords, collections...
    ///
    /// The facade adds orchestration only where several stores cooperate;
    /// pure catalog operations pass through undecorated.
    pub fn catalog_mut(&mut self) -> &mut Catalog {
        &mut self.catalog
    }

    /// Imports files (`docs/engine-api.md` §6). `progress` receives
    /// `(done, total)` per candidate file.
    pub fn import(
        &mut self,
        source: &Path,
        options: &ImportOptions,
        progress: impl FnMut(u64, u64),
    ) -> Result<ImportReport> {
        crate::import::import(&mut self.catalog, &self.root, source, options, progress)
    }

    /// Returns the preview of the asset's current version, rendering it into
    /// the cache first when nothing valid exists (§11).
    pub fn preview(&mut self, asset: AssetId, kind: PreviewKind) -> Result<PreviewFile> {
        crate::preview::preview(&mut self.catalog, &self.cache, &self.root, asset, kind)
    }

    /// Returns the cached preview of the asset's current version when a
    /// valid one exists, without ever rendering (§11). Lets a client fill
    /// what is already on disk instantly and schedule the rest.
    pub fn cached_preview(&self, asset: AssetId, kind: PreviewKind) -> Result<Option<PreviewFile>> {
        Ok(self
            .catalog
            .valid_preview(asset, kind)?
            .map(|row| PreviewFile {
                path: self.cache.absolute_path(&row.relative_path),
                width: row.width,
                height: row.height,
                freshly_generated: false,
            }))
    }

    /// Exports a version at its head revision (§12) and returns the file.
    pub fn export(
        &mut self,
        version: VersionId,
        settings: &ExportSettings,
        preset: Option<ExportPresetId>,
        destination_dir: &Path,
    ) -> Result<PathBuf> {
        crate::export::export_version(
            &mut self.catalog,
            &self.root,
            version,
            settings,
            preset,
            destination_dir,
        )
    }

    /// Exports several versions with one ad-hoc recipe (§12). `progress`
    /// receives `(done, total)` per version; one failure does not stop the
    /// batch.
    pub fn export_batch(
        &mut self,
        versions: &[VersionId],
        settings: &ExportSettings,
        destination_dir: &Path,
        progress: impl FnMut(u64, u64),
    ) -> Result<ExportReport> {
        crate::export::export_batch(
            &mut self.catalog,
            &self.root,
            versions,
            settings,
            None,
            destination_dir,
            progress,
        )
    }

    /// Exports several versions with a stored preset (§12): the preset's
    /// recipe drives the batch and each success is journaled against it.
    pub fn export_with_preset(
        &mut self,
        versions: &[VersionId],
        preset: ExportPresetId,
        destination_dir: &Path,
        progress: impl FnMut(u64, u64),
    ) -> Result<ExportReport> {
        let stored = self.catalog.export_preset(preset)?;
        let settings =
            ExportSettings::parse(&stored.settings_json).map_err(crate::export::export_err)?;
        crate::export::export_batch(
            &mut self.catalog,
            &self.root,
            versions,
            &settings,
            Some(preset),
            destination_dir,
            progress,
        )
    }

    /// Stores a named export preset (§12), validating the recipe first.
    pub fn create_export_preset(
        &mut self,
        name: &str,
        settings: &ExportSettings,
    ) -> Result<ExportPresetId> {
        settings.validate().map_err(crate::export::export_err)?;
        self.catalog.create_export_preset(name, &settings.to_json())
    }

    /// Lists every stored export preset, ordered by name (§12).
    pub fn export_presets(&self) -> Result<Vec<ExportPreset>> {
        self.catalog.export_presets()
    }

    /// Opens an edit session on a version (§10.1). The session borrows the
    /// library exclusively: nothing else mutates while editing.
    pub fn edit(&mut self, version: VersionId) -> Result<EditSession<'_>> {
        EditSession::open(&mut self.catalog, version)
    }

    /// Writes the XMP sidecar of an asset — the On Demand synchronization
    /// of `docs/catalog.md` §29 — and returns its path.
    pub fn write_xmp(&self, asset: AssetId) -> Result<PathBuf> {
        crate::xmp::write_xmp_sidecar(&self.catalog, &self.root, asset)
    }
}
