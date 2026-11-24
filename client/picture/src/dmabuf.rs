//! A picture the driver owns, imported with no copy: a VA-API decode's DMA-BUF as an NV12 texture, a captured screen's
//! as `Bgra8Unorm`. wgpu's `texture_from_dmabuf_fd` cannot hold the producer's buffer until the GPU is done with it, so
//! each goes in through `texture_from_raw`.

use crate::choose::{Candidate, chosen};
use crate::release::Releaser;
use crate::yuv::{Imported, Painter};
use ash::{ext, khr, vk};
use capture::Native;
use media::Surface;
pub(crate) use media::startup::DRM;
use media::startup::{asleep, node_path, render_nodes};
use media::vulkan_image::fourcc::{GR88, NV12, R8};
use media::vulkan_image::{self, ImageLayout, On, Use, plane};
use std::os::fd::{FromRawFd, IntoRawFd, OwnedFd};
use std::sync::Arc;
use wgpu::hal::api::Vulkan;

/// What a decoded picture needs of the device, asked for when the device is made; a captured one needs only the
/// DMA-BUF half.
pub const FEATURES: wgpu::Features =
    wgpu::Features::TEXTURE_FORMAT_NV12.union(wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF);

/// Why a picture did not become a texture: a decoded one is then painted from memory, a captured one not at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The device was not made with what the picture needs of [`FEATURES`] and the hand-over from another API
    /// (`VK_EXT_queue_family_foreign`), or is not a Vulkan one.
    NoDevice,
    /// A picture that is not one NV12 buffer of two planes.
    Layout,
    /// The driver would not take the buffer: another GPU's, a modifier the device says it cannot read, or one whose
    /// writer's fence cannot be waited on.
    Driver,
    /// A side is larger than the device's largest texture.
    Size,
}

impl From<vulkan_image::Refused> for Refused {
    fn from(refused: vulkan_image::Refused) -> Self {
        match refused {
            vulkan_image::Refused::Unreadable => Self::Driver,
            vulkan_image::Refused::Size => Self::Size,
        }
    }
}

/// The render node of the GPU `info` describes, to decode on: another vendor's driver refuses the buffer, and may panic
/// on its modifier. `None` for a software adapter, or where the machine's GPUs cannot be read.
pub fn render_node(info: &wgpu::AdapterInfo) -> Option<String> {
    let mut found: Vec<String> = render_nodes(DRM)
        .filter(|entry| {
            media::pci_ids(&entry.path())
                .is_some_and(|(vendor, device)| u32::from(vendor.0) == info.vendor && u32::from(device) == info.device)
        })
        .map(|entry| node_path(&entry))
        .collect();
    found.sort();
    found.into_iter().next()
}

/// The render nodes under `root`, sorted: the GPUs there are, asleep or not.
pub(crate) fn present_nodes(root: &str) -> Vec<String> {
    let mut nodes: Vec<String> = render_nodes(root).map(|entry| node_path(&entry)).collect();
    nodes.sort();
    nodes
}

/// The render nodes of the GPUs the kernel has put to sleep, read before listing the adapters wakes them. A sleeping
/// GPU is not the desktop's and its decoder is not probed: on Machine B, an NVIDIA GPU's VA-API open woken for it
/// took 1.5 s to fail.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Asleep(Vec<String>);

impl Asleep {
    /// The GPUs asleep on this machine now.
    pub fn now() -> Self {
        Self::read(DRM)
    }

    /// Whether any GPU is asleep.
    pub(crate) fn any(&self) -> bool {
        !self.0.is_empty()
    }

    pub(crate) fn read(root: &str) -> Self {
        let mut nodes: Vec<String> = render_nodes(root).filter(asleep).map(|entry| node_path(&entry)).collect();
        nodes.sort();
        Self(nodes)
    }

    /// Whether the GPU on `node` is asleep.
    pub(crate) fn holds(&self, node: &str) -> bool {
        self.0.iter().any(|asleep| asleep == node)
    }
}

