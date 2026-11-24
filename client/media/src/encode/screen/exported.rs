//! A VA-API encoder's NV12 frames as images our pass writes: each surface of the encoder's pool exported once
//! (`vaExportSurfaceHandle`) and its two layers imported onto the pass's Vulkan device where they are, kept for as long
//! as the pool. FFmpeg's `av_hwframe_map` would export and close them again for every picture.

use std::os::fd::{FromRawFd, OwnedFd};

use ash::{khr, vk};

use super::pass::{Loaded, Plane};
use super::va::{
    EXPORT_SEPARATE_LAYERS, EXPORT_WRITE_ONLY, MEM_TYPE_DRM_PRIME_2, PrimeDescriptor, PrimeLayer, SUCCESS,
    va_export_surface_handle,
};
use crate::av::{Frame, HwFrames, VaDisplay};
use crate::device::vulkan_image::fourcc::{GR88, NV12, R8};
use crate::device::vulkan_image::{self, ImageLayout, On, Use, plane};
use crate::encode::EncodeError;

/// One layer of a surface as an image of the device.
struct Imported {
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
}

/// One surface's luma and chroma.
struct Planes {
    luma: Imported,
    chroma: Imported,
}

impl Planes {
    fn written(&self) -> [Plane; 2] {
        [&self.luma, &self.chroma].map(|layer| Plane { image: layer.image, view: layer.view })
    }
}

/// The encoder's pool, and the planes of each of its surfaces handed out so far.
pub(super) struct Exported {
    pool: HwFrames,
    display: VaDisplay,
    /// The pictures' width and height, both even.
    size: [u32; 2],
    instance: ash::Instance,
    physical: vk::PhysicalDevice,
    device: ash::Device,
    memory_fd: khr::external_memory_fd::Device,
    /// Each surface written so far, by its id: never more than the pool's fixed count of surfaces.
    planes: Vec<(u32, Planes)>,
}

impl Exported {
    /// The planes of `pool`'s surfaces, a VA-API pool of NV12 whose size is fixed, imported onto `loaded`'s device.
    pub(super) fn new(pool: HwFrames, loaded: &Loaded) -> Result<Self, EncodeError> {
        let side = |value: u32| Some(value).filter(|side| *side > 0 && side % 2 == 0);
        let (Some(width), Some(height)) = (side(pool.size().0), side(pool.size().1)) else {
            return Err(EncodeError::Settings);
        };
        // A pool that grows has no list of its surfaces, which is what bounds the planes kept.
        if pool.pool_size() == 0 {
            return Err(EncodeError::Screen);
        }
        let display = pool.vaapi_display().ok_or(EncodeError::Screen)?;
        let memory_fd = khr::external_memory_fd::Device::new(&loaded.instance, &loaded.device);
        Ok(Self {
            pool,
            display,
            size: [width, height],
            instance: loaded.instance.clone(),
            physical: loaded.physical,
            device: loaded.device.clone(),
            memory_fd,
            planes: Vec::new(),
        })
    }

    /// The pool the encoder is opened on.
    pub(super) fn pool(&self) -> &HwFrames {
        &self.pool
    }

    /// The pictures' width and height.
    pub(super) const fn size(&self) -> [u32; 2] {
        self.size
    }

    /// A free frame of the pool, and its luma and chroma as the pass writes them: its surface exported and imported
    /// the first time the pool hands it out.
    pub(super) fn next(&mut self) -> Result<(Frame, [Plane; 2]), EncodeError> {
        let frame = self.pool.take().map_err(|_| EncodeError::Convert)?;
        // A VA-API picture names its surface in its fourth plane pointer.
        let id = frame.vaapi_surface().ok_or(EncodeError::Convert)?;
        if let Some((_, planes)) = self.planes.iter().find(|(kept, _)| *kept == id) {
            return Ok((frame, planes.written()));
        }
        if !self.pool.has_vaapi_surface(id) {
            return Err(EncodeError::Convert);
        }
        let planes = self.export(id)?;
        let written = planes.written();
        self.planes.push((id, planes));
        Ok((frame, written))
    }

