//! A DMA-BUF made a Vulkan image where it is, on whichever device reads or writes it (the window's, or an encoder's), the
//! device asked first whether it takes the buffer's format and modifier for that use, as Vulkan requires of an explicit
//! modifier.

use ash::{khr, vk};
use std::os::fd::{AsRawFd, IntoRawFd};

/// The DRM fourccs of the NV12 pictures imported and exported here (`VA_FOURCC_NV12` is the same code).
pub mod fourcc {
    /// `DRM_FORMAT_NV12`: luma, then interleaved chroma, in one layer.
    pub const NV12: u32 = u32::from_le_bytes(*b"NV12");
    /// `DRM_FORMAT_R8`: an NV12 picture's luma as a layer of its own.
    pub const R8: u32 = u32::from_le_bytes(*b"R8  ");
    /// `DRM_FORMAT_GR88`: an NV12 picture's chroma as a layer of its own, Cb in its first byte.
    pub const GR88: u32 = u32::from_le_bytes(*b"GR88");
}

/// The device an image is imported on, and the functions an import needs of it.
#[derive(Clone, Copy)]
pub struct On<'a> {
    /// The instance the device was made from.
    pub instance: &'a ash::Instance,
    /// The device's GPU.
    pub physical: vk::PhysicalDevice,
    /// The device.
    pub device: &'a ash::Device,
    /// `VK_KHR_external_memory_fd`, loaded for the device.
    pub memory_fd: &'a khr::external_memory_fd::Device,
}

/// Where the planes of a picture are in its one buffer, and the formats its views read it in.
pub struct ImageLayout<const PLANES: usize> {
    /// The image's own format.
    pub format: vk::Format,
    /// Each plane's offset and pitch in the buffer.
    pub planes: [vk::SubresourceLayout; PLANES],
    /// The buffer's DRM format modifier.
    pub modifier: u64,
    /// Every format a view of the image takes besides its own; none for an image read only as itself.
    pub views: &'static [vk::Format],
}

/// A plane at `offset` whose rows are `row_pitch` bytes apart.
pub fn plane(offset: u64, row_pitch: u64) -> vk::SubresourceLayout {
    vk::SubresourceLayout { offset, row_pitch, size: 0, array_pitch: 0, depth_pitch: 0 }
}

/// Why a buffer did not become an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The device does not read it: another GPU's buffer, or a format or modifier the device says it cannot read.
    Unreadable,
    /// A side is larger than the device reads in that format and modifier.
    Size,
}

/// What a shader does with an imported image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Use {
    /// Samples it: a picture another API made, to be shown or converted.
    Read,
    /// Writes it as a storage image: a picture another API reads once the write is done.
    Written,
}

impl Use {
    const fn flags(self) -> vk::ImageUsageFlags {
        match self {
            Self::Read => vk::ImageUsageFlags::SAMPLED,
            Self::Written => vk::ImageUsageFlags::STORAGE,
        }
    }
}

