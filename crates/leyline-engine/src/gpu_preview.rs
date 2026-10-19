//! The preview on the GPU, at the stage cache's seam (ADR 0156 §4).
//!
//! The CPU runs the plan up to the deepest valid checkpoint, as it always
//! did. If **every** stage after it has a preview kernel, this module runs
//! them on a GPU-resident copy of that checkpoint and returns the 8-bit
//! frame; otherwise it answers `None` and the CPU carries on, unchanged.
//! Nothing here is ever used by an export, a print or the golden manifest.

use std::sync::{Arc, OnceLock};

use leyline_gpu::{DisplayOp, Frame, Gpu, Op, Transfer};

use crate::pixels::Pixels;
use leyline_core::Settings;

use crate::stages::{Stage, Version};

/// The device, acquired once per process on first use. `LEYLINE_GPU=off`
/// keeps the preview on the CPU, for a comparison or a driver that
/// misbehaves; `LEYLINE_GPU=any` accepts a software adapter, to exercise
/// the GPU path on a machine that has none (it is slower than the CPU
/// there, which is why nothing else accepts one). Nothing else reads it.
fn gpu() -> Option<&'static Gpu> {
    static GPU: OnceLock<Option<Gpu>> = OnceLock::new();
    GPU.get_or_init(|| match std::env::var("LEYLINE_GPU").as_deref() {
        Ok("off") => None,
        Ok("any") => Gpu::acquire_any(),
        // The engine's own tests run on whatever adapter the machine has,
        // lavapipe included; a user's preview never runs on a software one.
        _ if cfg!(test) => Gpu::acquire_any(),
        _ => Gpu::acquire(),
    })
    .as_ref()
}

/// What the preview's renders run on, for *About* (ADR 0156 §2): the
/// adapter, or `None` for the CPU.
pub(crate) fn preview_backend() -> Option<String> {
    gpu().map(Gpu::describe)
}

/// The preview kernel of one stage version, built from the settings it
/// renders (ADR 0156 §3) — a table beside `STAGES`, keyed by name and
/// version like `INPUT_DECODE`, so a frozen `Version` is never edited to
/// gain one. `scale` is the render's, as `StageContext::scale`. `None`
/// means the GPU cannot run this version.
fn kernel(stage: &Stage, version: &Version, settings: &Settings, scale: f32) -> Option<Op> {
    match (stage.name, version.version) {
        ("gains", 1) => Some(Op::Gains(crate::stages::gains::v1::channel_gains(
            settings.white_balance.as_ref(),
            settings.exposure,
        ))),
        ("contrast", 1) => display(DisplayOp::Contrast {
            k: f32::from(settings.contrast as i16) / 100.0,
        }),
        ("highlights_shadows", 1) => display(DisplayOp::HighlightsShadows {
            highlights: f32::from(settings.highlights as i16) / 100.0,
            shadows: f32::from(settings.shadows as i16) / 100.0,
        }),
        // The same arithmetic as `whites_blacks` v1, in the same `f32`.
        ("whites_blacks", 1) => {
            let white = 1.0 - f32::from(settings.whites as i16) / 100.0 * 0.25;
            let black = -f32::from(settings.blacks as i16) / 100.0 * 0.25;
            display(DisplayOp::WhitesBlacks {
                black,
                scale: 1.0 / (white - black),
            })
        }
        ("parametric_curve", 1) => display(DisplayOp::Curve(Arc::new(
            crate::stages::tone_curve::v1::build_curve_lut(
                &crate::stages::parametric_curve::v1::control_points(&settings.parametric_curve),
            )
            .to_vec(),
        ))),
        ("levels", 1) => {
            let levels = &settings.levels;
            display(DisplayOp::Levels {
                in_black: levels.input_black as f32 / 255.0,
                in_range: (levels.input_white - levels.input_black) as f32 / 255.0,
                out_black: levels.output_black as f32 / 255.0,
                out_range: (levels.output_white - levels.output_black) as f32 / 255.0,
                exponent: (1.0 / levels.gamma) as f32,
            })
        }
        // The master curve alone: v2 with channel curves stays on the CPU.
        ("tone_curve", 1 | 2) if !settings.tone_curve.has_channel_curves() => {
            display(DisplayOp::Curve(Arc::new(
                crate::stages::tone_curve::v1::build_curve_lut(&settings.tone_curve.points)
                    .to_vec(),
            )))
        }
        ("vibrance", 1) => display(DisplayOp::Saturate {
            k: f32::from(settings.vibrance as i16) / 100.0,
            vibrance: true,
        }),
        ("saturation", 1) => display(DisplayOp::Saturate {
            k: f32::from(settings.saturation as i16) / 100.0,
            vibrance: false,
        }),
        ("hsl", 1) => display(DisplayOp::Hsl {
            bands: settings.hsl.map(|band| {
                [
                    f32::from(band.hue as i16),
                    f32::from(band.saturation as i16),
                    f32::from(band.luminance as i16),
                ]
            }),
        }),
        ("monochrome", 1) => display(DisplayOp::Monochrome),
        ("color_grading", 1) => {
            let grading = &settings.color_grading;
            let zones = [&grading.shadows, &grading.midtones, &grading.highlights];
            display(DisplayOp::ColorGrading {
                tints: zones.map(crate::stages::color_grading::v1::zone_tint),
                luminance: zones.map(|zone| f32::from(zone.luminance as i16) / 100.0),
                balance: f32::from(grading.balance as i16) / 100.0,
                blending: f32::from(grading.blending as i16) / 100.0,
            })
        }
        // The same σ as both versions, from the same weights; v2 at
        // masking 0 renders exactly v1, so it skips the mask here too.
        ("sharpen", 1 | 2) => {
            let sharpening = &settings.sharpening;
            let sigma = (sharpening.radius * f64::from(scale)) as f32;
            let masking = if version.version == 2 {
                sharpening.masking
            } else {
                0
            };
            let edge = (masking > 0).then(|| {
                let threshold = f32::from(masking as i16) / 100.0 * 0.25;
                [threshold, (threshold * 0.5).max(1e-4)]
            });
            Some(Op::Sharpen {
                k: f32::from(sharpening.amount as i16) / 100.0,
                weights: Arc::new(if sigma <= 0.0 {
                    vec![1.0]
                } else {
                    crate::stages::kernel::v1::gaussian_weights(sigma)
                }),
                edge,
                transfer: transfer(),
            })
        }
        // The same derivations as `vignette` v1, in the same `f32`.
        ("vignette", 1) => {
            let vignette = &settings.vignette;
            let midpoint = (vignette.midpoint as f32 / 100.0).clamp(0.0, 1.0);
            let exponent = (2.0 * 3.0f32.powf(vignette.roundness as f32 / 100.0)).clamp(1.0, 6.0);
            Some(Op::Vignette {
                amount: vignette.amount as f32 / 100.0,
                midpoint,
                exponent,
                corner: 2.0f32.powf(1.0 / exponent),
                width: ((vignette.feather as f32 / 100.0) * (1.0 - midpoint)).max(1e-4),
            })
        }
        ("crop", 1) => settings.crop.as_ref().map(|rect| Op::Crop {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        }),
        ("output_rendering", 1) => Some(Op::OutputRendering {
            knee: crate::stages::output_rendering::v1::knee(
                settings.output_rendering.highlight_rolloff,
            ),
            matrix: leyline_color::REC2020_TO_LINEAR_SRGB.map(|row| row.map(|v| v as f32)),
            display_table: display_table(),
        }),
        _ => None,
    }
}

