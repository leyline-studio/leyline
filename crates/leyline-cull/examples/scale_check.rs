//! Does measuring at thumbnail scale rank frames the way full resolution does?
//!
//! ADR 0084 §4's affordability rests on analysing the body's embedded preview
//! at an eighth (ADR 0083) rather than the sensor decode. That is only sound
//! if the *ranking* survives the downscale — the absolute score obviously
//! does not, since gradient energy depends on sampling.
//!
//! Run: `cargo run -p leyline-cull --example scale_check --release -- <jpeg>...`
use leyline_cull::{Frame, fingerprint, quality};

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    let mut full = Vec::new();
    let mut eighth = Vec::new();
    let mut analysis_us: Vec<f64> = Vec::new();

    println!(
        "{:<28} {:>10} {:>10} {:>8} {:>8}",
        "file", "focus@full", "focus@1/8", "clip%", "print"
    );
    for path in &paths {
        let img = image::open(path).expect("readable image").to_rgb8();
        let (w, h) = img.dimensions();
        let qf = quality(Frame {
            width: w,
            height: h,
            rgb: img.as_raw(),
        })
        .expect("measurable");

        let small =
            image::imageops::resize(&img, w / 8, h / 8, image::imageops::FilterType::Triangle);
        let qe = quality(Frame {
            width: w / 8,
            height: h / 8,
            rgb: small.as_raw(),
        })
        .expect("measurable");
        let print = fingerprint(Frame {
            width: w / 8,
            height: h / 8,
            rgb: small.as_raw(),
        })
        .expect("hashable");

        let name = path.rsplit('/').next().unwrap_or(path);
        println!(
            "{name:<28} {:>10.4} {:>10.4} {:>7.2}% {:>8x}",
            qf.focus,
            qe.focus,
            qf.clipped_highlights * 100.0,
            print.0
        );
        full.push(qf.focus);
        eighth.push(qe.focus);

        // What the measure itself costs on a thumbnail-sized buffer — the
        // number that decides whether culling a shoot is affordable, next to
        // the ~40 ms ADR 0083 measured for the scaled decode that feeds it.
        let t = std::time::Instant::now();
        for _ in 0..20 {
            let _ = quality(Frame {
                width: w / 8,
                height: h / 8,
                rgb: small.as_raw(),
            });
            let _ = fingerprint(Frame {
                width: w / 8,
                height: h / 8,
                rgb: small.as_raw(),
            });
        }
        analysis_us.push(t.elapsed().as_secs_f64() * 1e6 / 20.0);
    }

    // Spearman: does the eighth-scale ranking agree with the full one?
    let rank = |v: &Vec<f32>| {
        let mut idx: Vec<usize> = (0..v.len()).collect();
        idx.sort_by(|a, b| v[*a].partial_cmp(&v[*b]).unwrap());
        let mut r = vec![0.0f64; v.len()];
        for (position, i) in idx.iter().enumerate() {
            r[*i] = position as f64;
        }
        r
    };
    let (rf, re) = (rank(&full), rank(&eighth));
    let n = rf.len() as f64;
    let d2: f64 = rf.iter().zip(&re).map(|(a, b)| (a - b) * (a - b)).sum();
    let rho = 1.0 - 6.0 * d2 / (n * (n * n - 1.0));
    analysis_us.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "\nmesure seule sur la vignette : mediane {:.0} us/image ({} x {})",
        analysis_us[analysis_us.len() / 2],
        analysis_us.len(),
        20
    );
    println!("\nSpearman(full, 1/8) sur {} images : {rho:.4}", rf.len());
}
