//! Golden renders — the standing proof of the promise `docs/pipeline.md`
//! §5.1 makes: *this RAW, these settings, these pixels, in ten years*.
//!
//! Each entry pins the BLAKE3 digest of one render's RGB8 output, plus its
//! dimensions and a few sampled pixels so a failure says something more
//! useful than "the hash moved". Every case is deterministic: synthetic
//! images, fixed settings, no clock, no file system, no thread-count
//! dependence (the pipeline's parallelism is over disjoint rows).
//!
//! # What an entry is pinned *to*
//!
//! An entry records the `stages` map it rendered through, and the replay
//! feeds that exact map back to [`render`]. This is what makes the fixture a
//! freeze rather than a snapshot: the day a `sharpen::v2` ships, the entries
//! citing `sharpen::v1` keep rendering v1 and must stay bit-identical —
//! precisely the promise. Nothing about a new version can move them, so
//! nothing about a new version can pressure anyone into re-blessing.
//!
//! What a new version *does* trigger is a missing-coverage failure: the
//! versions this engine would pin today ([`super::pin`]) must themselves
//! appear in the manifest, and so must every published `(stage, version)`
//! pair in the registry. Blessing then *adds* the new entries:
//!
//! ```text
//! LEYLINE_BLESS_GOLDEN=1 cargo test -p leyline-engine --lib golden
//! ```
//!
//! Blessing is additive by construction: it never rewrites an entry it
//! finds. These fixtures were what proved the ADR 0042 migration pixel-exact
//! across the eleven `processN.rs` modules it replaced.
//!
//! The manifest has been regenerated from scratch exactly twice, each time
//! under a decision that said so in as many words, and never otherwise:
//! ADR 0043, which collapsed the pre-publication rendering history, and
//! ADR 0044, which moved the working space to linear Rec. 2020 and
//! therefore moved every pixel in the pipeline. Both were possible only
//! because nothing had been published — no revision in the world cited the
//! renderings they replaced. After the first release neither would be, and
//! a moved digest is simply a defect. The way to change one on purpose is
//! to ship a new stage version, which *adds* entries and leaves the
//! existing ones exactly where they are.
//!
//! For the same reason the settings fragments below are frozen too: an entry
//! can only be replayed if its case still means what it meant. Exercising an
//! operator differently is a *new* case, never an edit of one that ships.
//!
//! # What these entries do *not* prove
//!
//! That the rendering is good. A digest freezes a decision; it cannot make
//! one. The evidence that ADR 0044's new working space renders *better* —
//! recovered window highlights, colors that survive a saturation slider —
//! is elsewhere, in renders of real photographs compared side by side
//! (ADR 0044 §7.2). What lives here is the promise that whatever was
//! decided then stays decided.

use std::collections::BTreeMap;
use std::io::Cursor;

use leyline_color::DcpProfile;
use leyline_core::{
    BrushStroke, CameraProfile, ColorGrading, ColorGradingZone, ColorRange, Crop, CurvePoint,
    Grain, HslBand, LensCorrection, LocalAdjustment, LocalAdjustmentValues, LuminanceRange, Mask,
    NoiseReduction, Point, RangeMask, Settings, Sharpening, SpotRemoval, StageVersions, ToneCurve,
    Vignette, WhiteBalance,
};
use leyline_raw::RawImage;
use serde::{Deserialize, Serialize};

use super::{SourceColor, fixture, pin, registry};
use crate::render::{LensShot, SensorShot, render};

/// Path of the committed manifest, relative to the crate root.
const MANIFEST: &str = "tests/golden/renders.json";

/// One pinned render.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Golden {
    /// The stage versions this render went through — and the ones it is
    /// replayed through forever after.
    stages: StageVersions,
    width: u32,
    height: u32,
    /// BLAKE3 of the RGB8 output — the bit-identity assertion itself.
    digest: String,
    /// A handful of evenly-spaced RGB samples. They prove nothing the
    /// digest doesn't, but when a digest moves they say *how* it moved.
    samples: Vec<[u8; 3]>,
}

/// The manifest: every pinned variant of every case. A case gets a second
/// entry the day one of the operators it exercises gets a second version.
type Manifest = BTreeMap<String, Vec<Golden>>;

