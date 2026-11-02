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

use std::sync::{Arc, OnceLock};

/// A device the preview can run kernels on.
#[derive(Debug)]
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    info: wgpu::AdapterInfo,
    pipelines: Pipelines,
    /// The first error wgpu reported, or the device's loss (ADR 0170 §1).
    failure: Arc<OnceLock<String>>,
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
        // Vulkan, Metal and DX12 only, never OpenGL: Studio's own renderer
        // holds a GL context on the UI thread, and wgpu's GL backend, merely
        // probing for an adapter there, panicked on `eglMakeCurrent`
        // (`BadAccess`) — a crash on entering Develop, found by running
        // Studio, not by any test. `WGPU_BACKEND` still overrides, for a
        // diagnosis.
        let mut descriptor = wgpu::InstanceDescriptor::from_env_or_default();
        if std::env::var_os("WGPU_BACKEND").is_none() {
            descriptor.backends = wgpu::Backends::PRIMARY;
        }
        let instance = wgpu::Instance::new(&descriptor);
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
        // wgpu's default handler panics; a preview that may always fall back
        // to the CPU records the failure instead (ADR 0170 §1). The first
        // one wins: what follows a loss is only its echo.
        let failure = Arc::new(OnceLock::new());
        let on_error = Arc::clone(&failure);
        device.on_uncaptured_error(Box::new(move |error| {
            let _ = on_error.set(error.to_string());
        }));
        let on_loss = Arc::clone(&failure);
        device.set_device_lost_callback(move |reason, message| {
            let _ = on_loss.set(format!("device lost ({reason:?}): {message}"));
        });
        let pipelines = Pipelines::new(&device);
        Some(Gpu {
            device,
            queue,
            info,
            pipelines,
            failure,
        })
    }

    /// Why this device can no longer be trusted, once wgpu has reported an
    /// error or lost it; `None` while it is healthy (ADR 0170 §1).
    pub fn failure(&self) -> Option<&str> {
        self.failure.get().map(String::as_str)
    }

    /// Marks the device failed from outside: what a caller does when work on
    /// it panicked (ADR 0170 §3). The first reason recorded is kept.
    pub fn retire(&self, reason: &str) {
        let _ = self.failure.set(reason.to_owned());
    }

    /// Gives the device up, as a driver reset does: every later call on it
    /// fails, and [`Gpu::failure`] says so once wgpu has noticed. What the
    /// tests of ADR 0170 lose a device with, on any machine.
    pub fn destroy(&self) {
        self.device.destroy();
        // A lost device is reported on the next poll.
        let _ = self.device.poll(wgpu::PollType::Wait);
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
