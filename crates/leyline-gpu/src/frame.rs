//! A frame resident on the GPU, and the operations a preview runs on it
//! (ADR 0156 §4–§5).
//!
//! The buffer the CPU hands over is the stage cache's resume point: at 4K it
//! is ~134 MB of `f32`, so it is uploaded once and kept, and each frame of a
//! drag starts from a GPU-side copy of it. The result comes back as 8-bit
//! RGB, the form every preview already has.

use std::sync::Arc;

use wgpu::util::DeviceExt;

use crate::Gpu;

/// Invocations per workgroup, along x.
const WORKGROUP: u32 = 64;
/// The most workgroups one dispatch dimension may hold (WebGPU's floor).
const MAX_GROUPS: u32 = 65_535;

/// One step of a preview on the GPU, with everything it needs already
/// computed on the CPU — the engine builds these from a stage version and
/// its settings, so the GPU never decides a value the CPU would not.
#[derive(Debug, Clone)]
pub enum Op {
    /// Every sample times its channel's gain, floored at 0: white balance
    /// and exposure (`gains` v1).
    Gains([f32; 3]),
    /// A tone operator on the display axis.
    Display(DisplayOp, Transfer),
    /// `crop` v1: the frame from here on is this rectangle, given as
    /// fractions of the frame and rounded exactly as the CPU rounds them.
    /// Applied when the output is read, so it may only be followed by
    /// [`Op::OutputRendering`].
    Crop {
        /// Left edge, as a fraction of the width.
        x: f64,
        /// Top edge, as a fraction of the height.
        y: f64,
        /// Width, as a fraction of the width.
        width: f64,
        /// Height, as a fraction of the height.
        height: f64,
    },
    /// The output rendering (`output_rendering` v1): the highlight rolloff
    /// at `knee`, the working space to linear sRGB through `matrix`, the
    /// clamp, and the display curve through `display_table` — the very
    /// table the CPU looks up, `LUT_SIZE + 1` entries over `[0, 1]`.
    OutputRendering {
        /// Where the highlight rolloff starts, in linear light.
        knee: f32,
        /// Working space (Rec. 2020) to linear sRGB, row by row.
        matrix: [[f32; 3]; 3],
        /// The CPU's display-curve table, entry `i` at `i / (len − 1)`.
        display_table: Arc<Vec<f32>>,
    },
}

/// A tone operator on the display axis: the CPU's `in_display` around a
/// per-sample function, with the CPU's own transfer tables (ADR 0156 §5).
#[derive(Debug, Clone)]
pub enum DisplayOp {
    /// `contrast` v1, at `k` = amount / 100.
    Contrast {
        /// The amount, in [-1, 1].
        k: f32,
    },
    /// `whites_blacks` v1: `(x − black) · scale`, floored at 0.
    WhitesBlacks {
        /// The display value that becomes black.
        black: f32,
        /// `1 / (white − black)`.
        scale: f32,
    },
    /// `highlights_shadows` v1, each at amount / 100.
    HighlightsShadows {
        /// The highlights amount, in [-1, 1].
        highlights: f32,
        /// The shadows amount, in [-1, 1].
        shadows: f32,
    },
    /// `levels` v1, the four limits already divided by 255.
    Levels {
        /// Input black.
        in_black: f32,
        /// Input white minus input black.
        in_range: f32,
        /// Output black.
        out_black: f32,
        /// Output white minus output black.
        out_range: f32,
        /// `1 / gamma`.
        exponent: f32,
    },
    /// A curve the CPU already tabulated (`tone_curve` v1, the parametric
    /// curve): looked up exactly as the CPU looks it up.
    Curve(Arc<Vec<f32>>),
    /// `saturation` v1 (`vibrance` false) and `vibrance` v1 (true), at
    /// `k` = amount / 100, headroom preserved as the CPU's
    /// `preserving_headroom` does.
    Saturate {
        /// The amount, in [-1, 1].
        k: f32,
        /// Weigh by how unsaturated a pixel already is.
        vibrance: bool,
    },
}

