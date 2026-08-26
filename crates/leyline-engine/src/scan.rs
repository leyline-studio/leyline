//! Looking at a folder before importing it (ADR 0065).
//!
//! A scan describes what an import *would* take: the same enumeration, the
//! same extension filter, the same order — and **not one write**. No asset,
//! no copied file, no cache entry. That is what makes it safe to run on a
//! card one has not decided about yet.
//!
//! Each candidate can carry the preview the camera itself wrote inside the
//! file, reduced to a contact-sheet size. Nothing here goes through the
//! develop pipeline: a candidate has no revision to render, and paying a
//! decode per file is exactly what selecting beforehand is meant to avoid.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use leyline_catalog::Catalog;
use leyline_core::{MediaType, Result};
use leyline_raw::{RawImage, ThumbnailKind};

/// Longest edge of a candidate's thumbnail, in pixels. Enough for a contact
/// sheet on a high-density display, small enough that a whole card's worth
/// fits in memory while the choice is being made.
const THUMBNAIL_EDGE: u32 = 256;

/// JPEG quality of a candidate's thumbnail. It is looked at for a few
/// seconds and then thrown away; there is nothing to gain past this.
const THUMBNAIL_QUALITY: u8 = 80;

/// What a scan is asked to look at (ADR 0065 §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanOptions {
    /// Descend into subdirectories of the source.
    pub recursive: bool,
    /// Extract each candidate's embedded preview. Off for a caller that
    /// displays nothing: the scan is then pure metadata.
    pub thumbnails: bool,
}

impl Default for ScanOptions {
    /// Recursive, with previews — what a client showing a contact sheet
    /// wants.
    fn default() -> Self {
        ScanOptions {
            recursive: true,
            thumbnails: true,
        }
    }
}

/// One file an import would take, described without being taken.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportCandidate {
    /// Where the file is, as found.
    pub path: PathBuf,
    /// Its name, extension included.
    pub filename: String,
    /// What the catalog would record it as.
    pub media_type: MediaType,
    /// Size in bytes.
    pub file_size: u64,
    /// Capture instant, UTC epoch milliseconds, when the file says.
    pub capture_date: Option<i64>,
    /// Body that took it, `manufacturer model`, when the file says.
    pub camera: Option<String>,
    /// A file of this name and size is already in the library — a reliable
    /// hint, never the verdict (ADR 0065 §3). The import's own checksum
    /// comparison is the only exact answer, and it is the one that refuses.
    pub already_imported: bool,
    /// The embedded preview, as JPEG, oriented and reduced. `None` when the
    /// file carries none, when it cannot be read, or when the scan was asked
    /// not to extract any.
    pub thumbnail: Option<Vec<u8>>,
}

/// Lists what an import of `source` would take (ADR 0065 §1).
///
/// `progress` is called after each candidate with `(done, total)`, like the
/// import itself (`docs/engine-api.md` §6) — extracting previews takes long
/// enough on a full card to be worth reporting.
pub fn scan(
    catalog: &Catalog,
    source: &Path,
    options: &ScanOptions,
    mut progress: impl FnMut(u64, u64),
) -> Result<Vec<ImportCandidate>> {
    let files = crate::import::collect_files(source, options.recursive)?;
    // One query instead of one per candidate: the comparison is against the
    // whole library, and a per-file lookup would scan `assets` on a column
    // no index leads with (`docs/catalog.md` §32).
    let known: HashSet<(String, u64)> = catalog.asset_names_and_sizes()?.into_iter().collect();

    let total = files.len() as u64;
    let mut candidates = Vec::new();
    for (done, path) in files.iter().enumerate() {
        if let Some(candidate) = describe(path, options.thumbnails, &known) {
            candidates.push(candidate);
        }
        progress(done as u64 + 1, total);
    }
    Ok(candidates)
}

/// Describes one file, or `None` when an import would not take it at all —
/// an unsupported extension, an unreadable entry. A file the import would
/// *attempt* is always described, even if it would later be refused: the
/// point of the list is to show what is there.
fn describe(
    path: &Path,
    thumbnails: bool,
    known: &HashSet<(String, u64)>,
) -> Option<ImportCandidate> {
    let filename = path.file_name().and_then(|n| n.to_str())?.to_owned();
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let media_type = crate::import::media_type(&extension)?;
    let file_size = std::fs::metadata(path).ok()?.len();

    // Header reads only, and failures are silent: a file whose header is
    // unreadable is still a file the user may want to try importing.
    let raw = matches!(media_type, MediaType::Raw | MediaType::Dng)
        .then(|| leyline_raw::identify(path).ok())
        .flatten();
    let exif = if raw.is_none() {
        crate::exif::read_exif(path)
    } else {
        None
    };

    Some(ImportCandidate {
        already_imported: known.contains(&(filename.clone(), file_size)),
        filename,
        media_type,
        file_size,
        capture_date: raw
            .as_ref()
            .and_then(|m| m.capture_ms)
            .or_else(|| exif.as_ref().and_then(|facts| facts.capture_date)),
        camera: raw
            .as_ref()
            .map(|m| camera_name(&m.make, &m.model))
            .or_else(|| {
                exif.as_ref()
                    .and_then(|facts| facts.metadata.as_ref())
                    .and_then(|meta| meta.camera.as_ref())
                    .map(|camera| camera_name(&camera.manufacturer, &camera.model))
            }),
        thumbnail: thumbnails.then(|| thumbnail(path, media_type)).flatten(),
        path: path.to_owned(),
    })
}

