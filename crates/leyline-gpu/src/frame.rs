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
    /// result back as packed 8-bit RGB. `ops` must end with
    /// [`Op::OutputRendering`], the only step that produces 8-bit values;
    /// `None` otherwise, or when the device fails.
    pub fn render(&self, gpu: &Gpu, ops: &[Op]) -> Option<Vec<u8>> {
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
        for step in steps {
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
        params.extend_from_slice(&knee.to_le_bytes());
        params.extend_from_slice(&self.pixels.to_le_bytes());
        params.extend_from_slice(&stride.to_le_bytes());
        let table_entries = u32::try_from(display_table.len()).ok()?;
        params.extend_from_slice(&table_entries.to_le_bytes());
        uniforms.push(uniform(device, &params));
        let table = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("display table"),
            contents: &bytes_of_f32(display_table),
            usage: wgpu::BufferUsages::STORAGE,
        });
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
            pass.dispatch_workgroups(groups_x, groups_y, 1);
        }
        encoder.copy_buffer_to_buffer(&self.output, 0, &self.readback, 0, self.output.size());
        gpu.queue().submit([encoder.finish()]);

        let (sender, receiver) = std::sync::mpsc::channel();
        self.readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        device.poll(wgpu::PollType::Wait).ok()?;
        receiver.recv().ok()?.ok()?;
        let mapped = self.readback.slice(..).get_mapped_range();
        let mut rgb = Vec::with_capacity(self.pixels as usize * 3);
        for packed in mapped.chunks_exact(4) {
            rgb.extend_from_slice(&packed[..3]);
        }
        drop(mapped);
        self.readback.unmap();
        Some(rgb)
    }
}

/// The compiled pipelines, built once per device.
#[derive(Debug)]
pub(crate) struct Pipelines {
    gains: wgpu::ComputePipeline,
    output: wgpu::ComputePipeline,
}

impl Pipelines {
    pub(crate) fn new(device: &wgpu::Device) -> Pipelines {
        Pipelines {
            gains: pipeline(device, "gains", GAINS),
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
    let base = pixel * 3u;
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
