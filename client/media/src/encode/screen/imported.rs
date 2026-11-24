//! The compositor's buffers as images of the encoder's Vulkan device, each imported once and kept for as long as the
//! stream lends it again.

use std::os::fd::OwnedFd;

use ash::{khr, vk};

use super::kept::{Kept, Key};
use crate::device::vulkan_image::{self, ImageLayout, On, Use, plane};
use crate::encode::EncodeError;
use capture::{Frame as Captured, Layout, Native};

/// One lent buffer as an image of the device, and the view the pass reads it through.
pub(super) struct Screen {
    pub(super) image: vk::Image,
    memory: vk::DeviceMemory,
    pub(super) view: vk::ImageView,
}

/// The device's functions an import needs, and the buffers imported so far, the most recently read last.
pub(super) struct Screens {
    instance: ash::Instance,
    physical: vk::PhysicalDevice,
    device: ash::Device,
    memory_fd: khr::external_memory_fd::Device,
    semaphore_fd: khr::external_semaphore_fd::Device,
    layout: Layout,
    kept: Kept<Screen>,
}

impl Screens {
    pub(super) fn new(
        instance: ash::Instance,
        physical: vk::PhysicalDevice,
        device: ash::Device,
        layout: Layout,
    ) -> Self {
        let memory_fd = khr::external_memory_fd::Device::new(&instance, &device);
        let semaphore_fd = khr::external_semaphore_fd::Device::new(&instance, &device);
        Self { instance, physical, device, memory_fd, semaphore_fd, layout, kept: Kept::default() }
    }

    /// The image `captured`'s buffer is, imported the first time the stream lends it. One that makes way for it is
    /// freed once `settled` says no pass reads any, and leaked where one may.
    pub(super) fn of(&mut self, captured: &Captured, settled: impl FnOnce() -> bool) -> Result<&Screen, EncodeError> {
        if captured.layout() != self.layout {
            return Err(EncodeError::FrameSize);
        }
        let Self { instance, physical, device, memory_fd, layout, kept, .. } = self;
        let on = On { instance, physical: *physical, device, memory_fd };
        kept.of(
            captured,
            |key, fd| import(&on, *layout, key, fd),
            |oldest| {
                if settled() {
                    // SAFETY: made by `import` on this device, and every pass that read it is done: the pass settled.
                    unsafe { free(device, &oldest) };
                }
            },
        )
    }

    /// A semaphore that is signalled once whoever wrote `captured`'s buffer is done: the fence the kernel keeps on it,
    /// exported for reading.
    pub(super) fn written(&self, captured: &Captured, into: vk::Semaphore) -> Result<(), EncodeError> {
        let Native::DmaBuf(dmabuf) = captured.native();
        let fence = dmabuf.written().map_err(|_| EncodeError::Convert)?;
        let import = vk::ImportSemaphoreFdInfoKHR::default()
            .semaphore(into)
            .flags(vk::SemaphoreImportFlags::TEMPORARY)
            .handle_type(vk::ExternalSemaphoreHandleTypeFlags::SYNC_FD)
            .fd(std::os::fd::IntoRawFd::into_raw_fd(fence));
        // SAFETY: `into` is a binary semaphore of this device with no pending wait, and the fd a sync file it takes on
        // success.
        unsafe { self.semaphore_fd.import_semaphore_fd(&import) }.map_err(|_| EncodeError::Convert)
    }

    /// Frees every kept image.
    ///
    /// # Safety
    /// Nothing pending reads any of them.
    pub(super) unsafe fn clear(&mut self) {
        for screen in self.kept.take() {
            // SAFETY: the caller's promise.
            unsafe { free(&self.device, &screen) };
        }
    }
}

fn import(on: &On<'_>, layout: Layout, key: Key, fd: OwnedFd) -> Result<Screen, EncodeError> {
    let format = vk::Format::B8G8R8A8_UNORM;
    let image_layout = ImageLayout {
        format,
        planes: [plane(key.offset.into(), key.stride.into())],
        modifier: key.modifier,
        views: &[],
    };
    let size = vk::Extent2D { width: layout.width, height: layout.height };
    // SAFETY: the device is live and outlives the screens; `fd` is the lent DMA-BUF, laid out as the capture says.
    let (image, memory) =
        unsafe { vulkan_image::import(on, size, &image_layout, Use::Read, fd) }.map_err(|_| EncodeError::Convert)?;
    let range =
        vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
    let view = vk::ImageViewCreateInfo::default()
        .image(image)
        .view_type(vk::ImageViewType::TYPE_2D)
        .format(format)
        .subresource_range(range);
    // SAFETY: `image` was made on this device just above, in the view's format.
    match unsafe { on.device.create_image_view(&view, None) } {
        Ok(view) => Ok(Screen { image, memory, view }),
        Err(_) => {
            // SAFETY: made just above and never used.
            unsafe { on.device.destroy_image(image, None) };
            // SAFETY: bound only to that image, now destroyed.
            unsafe { on.device.free_memory(memory, None) };
            Err(EncodeError::Convert)
        }
    }
}

/// # Safety
/// `screen` was made by `import` on this device and nothing pending reads it.
unsafe fn free(device: &ash::Device, screen: &Screen) {
    // SAFETY: the caller's promise.
    unsafe { device.destroy_image_view(screen.view, None) };
    // SAFETY: as above; the view of it is gone.
    unsafe { device.destroy_image(screen.image, None) };
    // SAFETY: bound only to that image, now destroyed.
    unsafe { device.free_memory(screen.memory, None) };
}