/// Deterministic stand-in for a decoded RAW: smooth ramps so tone
/// operators have something continuous to act on, a per-pixel hash so the
/// blurs and the dark-channel filter have real high-frequency content, and
/// a saturated block so the color operators are not exercised on grey.
fn synthetic_image(width: u32, height: u32) -> RawImage {
    let mut data = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            if (width / 4..width / 2).contains(&x) && (height / 4..height / 2).contains(&y) {
                data.extend_from_slice(&[210, 70, 45]);
                continue;
            }
            let noise = (x.wrapping_mul(31).wrapping_add(y.wrapping_mul(17)) % 32) as u8;
            data.push((x * 255 / width) as u8 ^ noise);
            data.push((y * 255 / height) as u8);
            data.push((((x + y) * 255) / (width + height)) as u8 ^ noise);
        }
    }
    RawImage {
        width,
        height,
        bits: 8,
        data,
    }
}

/// A real bundled Lensfun profile with distortion, vignetting and TCA data
/// at 20mm — the same fixture `leyline-lens` and the lens stage unit
/// tests use, so the golden exercises the actual profile lookup rather
/// than a synthetic stand-in.
fn canon_shot() -> LensShot {
    LensShot {
        camera_make: "Canon".to_owned(),
        camera_model: "Canon EOS 5D Mark III".to_owned(),
        lens_make: Some("Canon".to_owned()),
        lens_model: Some("Canon EF 16-35mm f/2.8L II USM".to_owned()),
        focal_mm: 20.0,
        aperture_f: Some(2.8),
    }
}

/// A body the frozen noise profile table really knows, at a sensitivity it
/// really measured (ADR 0072 §6) — so the golden exercises the lookup, the
/// interpolation and the transport rather than the fallback that stands in
/// for all three when nothing matches.
fn canon_sensor() -> SensorShot {
    SensorShot {
        camera_make: "Canon".to_owned(),
        camera_model: "EOS 5D Mark III".to_owned(),
        iso: 3200.0,
    }
}

/// One frozen case: the settings, and the facts about the file that are not
/// settings. The sensor belongs here rather than in [`capture`] because two
/// cases differ by it alone — a profiled render and its fallback — and a
/// case is only replayable if it still means what it meant (ADR 0072 §7).
#[derive(Debug, Clone)]
struct Case {
    settings: Settings,
    sensor: Option<SensorShot>,
}

/// A minimal in-memory DCP carrying an identity `ColorMatrix1`. The matrix
/// values do not matter to the freeze — only that the stage runs and that
/// it keeps producing the same pixels through it.
fn sample_profile() -> DcpProfile {
    use tiff::encoder::{TiffEncoder, colortype::Gray8};
    use tiff::tags::Tag;

    /// DNG `ColorMatrix1`.
    const COLOR_MATRIX_1: u16 = 50721;
    const IDENTITY: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    let mut buffer = Cursor::new(Vec::new());
    {
        let mut encoder = TiffEncoder::new(&mut buffer).unwrap();
        let mut image = encoder.new_image::<Gray8>(1, 1).unwrap();
        let values: Vec<tiff::encoder::SRational> = IDENTITY
            .into_iter()
            .flatten()
            .map(|v| tiff::encoder::SRational {
                n: (v * 10000.0).round() as i32,
                d: 10000,
            })
            .collect();
        image
            .encoder()
            .write_tag(Tag::Unknown(COLOR_MATRIX_1), values.as_slice())
            .unwrap();
        image.write_data(&[0u8]).unwrap();
    }
    DcpProfile::parse(&buffer.into_inner()).unwrap()
}

// ---------------------------------------------------------------------------
// Settings fragments, each exercising one operator family
// ---------------------------------------------------------------------------

fn white_balance() -> Option<WhiteBalance> {
    Some(WhiteBalance {
        temperature: 5200,
        tint: 8,
    })
}

fn tone(settings: Settings) -> Settings {
    Settings {
        white_balance: white_balance(),
        exposure: 0.7,
        contrast: 25,
        highlights: -40,
        shadows: 35,
        whites: 10,
        blacks: -10,
        ..settings
    }
}

fn color(settings: Settings) -> Settings {
    Settings {
        vibrance: 30,
        saturation: 12,
        ..settings
    }
}