    /// Surface `id` exported for writing and its two layers imported; the descriptors the export hands over are
    /// closed once each import holds its own.
    fn export(&self, id: u32) -> Result<Planes, EncodeError> {
        let mut described = PrimeDescriptor::default();
        // SAFETY: the display is the pool's, live with it; `id` is one of its surfaces; the descriptor is room for what
        // a DRM PRIME 2 export writes.
        let status = unsafe {
            va_export_surface_handle(
                self.display.as_ptr(),
                id,
                MEM_TYPE_DRM_PRIME_2,
                EXPORT_WRITE_ONLY | EXPORT_SEPARATE_LAYERS,
                (&raw mut described).cast(),
            )
        };
        if status != SUCCESS {
            return Err(EncodeError::Screen);
        }
        let objects = usize::try_from(described.num_objects).unwrap_or(0).min(described.objects.len());
        let owned: Vec<Option<OwnedFd>> = described.objects[..objects]
            .iter()
            // SAFETY: a successful export hands the caller each object's descriptor to close, once.
            .map(|object| (object.fd >= 0).then(|| unsafe { OwnedFd::from_raw_fd(object.fd) }))
            .collect();
        let [width, height] = self.size;
        let laid_out = described.fourcc == NV12
            && described.num_layers == 2
            && described.width >= width
            && described.height >= height
            && objects == usize::try_from(described.num_objects).unwrap_or(0);
        if !laid_out {
            return Err(EncodeError::Screen);
        }
        let layer = |at: usize, drm, format, size| {
            let layer: &PrimeLayer = &described.layers[at];
            let object = usize::try_from(layer.object_index[0]).map_err(|_| EncodeError::Screen)?;
            let Some(Some(fd)) = owned.get(object) else { return Err(EncodeError::Screen) };
            if layer.drm_format != drm || layer.num_planes != 1 {
                return Err(EncodeError::Screen);
            }
            let modifier = described.objects[object].drm_format_modifier;
            let layout = ImageLayout {
                format,
                planes: [plane(layer.offset[0].into(), layer.pitch[0].into())],
                modifier,
                views: &[],
            };
            self.import(&layout, size, fd.try_clone().map_err(|_| EncodeError::Convert)?)
        };
        let luma = layer(0, R8, vk::Format::R8_UNORM, vk::Extent2D { width, height })?;
        let half = vk::Extent2D { width: width / 2, height: height / 2 };
        match layer(1, GR88, vk::Format::R8G8_UNORM, half) {
            Ok(chroma) => Ok(Planes { luma, chroma }),
            Err(refused) => {
                // SAFETY: imported just above on this device, and never used.
                unsafe { self.free(&luma) };
                Err(refused)
            }
        }
    }

    /// One layer imported as a storage image of `size` and its view; refused (`Screen`) where the device does not
    /// write that format and modifier without naming the format, as the shader does.
    fn import(&self, layout: &ImageLayout<1>, size: vk::Extent2D, fd: OwnedFd) -> Result<Imported, EncodeError> {
        if !self.writes_without_format(layout.format, layout.modifier) {
            return Err(EncodeError::Screen);
        }
        let on =
            On { instance: &self.instance, physical: self.physical, device: &self.device, memory_fd: &self.memory_fd };
        // SAFETY: the device is live and outlives the planes, which `clear` frees; `fd` is the layer's DMA-BUF, laid
        // out as the export says.
        let (image, memory) =
            unsafe { vulkan_image::import(&on, size, layout, Use::Written, fd) }.map_err(|_| EncodeError::Screen)?;
        let range =
            vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1);
        let view = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(layout.format)
            .subresource_range(range);
        // SAFETY: `image` was made on this device just above, in the view's format.
        match unsafe { self.device.create_image_view(&view, None) } {
            Ok(view) => Ok(Imported { image, memory, view }),
            Err(_) => {
                // SAFETY: made just above and never used.
                unsafe { self.device.destroy_image(image, None) };
                // SAFETY: bound only to that image, now destroyed.
                unsafe { self.device.free_memory(memory, None) };
                Err(EncodeError::Screen)
            }
        }
    }

    /// Whether the GPU writes `format` laid out as `modifier` as a storage image whose format the shader leaves out.
    fn writes_without_format(&self, format: vk::Format, modifier: u64) -> bool {
        let mut count = vk::DrmFormatModifierPropertiesList2EXT::default();
        let mut properties = vk::FormatProperties2::default().push_next(&mut count);
        // SAFETY: the GPU is the device's, of its live instance, and the chain is the one the extension defines.
        unsafe { self.instance.get_physical_device_format_properties2(self.physical, format, &mut properties) };
        let mut listed = vec![
            vk::DrmFormatModifierProperties2EXT::default();
            usize::try_from(count.drm_format_modifier_count).unwrap_or(0)
        ];
        let mut list = vk::DrmFormatModifierPropertiesList2EXT::default().drm_format_modifier_properties(&mut listed);
        let mut properties = vk::FormatProperties2::default().push_next(&mut list);
        // SAFETY: as above, with room for as many as the first call said.
        unsafe { self.instance.get_physical_device_format_properties2(self.physical, format, &mut properties) };
        listed.iter().any(|each| {
            each.drm_format_modifier == modifier
                && each.drm_format_modifier_plane_count == 1
                && each.drm_format_modifier_tiling_features.contains(
                    vk::FormatFeatureFlags2::STORAGE_IMAGE | vk::FormatFeatureFlags2::STORAGE_WRITE_WITHOUT_FORMAT,
                )
        })
    }

    /// Frees one imported layer.
    ///
    /// # Safety
    /// `layer` was imported on this device and nothing pending writes it.
    unsafe fn free(&self, layer: &Imported) {
        // SAFETY: the caller's promise.
        unsafe { self.device.destroy_image_view(layer.view, None) };
        // SAFETY: as above; the view of it is gone.
        unsafe { self.device.destroy_image(layer.image, None) };
        // SAFETY: bound only to that image, now destroyed.
        unsafe { self.device.free_memory(layer.memory, None) };
    }

    /// Frees every surface's planes.
    ///
    /// # Safety
    /// Nothing pending writes any of them.
    pub(super) unsafe fn clear(&mut self) {
        for (_, planes) in std::mem::take(&mut self.planes) {
            // SAFETY: the caller's promise.
            unsafe { self.free(&planes.luma) };
            // SAFETY: as for the luma.
            unsafe { self.free(&planes.chroma) };
        }
    }

    /// How many surfaces have been exported, one each.
    #[cfg(test)]
    pub(super) fn exported(&self) -> usize {
        self.planes.len()
    }
}
