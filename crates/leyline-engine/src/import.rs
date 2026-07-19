//! The import pipeline (`docs/engine-api.md` §6).
//!
//! For each candidate file: checksum (BLAKE3), duplicate detection (§12),
//! RAW identification through LibRaw, optional copy into `Photos/`, folder
//! and asset registration with the mandatory develop trio (§18), and EXIF
//! metadata. Per-file problems never abort the batch: they land in the
//! report as skips with their reason.
//!
//! This is the synchronous core; the future `Library` facade wraps it in a
//! job and forwards `progress` as `JobProgress` events.

use std::io::Read;
use std::path::{Path, PathBuf};

use leyline_catalog::{
    CHECKSUM_LEN, CameraInfo, Catalog, Metadata, NewAsset, Rational, RegisteredAsset,
};
use leyline_core::{LeylineError, MediaType, Result};
use leyline_raw::RawMetadata;

/// Import options (`docs/engine-api.md` §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportOptions {
    /// Copy files into `Photos/`, or reference them where they are. A
    /// referenced file must already live under the library root: the
    /// catalog only stores root-relative paths (`docs/catalog.md` §2.3).
    pub copy_files: bool,
    /// Descend into subdirectories of the source.
    pub recursive: bool,
}

/// One successfully imported file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedFile {
    /// The created asset and its develop trio.
    pub registered: RegisteredAsset,
    /// Library-relative path of the file, forward-slashed.
    pub relative_path: String,
}

/// One file the import left aside, with the human-readable reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedFile {
    /// The file as found in the source.
    pub path: PathBuf,
    /// Why it was not imported.
    pub reason: String,
}

/// Outcome of one import batch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportReport {
    /// Files now in the catalog, in import order.
    pub imported: Vec<ImportedFile>,
    /// Files left aside, with reasons.
    pub skipped: Vec<SkippedFile>,
}

/// Imports `source` (a file or a directory) into the library.
///
/// `progress` is called after each candidate file with `(done, total)` —
/// the batching contract of `docs/engine-api.md` §6.
pub fn import(
    catalog: &mut Catalog,
    library_root: &Path,
    source: &Path,
    options: &ImportOptions,
    mut progress: impl FnMut(u64, u64),
) -> Result<ImportReport> {
    let mut files = Vec::new();
    collect(source, options.recursive, &mut files)?;
    files.sort();

    let total = files.len() as u64;
    let mut report = ImportReport::default();
    for (done, file) in files.iter().enumerate() {
        match import_one(catalog, library_root, source, file, options) {
            Ok(imported) => report.imported.push(imported),
            Err(Skip(reason)) => report.skipped.push(SkippedFile {
                path: file.clone(),
                reason,
            }),
        }
        progress(done as u64 + 1, total);
    }
    Ok(report)
}

/// Why one file was left aside. Every per-file error becomes a skip.
struct Skip(String);

impl From<LeylineError> for Skip {
    fn from(error: LeylineError) -> Skip {
        Skip(error.to_string())
    }
}

impl From<std::io::Error> for Skip {
    fn from(error: std::io::Error) -> Skip {
        Skip(format!("i/o error: {error}"))
    }
}

/// Runs the whole §6 pipeline for one file.
fn import_one(
    catalog: &mut Catalog,
    library_root: &Path,
    source: &Path,
    file: &Path,
    options: &ImportOptions,
) -> std::result::Result<ImportedFile, Skip> {
    let filename = file
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Skip("file name is not valid UTF-8".to_owned()))?
        .to_owned();
    let extension = file
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let media_type = media_type(&extension)
        .ok_or_else(|| Skip(format!("unsupported extension {extension:?}")))?;

    let (checksum, file_size) = checksum(file)?;
    if let Some(existing) = catalog.find_asset_by_checksum(&checksum)? {
        return Err(Skip(format!("duplicate of asset {existing}")));
    }

    // RAW files identify themselves before anything is written: a file
    // LibRaw cannot read is skipped whole.
    let raw = if media_type == MediaType::Raw || media_type == MediaType::Dng {
        Some(
            leyline_raw::identify(file)
                .map_err(|e| Skip(format!("raw identification failed: {e}")))?,
        )
    } else {
        None
    };

    // Non-RAW images are probed for their dimensions when the header is
    // readable; unlike RAW files they still import when it is not —
    // decode problems surface at render time (preview, export), never as
    // an import refusal.
    let probed = match media_type {
        MediaType::Jpeg | MediaType::Png | MediaType::Tiff => {
            crate::source::probe_dimensions(file).ok()
        }
        _ => None,
    };

    let relative_path = if options.copy_files {
        copy_into_photos(library_root, source, file, &filename)?
    } else {
        reference_in_place(library_root, file)?
    };
    let (folder_path, _) = relative_path
        .rsplit_once('/')
        .ok_or_else(|| Skip("file sits at the library root, not in a folder".to_owned()))?;
    let folder = catalog.ensure_folder(folder_path)?;

    let registered = catalog.add_asset(&NewAsset {
        folder,
        filename,
        extension,
        media_type,
        file_size,
        checksum,
        width: raw.as_ref().map(|m| m.width).or(probed.map(|(w, _)| w)),
        height: raw.as_ref().map(|m| m.height).or(probed.map(|(_, h)| h)),
        capture_date: raw.as_ref().and_then(|m| m.capture_ms),
        capture_offset_minutes: None,
    })?;
    if let Some(raw) = raw {
        catalog.set_metadata(registered.asset, &exif_metadata(&raw))?;
    }
    Ok(ImportedFile {
        registered,
        relative_path,
    })
}

