//! The instance the window paints with, made one way for the window and for the question of which GPU paints: Vulkan,
//! and GL only for a machine whose Vulkan cannot run the window's shaders (llvmpipe's has no half floats). GL's EGL
//! loads the system's Mesa, whose exported winsys the carried VA driver's would collide with in one process.

use crate::choose::Candidate;
use ash::{ext, khr};
use media::EndsAlone;
use std::ffi::CStr;
use wgpu::hal::api::Vulkan;
use wgpu::hal::vulkan::CreateDeviceCallbackArgs;

/// The backends the window is painted through here, in the order they are preferred.
pub(crate) const ORDER: &[wgpu::Backend] = &[wgpu::Backend::Vulkan, wgpu::Backend::Gl];

const BACKENDS: wgpu::Backends = wgpu::Backends::VULKAN.union(wgpu::Backends::GL);

/// The backends an instance is made with, given whether a Vulkan adapter runs the window's shaders: Vulkan alone
/// then, so GL's EGL is never loaded beside a GPU's own video driver; GL beside it only where none does.
fn backends(vulkan_paints: bool) -> wgpu::Backends {
    if vulkan_paints { wgpu::Backends::VULKAN } else { BACKENDS }
}

/// What the window's instance is made with: its backends, and no object names even in a debug build, since naming
/// through the loader crashes while a share's start unloads a driver; `WGPU_DEBUG=1` asks for them knowingly.
fn instance_descriptor(display: Option<Display>, backends: wgpu::Backends) -> wgpu::InstanceDescriptor {
    let flags = wgpu::InstanceFlags::from_build_config().difference(wgpu::InstanceFlags::DEBUG).with_env();
    let descriptor =
        wgpu::InstanceDescriptor { backends, flags, ..wgpu::InstanceDescriptor::new_without_display_handle() };
    match display {
        Some(display) => descriptor.with_display_handle(display),
        None => descriptor,
    }
}

/// The display a window is on, which GL needs its instance made with to present to it (Wayland's EGL display).
pub type Display = Box<dyn wgpu::wgt::WgpuHasDisplayHandle>;

/// The window's instance, made under the loader's lock and ended under it: its end unloads the drivers. `display` is
/// the one its windows are on; none for an instance that shows no window.
pub fn instance(display: Option<Display>) -> EndsAlone<wgpu::Instance> {
    let vulkan = made(None, wgpu::Backends::VULKAN);
    let paints =
        listed(&vulkan, wgpu::Backends::VULKAN).iter().any(|adapter| Candidate::of(adapter, true, false).paints);
    match backends(paints) {
        wgpu::Backends::VULKAN => vulkan,
        chosen => {
            drop(vulkan);
            made(display, chosen)
        }
    }
}

fn made(display: Option<Display>, backends: wgpu::Backends) -> EndsAlone<wgpu::Instance> {
    let _alone = media::alone();
    EndsAlone::new(wgpu::Instance::new(instance_descriptor(display, backends)))
}

/// The instance's adapters, listed under the loader's lock: the listing is where the loader scans drivers.
pub fn adapters(instance: &wgpu::Instance) -> EndsAlone<Vec<wgpu::Adapter>> {
    listed(instance, BACKENDS)
}

fn listed(instance: &wgpu::Instance, backends: wgpu::Backends) -> EndsAlone<Vec<wgpu::Adapter>> {
    let _alone = media::alone();
    EndsAlone::new(pollster::block_on(instance.enumerate_adapters(backends)))
}

/// An instance and the adapters it listed, the one place that lists: the window's choice of GPU reads it, and so does
/// the learning of which driver serves which GPU. The adapters end before the instance does.
pub(crate) struct Listing {
    adapters: EndsAlone<Vec<wgpu::Adapter>>,
    _instance: EndsAlone<wgpu::Instance>,
}

impl Listing {
    /// Every adapter the window could paint with.
    pub(crate) fn of_window() -> Self {
        let instance = instance(None);
        Self { adapters: listed(&instance, BACKENDS), _instance: instance }
    }

    /// The Vulkan adapters alone, which is all that choosing a driver is about.
    pub(crate) fn of_vulkan() -> Self {
        Self::of(wgpu::Backends::VULKAN)
    }

    fn of(backends: wgpu::Backends) -> Self {
        let instance = made(None, backends);
        Self { adapters: listed(&instance, backends), _instance: instance }
    }
}

impl std::ops::Deref for Listing {
    type Target = [wgpu::Adapter];

    fn deref(&self) -> &[wgpu::Adapter] {
        &self.adapters
    }
}

/// `adapter`'s device and queue, made under the loader's lock. A Vulkan device also takes the hand-over of another
/// API's buffers (`VK_EXT_queue_family_foreign`) and a wait on their writer's sync file
/// (`VK_KHR_external_semaphore_fd`) where it offers them, which a decoded or captured picture's import needs.
pub fn device(
    adapter: &wgpu::Adapter,
    descriptor: &wgpu::DeviceDescriptor<'_>,
) -> Result<(EndsAlone<wgpu::Device>, EndsAlone<wgpu::Queue>), DeviceError> {
    let _alone = media::alone();
    // SAFETY: the handle is only used to open the device, while `adapter` lives.
    let opened = unsafe { adapter.as_hal::<Vulkan>() }.map(|hal| {
        let capabilities = hal.physical_device_capabilities();
        let offered: Vec<&'static CStr> = [ext::queue_family_foreign::NAME, khr::external_semaphore_fd::NAME]
            .into_iter()
            .filter(|name| capabilities.supports_extension(name))
            .collect();
        let add = move |args: CreateDeviceCallbackArgs<'_, '_, '_>| args.extensions.extend(&offered);
        // SAFETY: the features and limits are the descriptor's, which the device is then made from; the callback
        // only adds extensions the adapter supports.
        unsafe {
            hal.open_with_callback(
                descriptor.required_features,
                &descriptor.required_limits,
                &descriptor.memory_hints,
                Some(Box::new(add)),
            )
        }
    });
    let (device, queue) = match opened {
        // SAFETY: `open` is this adapter's device, opened from the descriptor it is now made with.
        Some(open) => unsafe { adapter.create_device_from_hal(open.map_err(DeviceError::Open)?, descriptor) }?,
        None => pollster::block_on(adapter.request_device(descriptor))?,
    };
    Ok((EndsAlone::new(device), EndsAlone::new(queue)))
}

/// Why the window's device was not made.
#[derive(Debug)]
pub enum DeviceError {
    /// The driver did not open the device.
    Open(wgpu::hal::DeviceError),
    /// wgpu did not make a device of the one opened.
    Request(wgpu::RequestDeviceError),
}

impl From<wgpu::RequestDeviceError> for DeviceError {
    fn from(refused: wgpu::RequestDeviceError) -> Self {
        Self::Request(refused)
    }
}

impl std::fmt::Display for DeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open(refused) => write!(f, "the driver did not open the device: {refused}"),
            Self::Request(refused) => refused.fmt(f),
        }
    }
}

impl std::error::Error for DeviceError {}

#[cfg(test)]
mod tests {
    use super::backends;

    #[test]
    fn gl_is_made_only_where_no_vulkan_adapter_runs_the_windows_shaders() {
        assert_eq!(backends(true), wgpu::Backends::VULKAN);
        assert!(backends(false).contains(wgpu::Backends::GL | wgpu::Backends::VULKAN));
    }
}