fn detail(settings: Settings) -> Settings {
    Settings {
        noise_reduction: NoiseReduction {
            luminance: 40,
            color: 30,
        },
        sharpening: Sharpening {
            amount: 60,
            radius: 1.2,
            masking: 0,
        },
        ..settings
    }
}

/// The edge mask of ADR 0096 — a **new** case rather than an edit of
/// `detail`, whose fragment is frozen with the entry that cites it.
fn detail_masking(settings: Settings) -> Settings {
    Settings {
        sharpening: Sharpening {
            amount: 60,
            radius: 1.2,
            masking: 55,
        },
        ..settings
    }
}

fn geometry(settings: Settings) -> Settings {
    Settings {
        rotation: 2.0,
        crop: Some(Crop {
            x: 0.1,
            y: 0.1,
            width: 0.8,
            height: 0.8,
        }),
        ..settings
    }
}

fn lens(settings: Settings) -> Settings {
    Settings {
        lens_correction: LensCorrection {
            enabled: true,
            profile: "auto".to_owned(),
        },
        ..settings
    }
}

/// The camera profile stage needs two things: a resolved `DcpProfile` —
/// every case is rendered with [`sample_profile`] — and a revision that
/// says it wants one. Path and checksum only have to satisfy
/// `Settings::validate`; the stage itself reads neither.
fn camera_profile(settings: Settings) -> Settings {
    Settings {
        camera_profile: Some(CameraProfile {
            enabled: true,
            path: "Profiles/Camera/sample.dcp".to_owned(),
            checksum: format!("blake3:{}", "0".repeat(64)),
        }),
        ..settings
    }
}

fn tone_curve(settings: Settings) -> Settings {
    Settings {
        tone_curve: ToneCurve {
            points: vec![
                CurvePoint { x: 0.0, y: 0.03 },
                CurvePoint { x: 0.3, y: 0.22 },
                CurvePoint { x: 0.7, y: 0.79 },
                CurvePoint { x: 1.0, y: 0.97 },
            ],
        },
        ..settings
    }
}

fn spots(settings: Settings) -> Settings {
    Settings {
        spot_removal: vec![
            SpotRemoval {
                target: Point { x: 0.3, y: 0.4 },
                source: Point { x: 0.6, y: 0.7 },
                radius: 0.08,
                feather: 0.5,
                opacity: 0.9,
            },
            SpotRemoval {
                target: Point { x: 0.7, y: 0.2 },
                source: Point { x: 0.4, y: 0.5 },
                radius: 0.05,
                feather: 0.2,
                opacity: 1.0,
            },
        ],
        ..settings
    }
}

fn local_values() -> LocalAdjustmentValues {
    LocalAdjustmentValues {
        temperature: Some(6500),
        tint: Some(5),
        exposure: Some(0.5),
        contrast: Some(20),
        highlights: Some(-20),
        shadows: Some(20),
        whites: Some(5),
        blacks: Some(-5),
        vibrance: Some(15),
        saturation: Some(10),
    }
}

/// A range-refined local adjustment (ADR 0048): one entry with both terms, so
/// the luminance band, the hue band and the saturation weight are all frozen
/// together. A separate case from `locals` rather than a change to it — an
/// entry that ships is never edited (see the module docs).
fn locals_range(settings: Settings) -> Settings {
    Settings {
        local_adjustments: vec![LocalAdjustment {
            mask: Mask::Everything,
            range: Some(RangeMask {
                luminance: Some(LuminanceRange {
                    min: 0.25,
                    max: 0.75,
                    softness: 0.15,
                }),
                color: Some(ColorRange {
                    center: 210.0,
                    width: 40.0,
                    softness: 20.0,
                }),
            }),
            opacity: 0.85,
            adjustments: local_values(),
        }],
        ..settings
    }
}

