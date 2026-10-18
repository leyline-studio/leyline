//! GPU compute for the preview path alone (ADR 0156).
//!
//! Nothing here ever renders an export, a print or a golden case: those
//! stay on the CPU, under `docs/pipeline.md` §5.1. The GPU is optional —
//! [`Gpu::acquire`] answers `None` on a machine without a usable adapter,
//! and the caller keeps the CPU path it always had (§2).

// ADR 0152: no panic on a production path.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]
#![forbid(unsafe_code)]

/// A device the preview can run kernels on.
#[derive(Debug)]
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    info: wgpu::AdapterInfo,
    pipelines: Pipelines,
}

impl Gpu {
    /// A hardware adapter and a device on it; `None` when there is none, or
    /// when the only adapter is a software rasteriser — llvmpipe renders on
    /// the CPU the preview path already uses, only slower. Never an error: a
    /// machine without a GPU is a supported machine (ADR 0156 §2).
    pub fn acquire() -> Option<Gpu> {
        Gpu::acquire_with(false)
    }

    /// [`Gpu::acquire`], accepting a software adapter too: what a test of
    /// the kernels runs on, on every machine that runs the gate.
    pub fn acquire_any() -> Option<Gpu> {
        Gpu::acquire_with(true)
    }

    fn acquire_with(software: bool) -> Option<Gpu> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .ok()?;
        let info = adapter.get_info();
        if info.device_type == wgpu::DeviceType::Cpu && !software {
            return None;
        }
        // The adapter's own limits, not WebGPU's defaults: one 4K frame of
        // `f32` RGB is ~134 MB, past the default 128 MB per binding.
        let descriptor = wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            ..wgpu::DeviceDescriptor::default()
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&descriptor)).ok()?;
        let pipelines = Pipelines::new(&device);
        Some(Gpu {
            device,
            queue,
            info,
            pipelines,
        })
    }

    /// What the adapter calls itself and which backend runs it, for
    /// *About* and for logs (ADR 0156 §2).
    pub fn describe(&self) -> String {
        format!("{} ({:?})", self.info.name, self.info.backend)
    }

    /// The device, for the kernels of this crate.
    pub(crate) fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// The queue, for the kernels of this crate.
    pub(crate) fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub(crate) fn pipelines(&self) -> &Pipelines {
        &self.pipelines
    }
}

mod frame;
use frame::Pipelines;
pub use frame::{DisplayOp, Frame, Op, Transfer};

mod scale;
pub use scale::scale_samples;