/// Whether H.264 decodes in hardware on VA-API on this render node, else its pictures are decoded elsewhere and cannot
/// be imported: a discrete NVIDIA GPU on Mesa's drivers paints well and has no VA-API decoder.
fn decodes_h264(node: &str) -> bool {
    let sources = media::Sources { render_nodes: vec![node.to_string()], ..media::Sources::default() };
    media::Decoder::open(media::Codec::H264, &sources)
        .is_ok_and(|decoder| decoder.path() == media::Path::Hardware(media::Backend::VaApi))
}

/// Whether `adapter` can take a decoder's pictures as they are, and on which render node; a GPU that was asleep is
/// taken as one that cannot, unprobed.
fn importing(adapter: &wgpu::Adapter, asleep: &Asleep) -> Option<String> {
    let info = adapter.get_info();
    if info.backend != wgpu::Backend::Vulkan || !adapter.features().contains(FEATURES) {
        return None;
    }
    render_node(&info).filter(|node| !asleep.holds(node) && decodes_h264(node))
}

/// The render node the decoder should open so its pictures are on the GPU the window paints with, or `None` where no
/// GPU here can import them (the pictures are then copied to memory).
pub(crate) fn painting_render_node() -> Option<String> {
    let asleep = Asleep::now();
    let adapters = crate::vulkan::Listing::of_window();
    // Not under the loader's lock: each probe takes it for its own decoder.
    let nodes: Vec<Option<String>> = adapters.iter().map(|adapter| importing(adapter, &asleep)).collect();
    let candidates: Vec<Candidate> =
        adapters.iter().zip(&nodes).map(|(adapter, node)| Candidate::of(adapter, true, node.is_some())).collect();
    chosen(&candidates, crate::vulkan::ORDER).and_then(|at| nodes[at].clone())
}

/// The adapter among `adapters` that paints `surface`, with the render node to open the decoder on when it also takes
/// the decoder's pictures as they are (`None`: painted from memory); `None` as a whole where no GPU here shows the
/// window. `asleep` is read before `adapters` were listed.
pub fn painting_that_presents(
    adapters: &[wgpu::Adapter],
    asleep: &Asleep,
    surface: &wgpu::Surface<'_>,
) -> Option<(wgpu::Adapter, Option<String>)> {
    let nodes: Vec<Option<String>> = adapters.iter().map(|adapter| importing(adapter, asleep)).collect();
    let candidates: Vec<Candidate> = adapters
        .iter()
        .zip(&nodes)
        .map(|(adapter, node)| Candidate::of(adapter, adapter.is_surface_supported(surface), node.is_some()))
        .collect();
    chosen(&candidates, crate::vulkan::ORDER).and_then(|at| Some((adapters.get(at)?.clone(), nodes.get(at)?.clone())))
}

/// An NV12 image's own format, and its planes' as their views read them.
const NV12_VIEWS: &[vk::Format] = &[vk::Format::G8_B8R8_2PLANE_420_UNORM, vk::Format::R8_UNORM, vk::Format::R8G8_UNORM];

fn layout(surface: &Surface) -> Result<ImageLayout<2>, Refused> {
    let mut objects = surface.objects();
    let (Some(object), None) = (objects.next(), objects.next()) else { return Err(Refused::Layout) };
    let planes: Vec<_> = match surface.layers() {
        [one] if one.fourcc == NV12 => one.planes.iter().collect(),
        [luma, chroma] if luma.fourcc == R8 && chroma.fourcc == GR88 => {
            luma.planes.iter().chain(&chroma.planes).collect()
        }
        _ => return Err(Refused::Layout),
    };
    let [luma, chroma] = planes.as_slice() else { return Err(Refused::Layout) };
    Ok(ImageLayout {
        format: vk::Format::G8_B8R8_2PLANE_420_UNORM,
        planes: [plane(luma.offset, luma.pitch), plane(chroma.offset, chroma.pitch)],
        modifier: object.modifier.raw(),
        views: NV12_VIEWS,
    })
}

/// The Vulkan functions an import and its hand-over need, loaded once for the device the window paints with: only on
/// a device that imports DMA-BUFs and has `VK_EXT_queue_family_foreign`, without which another API's buffer cannot be
/// handed to this device's queue, and `VK_KHR_external_semaphore_fd`, without which a read cannot wait for its writer.
pub(crate) struct Importer {
    instance: ash::Instance,
    physical: vk::PhysicalDevice,
    device: Arc<ash::Device>,
    memory_fd: khr::external_memory_fd::Device,
    semaphore_fd: khr::external_semaphore_fd::Device,
    family: u32,
    releaser: Releaser,
}