fn locals(settings: Settings) -> Settings {
    Settings {
        local_adjustments: vec![
            LocalAdjustment {
                mask: Mask::Radial {
                    cx: 0.45,
                    cy: 0.5,
                    rx: 0.3,
                    ry: 0.25,
                    angle: 15.0,
                    feather: 0.5,
                    inverted: false,
                },
                range: None,
                opacity: 0.9,
                adjustments: local_values(),
            },
            LocalAdjustment {
                mask: Mask::Gradient {
                    x0: 0.0,
                    y0: 0.0,
                    x1: 0.0,
                    y1: 0.6,
                },
                range: None,
                opacity: 0.7,
                adjustments: local_values(),
            },
            LocalAdjustment {
                mask: Mask::Brush {
                    strokes: (0..12)
                        .map(|i| BrushStroke {
                            x: 0.2 + f64::from(i) / 30.0,
                            y: 0.55,
                            radius: 0.06,
                            flow: 0.6,
                            hardness: 0.4,
                        })
                        .collect(),
                },
                range: None,
                opacity: 1.0,
                adjustments: local_values(),
            },
        ],
        ..settings
    }
}

/// Black and white (ADR 0088 §3), laid over the HSL mixer on purpose: what
/// this case freezes is not "the image went grey" — that is one line — but
/// the fact that the collapse happens **after** the mixer, so the mixer's
/// eight luminance sliders are what decides which colour becomes which
/// grey. A `monochrome` that ever moved before `hsl` would change these
/// pixels and be caught here.
fn monochrome(settings: Settings) -> Settings {
    Settings {
        monochrome: true,
        ..hsl_grading(settings)
    }
}

fn hsl_grading(settings: Settings) -> Settings {
    Settings {
        hsl: [
            HslBand {
                hue: 10,
                saturation: 20,
                luminance: -10,
            },
            HslBand {
                hue: -15,
                saturation: 30,
                luminance: 5,
            },
            HslBand {
                hue: 5,
                saturation: -20,
                luminance: 15,
            },
            HslBand {
                hue: 20,
                saturation: 10,
                luminance: -5,
            },
            HslBand {
                hue: -10,
                saturation: 25,
                luminance: 0,
            },
            HslBand {
                hue: 30,
                saturation: -15,
                luminance: 10,
            },
            HslBand {
                hue: 0,
                saturation: 40,
                luminance: -20,
            },
            HslBand {
                hue: -25,
                saturation: 5,
                luminance: 8,
            },
        ],
        color_grading: ColorGrading {
            shadows: ColorGradingZone {
                hue: 220,
                saturation: 30,
                luminance: -5,
            },
            midtones: ColorGradingZone {
                hue: 40,
                saturation: 15,
                luminance: 0,
            },
            highlights: ColorGradingZone {
                hue: 55,
                saturation: 25,
                luminance: 5,
            },
            balance: 10,
            blending: 50,
        },
        ..settings
    }
}

/// The vignette (ADR 0090 §2), captured **over a crop** on purpose: what
/// this case freezes is not "the corners went dark" but the §1 claim — the
/// vignette is centred on the *cropped* frame. A `vignette` that ever ran
/// before `crop::v1` would draw on another rectangle and move these pixels.
fn vignette(settings: Settings) -> Settings {
    Settings {
        vignette: Vignette {
            amount: -60,
            midpoint: 40,
            roundness: 20,
            feather: 60,
        },
        ..geometry(settings)
    }
}

/// Grain (ADR 0090 §3). This digest *is* the determinism assertion: the
/// field is a pure function of the pixel's full-resolution coordinates, so
/// it must come back identical on every platform, at every thread count,
/// forever. The day anything seeds it — a clock, a row id, a generator —
/// this case is what fails.
fn grain(settings: Settings) -> Settings {
    Settings {
        grain: Grain {
            amount: 70,
            size: 30,
            roughness: 60,
        },
        ..settings
    }
}

fn presence(settings: Settings) -> Settings {
    Settings {
        clarity: 40,
        texture: 30,
        dehaze: 35,
        ..settings
    }
}

/// A fixed 2×2×2 look, frozen with the cases exactly like the made-up camera
/// matrix above: what the freeze needs is that the same table enters the
/// pipeline every time, not that anyone would grade with it. It swaps green
/// into red and warms the whites, so a render through it is unmistakably
/// different from one without.
fn sample_lut() -> leyline_color::CubeLut {
    leyline_color::CubeLut::parse(
        "LUT_3D_SIZE 2\n\
         0.00 0.00 0.00\n\
         0.00 0.60 0.10\n\
         0.90 0.00 0.10\n\
         0.90 0.60 0.20\n\
         0.00 0.10 0.80\n\
         0.10 0.60 0.90\n\
         0.90 0.10 0.90\n\
         1.00 0.95 0.85\n",
    )
    .expect("the sample LUT is well formed")
}

