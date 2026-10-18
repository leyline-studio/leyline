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
}

impl Gpu {
    /// The first adapter wgpu offers, and a device on it; `None` when there
    /// is none or it refuses a device. Never an error: a machine without a
    /// GPU is a supported machine (ADR 0156 §2).
    pub fn acquire() -> Option<Gpu> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .ok()?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
        Some(Gpu {
            device,
            queue,
            info: adapter.get_info(),
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
}

mod scale;
pub use scale::scale_samples;