impl Importer {
    /// `None` where the device cannot take a buffer another API wrote.
    pub(crate) fn new(device: &wgpu::Device) -> Option<Self> {
        if !device.features().contains(wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF) {
            return None;
        }
        // SAFETY: the handle is only read here, while `device` lives.
        let hal = unsafe { device.as_hal::<Vulkan>() }?;
        let enabled = hal.enabled_device_extensions();
        if ![ext::queue_family_foreign::NAME, khr::external_semaphore_fd::NAME]
            .iter()
            .all(|name| enabled.contains(name))
        {
            return None;
        }
        let (instance, raw) = (hal.shared_instance().raw_instance().clone(), hal.raw_device().clone());
        let memory_fd = khr::external_memory_fd::Device::new(&instance, &raw);
        let semaphore_fd = khr::external_semaphore_fd::Device::new(&instance, &raw);
        let releaser = Releaser::start().ok()?;
        Some(Self {
            releaser,
            physical: hal.raw_physical_device(),
            family: hal.queue_family_index(),
            device: Arc::new(raw),
            memory_fd,
            semaphore_fd,
            instance,
        })
    }

    /// A semaphore of this device that `written`, a sync file, signals; it takes the file.
    fn signalled_by(&self, written: OwnedFd) -> Result<vk::Semaphore, Refused> {
        // SAFETY: a binary semaphore of this device, made with nothing chained.
        let semaphore = unsafe { self.device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) }
            .map_err(|_| Refused::Driver)?;
        let raw = written.into_raw_fd();
        let import = vk::ImportSemaphoreFdInfoKHR::default()
            .semaphore(semaphore)
            .flags(vk::SemaphoreImportFlags::TEMPORARY)
            .handle_type(vk::ExternalSemaphoreHandleTypeFlags::SYNC_FD)
            .fd(raw);
        // SAFETY: `semaphore` is this device's with no pending wait, and `raw` a sync file, which it takes on success.
        if unsafe { self.semaphore_fd.import_semaphore_fd(&import) }.is_err() {
            // SAFETY: refused, so the file is still ours, and nothing else holds it.
            drop(unsafe { OwnedFd::from_raw_fd(raw) });
            // SAFETY: made above and used by nothing.
            unsafe { self.device.destroy_semaphore(semaphore, None) };
            return Err(Refused::Driver);
        }
        Ok(semaphore)
    }

    /// Hands `image` from its producer, another API, to this device's queue, ahead of every frame that reads it, once
    /// `written`, the sync file its writer left, signals. The picture holds the producer's buffer until wgpu destroys
    /// the image, so it is never handed back.
    fn acquire(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        (texture, image): (&wgpu::Texture, vk::Image),
        written: Option<OwnedFd>,
    ) -> Result<(), Refused> {
        // SAFETY: the handle only stages a wait on the next submission, the one below, while `queue` lives.
        let hal = unsafe { queue.as_hal::<Vulkan>() }.ok_or(Refused::NoDevice)?;
        let wait = written.map(|written| self.signalled_by(written)).transpose()?;
        let barrier = vk::ImageMemoryBarrier::default()
            .src_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
            .dst_queue_family_index(self.family)
            .old_layout(vk::ImageLayout::GENERAL)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .dst_access_mask(vk::AccessFlags::SHADER_READ)
            .image(image)
            .subresource_range(
                vk::ImageSubresourceRange::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .level_count(1)
                    .layer_count(1),
            );
        let record = |hal: Option<&mut wgpu::hal::vulkan::CommandEncoder>| {
            let Some(hal) = hal else { return };
            // SAFETY: the buffer wgpu is recording for this encoder, which records nothing else, left open.
            let commands = unsafe { hal.raw_handle() };
            // SAFETY: `image` is a live image of this device, which the encoder's device is; no pass is open.
            unsafe {
                self.device.cmd_pipeline_barrier(
                    commands,
                    vk::PipelineStageFlags::ALL_COMMANDS,
                    vk::PipelineStageFlags::FRAGMENT_SHADER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier],
                )
            };
        };
        // wgpu refuses its own commands in an encoder that records raw ones, so the hand-over is a submission of its
        // own, ahead of the frames that draw the picture; the texture's use in it, told to wgpu, keeps the image until
        // the hand-over is done, should the picture be replaced before it is drawn.
        let mut raw = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("hand-over") });
        // SAFETY: the encoder is this function's, used by nothing else while `record` runs, and left open.
        unsafe { raw.as_hal_mut::<Vulkan, _, _>(record) };
        let mut used = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("handed over") });
        let read = wgpu::TextureTransition { texture, selector: None, state: wgpu::TextureUses::RESOURCE };
        used.transition_resources(std::iter::empty(), std::iter::once(read));
        if let Some(semaphore) = wait {
            // The barrier's first scope, every command, takes in this wait, so the hand-over follows the writer.
            hal.add_wait_semaphore(semaphore, None, vk::PipelineStageFlags::ALL_COMMANDS);
        }
        drop(hal);
        queue.submit([raw.finish(), used.finish()]);
        if let Some(semaphore) = wait {
            let device = Arc::clone(&self.device);
            // SAFETY: the submission that waits on it is done, and nothing else uses it.
            queue.on_submitted_work_done(move || unsafe { device.destroy_semaphore(semaphore, None) });
        }
        Ok(())
    }
}