/// A display-axis operation with the CPU's transfer tables.
fn display(op: DisplayOp) -> Option<Op> {
    Some(Op::Display(op, transfer()))
}

/// The CPU's transfer tables, shared with every frame.
fn transfer() -> Transfer {
    static TRANSFER: OnceLock<Transfer> = OnceLock::new();
    TRANSFER
        .get_or_init(|| {
            let (to_linear, to_display) = crate::stages::kernel::v1::tables();
            Transfer {
                to_linear: Arc::new(to_linear.to_vec()),
                to_display: Arc::new(to_display.to_vec()),
            }
        })
        .clone()
}

/// The CPU's display-curve table, shared with every frame.
fn display_table() -> Arc<Vec<f32>> {
    static TABLE: OnceLock<Arc<Vec<f32>>> = OnceLock::new();
    Arc::clone(TABLE.get_or_init(|| Arc::new(crate::stages::kernel::v1::tables().1.to_vec())))
}

/// Whether the GPU could run every stage of `rest` for `settings`
/// (ADR 0166 §3), with a device to run them on.
pub(crate) fn covers(
    rest: &[(&'static Stage, &'static Version)],
    settings: &Settings,
    scale: f32,
) -> bool {
    gpu().is_some()
        && rest
            .iter()
            .all(|(stage, version)| kernel(stage, version, settings, scale).is_some())
}

/// The resident copy of one checkpoint, and which checkpoint it is.
#[derive(Debug)]
pub(crate) struct ResidentFrame {
    key: (usize, u64),
    frame: Frame,
}

/// Renders `rest` — the stages after a checkpoint of `applied` stages whose
/// prefix fingerprint is `fingerprint` — on the GPU, from `pixels`. `None`
/// when there is no device, when one of those stages has no kernel, or when
/// the device fails: the caller then renders on the CPU (ADR 0156 §2).
pub(crate) fn render(
    resident: &mut Option<ResidentFrame>,
    rest: &[(&'static Stage, &'static Version)],
    settings: &Settings,
    scale: f32,
    applied: usize,
    fingerprint: u64,
    pixels: &Pixels,
) -> Option<(u32, u32, Vec<u8>)> {
    let gpu = gpu()?;
    let ops = rest
        .iter()
        .map(|(stage, version)| kernel(stage, version, settings, scale))
        .collect::<Option<Vec<Op>>>()?;
    let key = (applied, fingerprint);
    if resident.as_ref().is_none_or(|r| r.key != key) {
        *resident = None;
        let frame = Frame::upload(gpu, pixels.width, pixels.height, &pixels.data)?;
        *resident = Some(ResidentFrame { key, frame });
    }
    let frame = &resident.as_ref()?.frame;
    let rgb = frame.render(gpu, &ops)?;
    #[cfg(test)]
    tests::RENDERED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    Some(rgb)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::atomic::AtomicUsize;

    /// Frames this process rendered on the GPU, so a test can tell the
    /// path it took from the pixels it got.
    pub(crate) static RENDERED: AtomicUsize = AtomicUsize::new(0);
}
