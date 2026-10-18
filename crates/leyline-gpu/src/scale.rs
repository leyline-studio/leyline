//! The first kernel (ADR 0156 §7): every sample times one factor — the core
//! of an exposure gain — run to prove the whole chain, upload, dispatch and
//! read-back, on every backend the deliverables ship with.

use wgpu::util::DeviceExt;

use crate::Gpu;

/// Samples per workgroup; a multiple of every vendor's wave width.
const WORKGROUP: u32 = 64;

const SHADER: &str = r"
struct Params { factor: f32, count: u32 }
@group(0) @binding(0) var<storage, read_write> samples: array<f32>;
@group(0) @binding(1) var<uniform> params: Params;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x < params.count) {
        samples[id.x] = samples[id.x] * params.factor;
    }
}
";

/// `samples`, each multiplied by `factor`, computed on `gpu`. `None` when
/// the device fails at any step: the caller renders on the CPU instead
/// (ADR 0156 §2).
pub fn scale_samples(gpu: &Gpu, samples: &[f32], factor: f32) -> Option<Vec<f32>> {
    let count = u32::try_from(samples.len()).ok()?;
    if count == 0 {
        return Some(Vec::new());
    }
    let device = gpu.device();
    let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    let storage = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("samples"),
        contents: &bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    });
    let mut params = factor.to_le_bytes().to_vec();
    params.extend_from_slice(&count.to_le_bytes());
    let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("params"),
        contents: &params,
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: bytes.len() as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("scale"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("scale"),
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("scale"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: storage.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: uniform.as_entire_binding(),
            },
        ],
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(count.div_ceil(WORKGROUP), 1, 1);
    }
    encoder.copy_buffer_to_buffer(&storage, 0, &readback, 0, bytes.len() as u64);
    gpu.queue().submit([encoder.finish()]);

    let (sender, receiver) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    device.poll(wgpu::PollType::Wait).ok()?;
    receiver.recv().ok()?.ok()?;
    let mapped = readback.slice(..).get_mapped_range();
    let out = mapped
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect();
    drop(mapped);
    readback.unmap();
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On every machine that runs the gate there is at least a software
    /// adapter (lavapipe) — when there is none, the test says so and checks
    /// the one promise left: `acquire` answers `None` rather than failing.
    #[test]
    fn the_chain_runs_and_matches_the_cpu() {
        let Some(gpu) = Gpu::acquire_any() else {
            eprintln!("no GPU adapter on this machine: the CPU path is the only one");
            return;
        };
        eprintln!("adapter: {}", gpu.describe());
        let samples: Vec<f32> = (0..10_000).map(|i| i as f32 / 10_000.0).collect();
        let out = scale_samples(&gpu, &samples, 2.0_f32.powf(0.7)).unwrap();
        assert_eq!(out.len(), samples.len());
        for (gpu_value, cpu_value) in out.iter().zip(&samples) {
            let expected = cpu_value * 2.0_f32.powf(0.7);
            assert!(
                (gpu_value - expected).abs() <= f32::EPSILON * 4.0,
                "{gpu_value} vs {expected}"
            );
        }
    }
}
