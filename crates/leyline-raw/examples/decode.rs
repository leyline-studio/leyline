//! Decodes a RAW file and writes a viewable PNG next to it.
//!
//! ```sh
//! cargo run -p leyline-raw --example decode -- photo.CR3 [out.png]
//! ```

use std::path::{Path, PathBuf};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(input) = args.next() else {
        eprintln!("usage: decode <raw-file> [out.png]");
        std::process::exit(2);
    };
    let input = PathBuf::from(input);
    let output = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| input.with_extension("png"));

    let decoded = match leyline_raw::decode(&input, &leyline_raw::DecodeParams::default()) {
        Ok(decoded) => decoded,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    let m = &decoded.metadata;
    println!(
        "{} {} — {}x{} px{}{}{}{}",
        m.make,
        m.model,
        decoded.image.width,
        decoded.image.height,
        m.iso.map(|v| format!(" — ISO {v}")).unwrap_or_default(),
        m.shutter_s
            .map(|v| format!(" — {:.5} s", v))
            .unwrap_or_default(),
        m.aperture_f
            .map(|v| format!(" — f/{v:.1}"))
            .unwrap_or_default(),
        m.focal_mm
            .map(|v| format!(" — {v:.0} mm"))
            .unwrap_or_default(),
    );

    write_png(&output, &decoded.image);
    println!("wrote {}", output.display());
}

fn write_png(path: &Path, image: &leyline_raw::RawImage) {
    let file = std::fs::File::create(path).expect("create output file");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), image.width, image.height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .expect("write png header")
        .write_image_data(&image.data)
        .expect("write png data");
}