/// The CPU's transfer tables between the linear and display axes, shared
/// by every display-axis operation.
#[derive(Debug, Clone)]
pub struct Transfer {
    /// sRGB → linear over [0, 1].
    pub to_linear: Arc<Vec<f32>>,
    /// Linear → sRGB over [0, 1].
    pub to_display: Arc<Vec<f32>>,
}

/// A frame uploaded once, rendered many times.
#[derive(Debug)]
pub struct Frame {
    width: u32,
    height: u32,
    pixels: u32,
    source: wgpu::Buffer,
    work: wgpu::Buffer,
    output: wgpu::Buffer,
    readback: wgpu::Buffer,
}

impl Frame {
    /// Uploads `samples` — `width × height` RGB `f32`, the CPU's working
    /// buffer. `None` when the device refuses the allocation: the caller
    /// renders on the CPU (ADR 0156 §2).
    pub fn upload(gpu: &Gpu, width: u32, height: u32, samples: &[f32]) -> Option<Frame> {
        let pixels = width.checked_mul(height)?;
        if samples.len() != pixels as usize * 3 || pixels == 0 {
            return None;
        }
        let device = gpu.device();
        let float_bytes = u64::from(pixels) * 12;
        let source = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("frame source"),
            contents: &bytes_of_f32(samples),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        let work = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame work"),
            size: float_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let output = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame output"),
            size: u64::from(pixels) * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame readback"),
            size: u64::from(pixels) * 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Some(Frame {
            width,
            height,
            pixels,
            source,
            work,
            output,
            readback,
        })
    }

