//! `dehaze` v1 on the GPU: the CPU's dark channel prior, pass for pass,
//! with its one global step — the atmospheric light — found on the device
//! too, so a drag never waits on a read-back mid-frame.
//!
//! The light is the brightest pixel among the `take` largest values of the
//! dark channel. The CPU sorts for them; here a two-step radix select over
//! the values' bits (each non-negative `f32` orders as its `u32`) finds the
//! cut, then two atomic passes find the brightest pixel at or above it.

use wgpu::util::DeviceExt;

use super::{Frame, Transfer, entry, pipeline, record, table, uniform};
use crate::Gpu;

/// Bins of one radix step: sixteen bits of the key.
const BINS: u64 = 1 << 16;

/// Slots of the state buffer the passes hand each other, named by
/// [`DEHAZE`]'s constants.
const STATE_WORDS: usize = 8;

/// What one dehaze records, besides its tables.
pub(super) struct DehazeParams {
    pub(super) groups: (u32, u32),
    pub(super) stride: u32,
    pub(super) k: f32,
    pub(super) patch: u32,
    pub(super) take: u32,
    pub(super) omega: f32,
    pub(super) min_transmission: f32,
}

/// The passes of [`super::Op::Dehaze`], entry points of one shader module.
#[derive(Debug)]
pub(super) struct DehazePipelines {
    channel_min: wgpu::ComputePipeline,
    min_rows: wgpu::ComputePipeline,
    min_columns: wgpu::ComputePipeline,
    histogram_high: wgpu::ComputePipeline,
    select_high: wgpu::ComputePipeline,
    histogram_low: wgpu::ComputePipeline,
    select_low: wgpu::ComputePipeline,
    brightest: wgpu::ComputePipeline,
    brightest_index: wgpu::ComputePipeline,
    atmosphere: wgpu::ComputePipeline,
    normalized_min: wgpu::ComputePipeline,
    apply: wgpu::ComputePipeline,
}

impl DehazePipelines {
    pub(super) fn new(device: &wgpu::Device, axis: &str) -> DehazePipelines {
        let source = format!("{axis}{DEHAZE}");
        let stage = |entry_point: &str| {
            pipeline(
                device,
                &format!("dehaze {entry_point}"),
                &source,
                entry_point,
            )
        };
        DehazePipelines {
            channel_min: stage("channel_min"),
            min_rows: stage("min_rows"),
            min_columns: stage("min_columns"),
            histogram_high: stage("histogram_high"),
            select_high: stage("select_high"),
            histogram_low: stage("histogram_low"),
            select_low: stage("select_low"),
            brightest: stage("brightest"),
            brightest_index: stage("brightest_index"),
            atmosphere: stage("atmosphere"),
            normalized_min: stage("normalized_min"),
            apply: stage("apply"),
        }
    }
}

