//! The window's GPU off Linux: DirectX 12, where a decoder's pictures are painted where they are. Its names are
//! Linux's, so the window asks one way on every system.

use crate::choose::{Candidate, chosen};
use media::EndsAlone;

/// What a decoded picture needs of the device: its NV12 format, which DirectX 12 always offers.
pub const IMPORT_FEATURES: wgpu::Features = wgpu::Features::TEXTURE_FORMAT_NV12;

/// The backends the window is painted through, the first that can show it chosen.
const ORDER: &[wgpu::Backend] = &[wgpu::Backend::Dx12];

/// The backends in `ORDER` as one set, or those `WGPU_BACKEND` names.
fn backends() -> wgpu::Backends {
    ORDER.iter().fold(wgpu::Backends::empty(), |all, &backend| all | backend.into()).with_env()
}

/// The GPUs asleep on this machine: none is known to be here.
pub struct Asleep;

impl Asleep {
    /// None asleep: nothing here reads a GPU's power state.
    pub fn now() -> Self {
        Self
    }
}

/// The display a window is on, which an instance may be made with.
pub type Display = Box<dyn wgpu::wgt::WgpuHasDisplayHandle>;

/// The window's instance, with the display its windows are on; none for an instance that shows no window.
pub fn instance(display: Option<Display>) -> EndsAlone<wgpu::Instance> {
    let flags = wgpu::InstanceFlags::from_build_config().with_env();
    let descriptor = wgpu::InstanceDescriptor {
        backends: backends(),
        flags,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    };
    EndsAlone::new(wgpu::Instance::new(match display {
        Some(display) => descriptor.with_display_handle(display),
        None => descriptor,
    }))
}

/// The instance's adapters.
pub fn adapters(instance: &wgpu::Instance) -> EndsAlone<Vec<wgpu::Adapter>> {
    EndsAlone::new(pollster::block_on(instance.enumerate_adapters(backends())))
}

/// `adapter`'s device and queue.
pub fn device(
    adapter: &wgpu::Adapter,
    descriptor: &wgpu::DeviceDescriptor<'_>,
) -> Result<(EndsAlone<wgpu::Device>, EndsAlone<wgpu::Queue>), wgpu::RequestDeviceError> {
    let (device, queue) = pollster::block_on(adapter.request_device(descriptor))?;
    Ok((EndsAlone::new(device), EndsAlone::new(queue)))
}

/// The adapter among `adapters` that paints `surface`, with no render node: on DirectX 12 a decoder is handed the
/// window's own device instead.
pub fn painting_that_presents(
    adapters: &[wgpu::Adapter],
    _: &Asleep,
    surface: &wgpu::Surface<'_>,
) -> Option<(wgpu::Adapter, Option<String>)> {
    let candidates: Vec<Candidate> = adapters
        .iter()
        .map(|adapter| Candidate::of(adapter, adapter.is_surface_supported(surface), imports(adapter)))
        .collect();
    chosen(&candidates, ORDER).and_then(|at| Some((adapters.get(at)?.clone(), None)))
}

/// Whether a decoder's pictures are painted where they are on `adapter`: DirectX 12 with NV12.
fn imports(adapter: &wgpu::Adapter) -> bool {
    adapter.get_info().backend == wgpu::Backend::Dx12 && adapter.features().contains(IMPORT_FEATURES)
}