    /// The frame's size, in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Runs `ops` on a fresh copy of the uploaded buffer and reads the
    /// result back as its width, height and packed 8-bit RGB. `ops` must
    /// end with [`Op::OutputRendering`], the only step that produces 8-bit
    /// values; `None` otherwise, or when the device fails.
    pub fn render(&self, gpu: &Gpu, ops: &[Op]) -> Option<(u32, u32, Vec<u8>)> {
        let (last, steps) = ops.split_last()?;
        let Op::OutputRendering {
            knee,
            matrix,
            display_table,
        } = last
        else {
            return None;
        };
        let device = gpu.device();
        let (groups_x, groups_y) = dispatch(self.pixels);
        let stride = groups_x * WORKGROUP;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_buffer_to_buffer(&self.source, 0, &self.work, 0, self.source.size());

        // Uniforms live until the submission; collected so they outlive it.
        let mut uniforms = Vec::with_capacity(ops.len());
        // The window the output reads: the whole frame, unless cropped.
        let mut window = (0, 0, self.width, self.height);
        for (index, step) in steps.iter().enumerate() {
            if let Op::Crop {
                x,
                y,
                width,
                height,
            } = step
            {
                // Nothing but the output may follow a crop: every kernel
                // before it runs on the whole frame.
                if index + 1 != steps.len() {
                    return None;
                }
                window = crop_window(self.width, self.height, *x, *y, *width, *height)?;
                continue;
            }
            if let Op::Display(op, transfer) = step {
                let (mode, values, curve) = display_params(op);
                let mut params = Vec::with_capacity(48);
                for word in [mode, self.pixels, stride, curve_entries(curve.as_deref())?] {
                    params.extend_from_slice(&word.to_le_bytes());
                }
                for value in values {
                    params.extend_from_slice(&value.to_le_bytes());
                }
                params.resize(48, 0);
                uniforms.push(uniform(device, &params));
                let tables = [
                    table(device, &transfer.to_display),
                    table(device, &transfer.to_linear),
                    table(device, curve.as_deref().unwrap_or(&transfer.to_display)),
                ];
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("display"),
                    layout: &gpu.pipelines().display.get_bind_group_layout(0),
                    entries: &[
                        entry(0, &self.work),
                        entry(1, uniforms.last()?),
                        entry(2, &tables[0]),
                        entry(3, &tables[1]),
                        entry(4, &tables[2]),
                    ],
                });
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
                pass.set_pipeline(&gpu.pipelines().display);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups(groups_x, groups_y, 1);
                continue;
            }
            let Op::Gains(gains) = step else {
                return None;
            };
            let mut params = Vec::with_capacity(32);
            for gain in gains {
                params.extend_from_slice(&gain.to_le_bytes());
            }
            params.extend_from_slice(&self.pixels.to_le_bytes());
            params.extend_from_slice(&stride.to_le_bytes());
            params.resize(32, 0);
            uniforms.push(uniform(device, &params));
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("gains"),
                layout: &gpu.pipelines().gains.get_bind_group_layout(0),
                entries: &[entry(0, &self.work), entry(1, uniforms.last()?)],
            });
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(&gpu.pipelines().gains);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(groups_x, groups_y, 1);
        }

        let mut params = Vec::with_capacity(64);
        for row in matrix {
            for value in row {
                params.extend_from_slice(&value.to_le_bytes());
            }
            params.extend_from_slice(&0.0f32.to_le_bytes()); // a `vec4` row
        }
        let (x0, y0, out_width, out_height) = window;
        let out_pixels = out_width.checked_mul(out_height)?;
        let (out_groups_x, out_groups_y) = dispatch(out_pixels);
        let out_stride = out_groups_x * WORKGROUP;
        params.extend_from_slice(&knee.to_le_bytes());
        params.extend_from_slice(&out_pixels.to_le_bytes());
        params.extend_from_slice(&out_stride.to_le_bytes());
        let table_entries = u32::try_from(display_table.len()).ok()?;
        params.extend_from_slice(&table_entries.to_le_bytes());
        for word in [self.width, x0, y0, out_width] {
            params.extend_from_slice(&word.to_le_bytes());
        }
        uniforms.push(uniform(device, &params));
        let table = table(device, display_table);
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("output rendering"),
            layout: &gpu.pipelines().output.get_bind_group_layout(0),
            entries: &[
                entry(0, &self.work),
                entry(1, uniforms.last()?),
                entry(2, &table),
                entry(3, &self.output),
            ],
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(&gpu.pipelines().output);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(out_groups_x, out_groups_y, 1);
        }
        let out_bytes = u64::from(out_pixels) * 4;
        encoder.copy_buffer_to_buffer(&self.output, 0, &self.readback, 0, out_bytes);
        gpu.queue().submit([encoder.finish()]);

        let (sender, receiver) = std::sync::mpsc::channel();
        self.readback
            .slice(..out_bytes)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        device.poll(wgpu::PollType::Wait).ok()?;
        receiver.recv().ok()?.ok()?;
        let mapped = self.readback.slice(..out_bytes).get_mapped_range();
        let mut rgb = Vec::with_capacity(out_pixels as usize * 3);
        for packed in mapped.chunks_exact(4) {
            rgb.extend_from_slice(&packed[..3]);
        }
        drop(mapped);
        self.readback.unmap();
        Some((out_width, out_height, rgb))
    }
}

/// The compiled pipelines, built once per device.
#[derive(Debug)]
pub(crate) struct Pipelines {
    gains: wgpu::ComputePipeline,
    display: wgpu::ComputePipeline,
    output: wgpu::ComputePipeline,
}

impl Pipelines {
    pub(crate) fn new(device: &wgpu::Device) -> Pipelines {
        Pipelines {
            gains: pipeline(device, "gains", GAINS),
            display: pipeline(device, "display", DISPLAY),
            output: pipeline(device, "output rendering", OUTPUT_RENDERING),
        }
    }
}

fn pipeline(device: &wgpu::Device, label: &str, source: &str) -> wgpu::ComputePipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    })
}

/// Workgroups for `pixels` invocations, folded into two dimensions: one
/// dimension stops at 65 535 groups, which a 4K frame exceeds.
fn dispatch(pixels: u32) -> (u32, u32) {
    let groups = pixels.div_ceil(WORKGROUP);
    let x = groups.min(MAX_GROUPS);
    (x, groups.div_ceil(x))
}

