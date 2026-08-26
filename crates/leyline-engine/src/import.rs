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
    CHECKSUM_LEN, CameraInfo, Catalog, LensInfo, Metadata, NewAsset, Rational, RegisteredAsset,
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
    /// Attach a camera's own rendering to the RAW it was shot with, so a
    /// body set to RAW+JPEG yields one photo and not two (ADR 0079 §4).
    ///
    /// An import option and not a preference: it governs a library, not the
    /// installation, and fails the first admission condition of ADR 0078 §1.
    pub pair_companions: bool,
    /// Warm the thumbnail cache for what was just imported (ADR 0082 §4).
    ///
    /// On by default, and off for a caller that displays nothing — the same
    /// flag `ScanOptions` carries, for the same reason (ADR 0065 §2). Turning
    /// it off costs nothing but a first browse that fills in as it goes: the
    /// grid needs no warming to be usable (ADR 0082 §1).
    pub thumbnails: bool,
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
    progress: impl FnMut(u64, u64),
) -> Result<ImportReport> {
    let files = collect_files(source, options.recursive)?;
    import_files(catalog, library_root, source, &files, options, progress)
}

/// Imports exactly `files`, which must live under `source` (ADR 0065 §4).
///
/// The same per-file pipeline as [`import`] — this is what [`import`] itself
/// runs once it has enumerated the folder. A file outside `source` is
/// skipped rather than filed somewhere arbitrary: `source` is what gives a
/// copied file its place under `Photos/`.
pub fn import_files(
    catalog: &mut Catalog,
    library_root: &Path,
    source: &Path,
    files: &[PathBuf],
    options: &ImportOptions,
    mut progress: impl FnMut(u64, u64),
) -> Result<ImportReport> {
    let total = files.len() as u64;
    let mut report = ImportReport::default();
    for (done, file) in files.iter().enumerate() {
        let outside = source.is_dir() && !file.starts_with(source);
        let outcome = if outside {
            Err(Skip(format!(
                "file sits outside the import source {}",
                source.display()
            )))
        } else {
            import_one(catalog, library_root, source, file, options)
        };
        match outcome {
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

    // What LibRaw did not read still has EXIF, and the capture date is the
    // most visible of it (ADR 0056). LibRaw keeps absolute precedence, so
    // this runs only where there is nothing today; and it runs before the
    // asset is written, since it feeds `NewAsset` as much as `metadata`.
    let exif = if raw.is_none() {
        crate::exif::read_exif(file)
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

    let registered = catalog.add_asset(
        &NewAsset {
            folder,
            filename,
            extension,
            media_type,
            file_size,
            checksum,
            width: raw.as_ref().map(|m| m.width).or(probed.map(|(w, _)| w)),
            height: raw.as_ref().map(|m| m.height).or(probed.map(|(_, h)| h)),
            capture_date: raw
                .as_ref()
                .and_then(|m| m.capture_ms)
                .or_else(|| exif.as_ref().and_then(|facts| facts.capture_date)),
            // Only the EXIF path can know it: LibRaw reports a timestamp
            // without ever saying which zone it was written in (ADR 0056 §4).
            capture_offset_minutes: exif.as_ref().and_then(|facts| facts.capture_offset_minutes),
        },
        // The initial revision is stored, so it is pinned like any other
        // (`docs/pipeline.md` §3.3): neutral values, plus the versions of the
        // stages a neutral revision still runs — the two that frame the
        // pipeline (ADR 0044 §3).
        &initial_settings(media_type),
    )?;
    if let Some(raw) = raw {
        catalog.set_metadata(registered.asset, &exif_metadata(&raw))?;
    } else if let Some(metadata) = exif.and_then(|facts| facts.metadata) {
        // Best-effort, unlike the RAW path: a metadata block the catalog
        // refuses must not turn an otherwise correct import into a skip
        // (ADR 0056 §5). The asset is already in, and only its metadata row
        // would be lost.
        let _ = catalog.set_metadata(registered.asset, &metadata);
    }

    // An XMP sidecar next to the *source* file seeds the fresh asset
    // (ADR 0047 §2): this is the migration path from another program, where
    // rating, labels and keywords are the work that took years. Read from
    // the source rather than from the copy, since that is where the other
    // program left it, and never copied into `Photos/` — the catalog is the
    // source of truth from here on (`docs/catalog.md` §2.4).
    //
    // Best-effort, like the thumbnail pass: a sidecar that cannot be applied
    // never turns a successful import into a skip. The asset is already in
    // the catalog and correct; only the seeding is lost.
    if let Some(sidecar) = crate::xmp::read_xmp_sidecar(file) {
        let _ = crate::xmp::apply_xmp_sidecar(catalog, registered.asset, &sidecar);
    }

    // A camera set to RAW+JPEG wrote two files for one shot: attach them
    // (ADR 0079 §4). This runs last because it reads the metadata row that
    // the block above has just written — the body is one of the three terms
    // of the criterion, and pairing before it would compare against nothing.
    //
    // Best-effort, like the sidecar: an asset that could not be paired is a
    // photo listed twice, which the explicit pass can still fix later. An
    // import that failed over it would be worse.
    if options.pair_companions {
        let _ = catalog.pair_asset(registered.asset);
    }

    Ok(ImportedFile {
        registered,
        relative_path,
    })
}

/// The develop state a newly imported file starts from.
///
/// Neutral, with one decision taken from the file's type: a JPEG, PNG or
/// TIFF carries no highlight headroom — its white *is* white — so it starts
/// with the roll-off off, and imports that used to round-trip unchanged
/// still do. A RAW starts with the default shoulder, because it has
/// headroom to spend on it (ADR 0044 §3).
///
/// It is an opening value, not a rule: the setting is recorded in the
/// revision like any other, and the user can move it either way.
fn initial_settings(media_type: MediaType) -> leyline_core::Settings {
    let mut settings = crate::stages::neutral_settings();
    if !matches!(media_type, MediaType::Raw | MediaType::Dng) {
        settings.output_rendering.highlight_rolloff = 0;
    }
    settings
}

/// Every file under `source` an import would consider, sorted by path —
/// hidden entries excluded, extensions not yet judged.
///
/// Shared with the scan (ADR 0065 §1): two enumerations that could diverge
/// would make the list a client shows differ from what an import then takes.
pub(crate) fn collect_files(source: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect(source, recursive, &mut files)?;
    files.sort();
    Ok(files)
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
    let lens = match (&raw.lens_make, &raw.lens_model) {
        (None, None) => None,
        (make, model) => Some(LensInfo {
            manufacturer: make.clone().unwrap_or_default(),
            model: model.clone().unwrap_or_default(),
            mount: None,
        }),
    };
    Metadata {
        camera,
        lens,
        iso: raw.iso.map(|iso| iso.round() as u32),
        shutter: raw.shutter_s.and_then(shutter_rational),
        aperture: raw.aperture_f.map(|f| tenths(f64::from(f))),
        focal_length: raw.focal_mm.map(|mm| tenths(f64::from(mm))),
        gps_latitude: raw.gps_latitude,
        gps_longitude: raw.gps_longitude,
        gps_altitude: raw.gps_altitude,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The frozen noise table is keyed by the names *this* pipeline hands it,
    /// not by the ones a test author types — and a table that matched nothing
    /// would still denoise, silently, with the fallback of ADR 0072 §7. So
    /// the match is asserted against a real file:
    ///
    /// ```text
    /// LEYLINE_TEST_RAW=/path/to/file.CR2 cargo test -p leyline-engine -- --ignored
    /// ```
    #[test]
    #[ignore = "needs a real RAW file via LEYLINE_TEST_RAW"]
    fn a_real_raw_finds_its_measured_noise_profile() {
        use crate::stages::kernel::v3::{NoiseModel, model_for};

        let path = std::env::var("LEYLINE_TEST_RAW").expect("set LEYLINE_TEST_RAW");
        let raw = leyline_raw::identify(std::path::Path::new(&path)).expect("LibRaw reads it");
        let meta = exif_metadata(&raw);
        let sensor =
            crate::render::sensor_shot(&meta).expect("the file names a body and a sensitivity");
        let model = model_for(
            Some(&sensor),
            crate::stages::SourceColor::Camera {
                to_xyz: None,
                multipliers: None,
            },
            true,
        );
        assert_ne!(
            model,
            NoiseModel::fallback(),
            "no measured profile for {} {} at ISO {}",
            sensor.camera_make,
            sensor.camera_model,
            sensor.iso
        );
    }

    fn raw() -> RawMetadata {
        RawMetadata {
            make: String::new(),
            model: String::new(),
            lens_make: None,
            lens_model: None,
            width: 0,
            height: 0,
            iso: None,
            shutter_s: None,
            aperture_f: None,
            focal_mm: None,
            capture_ms: None,
            flip: 0,
            gps_latitude: None,
            gps_longitude: None,
            gps_altitude: None,
            camera_to_xyz: None,
            camera_multipliers: None,
        }
    }

    #[test]
    fn no_lens_data_leaves_lens_unset() {
        assert_eq!(exif_metadata(&raw()).lens, None);
    }

    #[test]
    fn no_gps_data_leaves_coordinates_unset() {
        let meta = exif_metadata(&raw());
        assert_eq!(meta.gps_latitude, None);
        assert_eq!(meta.gps_longitude, None);
        assert_eq!(meta.gps_altitude, None);
    }

    #[test]
    fn gps_data_passes_through_as_decimal_degrees() {
        // Southern/western hemisphere: LibRaw's parsed_gps already carries
        // the sign convention (`leyline-raw`'s dms_to_decimal), so
        // exif_metadata is a plain pass-through, nothing to re-derive here.
        let meta = exif_metadata(&RawMetadata {
            gps_latitude: Some(-33.865),
            gps_longitude: Some(151.209),
            gps_altitude: Some(42.0),
            ..raw()
        });
        assert_eq!(meta.gps_latitude, Some(-33.865));
        assert_eq!(meta.gps_longitude, Some(151.209));
        assert_eq!(meta.gps_altitude, Some(42.0));
    }

    #[test]
    fn lens_make_and_model_populate_lens_info() {
        let meta = exif_metadata(&RawMetadata {
            lens_make: Some("Canon".to_owned()),
            lens_model: Some("EF 24-70mm f/2.8L II USM".to_owned()),
            ..raw()
        });
        assert_eq!(
            meta.lens,
            Some(LensInfo {
                manufacturer: "Canon".to_owned(),
                model: "EF 24-70mm f/2.8L II USM".to_owned(),
                mount: None,
            })
        );
    }

    #[test]
    fn lens_model_without_make_still_populates_lens_info() {
        // Some cameras (compacts, older bodies) only report the lens model.
        let meta = exif_metadata(&RawMetadata {
            lens_model: Some("18-55mm".to_owned()),
            ..raw()
        });
        assert_eq!(
            meta.lens,
            Some(LensInfo {
                manufacturer: String::new(),
                model: "18-55mm".to_owned(),
                mount: None,
            })
        );
    }
}