/// Applies that look at three quarters strength (ADR 0053) — the reference the
/// LUT stage is frozen against.
fn lut(settings: Settings) -> Settings {
    Settings {
        lut: Some(leyline_core::Lut {
            enabled: true,
            // The stage receives an already-resolved table, so this reference
            // only has to be a *valid* one; the bytes it names are never read
            // here (`crate::lut::resolve_from_settings` does that in the
            // library paths).
            path: "Profiles/LUT/sample.cube".to_owned(),
            checksum: format!("blake3:{}", "0".repeat(64)),
            strength: 75,
        }),
        ..settings
    }
}

/// Straightens converging verticals (ADR 0052) — its own case rather than a
/// term of `geometry`, whose entries are frozen with it.
fn perspective(settings: Settings) -> Settings {
    Settings {
        perspective: Some(leyline_core::Perspective {
            vertical: 40,
            horizontal: -15,
        }),
        ..settings
    }
}

/// Asks the decoder to reconstruct clipped highlights (ADR 0050). The mode
/// itself is a decoder configuration, which no synthetic buffer can exercise —
/// what this case freezes is the other half of `input::v2`: the gain it gives
/// back for the renormalization the decoder applied (§3).
fn highlight_reconstruction(settings: Settings) -> Settings {
    Settings {
        highlight_reconstruction: leyline_core::HighlightReconstruction::Rebuild,
        ..settings
    }
}

/// Activates the two-version fixture stage of ADR 0043 §7 at `version`.
/// These two cases are what keep the manifest carrying, at all times, a
/// stage whose older version stays pinned while a newer one exists — the
/// shape every real operator takes the day it gets a `v2`.
fn fixture_stage(settings: Settings, version: u16) -> Settings {
    let mut settings = Settings {
        exposure: 0.3,
        ..settings
    };
    settings
        .extra
        .insert(fixture::MARKER.to_owned(), serde_json::Value::Bool(true));
    settings.stages.insert(fixture::NAME.to_owned(), version);
    settings
}

// ---------------------------------------------------------------------------
// The case matrix
// ---------------------------------------------------------------------------

/// Every case: `(key, settings)`.
///
/// One `neutral` floor, one case per operator family, and an `everything`
/// case running them all together — the combination most likely to expose a
/// stage ordering mistake.
fn cases() -> Vec<(String, Case)> {
    let base = Settings::default();
    let mut named: Vec<(&str, Settings)> = vec![
        ("neutral", base.clone()),
        ("tone", tone(base.clone())),
        ("color", color(base.clone())),
        ("detail", detail(base.clone())),
        ("detail_masking", detail_masking(base.clone())),
        ("geometry", geometry(base.clone())),
        ("lens", lens(base.clone())),
        ("camera_profile", camera_profile(base.clone())),
        ("tone_curve", tone_curve(base.clone())),
        ("spots", spots(base.clone())),
        ("locals", locals(base.clone())),
        ("locals_range", locals_range(base.clone())),
        ("hsl_grading", hsl_grading(base.clone())),
        ("monochrome", monochrome(base.clone())),
        ("presence", presence(base.clone())),
        ("vignette", vignette(base.clone())),
        ("grain", grain(base.clone())),
        (
            "highlight_reconstruction",
            highlight_reconstruction(base.clone()),
        ),
        ("perspective", perspective(base.clone())),
        ("lut", lut(base.clone())),
        ("fixture_v1", fixture_stage(base.clone(), 1)),
        ("fixture_v2", fixture_stage(base.clone(), 2)),
    ];

    // The composite deliberately leaves `camera_profile` out: it is pinned
    // as it was the day it was captured, and a fragment a manifest entry
    // already cites is frozen with it. The profile stage runs first, at rank
    // 10, so its own case is where it is exercised.
    let all = presence(hsl_grading(locals(spots(tone_curve(lens(detail(color(
        tone(base.clone()),
    ))))))));
    named.push(("everything", geometry(all)));

    let mut cases: Vec<(String, Case)> = named
        .into_iter()
        .map(|(name, settings)| {
            (
                name.to_owned(),
                Case {
                    settings,
                    sensor: Some(canon_sensor()),
                },
            )
        })
        .collect();

    // The same detail settings on a body the table does not know: the
    // fallback model of ADR 0072 §7 is a rendering like any other, and a
    // rendering nothing freezes is a rendering that can move.
    cases.push((
        "detail_unprofiled".to_owned(),
        Case {
            settings: detail(base),
            sensor: None,
        },
    ));

    cases
}