fn uniform(device: &wgpu::Device, bytes: &[u8]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("params"),
        contents: bytes,
        usage: wgpu::BufferUsages::UNIFORM,
    })
}

fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

fn table(device: &wgpu::Device, values: &[f32]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("table"),
        contents: &bytes_of_f32(values),
        usage: wgpu::BufferUsages::STORAGE,
    })
}

/// `crop` v1's rectangle in pixels — the CPU's arithmetic, in the CPU's
/// `f64`, so both cut the same pixels.
fn crop_window(
    width: u32,
    height: u32,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Option<(u32, u32, u32, u32)> {
    if width == 0 || height == 0 {
        return None;
    }
    let (fw, fh) = (f64::from(width), f64::from(height));
    let x0 = ((x * fw).round() as u32).min(width - 1);
    let y0 = ((y * fh).round() as u32).min(height - 1);
    let out_w = ((w * fw).round() as u32).clamp(1, width - x0);
    let out_h = ((h * fh).round() as u32).clamp(1, height - y0);
    Some((x0, y0, out_w, out_h))
}

/// The shader's mode, its five values, and the curve table if any.
fn display_params(op: &DisplayOp) -> (u32, [f32; 5], Option<Arc<Vec<f32>>>) {
    match op {
        DisplayOp::Contrast { k } => (0, [*k, 0.0, 0.0, 0.0, 0.0], None),
        DisplayOp::WhitesBlacks { black, scale } => (1, [*black, *scale, 0.0, 0.0, 0.0], None),
        DisplayOp::HighlightsShadows {
            highlights,
            shadows,
        } => (2, [*highlights, *shadows, 0.0, 0.0, 0.0], None),
        DisplayOp::Levels {
            in_black,
            in_range,
            out_black,
            out_range,
            exponent,
        } => (
            3,
            [*in_black, *in_range, *out_black, *out_range, *exponent],
            None,
        ),
        DisplayOp::Curve(table) => (4, [0.0; 5], Some(Arc::clone(table))),
        DisplayOp::Saturate { k, vibrance } => (
            5,
            [*k, if *vibrance { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
            None,
        ),
    }
}

/// Entries of the curve table bound for this operation, the transfer table
/// standing in when there is none.
fn curve_entries(curve: Option<&Vec<f32>>) -> Option<u32> {
    u32::try_from(curve.map_or(0, Vec::len)).ok()
}

fn bytes_of_f32(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// `gains` v1: each sample times its channel's gain, floored at 0.
const GAINS: &str = r"
struct Params { gains: vec3<f32>, pixels: u32, stride: u32 }
@group(0) @binding(0) var<storage, read_write> samples: array<f32>;
@group(0) @binding(1) var<uniform> params: Params;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = id.y * params.stride + id.x;
    if (pixel >= params.pixels) { return; }
    let base = pixel * 3u;
    samples[base] = max(samples[base] * params.gains.x, 0.0);
    samples[base + 1u] = max(samples[base + 1u] * params.gains.y, 0.0);
    samples[base + 2u] = max(samples[base + 2u] * params.gains.z, 0.0);
}
";

/// `output_rendering` v1, then the 8-bit quantisation of `Pixels::to_rgb8`
/// (round half away from zero, as `f32::round` does — not WGSL's `round`,
/// which rounds half to even).
const OUTPUT_RENDERING: &str = r"
struct Params {
    row0: vec4<f32>,
    row1: vec4<f32>,
    row2: vec4<f32>,
    knee: f32,
    pixels: u32,
    stride: u32,
    table_entries: u32,
    source_width: u32,
    x0: u32,
    y0: u32,
    out_width: u32,
}
@group(0) @binding(0) var<storage, read_write> samples: array<f32>;
@group(0) @binding(1) var<uniform> params: Params;
@group(0) @binding(2) var<storage, read> display_table: array<f32>;
@group(0) @binding(3) var<storage, read_write> packed: array<u32>;

fn rolloff(value: f32, knee: f32) -> f32 {
    if (value <= knee || knee >= 1.0) { return min(value, 1.0); }
    let white = 2.0 - knee;
    if (value >= white) { return 1.0; }
    let t = (value - knee) / (white - knee);
    let eased = 1.0 - (1.0 - t) * (1.0 - t);
    return knee + (1.0 - knee) * eased;
}

fn display(value: f32) -> f32 {
    let size = params.table_entries - 1u;
    let x = clamp(value, 0.0, 1.0) * f32(size);
    let i = min(u32(x), size - 1u);
    let t = x - f32(i);
    return display_table[i] + (display_table[i + 1u] - display_table[i]) * t;
}

fn quantize(value: f32) -> u32 {
    return u32(floor(clamp(value, 0.0, 1.0) * 255.0 + 0.5));
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = id.y * params.stride + id.x;
    if (pixel >= params.pixels) { return; }
    // The crop's window into the frame: row and column of the output pixel,
    // offset into the source.
    let source = (params.y0 + pixel / params.out_width) * params.source_width
        + params.x0 + pixel % params.out_width;
    let base = source * 3u;
    let rolled = vec3<f32>(
        rolloff(samples[base], params.knee),
        rolloff(samples[base + 1u], params.knee),
        rolloff(samples[base + 2u], params.knee),
    );
    let r4 = vec4<f32>(rolled, 0.0);
    let srgb = vec3<f32>(dot(params.row0, r4), dot(params.row1, r4), dot(params.row2, r4));
    let r = quantize(display(clamp(srgb.x, 0.0, 1.0)));
    let g = quantize(display(clamp(srgb.y, 0.0, 1.0)));
    let b = quantize(display(clamp(srgb.z, 0.0, 1.0)));
    packed[pixel] = r | (g << 8u) | (b << 16u);
}
";

/// The display-axis operators: `in_display` (to the display axis, the
/// operator, back to linear from a value floored at 0), with the CPU's
/// tables below 1 and the exact sRGB formulas above, as `display`/`linear`
/// do.
const DISPLAY: &str = r"
struct Params {
    mode: u32,
    pixels: u32,
    stride: u32,
    curve_entries: u32,
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
}
@group(0) @binding(0) var<storage, read_write> samples: array<f32>;
@group(0) @binding(1) var<uniform> params: Params;
@group(0) @binding(2) var<storage, read> to_display_table: array<f32>;
@group(0) @binding(3) var<storage, read> to_linear_table: array<f32>;
@group(0) @binding(4) var<storage, read> curve_table: array<f32>;

fn lookup_display(v: f32) -> f32 {
    let size = arrayLength(&to_display_table) - 1u;
    let x = clamp(v, 0.0, 1.0) * f32(size);
    let i = min(u32(x), size - 1u);
    let t = x - f32(i);
    return to_display_table[i] + (to_display_table[i + 1u] - to_display_table[i]) * t;
}

fn lookup_linear(v: f32) -> f32 {
    let size = arrayLength(&to_linear_table) - 1u;
    let x = clamp(v, 0.0, 1.0) * f32(size);
    let i = min(u32(x), size - 1u);
    let t = x - f32(i);
    return to_linear_table[i] + (to_linear_table[i + 1u] - to_linear_table[i]) * t;
}

fn lookup_curve(v: f32) -> f32 {
    let size = params.curve_entries - 1u;
    let x = clamp(v, 0.0, 1.0) * f32(size);
    let i = min(u32(x), size - 1u);
    let t = x - f32(i);
    return curve_table[i] + (curve_table[i + 1u] - curve_table[i]) * t;
}

fn to_display(v: f32) -> f32 {
    if (v <= 1.0) { return lookup_display(v); }
    if (v <= 0.0031308) { return v * 12.92; }
    return 1.055 * pow(v, 1.0 / 2.4) - 0.055;
}

fn to_linear(v: f32) -> f32 {
    if (v <= 1.0) { return lookup_linear(v); }
    if (v <= 0.04045) { return v / 12.92; }
    return pow((v + 0.055) / 1.055, 2.4);
}

// The per-sample function of the curve-like modes, defined on [0, 1].
fn curve(x: f32) -> f32 {
    switch params.mode {
        case 0u: {
            let k = params.a;
            if (k >= 0.0) {
                let s = x * x * (3.0 - 2.0 * x);
                return max((1.0 - k) * x + k * s, 0.0);
            }
            let flat = 0.25 + 0.5 * x;
            return max((1.0 + k) * x - k * flat, 0.0);
        }
        case 1u: {
            return max((x - params.a) * params.b, 0.0);
        }
        case 3u: {
            let t = clamp((x - params.a) / params.b, 0.0, 1.0);
            var shaped = t;
            if (params.e != 1.0) { shaped = pow(t, params.e); }
            return params.c + params.d * shaped;
        }
        default: {
            return lookup_curve(x);
        }
    }
}

// `display_curve`: the function below 1, its gain at white above.
fn display_curve(x: f32) -> f32 {
    if (x <= 1.0) { return curve(x); }
    return x * curve(1.0);
}

fn highlights_shadows(rgb: vec3<f32>) -> vec3<f32> {
    let l = clamp(0.2627 * rgb.x + 0.6780 * rgb.y + 0.0593 * rgb.z, 0.0, 1.0);
    let delta = 0.5 * (params.b * (1.0 - l) * (1.0 - l) + params.a * l * l);
    var out = rgb;
    for (var c = 0u; c < 3u; c = c + 1u) {
        let x = rgb[c];
        var moved: f32;
        if (x > 1.0) {
            if (delta >= 0.0) { moved = x; } else { moved = x * (1.0 + delta); }
        } else if (delta >= 0.0) {
            moved = x + delta * (1.0 - x);
        } else {
            moved = x + delta * x;
        }
        out[c] = max(moved, 0.0);
    }
    return out;
}

// `saturate` with `preserving_headroom` around it: the pixel is brought into
// the unit cube by its peak, adjusted there, and scaled back.
fn saturate(input: vec3<f32>) -> vec3<f32> {
    let peak = max(1.0, max(input.x, max(input.y, input.z)));
    var rgb = input;
    if (peak > 1.0) { rgb = rgb / peak; }
    let l = 0.2627 * rgb.x + 0.6780 * rgb.y + 0.0593 * rgb.z;
    var factor = 1.0 + params.a;
    if (params.b > 0.5) {
        let chroma = max(0.0, max(rgb.x, max(rgb.y, rgb.z))) - min(1.0, min(rgb.x, min(rgb.y, rgb.z)));
        factor = 1.0 + params.a * (1.0 - chroma);
    }
    rgb = clamp(vec3<f32>(l) + (rgb - vec3<f32>(l)) * factor, vec3<f32>(0.0), vec3<f32>(1.0));
    if (peak > 1.0) { rgb = rgb * peak; }
    return rgb;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = id.y * params.stride + id.x;
    if (pixel >= params.pixels) { return; }
    let base = pixel * 3u;
    var rgb = vec3<f32>(
        to_display(samples[base]),
        to_display(samples[base + 1u]),
        to_display(samples[base + 2u]),
    );
    if (params.mode == 2u) {
        rgb = highlights_shadows(rgb);
    } else if (params.mode == 5u) {
        rgb = saturate(rgb);
    } else {
        rgb = vec3<f32>(display_curve(rgb.x), display_curve(rgb.y), display_curve(rgb.z));
    }
    samples[base] = to_linear(max(rgb.x, 0.0));
    samples[base + 1u] = to_linear(max(rgb.y, 0.0));
    samples[base + 2u] = to_linear(max(rgb.z, 0.0));
}
";
