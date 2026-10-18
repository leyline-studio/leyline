//! The preview on the GPU, at the stage cache's seam (ADR 0156 §4).
//!
//! The CPU runs the plan up to the deepest valid checkpoint, as it always
//! did. If **every** stage after it has a preview kernel, this module runs
//! them on a GPU-resident copy of that checkpoint and returns the 8-bit
//! frame; otherwise it answers `None` and the CPU carries on, unchanged.
//! Nothing here is ever used by an export, a print or the golden manifest.

use std::sync::{Arc, OnceLock};

use leyline_gpu::{Frame, Gpu, Op};

use crate::pixels::Pixels;
use crate::stages::{Context, Stage, Version};

/// The device, acquired once per process on first use. `LEYLINE_GPU=off`
/// keeps the preview on the CPU, for a comparison or a driver that
/// misbehaves; nothing else reads it.
fn gpu() -> Option<&'static Gpu> {
    static GPU: OnceLock<Option<Gpu>> = OnceLock::new();
    GPU.get_or_init(|| {
        if std::env::var("LEYLINE_GPU").is_ok_and(|v| v == "off") {
            return None;
        }
        // The engine's own tests run on whatever adapter the machine has,
        // lavapipe included; a user's preview never runs on a software one.
        if cfg!(test) {
            Gpu::acquire_any()
        } else {
            Gpu::acquire()
        }
    })
    .as_ref()
}

/// What the preview's renders run on, for *About* (ADR 0156 §2): the
/// adapter, or `None` for the CPU.
pub fn preview_backend() -> Option<String> {
    gpu().map(Gpu::describe)
}

/// The preview kernel of one stage version, built from the settings it
/// renders (ADR 0156 §3) — a table beside `STAGES`, keyed by name and
/// version like `INPUT_DECODE`, so a frozen `Version` is never edited to
/// gain one. `None` means the GPU cannot run this version.
fn kernel(stage: &Stage, version: &Version, ctx: &Context) -> Option<Op> {
    match (stage.name, version.version) {
        ("gains", 1) => Some(Op::Gains(crate::stages::gains::v1::channel_gains(
            ctx.settings.white_balance.as_ref(),
            ctx.settings.exposure,
        ))),
        ("output_rendering", 1) => Some(Op::OutputRendering {
            knee: crate::stages::output_rendering::v1::knee(
                ctx.settings.output_rendering.highlight_rolloff,
            ),
            matrix: leyline_color::REC2020_TO_LINEAR_SRGB.map(|row| row.map(|v| v as f32)),
            display_table: display_table(),
        }),
        _ => None,
    }
}

/// The CPU's display-curve table, shared with every frame.
fn display_table() -> Arc<Vec<f32>> {
    static TABLE: OnceLock<Arc<Vec<f32>>> = OnceLock::new();
    Arc::clone(TABLE.get_or_init(|| Arc::new(crate::stages::kernel::v1::tables().1.to_vec())))
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
    ctx: &Context,
    applied: usize,
    fingerprint: u64,
    pixels: &Pixels,
) -> Option<Vec<u8>> {
    let gpu = gpu()?;
    let ops = rest
        .iter()
        .map(|(stage, version)| kernel(stage, version, ctx))
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
