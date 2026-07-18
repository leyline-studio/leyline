//! Integration tests: the on-disk preview cache.

use leyline_core::{AssetId, PreviewKind, RevisionId};
use leyline_preview::{PreviewCache, Rgb8};

fn gradient(width: u32, height: u32) -> Rgb8 {
    let mut data = Vec::with_capacity(width as usize * height as usize * 3);
    for y in 0..height {
        for x in 0..width {
            data.extend([(x % 256) as u8, (y % 256) as u8, 128]);
        }
    }
    Rgb8::new(width, height, data).unwrap()
}

/// Decodes a cached PNG back to (width, height, pixels).
fn read_png(path: &std::path::Path) -> (u32, u32, Vec<u8>) {
    let file = std::io::BufReader::new(std::fs::File::open(path).unwrap());
    let decoder = png::Decoder::new(file);
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgb);
    assert_eq!(info.bit_depth, png::BitDepth::Eight);
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

#[test]
fn store_scales_and_writes_a_readable_png() {
    let dir = tempfile::tempdir().unwrap();
    let cache = PreviewCache::new(dir.path().join("Cache"));

    let stored = cache
        .store(
            AssetId::new(7),
            RevisionId::new(12),
            PreviewKind::Thumbnail,
            &gradient(1200, 800),
        )
        .unwrap();

    assert_eq!(stored.relative_path, "thumbnails/7/12.png");
    assert_eq!((stored.width, stored.height), (256, 171));

    let (width, height, _) = read_png(&cache.absolute_path(&stored.relative_path));
    assert_eq!((width, height), (256, 171));
}

#[test]
fn full_kind_keeps_native_resolution_and_pixels() {
    let dir = tempfile::tempdir().unwrap();
    let cache = PreviewCache::new(dir.path().join("Cache"));
    let image = gradient(64, 48);

    let stored = cache
        .store(
            AssetId::new(1),
            RevisionId::new(2),
            PreviewKind::Full,
            &image,
        )
        .unwrap();

    assert_eq!(stored.relative_path, "previews/4/1/2.png");
    let (width, height, pixels) = read_png(&cache.absolute_path(&stored.relative_path));
    assert_eq!((width, height), (64, 48));
    assert_eq!(pixels, image.data());
}

#[test]
fn store_replaces_the_previous_file_of_a_slot() {
    let dir = tempfile::tempdir().unwrap();
    let cache = PreviewCache::new(dir.path().join("Cache"));
    let slot = (AssetId::new(3), RevisionId::new(4), PreviewKind::Small);

    let first = cache
        .store(slot.0, slot.1, slot.2, &gradient(2000, 1000))
        .unwrap();
    let second = cache
        .store(slot.0, slot.1, slot.2, &gradient(1000, 2000))
        .unwrap();

    assert_eq!(first.relative_path, second.relative_path);
    let (width, height, _) = read_png(&cache.absolute_path(&second.relative_path));
    assert_eq!((width, height), (512, 1024));

    // No staging leftovers next to the final file.
    let parent = cache.absolute_path("previews/1/3");
    assert_eq!(std::fs::read_dir(parent).unwrap().count(), 1);
}

#[test]
fn remove_and_clear_are_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let cache = PreviewCache::new(dir.path().join("Cache"));

    let stored = cache
        .store(
            AssetId::new(5),
            RevisionId::new(6),
            PreviewKind::Thumbnail,
            &gradient(100, 100),
        )
        .unwrap();

    cache.remove(&stored.relative_path).unwrap();
    assert!(!cache.absolute_path(&stored.relative_path).exists());
    cache.remove(&stored.relative_path).unwrap();

    cache.clear().unwrap();
    assert!(!cache.root().exists());
    cache.clear().unwrap();
}