impl Painter {
    /// Import `surface` as an NV12 picture to draw. The decoder reuses a surface it is given back, so the picture
    /// holds it until wgpu destroys its texture, after the last submission that reads it.
    pub fn import(&self, device: &wgpu::Device, queue: &wgpu::Queue, surface: Surface) -> Result<Imported, Refused> {
        let importer = self.importer().ok_or(Refused::NoDevice)?;
        if !crate::yuv::holds(device, (surface.width(), surface.height())) {
            return Err(Refused::Size);
        }
        let layout = layout(&surface)?;
        let object = surface.objects().next().ok_or(Refused::Layout)?;
        let fd = object.fd.try_clone_to_owned().map_err(|_| Refused::Driver)?;
        let size = (surface.width(), surface.height());
        let colours = surface.colours();
        // SAFETY: `layout` is the DRM plane layout the decoder exported for the NV12 picture in `fd`.
        let (texture, image) = unsafe {
            texture(device, importer, "decoded picture", wgpu::TextureFormat::NV12, size, &layout, fd, surface)
        }?;
        importer.acquire(device, queue, (&texture, image), None)?;
        self.bind(device, texture, colours, size).ok_or(Refused::NoDevice)
    }

    /// Import a captured screen as a `Bgra8Unorm` picture to draw. It takes the frame, so the compositor has its buffer
    /// back once wgpu destroys the texture, after the last submission that reads it.
    pub(crate) fn import_captured(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: capture::Frame,
    ) -> Result<Imported, Refused> {
        let importer = self.importer().ok_or(Refused::NoDevice)?;
        let size = (frame.layout().width, frame.layout().height);
        if !crate::yuv::holds(device, size) {
            return Err(Refused::Size);
        }
        let (layout, fd, written) = match frame.native() {
            Native::DmaBuf(dmabuf) => (
                ImageLayout {
                    format: vk::Format::B8G8R8A8_UNORM,
                    planes: [plane(dmabuf.offset().into(), dmabuf.stride().into())],
                    modifier: dmabuf.modifier().raw(),
                    views: &[],
                },
                dmabuf.fd().try_clone_to_owned().map_err(|_| Refused::Driver)?,
                dmabuf.written().map_err(|_| Refused::Driver)?,
            ),
        };
        // SAFETY: `layout` is the plane the compositor lent for the BGRx picture in `fd`, with its modifier.
        let (texture, image) = unsafe {
            texture(device, importer, "captured picture", wgpu::TextureFormat::Bgra8Unorm, size, &layout, fd, frame)
        }?;
        importer.acquire(device, queue, (&texture, image), Some(written))?;
        self.bind_rgb(device, texture, size).ok_or(Refused::NoDevice)
    }
}