/// `manufacturer model`, or the model alone when the maker is unknown — the
/// same form the shot filters use (ADR 0064 §3), so a scan and a filtered
/// grid name the same body the same way.
fn camera_name(manufacturer: &str, model: &str) -> String {
    let manufacturer = manufacturer.trim();
    let model = model.trim();
    if manufacturer.is_empty() {
        model.to_owned()
    } else {
        format!("{manufacturer} {model}")
    }
}

/// A candidate's contact-sheet image, or `None` when the file has none to
/// give. Never an error: a preview that cannot be produced costs the row its
/// picture, never the scan.
fn thumbnail(path: &Path, media_type: MediaType) -> Option<Vec<u8>> {
    let image = file_image(path, media_type)?;
    let (reduced, _) = crate::downscale::downscale_to_fit(&image, THUMBNAIL_EDGE);
    encode_jpeg(&reduced)
}

/// The picture a file can give without developing it: a RAW's embedded
/// preview, or the image itself for the formats that are one.
///
/// Shared with the thumbnail path of ADR 0082 §1, which stores it in the
/// cache instead of encoding it for a scan — one dispatch, so a scan and a
/// grid cell never disagree about what a file can show of itself.
pub(crate) fn file_image(path: &Path, media_type: MediaType) -> Option<RawImage> {
    match media_type {
        MediaType::Raw | MediaType::Dng => embedded_preview(path),
        // Decoded whole, then reduced. More expensive than the embedded
        // preview of a RAW, and the only thing these formats offer.
        MediaType::Jpeg | MediaType::Png | MediaType::Tiff => {
            crate::source::decode(path, &leyline_raw::DecodeParams::default()).ok()
        }
        MediaType::Heif | MediaType::Psd | MediaType::Other => None,
    }
}

/// The preview a body wrote inside its RAW file, as 8-bit RGB, oriented.
///
/// Orientation is the delicate part: LibRaw rotates nothing here, unlike a
/// full decode. Most bodies tag the embedded JPEG itself, and that tag wins —
/// it describes those exact bytes. Only when it says nothing does the RAW's
/// own flip apply; trusting both would rotate twice.
fn embedded_preview(path: &Path) -> Option<RawImage> {
    let thumb = leyline_raw::thumbnail(path).ok()??;
    match thumb.kind {
        ThumbnailKind::Jpeg(bytes) => {
            let (image, tagged) = decode_jpeg_with_orientation(&bytes)?;
            Some(if tagged {
                image
            } else {
                rotate(image, thumb.flip)
            })
        }
        ThumbnailKind::Bitmap(image) if image.bits == 8 => Some(rotate(image, thumb.flip)),
        ThumbnailKind::Bitmap(_) => None,
    }
}

/// Decodes an in-memory JPEG, applying its own EXIF orientation, and says
/// whether it carried one.
fn decode_jpeg_with_orientation(bytes: &[u8]) -> Option<(RawImage, bool)> {
    use image::ImageDecoder as _;

    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut decoder = reader.into_decoder().ok()?;
    let orientation = decoder.orientation().ok()?;
    let mut decoded = image::DynamicImage::from_decoder(decoder).ok()?;
    decoded.apply_orientation(orientation);
    let rgb = decoded.into_rgb8();
    Some((
        RawImage {
            width: rgb.width(),
            height: rgb.height(),
            bits: 8,
            data: rgb.into_raw(),
        },
        orientation != image::metadata::Orientation::NoTransforms,
    ))
}

/// Applies a dcraw flip code (`RawMetadata::flip`) to an 8-bit RGB image.
fn rotate(image: RawImage, flip: i32) -> RawImage {
    let Some(buffer) = image::RgbImage::from_raw(image.width, image.height, image.data.clone())
    else {
        return image;
    };
    let rotated = match flip {
        3 => image::imageops::rotate180(&buffer),
        5 => image::imageops::rotate270(&buffer),
        6 => image::imageops::rotate90(&buffer),
        _ => return image,
    };
    RawImage {
        width: rotated.width(),
        height: rotated.height(),
        bits: 8,
        data: rotated.into_raw(),
    }
}

/// Encodes an 8-bit RGB image as JPEG in memory.
fn encode_jpeg(image: &RawImage) -> Option<Vec<u8>> {
    if image.bits != 8 || image.width == 0 || image.height == 0 {
        return None;
    }
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, THUMBNAIL_QUALITY)
        .encode(
            &image.data,
            image.width,
            image.height,
            image::ExtendedColorType::Rgb8,
        )
        .ok()?;
    Some(bytes)
}
