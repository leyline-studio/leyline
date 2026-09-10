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
use leyline_core::{AssetId, MediaType, Result};
use leyline_raw::{RawImage, ThumbnailKind};

use crate::flow::Flow;

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
    /// Answer the duplicate question by **content fingerprint** rather than
    /// by name and size (ADR 0095 §2).
    ///
    /// Exact, and it reads every scanned file in full — which is why it is
    /// off by default and asked for on purpose: a card preview that reads
    /// 20 GB is not a preview (ADR 0065 §3). Turn it on for the other
    /// question, the deliberate one about an archive already on disk:
    /// *which of these do I already hold?*
    pub exact: bool,
}

impl Default for ScanOptions {
    /// Recursive, with previews — what a client showing a contact sheet
    /// wants.
    fn default() -> Self {
        ScanOptions {
            recursive: true,
            thumbnails: true,
            exact: false,
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
    /// The library already holds this file — answered by whichever method
    /// the scan was asked for: names and sizes by default (a reliable hint,
    /// never the verdict, ADR 0065 §3), the content fingerprint under
    /// [`ScanOptions::exact`], where it *is* the verdict the import will
    /// give (ADR 0095 §3).
    pub already_imported: bool,
    /// Which asset this file duplicates, when the fingerprint said so.
    ///
    /// `None` outside [`ScanOptions::exact`]: only the fingerprint can name
    /// an asset, and the name-and-size hint deliberately reads nothing
    /// (ADR 0095 §3). Naming it is what turns a refusal into an answer.
    pub duplicate_of: Option<AssetId>,
    /// The embedded preview, as JPEG, oriented and reduced. `None` when the
    /// file carries none, when it cannot be read, or when the scan was asked
    /// not to extract any.
    pub thumbnail: Option<Vec<u8>>,
}

/// What a scan found, and whether it reached the end of the folder.
///
/// The flag matters more here than in a report of work done: a partial list
/// looks exactly like a complete one, and a client that showed « 412 photos »
/// after a scan the user stopped would be stating a fact about the folder
/// that is not true (ADR 0139 §3).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ScanReport {
    /// What an import would take, in folder order.
    pub candidates: Vec<ImportCandidate>,
    /// Whether the walk was stopped before the end of the folder.
    pub cancelled: bool,
}

/// Lists what an import of `source` would take (ADR 0065 §1).
///
/// `progress` is called after each candidate with `(done, total)`, like the
/// import itself (`docs/engine-api.md` §6) — extracting previews takes long
/// enough on a full card to be worth reporting.
pub fn scan<F: Into<Flow>>(
    catalog: &Catalog,
    source: &Path,
    options: &ScanOptions,
    mut progress: impl FnMut(u64, u64) -> F,
) -> Result<ScanReport> {
    let files = crate::import::collect_files(source, options.recursive)?;
    // One query instead of one per candidate: the comparison is against the
    // whole library, and a per-file lookup would scan `assets` on a column
    // no index leads with (`docs/catalog.md` §32).
    let known: HashSet<(String, u64)> = catalog.asset_names_and_sizes()?.into_iter().collect();

    let total = files.len() as u64;
    let mut report = ScanReport::default();
    for (done, path) in files.iter().enumerate() {
        if let Some(mut candidate) = describe(path, options.thumbnails, &known) {
            // The fingerprint, when it was asked for: the same question the
            // import will ask, answered before anything is written — and the
            // only one that can name the asset (ADR 0095 §3).
            if options.exact {
                if let Ok((checksum, _)) = crate::import::checksum(path) {
                    candidate.duplicate_of = catalog.find_asset_by_checksum(&checksum)?;
                    candidate.already_imported = candidate.duplicate_of.is_some();
                }
            }
            report.candidates.push(candidate);
        }
        if progress(done as u64 + 1, total).into().stops() {
            report.cancelled = true;
            break;
        }
    }
    Ok(report)
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
        // Only the exact pass can fill this, and it does so on the way out.
        duplicate_of: None,
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
        // A JPEG is its own preview, and asking its DCT for an eighth costs
        // half the decode (ADR 0083 §3). The fallback is the full decode the
        // develop pipeline uses, which stays exactly as it was.
        MediaType::Jpeg => std::fs::read(path)
            .ok()
            .and_then(|bytes| decode_jpeg(&bytes, THUMBNAIL_EDGE))
            .map(|(image, _)| image)
            .or_else(|| {
                crate::source::decode(path, &leyline_raw::DecodeParams::default(), false).ok()
            }),
        // Decoded whole, then reduced: no DCT to ask anything of.
        MediaType::Png | MediaType::Tiff => {
            crate::source::decode(path, &leyline_raw::DecodeParams::default(), false).ok()
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
            let (image, tagged) = decode_jpeg(&bytes, THUMBNAIL_EDGE)?;
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

/// Decodes an in-memory JPEG down to roughly `min_edge`, applies its own EXIF
/// orientation, and says whether it carried one (ADR 0083).
///
/// Nothing here needs the full image: a thumbnail throws all but 256 pixels
/// away. The JPEG's own DCT can skip that work — but only by a factor of 8 at
/// best, the entropy decode having to walk the whole scan whatever scale is
/// asked for.
fn decode_jpeg(bytes: &[u8], min_edge: u32) -> Option<(RawImage, bool)> {
    let orientation = jpeg_orientation(bytes)?;
    let rgb = scaled_rgb(bytes, min_edge).or_else(|| full_rgb(bytes))?;
    let mut decoded = image::DynamicImage::ImageRgb8(rgb);
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

/// The EXIF orientation of an in-memory JPEG, read from its headers alone.
///
/// Left to the `image` crate: it is the one that applies it too, and ADR 0082
/// defused the trap here — LibRaw does not rotate an embedded preview, most
/// bodies tag it, and honouring both the tag and the RAW's flip rotates twice.
fn jpeg_orientation(bytes: &[u8]) -> Option<image::metadata::Orientation> {
    use image::ImageDecoder as _;

    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    reader.into_decoder().ok()?.orientation().ok()
}

/// Decodes at the smallest DCT scale that still reaches `min_edge`.
///
/// `None` for anything this shortcut cannot read — a colour space that is not
/// plain RGB, a scan it refuses — so the caller falls back on the full decode
/// rather than costing the file its thumbnail.
fn scaled_rgb(bytes: &[u8], min_edge: u32) -> Option<image::RgbImage> {
    let edge = u16::try_from(min_edge).ok()?;
    let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(bytes));
    // `scale` reports the size it settled on: 1/8, 1/4, 1/2 or 1 of the
    // original, never what was asked for. Nothing computes that factor here.
    let (width, height) = decoder.scale(edge, edge).ok()?;
    let pixels = decoder.decode().ok()?;
    if decoder.info()?.pixel_format != jpeg_decoder::PixelFormat::RGB24 {
        return None;
    }
    image::RgbImage::from_raw(u32::from(width), u32::from(height), pixels)
}

/// The full decode, unchanged: the path every JPEG took before ADR 0083.
fn full_rgb(bytes: &[u8]) -> Option<image::RgbImage> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    Some(
        image::DynamicImage::from_decoder(reader.into_decoder().ok()?)
            .ok()?
            .into_rgb8(),
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Encodes a solid RGB JPEG of the given size, in memory.
    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let buffer = image::RgbImage::from_pixel(width, height, image::Rgb([90, 140, 200]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgb8(buffer)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        bytes
    }

    /// A big preview comes back reduced by the DCT, not whole (ADR 0083 §2):
    /// small enough to prove the scale happened, never below what was asked
    /// for, and still the same shape.
    #[test]
    fn a_large_jpeg_is_decoded_at_a_dct_scale() {
        let bytes = jpeg(5184, 3456);
        let (image, _) = decode_jpeg(&bytes, THUMBNAIL_EDGE).expect("decodable");

        assert!(
            image.width < 5184,
            "decoded at full size: {}x{}",
            image.width,
            image.height
        );
        assert!(
            image.width.max(image.height) >= THUMBNAIL_EDGE,
            "decoded below the edge the thumbnail needs: {}x{}",
            image.width,
            image.height
        );
        // 5184x3456 is 3:2, and every DCT scale keeps the ratio.
        assert_eq!(image.width * 2, image.height * 3);
        assert_eq!(image.data.len(), (image.width * image.height * 3) as usize);
    }

    /// A preview too small to serve must stay too small. `file_thumbnail`
    /// refuses anything under the class's edge and falls back on a real
    /// render (ADR 0082); a scale that quietly enlarged would defeat it.
    #[test]
    fn a_small_jpeg_is_not_enlarged() {
        let bytes = jpeg(160, 120);
        let (image, _) = decode_jpeg(&bytes, THUMBNAIL_EDGE).expect("decodable");

        assert_eq!((image.width, image.height), (160, 120));
        assert!(image.width.max(image.height) < THUMBNAIL_EDGE);
    }

    /// The scaled path and the full one must agree on the picture, or a
    /// thumbnail would change with the fallback that produced it.
    #[test]
    fn the_scaled_and_full_paths_agree() {
        let bytes = jpeg(1024, 768);
        let scaled = scaled_rgb(&bytes, THUMBNAIL_EDGE).expect("scalable");
        let full = full_rgb(&bytes).expect("decodable");

        assert_eq!(full.dimensions(), (1024, 768));
        assert!(scaled.width() < full.width());
        // Same flat colour, so the two decodes must land on the same pixel.
        let (a, b) = (scaled.get_pixel(1, 1).0, full.get_pixel(1, 1).0);
        for channel in 0..3 {
            assert!(
                a[channel].abs_diff(b[channel]) <= 2,
                "scaled {a:?} against full {b:?}"
            );
        }
    }
}
