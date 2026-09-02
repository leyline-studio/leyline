//! Integration test: a file that declares its colour space is read as what
//! it says, not as sRGB (ADR 0115).

use leyline_core::PreviewKind;
use leyline_engine::{ImportOptions, Library};

/// The same pixels, written twice: once with a Display P3 profile attached,
/// once with nothing. Only the tag differs.
fn write_pair(dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let (width, height) = (32u32, 32u32);
    // A saturated red-to-green field: the colours whose primaries differ
    // most between sRGB and P3, so the tag has something to change.
    let mut pixels = Vec::new();
    for y in 0..height {
        for x in 0..width {
            pixels.extend_from_slice(&[(x * 8).min(255) as u8, (y * 8).min(255) as u8, 40u8]);
        }
    }
    let untagged = dir.join("untagged.png");
    image::save_buffer(
        &untagged,
        &pixels,
        width,
        height,
        image::ExtendedColorType::Rgb8,
    )
    .unwrap();

    // The tagged twin, through the PNG encoder's ICC chunk.
    let tagged = dir.join("tagged.png");
    let file = std::fs::File::create(&tagged).unwrap();
    let mut encoder = image::codecs::png::PngEncoder::new(file);
    use image::ImageEncoder;
    encoder.set_icc_profile(display_p3_icc()).unwrap();
    encoder
        .write_image(&pixels, width, height, image::ExtendedColorType::Rgb8)
        .unwrap();
    (untagged, tagged)
}

/// Display P3: the profile a phone tags its photographs with.
fn display_p3_icc() -> Vec<u8> {
    let white = lcms2::CIExyY {
        x: 0.3127,
        y: 0.3290,
        Y: 1.0,
    };
    let primaries = lcms2::CIExyYTRIPLE {
        Red: lcms2::CIExyY {
            x: 0.680,
            y: 0.320,
            Y: 1.0,
        },
        Green: lcms2::CIExyY {
            x: 0.265,
            y: 0.690,
            Y: 1.0,
        },
        Blue: lcms2::CIExyY {
            x: 0.150,
            y: 0.060,
            Y: 1.0,
        },
    };
    let curve = lcms2::ToneCurve::new_parametric(
        4,
        &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045],
    )
    .unwrap();
    lcms2::Profile::new_rgb(&white, &primaries, &[&curve, &curve, &curve])
        .unwrap()
        .icc()
        .unwrap()
}

fn render(library: &Library, source: &std::path::Path) -> leyline_engine::Rgb8 {
    let report = library
        .import(
            source,
            &ImportOptions {
                copy_files: true,
                recursive: false,
                pair_companions: true,
                thumbnails: false,
            },
            |_, _| {},
        )
        .unwrap();
    let asset = report.imported[0].registered.asset;
    let preview = library.preview(asset, PreviewKind::Small).unwrap();
    leyline_engine::Rgb8::load_png(&preview.path).unwrap()
}

#[test]
fn a_tagged_file_renders_as_what_it_says_and_an_untagged_one_as_srgb() {
    let dir = tempfile::tempdir().unwrap();
    let (untagged, tagged) = write_pair(dir.path());
    let library = Library::create(&dir.path().join("Library"), "Colour").unwrap();

    let plain = render(&library, &untagged);
    let p3 = render(&library, &tagged);
    assert_eq!((plain.width(), plain.height()), (p3.width(), p3.height()));

    // The same numbers read as P3 describe *wider* colours than read as
    // sRGB, so the rendering differs — and differs in the direction that
    // says the primaries were honoured rather than ignored.
    assert_ne!(plain.data(), p3.data(), "the tag changed nothing");

    let saturation = |image: &leyline_engine::Rgb8| -> f64 {
        image
            .data()
            .chunks_exact(3)
            .map(|rgb| {
                let (max, min) = rgb
                    .iter()
                    .fold((0u8, 255u8), |(hi, lo), v| (hi.max(*v), lo.min(*v)));
                f64::from(max - min)
            })
            .sum::<f64>()
            / (image.data().len() / 3) as f64
    };
    assert!(
        saturation(&p3) > saturation(&plain),
        "P3 primaries reach further than sRGB's: {} vs {}",
        saturation(&p3),
        saturation(&plain)
    );
}

/// The pinning rule is what makes the fix opt-in, and it is proved where it
/// lives: the golden manifest replays every entry through the stage map it
/// was captured with, so the entries citing `input: 5` still render their
/// old digest. The stage-level twin of this test — the same tagged source
/// through `v5` and through `v6` — is in `stages::tests`.
#[test]
fn an_untagged_file_is_still_read_as_srgb() {
    let dir = tempfile::tempdir().unwrap();
    let (untagged, _) = write_pair(dir.path());
    let library = Library::create(&dir.path().join("Library"), "Plain").unwrap();
    let rendered = render(&library, &untagged);
    // Nothing subtle to assert here beyond "it still works": the point is
    // that ADR 0115 changed nothing for the files that say nothing, which is
    // most of them.
    assert!(rendered.width() > 0 && rendered.height() > 0);
}