/// The stage map this engine pins for `settings` today: what a revision
/// written now would record, and therefore what a new entry must cover.
fn current_stages(settings: &Settings) -> StageVersions {
    let mut settings = settings.clone();
    pin(&mut settings);
    settings.stages
}

/// Renders one case through `stages` and reduces it to its pinned form.
fn capture(case: &Case, stages: &StageVersions) -> Golden {
    // Small enough to keep the suite fast, large enough that the blur and
    // resampling stages have real neighbourhoods to work with.
    let image = synthetic_image(96, 64);
    let shot = canon_shot();
    let profile = sample_profile();
    let settings = Settings {
        stages: stages.clone(),
        ..case.settings.clone()
    };
    // A fixed, made-up camera matrix: what matters to the freeze is that the
    // colorimetry entering the pipeline is the same every time, not that it
    // belongs to a real body (ADR 0044 §3).
    let source = SourceColor::Camera {
        to_xyz: Some([
            [0.671_9, -0.099_4, -0.092_5],
            [-0.440_8, 1.242_6, 0.221_1],
            [-0.088_7, 0.212_9, 0.605_1],
        ]),
        // Fixed for the same reason as the matrix, and non-neutral so a case
        // asking for highlight reconstruction (ADR 0050) has a gain to give
        // back rather than nothing to do.
        multipliers: Some([2.0, 1.0, 1.5, 1.0]),
    };
    let look = sample_lut();
    let rendered = render(
        &image,
        &settings,
        Some(&shot),
        case.sensor.as_ref(),
        Some(&profile),
        Some(&look),
        &Default::default(),
        source,
    )
    .unwrap_or_else(|e| panic!("render failed: {e}"));

    let digest = blake3::hash(&rendered.data).to_hex().to_string();
    let step = (rendered.data.len() / 3 / 16).max(1);
    let samples = (0..16)
        .filter_map(|i| {
            let at = i * step * 3;
            rendered
                .data
                .get(at..at + 3)
                .map(|px| [px[0], px[1], px[2]])
        })
        .collect();
    Golden {
        stages: stages.clone(),
        width: rendered.width,
        height: rendered.height,
        digest,
        samples,
    }
}

fn manifest_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(MANIFEST)
}

fn read_manifest() -> Manifest {
    let path = manifest_path();
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\nrun with LEYLINE_BLESS_GOLDEN=1 to capture it",
            path.display()
        )
    });
    serde_json::from_str(&raw).unwrap()
}

/// Adds the entries this engine needs and the manifest does not have yet,
/// leaving every entry it finds exactly as it is.
fn bless() {
    let path = manifest_path();
    let mut manifest: Manifest = std::fs::read_to_string(&path)
        .ok()
        .map(|raw| serde_json::from_str(&raw).expect("the manifest is valid JSON"))
        .unwrap_or_default();

    let mut added = 0;
    for (key, case) in cases() {
        let stages = current_stages(&case.settings);
        let entries = manifest.entry(key).or_default();
        if entries.iter().any(|entry| entry.stages == stages) {
            continue;
        }
        entries.push(capture(&case, &stages));
        entries.sort_by(|a, b| a.stages.iter().cmp(b.stages.iter()));
        added += 1;
    }

    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let json = serde_json::to_string_pretty(&manifest).unwrap();
    std::fs::write(&path, format!("{json}\n")).unwrap();
    eprintln!("blessed {added} new golden renders -> {}", path.display());
}