/// Collects candidate files under `source`, hidden entries excluded.
fn collect(source: &Path, recursive: bool, files: &mut Vec<PathBuf>) -> Result<()> {
    if source.is_file() {
        files.push(source.to_owned());
        return Ok(());
    }
    if !source.is_dir() {
        return Err(LeylineError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("import source {} does not exist", source.display()),
        )));
    }
    for entry in std::fs::read_dir(source)? {
        let path = entry?.path();
        let hidden = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_none_or(|n| n.starts_with('.'));
        if hidden {
            continue;
        }
        if path.is_dir() {
            if recursive {
                collect(&path, true, files)?;
            }
        } else {
            files.push(path);
        }
    }
    Ok(())
}

/// Streams the file through BLAKE3.
fn checksum(file: &Path) -> std::result::Result<([u8; CHECKSUM_LEN], u64), Skip> {
    let mut reader = std::fs::File::open(file)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        size += read as u64;
        hasher.update(&buffer[..read]);
    }
    Ok((*hasher.finalize().as_bytes(), size))
}

/// Copies the file under `Photos/`, mirroring its position relative to the
/// source (or flat for a single-file source). Never overwrites.
fn copy_into_photos(
    library_root: &Path,
    source: &Path,
    file: &Path,
    filename: &str,
) -> std::result::Result<String, Skip> {
    let mut relative = String::from("Photos");
    if source.is_dir() {
        let inside = file
            .strip_prefix(source)
            .expect("collected files live under the source directory");
        for component in inside.components() {
            let part = component
                .as_os_str()
                .to_str()
                .ok_or_else(|| Skip("path is not valid UTF-8".to_owned()))?;
            relative.push('/');
            relative.push_str(part);
        }
    } else {
        relative.push('/');
        relative.push_str(filename);
    }

    let destination = library_root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
    if destination.exists() {
        return Err(Skip(format!(
            "destination {relative} already exists in the library"
        )));
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(file, &destination)?;
    Ok(relative)
}

/// Resolves the root-relative path of a file referenced in place.
fn reference_in_place(library_root: &Path, file: &Path) -> std::result::Result<String, Skip> {
    let canonical_root = library_root.canonicalize()?;
    let canonical = file.canonicalize()?;
    let inside = canonical.strip_prefix(&canonical_root).map_err(|_| {
        Skip("file is outside the library root; referencing needs root-relative paths".to_owned())
    })?;
    let mut relative = String::new();
    for component in inside.components() {
        let part = component
            .as_os_str()
            .to_str()
            .ok_or_else(|| Skip("path is not valid UTF-8".to_owned()))?;
        if !relative.is_empty() {
            relative.push('/');
        }
        relative.push_str(part);
    }
    Ok(relative)
}

/// Maps a lowercase extension to its media type (`docs/catalog.md` §10).
pub(crate) fn media_type(extension: &str) -> Option<MediaType> {
    Some(match extension {
        "3fr" | "arw" | "cr2" | "cr3" | "crw" | "erf" | "kdc" | "mef" | "mos" | "mrw" | "nef"
        | "nrw" | "orf" | "pef" | "raf" | "raw" | "rw2" | "sr2" | "srw" | "x3f" => MediaType::Raw,
        "dng" => MediaType::Dng,
        "jpg" | "jpeg" => MediaType::Jpeg,
        "tif" | "tiff" => MediaType::Tiff,
        "png" => MediaType::Png,
        "heic" | "heif" => MediaType::Heif,
        "psd" => MediaType::Psd,
        _ => return None,
    })
}

/// Converts LibRaw's identification into the catalog metadata row.
///
/// LibRaw reports decimals, not the original EXIF rationals: the values are
/// converted back to conventional rationals (1/x shutters, tenths for
/// apertures and focals).
fn exif_metadata(raw: &RawMetadata) -> Metadata {
    let camera = if raw.make.is_empty() && raw.model.is_empty() {
        None
    } else {
        Some(CameraInfo {
            manufacturer: raw.make.clone(),
            model: raw.model.clone(),
        })
    };
    Metadata {
        camera,
        iso: raw.iso.map(|iso| iso.round() as u32),
        shutter: raw.shutter_s.and_then(shutter_rational),
        aperture: raw.aperture_f.map(|f| tenths(f64::from(f))),
        focal_length: raw.focal_mm.map(|mm| tenths(f64::from(mm))),
        ..Metadata::default()
    }
}

/// `1/x` for fast shutters, tenths of a second beyond one second.
fn shutter_rational(seconds: f32) -> Option<Rational> {
    let seconds = f64::from(seconds);
    if seconds <= 0.0 {
        None
    } else if seconds < 1.0 {
        Some(Rational {
            numerator: 1,
            denominator: (1.0 / seconds).round().max(1.0) as i64,
        })
    } else {
        Some(tenths(seconds))
    }
}

/// A value expressed in tenths (`5.6` → `56/10`).
fn tenths(value: f64) -> Rational {
    Rational {
        numerator: (value * 10.0).round() as i64,
        denominator: 10,
    }
}