/// # Safety
/// `fd` is a DMA-BUF holding a picture of `size` in `format`, laid out as `layout` says, and `owner` keeps the
/// producer from reusing it while it lives.
#[expect(clippy::too_many_arguments, reason = "one import's parts, each named where the two imports differ")]
unsafe fn texture<const PLANES: usize>(
    device: &wgpu::Device,
    importer: &Importer,
    label: &'static str,
    format: wgpu::TextureFormat,
    (width, height): (u32, u32),
    layout: &ImageLayout<PLANES>,
    fd: std::os::fd::OwnedFd,
    owner: impl Send + Sync + 'static,
) -> Result<(wgpu::Texture, vk::Image), Refused> {
    let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
    let descriptor = wgpu::TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    };
    // SAFETY: the device is a Vulkan one, checked by `as_hal`; the image is made to the descriptor the producer
    // exported, and the fd is a duplicate Vulkan owns from the allocation on.
    let hal = unsafe { device.as_hal::<Vulkan>() }.ok_or(Refused::NoDevice)?;
    let on = On {
        instance: &importer.instance,
        physical: importer.physical,
        device: &importer.device,
        memory_fd: &importer.memory_fd,
    };
    let extent = vk::Extent2D { width: size.width, height: size.height };
    // SAFETY: the importer is this device's, which outlives the texture; the caller promises `layout` and `fd`.
    let (image, memory) = unsafe { vulkan_image::import(&on, extent, layout, Use::Read, fd) }.map_err(Refused::from)?;
    let texture = hal_texture(&hal, importer, &descriptor, (image, memory), owner);
    drop(hal);
    // SAFETY: `texture` was made from this device to `descriptor`, and its contents are the producer's picture.
    let texture =
        unsafe { device.create_texture_from_hal::<Vulkan>(texture, &descriptor, wgpu::TextureUses::RESOURCE) };
    Ok((texture, image))
}

fn hal_texture(
    hal: &wgpu::hal::vulkan::Device,
    importer: &Importer,
    descriptor: &wgpu::TextureDescriptor<'_>,
    (image, memory): (vk::Image, vk::DeviceMemory),
    owner: impl Send + Sync + 'static,
) -> wgpu::hal::vulkan::Texture {
    let hal_descriptor = wgpu::hal::TextureDescriptor {
        label: descriptor.label,
        size: descriptor.size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: descriptor.dimension,
        format: descriptor.format,
        usage: wgpu::TextureUses::RESOURCE,
        memory_flags: wgpu::hal::MemoryFlags::empty(),
        view_formats: Vec::new(),
    };
    let (device, releaser) = (Arc::clone(&importer.device), importer.releaser.clone());
    let release = move || {
        // SAFETY: wgpu calls this once the texture is destroyed, so no submission uses the image any more.
        unsafe { device.destroy_image(image, None) };
        // SAFETY: the memory was bound only to that image, now destroyed.
        unsafe { device.free_memory(memory, None) };
        releaser.give(owner);
    };
    // SAFETY: image and memory were made together for this descriptor; `release` frees them, and gives the producer
    // its buffer back, when wgpu is done with the texture.
    unsafe {
        hal.texture_from_raw(
            image,
            &hal_descriptor,
            Some(Box::new(release)),
            wgpu::hal::vulkan::TextureMemory::External,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Asleep;

    #[test]
    fn only_a_gpu_the_kernel_put_to_sleep_is_asleep() {
        let root = std::env::temp_dir().join(format!("picture-asleep-{}", std::process::id()));
        for (node, status) in
            [("renderD128", Some("suspended\n")), ("renderD129", Some("active\n")), ("renderD130", None)]
        {
            let power = root.join(node).join("device/power");
            std::fs::create_dir_all(&power).unwrap();
            if let Some(status) = status {
                std::fs::write(power.join("runtime_status"), status).unwrap();
            }
        }
        std::fs::create_dir_all(root.join("card0/device/power")).unwrap();
        std::fs::write(root.join("card0/device/power/runtime_status"), "suspended\n").unwrap();
        let asleep = Asleep::read(root.to_str().unwrap());
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            asleep,
            Asleep(vec!["/dev/dri/renderD128".into()]),
            "awake, unmanaged and card nodes are not asleep"
        );
        assert!(asleep.holds("/dev/dri/renderD128") && !asleep.holds("/dev/dri/renderD129"));
    }
}