/// The freeze itself: every entry in the manifest still renders, through the
/// stage versions it records, exactly the pixels it recorded.
///
/// **Runs on the reference platform only**, and that is not a weakening of the
/// guard — it is what the guard actually promises. `docs/pipeline.md` §5.1
/// makes bit-identity conditional on "the same platform and toolchain", and
/// §5.2 says in as many words that `powf`/`ln`/`exp` come from the system math
/// library and do not agree to the last bit between platforms. The manifest's
/// digests were blessed on Linux with the pinned toolchain; asserting them on
/// macOS asserts something the specification explicitly refuses to promise, and
/// it does fail there — on the one case that calls `powf` (clarity/texture/
/// dehaze), with every sampled pixel identical and only the digest apart.
///
/// The two guards below are pure registry checks, so they keep running
/// everywhere. And `cargo test -- --ignored` still runs this one on any
/// platform, for whoever wants to *measure* the drift rather than trip over it.
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "golden digests are pinned to the reference platform (docs/pipeline.md §5.2)"
)]
#[test]
fn every_pinned_render_is_still_bit_identical() {
    if std::env::var_os("LEYLINE_BLESS_GOLDEN").is_some() {
        bless();
        return;
    }

    let expected = read_manifest();
    let cases: BTreeMap<String, Case> = cases().into_iter().collect();

    let mut drifted = Vec::new();
    let mut pinned = 0;
    for (key, entries) in &expected {
        let Some(case) = cases.get(key) else {
            drifted.push(format!("{key}: case disappeared"));
            continue;
        };
        for want in entries {
            pinned += 1;
            let got = capture(case, &want.stages);
            if got != *want {
                drifted.push(format!(
                    "{key} {:?}: {}x{} {} -> {}x{} {}\n    expected samples {:?}\n    \
                     actual   samples {:?}",
                    want.stages,
                    want.width,
                    want.height,
                    &want.digest[..16],
                    got.width,
                    got.height,
                    &got.digest[..16],
                    &want.samples[..4.min(want.samples.len())],
                    &got.samples[..4.min(got.samples.len())],
                ));
            }
        }
    }

    assert!(
        drifted.is_empty(),
        "{} of {pinned} golden renders drifted — a revision somewhere now renders \
         differently (docs/pipeline.md §5.1):\n  {}",
        drifted.len(),
        drifted.join("\n  ")
    );
}

/// The coverage half: what this engine renders *today* is pinned too.
///
/// A case whose current stage map has no entry is a rendering nobody has
/// frozen — either a new case, or an operator that just got a new version
/// and now runs unpinned. Blessing adds that entry without disturbing the
/// older ones, which is the whole difference with re-blessing.
#[test]
fn every_case_pins_the_versions_this_engine_renders_today() {
    if std::env::var_os("LEYLINE_BLESS_GOLDEN").is_some() {
        return;
    }

    let expected = read_manifest();
    let missing: Vec<String> = cases()
        .into_iter()
        .filter_map(|(key, case)| {
            let stages = current_stages(&case.settings);
            let pinned = expected
                .get(&key)
                .is_some_and(|entries| entries.iter().any(|entry| entry.stages == stages));
            (!pinned).then(|| format!("{key}: {stages:?}"))
        })
        .collect();

    assert!(
        missing.is_empty(),
        "{} case(s) render through versions no golden pins — bless to add them \
         (LEYLINE_BLESS_GOLDEN=1), which leaves the existing entries untouched:\n  {}",
        missing.len(),
        missing.join("\n  ")
    );
}

/// No published `(stage, version)` pair may sit outside the manifest.
///
/// The two tests above pin what the cases exercise; this one notices a
/// version that shipped without any case reaching it, since a stage nothing
/// renders is a stage nothing freezes.
#[test]
fn every_published_stage_version_is_pinned_by_some_case() {
    if std::env::var_os("LEYLINE_BLESS_GOLDEN").is_some() {
        return;
    }

    let expected = read_manifest();
    let mut unpinned = Vec::new();
    for stage in registry() {
        for version in stage.versions {
            let covered = expected
                .values()
                .flatten()
                .any(|entry| entry.stages.get(stage.name) == Some(&version.version));
            if !covered {
                unpinned.push(format!("{}::v{}", stage.name, version.version));
            }
        }
    }

    assert!(
        unpinned.is_empty(),
        "{} published stage version(s) are frozen by nothing — add a case to \
         `cases()` that activates them:\n  {}",
        unpinned.len(),
        unpinned.join("\n  ")
    );
}