/// `fd`'s buffer as an image of `size` laid out as `layout` says, read or written where it is as `usage` says: its
/// memory imported, never copied, and the fd owned by that memory once the import is made. Refused before any image is
/// made where the device does not take the format and modifier from a DMA-BUF for that use at that size.
///
/// # Safety
/// `on` is a live device that outlives the image and memory returned. `fd` is a DMA-BUF holding a picture of `size`
/// laid out as `layout` says.
pub unsafe fn import<const PLANES: usize>(
    on: &On<'_>,
    size: vk::Extent2D,
    layout: &ImageLayout<PLANES>,
    usage: Use,
    fd: std::os::fd::OwnedFd,
) -> Result<(vk::Image, vk::DeviceMemory), Refused> {
    let On { instance, physical, device, memory_fd } = *on;
    // Plane views of a multi-plane image read each plane in a format of its own, which the image lists.
    let flags = if layout.views.is_empty() {
        vk::ImageCreateFlags::empty()
    } else {
        vk::ImageCreateFlags::MUTABLE_FORMAT | vk::ImageCreateFlags::EXTENDED_USAGE
    };
    let (tiling, usage) = (vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT, usage.flags());
    // SAFETY: the physical device is the importer's, of its live instance.
    let readable = unsafe { reads(instance, physical, layout, flags, usage) };
    if !readable.is_some_and(|largest| size.width <= largest.width && size.height <= largest.height) {
        return Err(if readable.is_some() { Refused::Size } else { Refused::Unreadable });
    }
    let mut external =
        vk::ExternalMemoryImageCreateInfo::default().handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
    let mut modifier = vk::ImageDrmFormatModifierExplicitCreateInfoEXT::default()
        .drm_format_modifier(layout.modifier)
        .plane_layouts(&layout.planes);
    let mut views = vk::ImageFormatListCreateInfo::default().view_formats(layout.views);
    let mut info = vk::ImageCreateInfo::default()
        .flags(flags)
        .image_type(vk::ImageType::TYPE_2D)
        .format(layout.format)
        .extent(vk::Extent3D { width: size.width, height: size.height, depth: 1 })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(tiling)
        .usage(usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .push_next(&mut external)
        .push_next(&mut modifier);
    if !layout.views.is_empty() {
        info = info.push_next(&mut views);
    }
    // SAFETY: the device says it takes this format, modifier, usage and size from a DMA-BUF (asked above), and the
    // DRM layout pushed is the one its producer exported for `layout`.
    let image = unsafe { device.create_image(&info, None) }.map_err(|_| Refused::Unreadable)?;
    let raw_fd = fd.as_raw_fd();
    let imported = (|| {
        // SAFETY: `image` was made on this device just above and no line has destroyed it.
        let requirements = unsafe { device.get_image_memory_requirements(image) };
        let mut properties = vk::MemoryFdPropertiesKHR::default();
        // SAFETY: `raw_fd` is the DMA-BUF `fd` still holds open, and the handle type is the kind that fd reports.
        unsafe {
            memory_fd.get_memory_fd_properties(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT, raw_fd, &mut properties)
        }
        .map_err(|_| Refused::Unreadable)?;
        // SAFETY: the physical device belongs to the importer's live instance.
        let memory_types = unsafe { instance.get_physical_device_memory_properties(physical) };
        let usable = requirements.memory_type_bits & properties.memory_type_bits;
        let index = (0..memory_types.memory_type_count).find(|i| usable & (1 << i) != 0).ok_or(Refused::Unreadable)?;
        let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
        let mut import =
            vk::ImportMemoryFdInfoKHR::default().handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT).fd(raw_fd);
        let allocate = vk::MemoryAllocateInfo::default()
            .allocation_size(requirements.size)
            .memory_type_index(index)
            .push_next(&mut import)
            .push_next(&mut dedicated);
        // SAFETY: `allocate` asks for `requirements.size` at an index both the image requires and the fd accepts,
        // and `image` is that memory's dedicated image, so the allocation is one memory for one image.
        let memory = unsafe { device.allocate_memory(&allocate, None) }.map_err(|_| Refused::Unreadable)?;
        // The allocation owns the descriptor from here on; a failure before it leaves `fd` to close itself.
        let _ = fd.into_raw_fd();
        // SAFETY: `image` and `memory` were made on this device for each other in this call, and neither has been
        // bound or submitted before.
        unsafe { device.bind_image_memory(image, memory, 0) }.map_err(|_| {
            // SAFETY: the bind failed, so nothing refers to `memory`; this is the one free of it.
            unsafe { device.free_memory(memory, None) };
            Refused::Unreadable
        })?;
        Ok(memory)
    })();
    match imported {
        Ok(memory) => Ok((image, memory)),
        Err(refused) => {
            // SAFETY: the import failed, so no memory is bound to `image` and no submission names it; this is the
            // one destroy of it.
            unsafe { device.destroy_image(image, None) };
            Err(refused)
        }
    }
}

/// The largest picture the device reads from a DMA-BUF of `layout`'s format and modifier, made with `flags` for
/// `usage`, or `None` where it reads none: the device is asked before an image is made, as the modifier requires.
///
/// # Safety
/// `physical` belongs to `instance`, which is live.
unsafe fn reads<const PLANES: usize>(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
    layout: &ImageLayout<PLANES>,
    flags: vk::ImageCreateFlags,
    usage: vk::ImageUsageFlags,
) -> Option<vk::Extent3D> {
    let mut modifier = vk::PhysicalDeviceImageDrmFormatModifierInfoEXT::default()
        .drm_format_modifier(layout.modifier)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    let mut external = vk::PhysicalDeviceExternalImageFormatInfo::default()
        .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
    let mut views = vk::ImageFormatListCreateInfo::default().view_formats(layout.views);
    let mut asked = vk::PhysicalDeviceImageFormatInfo2::default()
        .format(layout.format)
        .ty(vk::ImageType::TYPE_2D)
        .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
        .usage(usage)
        .flags(flags)
        .push_next(&mut modifier)
        .push_next(&mut external);
    if !layout.views.is_empty() {
        asked = asked.push_next(&mut views);
    }
    let mut external_answer = vk::ExternalImageFormatProperties::default();
    let mut answer = vk::ImageFormatProperties2::default().push_next(&mut external_answer);
    // SAFETY: the caller's promise; every structure chained is one this query takes.
    unsafe { instance.get_physical_device_image_format_properties2(physical, &asked, &mut answer) }.ok()?;
    let largest = answer.image_format_properties.max_extent;
    let importable = external_answer
        .external_memory_properties
        .external_memory_features
        .contains(vk::ExternalMemoryFeatureFlags::IMPORTABLE);
    importable.then_some(largest)
}