impl Frame {
    /// Records `dehaze` v1 into `encoder`, in the CPU's order: the dark
    /// channel, the atmospheric light, the normalised dark channel, then
    /// the recovery.
    pub(super) fn dehaze(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        dehaze: &DehazeParams,
        transfer: &Transfer,
    ) -> Option<()> {
        let device = gpu.device();
        let [minimum, scratch, dark, normalized] = self.planes(device);
        let mut params = Vec::with_capacity(48);
        for word in [
            self.pixels,
            dehaze.stride,
            self.width,
            self.height,
            dehaze.patch,
            dehaze.take,
        ] {
            params.extend_from_slice(&word.to_le_bytes());
        }
        for value in [dehaze.k, dehaze.omega, dehaze.min_transmission] {
            params.extend_from_slice(&value.to_le_bytes());
        }
        params.resize(48, 0);
        let params = uniform(device, &params);
        let to_display = table(device, &transfer.to_display);
        let to_linear = table(device, &transfer.to_linear);
        let histogram = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dehaze histogram"),
            size: BINS * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // The brightest index starts above every pixel, the rest at 0.
        let mut state = [0u32; STATE_WORDS];
        state[STATE_INDEX] = u32::MAX;
        let state_bytes: Vec<u8> = state.iter().flat_map(|w| w.to_le_bytes()).collect();
        let state = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("dehaze state"),
            contents: &state_bytes,
            usage: wgpu::BufferUsages::STORAGE,
        });

        let pipelines = &gpu.pipelines().dehaze;
        let groups = dehaze.groups;
        let pass = |encoder: &mut wgpu::CommandEncoder,
                    pipeline: &wgpu::ComputePipeline,
                    entries: &[wgpu::BindGroupEntry<'_>],
                    groups: (u32, u32)| {
            record(device, encoder, pipeline, entries, groups);
        };
        let min_filter =
            |encoder: &mut wgpu::CommandEncoder, from: &wgpu::Buffer, to: &wgpu::Buffer| {
                let rows = [entry(1, &params), entry(4, from), entry(7, scratch)];
                pass(encoder, &pipelines.min_rows, &rows, groups);
                let columns = [entry(1, &params), entry(4, scratch), entry(7, to)];
                pass(encoder, &pipelines.min_columns, &columns, groups);
            };
        let single = (1, 1);

        let channel_min = [
            entry(0, &self.work),
            entry(1, &params),
            entry(2, &to_display),
            entry(7, minimum),
        ];
        pass(encoder, &pipelines.channel_min, &channel_min, groups);
        min_filter(encoder, minimum, dark);

        let histogram_pass = [entry(1, &params), entry(4, dark), entry(5, &histogram)];
        let select = [entry(5, &histogram), entry(6, &state)];
        pass(encoder, &pipelines.histogram_high, &histogram_pass, groups);
        let select_high = [entry(1, &params), entry(5, &histogram), entry(6, &state)];
        pass(encoder, &pipelines.select_high, &select_high, single);
        encoder.clear_buffer(&histogram, 0, None);
        let histogram_low = [
            entry(1, &params),
            entry(4, dark),
            entry(5, &histogram),
            entry(6, &state),
        ];
        pass(encoder, &pipelines.histogram_low, &histogram_low, groups);
        pass(encoder, &pipelines.select_low, &select, single);

        let brightest = [
            entry(0, &self.work),
            entry(1, &params),
            entry(2, &to_display),
            entry(4, dark),
            entry(6, &state),
        ];
        pass(encoder, &pipelines.brightest, &brightest, groups);
        pass(encoder, &pipelines.brightest_index, &brightest, groups);
        let atmosphere = [
            entry(0, &self.work),
            entry(2, &to_display),
            entry(6, &state),
        ];
        pass(encoder, &pipelines.atmosphere, &atmosphere, single);

        let normalized_min = [
            entry(0, &self.work),
            entry(1, &params),
            entry(2, &to_display),
            entry(6, &state),
            entry(7, minimum),
        ];
        pass(encoder, &pipelines.normalized_min, &normalized_min, groups);
        min_filter(encoder, minimum, normalized);

        let apply = [
            entry(0, &self.work),
            entry(1, &params),
            entry(2, &to_display),
            entry(3, &to_linear),
            entry(4, normalized),
            entry(6, &state),
        ];
        pass(encoder, &pipelines.apply, &apply, groups);
        Some(())
    }
}

/// [`DEHAZE`]'s `STATE_INDEX`: the brightest candidate's pixel.
const STATE_INDEX: usize = 4;

/// The passes, across [`super::AXIS`]. The state buffer's words: the cut's
/// high half and how many keys it still needs below it, the cut itself,
/// the brightest candidate's sum and pixel, and the light's three
/// channels.
const DEHAZE: &str = r"
struct Params {
    pixels: u32,
    stride: u32,
    width: u32,
    height: u32,
    radius: u32,
    take: u32,
    k: f32,
    omega: f32,
    min_transmission: f32,
}
@group(0) @binding(0) var<storage, read_write> samples: array<f32>;
@group(0) @binding(1) var<uniform> params: Params;
@group(0) @binding(2) var<storage, read> to_display_table: array<f32>;
@group(0) @binding(3) var<storage, read> to_linear_table: array<f32>;
@group(0) @binding(4) var<storage, read> plane: array<f32>;
@group(0) @binding(5) var<storage, read_write> histogram: array<atomic<u32>>;
@group(0) @binding(6) var<storage, read_write> state: array<atomic<u32>>;
@group(0) @binding(7) var<storage, read_write> plane_out: array<f32>;

const STATE_HIGH: u32 = 0u;
const STATE_NEEDED: u32 = 1u;
const STATE_CUT: u32 = 2u;
const STATE_SUM: u32 = 3u;
const STATE_INDEX: u32 = 4u;
const STATE_LIGHT: u32 = 5u;

var<workgroup> chunks: array<u32, 256>;

fn pixel_of(id: vec3<u32>) -> u32 {
    return id.y * params.stride + id.x;
}

fn light(c: u32) -> f32 {
    return bitcast<f32>(atomicLoad(&state[STATE_LIGHT + c]));
}

@compute @workgroup_size(64)
fn channel_min(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = pixel_of(id);
    if (pixel >= params.pixels) { return; }
    let base = pixel * 3u;
    plane_out[pixel] = min(
        min(to_display(samples[base]), to_display(samples[base + 1u])),
        to_display(samples[base + 2u]),
    );
}

@compute @workgroup_size(64)
fn min_rows(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = pixel_of(id);
    if (pixel >= params.pixels) { return; }
    let x = pixel % params.width;
    let row = pixel - x;
    let lo = select(0u, x - params.radius, x >= params.radius);
    let hi = min(x + params.radius, params.width - 1u);
    var m = plane[row + lo];
    for (var i = lo + 1u; i <= hi; i++) { m = min(m, plane[row + i]); }
    plane_out[pixel] = m;
}

@compute @workgroup_size(64)
fn min_columns(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = pixel_of(id);
    if (pixel >= params.pixels) { return; }
    let x = pixel % params.width;
    let y = pixel / params.width;
    let lo = select(0u, y - params.radius, y >= params.radius);
    let hi = min(y + params.radius, params.height - 1u);
    var m = plane[lo * params.width + x];
    for (var i = lo + 1u; i <= hi; i++) { m = min(m, plane[i * params.width + x]); }
    plane_out[pixel] = m;
}

@compute @workgroup_size(64)
fn histogram_high(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = pixel_of(id);
    if (pixel >= params.pixels) { return; }
    atomicAdd(&histogram[bitcast<u32>(plane[pixel]) >> 16u], 1u);
}

@compute @workgroup_size(64)
fn histogram_low(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = pixel_of(id);
    if (pixel >= params.pixels) { return; }
    let key = bitcast<u32>(plane[pixel]);
    if ((key >> 16u) != atomicLoad(&state[STATE_HIGH])) { return; }
    atomicAdd(&histogram[key & 0xffffu], 1u);
}

// The bin, from the top, where the count reaches `needed`, and what is
// still needed inside it: chunk sums in parallel, then one walk.
fn walk(t: u32, needed: u32) -> vec2<u32> {
    var sum = 0u;
    for (var i = 0u; i < 256u; i++) { sum += atomicLoad(&histogram[t * 256u + i]); }
    chunks[t] = sum;
    workgroupBarrier();
    var above = 0u;
    var chunk = 255u;
    while (chunk > 0u && above + chunks[chunk] < needed) {
        above += chunks[chunk];
        chunk -= 1u;
    }
    var bin = chunk * 256u + 255u;
    while (bin > chunk * 256u && above + atomicLoad(&histogram[bin]) < needed) {
        above += atomicLoad(&histogram[bin]);
        bin -= 1u;
    }
    return vec2<u32>(bin, needed - above);
}

@compute @workgroup_size(256)
fn select_high(@builtin(local_invocation_index) t: u32) {
    let found = walk(t, params.take);
    if (t == 0u) {
        atomicStore(&state[STATE_HIGH], found.x);
        atomicStore(&state[STATE_NEEDED], found.y);
    }
}

@compute @workgroup_size(256)
fn select_low(@builtin(local_invocation_index) t: u32) {
    let found = walk(t, atomicLoad(&state[STATE_NEEDED]));
    if (t == 0u) {
        atomicStore(&state[STATE_CUT], (atomicLoad(&state[STATE_HIGH]) << 16u) | found.x);
    }
}

fn candidate_sum(pixel: u32) -> f32 {
    let base = pixel * 3u;
    return to_display(samples[base]) + to_display(samples[base + 1u])
        + to_display(samples[base + 2u]);
}

@compute @workgroup_size(64)
fn brightest(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = pixel_of(id);
    if (pixel >= params.pixels) { return; }
    if (bitcast<u32>(plane[pixel]) < atomicLoad(&state[STATE_CUT])) { return; }
    atomicMax(&state[STATE_SUM], bitcast<u32>(candidate_sum(pixel)));
}

@compute @workgroup_size(64)
fn brightest_index(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = pixel_of(id);
    if (pixel >= params.pixels) { return; }
    if (bitcast<u32>(plane[pixel]) < atomicLoad(&state[STATE_CUT])) { return; }
    if (bitcast<u32>(candidate_sum(pixel)) != atomicLoad(&state[STATE_SUM])) { return; }
    atomicMin(&state[STATE_INDEX], pixel);
}

@compute @workgroup_size(1)
fn atmosphere() {
    let base = atomicLoad(&state[STATE_INDEX]) * 3u;
    for (var c = 0u; c < 3u; c++) {
        atomicStore(&state[STATE_LIGHT + c], bitcast<u32>(to_display(samples[base + c])));
    }
}

@compute @workgroup_size(64)
fn normalized_min(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = pixel_of(id);
    if (pixel >= params.pixels) { return; }
    let base = pixel * 3u;
    var m = to_display(samples[base]) / max(light(0u), 1e-3);
    m = min(m, to_display(samples[base + 1u]) / max(light(1u), 1e-3));
    m = min(m, to_display(samples[base + 2u]) / max(light(2u), 1e-3));
    plane_out[pixel] = m;
}

@compute @workgroup_size(64)
fn apply(@builtin(global_invocation_id) id: vec3<u32>) {
    let pixel = pixel_of(id);
    if (pixel >= params.pixels) { return; }
    let transmission = max(1.0 - params.omega * plane[pixel], params.min_transmission);
    let base = pixel * 3u;
    for (var c = 0u; c < 3u; c++) {
        let value = to_display(samples[base + c]);
        let a = light(c);
        var shifted: f32;
        if (params.k >= 0.0) {
            let recovered = max((value - a) / transmission + a, 0.0);
            shifted = max(value + (recovered - value) * params.k, 0.0);
        } else {
            let haze = (1.0 - transmission) * -params.k;
            shifted = max(value + (a - value) * haze, 0.0);
        }
        samples[base + c] = to_linear(shifted);
    }
}
";
