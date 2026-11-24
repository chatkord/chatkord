//! A captured screen still being written when it is lent is read only once its writer is done: the window's hand-over
//! of the buffer waits on the fence the kernel keeps on it. A second device on the window's GPU stands in for the
//! compositor: its buffer is Vulkan's own, which leaves the kernel no sync to do (amdgpu syncs a buffer made for
//! implicit sync itself, as a compositor's GBM buffers are), and it draws on the compute queue, another engine than the
//! window's, so only the window's wait can put the hand-over after the drawing.

use crate::common;

use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::time::{Duration, Instant};

use ash::{ext, khr, vk};
use capture::{DmaBuf, Frame, Layout, Modifier, Native, Pixels};
use common::{FORMAT, skipped};
use picture::{Current, Painter, Slot};
use wgpu::hal::api::Vulkan;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;
/// How long the stand-in compositor draws: far longer than a hand-over, so one that does not wait shows.
const DRAWING: Duration = Duration::from_millis(300);
/// The memory the stand-in compositor fills, again and again, to draw for [`DRAWING`].
const FILLED: vk::DeviceSize = 64 << 20;

/// The kernel's `struct dma_buf_import_sync_file`.
#[repr(C)]
struct ImportSyncFile {
    flags: u32,
    fd: i32,
}

/// `DMA_BUF_SYNC_WRITE`: the fence is a writer's, which every read waits on.
const SYNC_WRITE: u32 = 2;
/// `DMA_BUF_IOCTL_IMPORT_SYNC_FILE`, `_IOW('b', 3, struct dma_buf_import_sync_file)`.
const IMPORT_SYNC_FILE: rustix::ioctl::Opcode = rustix::ioctl::opcode::write::<ImportSyncFile>(b'b', 3);

/// The stand-in compositor: a device of the window's GPU with a compute queue, a linear BGRA picture it exports as a
/// DMA-BUF, and memory to fill.
struct Compositor {
    device: ash::Device,
    semaphore_fd: khr::external_semaphore_fd::Device,
    queue: vk::Queue,
    pool: vk::CommandPool,
    image: vk::Image,
    image_memory: vk::DeviceMemory,
    fill: vk::Buffer,
    fill_memory: vk::DeviceMemory,
    fd: OwnedFd,
    stride: u32,
}

impl Compositor {
    /// On the GPU of `window`, whose instance outlives it; none where the GPU has no queue that only computes.
    fn new(window: &wgpu::Device) -> Option<Self> {
        // SAFETY: the handles are used while `window`, whose instance they belong to, lives.
        let hal = unsafe { window.as_hal::<Vulkan>() }?;
        let (instance, physical) = (hal.shared_instance().raw_instance(), hal.raw_physical_device());
        // SAFETY: a physical device of this instance.
        let families = unsafe { instance.get_physical_device_queue_family_properties(physical) };
        let family = families.iter().position(|family| {
            family.queue_flags.contains(vk::QueueFlags::COMPUTE)
                && !family.queue_flags.contains(vk::QueueFlags::GRAPHICS)
        })? as u32;
        let priorities = [1.0];
        let queues = [vk::DeviceQueueCreateInfo::default().queue_family_index(family).queue_priorities(&priorities)];
        let extensions = [
            khr::external_memory_fd::NAME.as_ptr(),
            ext::external_memory_dma_buf::NAME.as_ptr(),
            ext::image_drm_format_modifier::NAME.as_ptr(),
            khr::external_semaphore_fd::NAME.as_ptr(),
        ];
        let info = vk::DeviceCreateInfo::default().queue_create_infos(&queues).enabled_extension_names(&extensions);
        // SAFETY: a device of a physical device of this instance, with extensions the window's device has too.
        let device = unsafe { instance.create_device(physical, &info, None) }.ok()?;
        // SAFETY: the one queue asked for of this family.
        let queue = unsafe { device.get_device_queue(family, 0) };
        let pool = vk::CommandPoolCreateInfo::default().queue_family_index(family);
        // SAFETY: a pool of this device's family.
        let pool = unsafe { device.create_command_pool(&pool, None) }.expect("a command pool");
        // SAFETY: a physical device of this instance.
        let types = unsafe { instance.get_physical_device_memory_properties(physical) };
        let local = |bits: u32| {
            (0..types.memory_type_count)
                .find(|index| {
                    bits & (1 << index) != 0
                        && types.memory_types[*index as usize]
                            .property_flags
                            .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
                })
                .expect("device memory")
        };

        let modifiers = [Modifier::LINEAR.raw()];
        let mut listed = vk::ImageDrmFormatModifierListCreateInfoEXT::default().drm_format_modifiers(&modifiers);
        let mut external =
            vk::ExternalMemoryImageCreateInfo::default().handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        let image = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk::Format::B8G8R8A8_UNORM)
            .extent(vk::Extent3D { width: WIDTH, height: HEIGHT, depth: 1 })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
            .usage(vk::ImageUsageFlags::TRANSFER_DST)
            .push_next(&mut listed)
            .push_next(&mut external);
        // SAFETY: a linear image of this device, exportable as a DMA-BUF.
        let image = unsafe { device.create_image(&image, None) }.expect("an exportable image");
        // SAFETY: `image` is live.
        let needs = unsafe { device.get_image_memory_requirements(image) };
        let mut export =
            vk::ExportMemoryAllocateInfo::default().handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
        let allocate = vk::MemoryAllocateInfo::default()
            .allocation_size(needs.size)
            .memory_type_index(local(needs.memory_type_bits))
            .push_next(&mut export)
            .push_next(&mut dedicated);
        // SAFETY: memory for `image` alone, of a type it accepts.
        let image_memory = unsafe { device.allocate_memory(&allocate, None) }.expect("the picture's memory");
        // SAFETY: fresh memory of the size the image needs, bound once.
        unsafe { device.bind_image_memory(image, image_memory, 0) }.expect("bound");
        let plane = vk::ImageSubresource::default().aspect_mask(vk::ImageAspectFlags::MEMORY_PLANE_0_EXT);
        // SAFETY: a modifier-tiled image's first memory plane.
        let stride = unsafe { device.get_image_subresource_layout(image, plane) }.row_pitch as u32;
        let get = vk::MemoryGetFdInfoKHR::default()
            .memory(image_memory)
            .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        // SAFETY: memory allocated exportable as a DMA-BUF.
        let fd = unsafe { khr::external_memory_fd::Device::new(instance, &device).get_memory_fd(&get) }.expect("an fd");

        let fill = vk::BufferCreateInfo::default().size(FILLED).usage(vk::BufferUsageFlags::TRANSFER_DST);
        // SAFETY: a plain buffer of this device.
        let fill = unsafe { device.create_buffer(&fill, None) }.expect("a buffer");
        // SAFETY: `fill` is live.
        let needs = unsafe { device.get_buffer_memory_requirements(fill) };
        let allocate = vk::MemoryAllocateInfo::default()
            .allocation_size(needs.size)
            .memory_type_index(local(needs.memory_type_bits));
        // SAFETY: memory for `fill`, of a type it accepts.
        let fill_memory = unsafe { device.allocate_memory(&allocate, None) }.expect("its memory");
        // SAFETY: fresh memory of the size the buffer needs, bound once.
        unsafe { device.bind_buffer_memory(fill, fill_memory, 0) }.expect("bound");
        let semaphore_fd = khr::external_semaphore_fd::Device::new(instance, &device);
        Some(Self {
            semaphore_fd,
            queue,
            pool,
            image,
            image_memory,
            fill,
            fill_memory,
            // SAFETY: the export made this file for us.
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
            stride,
            device,
        })
    }

    /// The picture as a capture would lend it.
    fn frame(&self) -> Frame {
        let fd = self.fd.try_clone().expect("duplicate the descriptor");
        let dmabuf = DmaBuf::new(fd, 0, self.stride, Modifier::LINEAR, ()).expect("a stride");
        let layout = Layout { pixels: Pixels::Bgrx, width: WIDTH, height: HEIGHT };
        Frame::new(layout, Native::DmaBuf(dmabuf), Instant::now())
    }

    /// Submits `fills` fills of its memory and returns, at once, the sync file their end signals; [`Drawing::end`]
    /// waits for them.
    fn draw(&self, fills: u32) -> Drawing<'_> {
        let allocate = vk::CommandBufferAllocateInfo::default().command_pool(self.pool).command_buffer_count(1);
        // SAFETY: a primary buffer of this device's pool.
        let commands = unsafe { self.device.allocate_command_buffers(&allocate) }.expect("a command buffer")[0];
        let begin = vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        // SAFETY: a fresh buffer, recorded once.
        unsafe { self.device.begin_command_buffer(commands, &begin) }.expect("recording");
        let after = [vk::MemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)];
        for fill in 0..fills {
            // SAFETY: the whole of a buffer made for transfers into it.
            unsafe { self.device.cmd_fill_buffer(commands, self.fill, 0, vk::WHOLE_SIZE, fill) };
            // SAFETY: a barrier between two transfers of this buffer, outside any pass.
            unsafe {
                self.device.cmd_pipeline_barrier(
                    commands,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &after,
                    &[],
                    &[],
                )
            };
        }
        // SAFETY: as above.
        unsafe { self.device.end_command_buffer(commands) }.expect("recorded");
        let mut export =
            vk::ExportSemaphoreCreateInfo::default().handle_types(vk::ExternalSemaphoreHandleTypeFlags::SYNC_FD);
        // SAFETY: a binary semaphore of this device that can be exported as a sync file.
        let semaphore =
            unsafe { self.device.create_semaphore(&vk::SemaphoreCreateInfo::default().push_next(&mut export), None) }
                .expect("a semaphore");
        // SAFETY: a fence of this device, unsignalled.
        let fence = unsafe { self.device.create_fence(&vk::FenceCreateInfo::default(), None) }.expect("a fence");
        let (buffers, signals) = ([commands], [semaphore]);
        let submit = vk::SubmitInfo::default().command_buffers(&buffers).signal_semaphores(&signals);
        // SAFETY: a recorded buffer of this queue's family; this queue is used by this thread alone.
        unsafe { self.device.queue_submit(self.queue, &[submit], fence) }.expect("submitted");
        let get = vk::SemaphoreGetFdInfoKHR::default()
            .semaphore(semaphore)
            .handle_type(vk::ExternalSemaphoreHandleTypeFlags::SYNC_FD);
        // SAFETY: the semaphore's signal is submitted, which a sync file's export needs.
        let written = unsafe { self.semaphore_fd.get_semaphore_fd(&get) }.expect("the drawing's end as a sync file");
        // SAFETY: the export made this file for us.
        Drawing { on: self, commands, semaphore, fence, written: unsafe { OwnedFd::from_raw_fd(written) } }
    }

    /// The fills of its memory that last about [`DRAWING`] on this GPU, found by timing a few.
    fn fills_for_drawing(&self) -> u32 {
        let mut fills = 4;
        loop {
            let started = Instant::now();
            self.draw(fills).end();
            let took = started.elapsed();
            if took >= DRAWING / 4 || fills >= 1 << 16 {
                let scaled = f64::from(fills) * DRAWING.as_secs_f64() / took.as_secs_f64();
                #[expect(clippy::cast_sign_loss, reason = "a positive count, bounded below")]
                return scaled.clamp(1.0, f64::from(1 << 16)) as u32;
            }
            fills *= 4;
        }
    }
}

impl Drop for Compositor {
    fn drop(&mut self) {
        let device = &self.device;
        // SAFETY: every submission was waited for, so nothing uses these any more.
        unsafe { device.destroy_buffer(self.fill, None) };
        // SAFETY: as above; bound only to that buffer, now destroyed.
        unsafe { device.free_memory(self.fill_memory, None) };
        // SAFETY: as above, the window's import of it gone.
        unsafe { device.destroy_image(self.image, None) };
        // SAFETY: as above; bound only to that image, now destroyed.
        unsafe { device.free_memory(self.image_memory, None) };
        // SAFETY: as above; its buffers were freed with their drawings.
        unsafe { device.destroy_command_pool(self.pool, None) };
        // SAFETY: every object of the device is destroyed above.
        unsafe { device.destroy_device(None) };
    }
}

/// The stand-in compositor's drawing, submitted.
struct Drawing<'on> {
    on: &'on Compositor,
    commands: vk::CommandBuffer,
    semaphore: vk::Semaphore,
    fence: vk::Fence,
    /// A sync file that signals when the drawing ends.
    written: OwnedFd,
}

impl Drawing<'_> {
    /// Waits for the drawing's end, and when it came.
    fn end(self) -> Instant {
        let device = &self.on.device;
        // SAFETY: the fence of the drawing's submission.
        unsafe { device.wait_for_fences(&[self.fence], true, u64::MAX) }.expect("the drawing ends");
        let ended = Instant::now();
        // SAFETY: the submission that used it has ended.
        unsafe { device.destroy_fence(self.fence, None) };
        // SAFETY: as above; nothing waits on it, its sync file exported.
        unsafe { device.destroy_semaphore(self.semaphore, None) };
        // SAFETY: as above, a buffer of this pool.
        unsafe { device.free_command_buffers(self.on.pool, &[self.commands]) };
        ended
    }
}

/// Puts `fence` on `buffer` as its writer's, as a compositor's drawing does.
fn written_by(buffer: impl AsFd, fence: &OwnedFd) {
    let import = ImportSyncFile { flags: SYNC_WRITE, fd: fence.as_raw_fd() };
    // SAFETY: the opcode is the kernel's for this struct, which it only reads.
    let setter = unsafe { rustix::ioctl::Setter::<IMPORT_SYNC_FILE, _>::new(import) };
    // SAFETY: `buffer` is a DMA-BUF, which answers this ioctl.
    unsafe { rustix::ioctl::ioctl(buffer, setter) }.expect("the fence is put on the buffer");
}

mod lab {
    use super::*;

    mod timing {
        use super::*;

        #[test]
        fn a_captured_screen_is_handed_over_only_once_its_writer_is_done() {
            tier::lab();
            let instance = picture::instance(None);
            let adapters = picture::adapters(&instance);
            let Some(adapter) = adapters.iter().find(|adapter| {
                adapter.get_info().backend == wgpu::Backend::Vulkan
                    && adapter.features().contains(wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF)
            }) else {
                return skipped("no GPU here imports a DMA-BUF");
            };
            let descriptor = wgpu::DeviceDescriptor {
                required_features: adapter.features() & picture::IMPORT_FEATURES,
                ..Default::default()
            };
            let (window, queue) = picture::device(adapter, &descriptor).expect("the window's device");
            let Some(compositor) = Compositor::new(&window) else {
                return skipped("this GPU has no queue that only computes, for a drawing beside the window's");
            };
            let fills = compositor.fills_for_drawing();
            let captured = compositor.frame();
            let Native::DmaBuf(lent) = captured.native();
            let started = Instant::now();
            let drawing = compositor.draw(fills);
            written_by(lent.fd(), &drawing.written);

            let painter = Painter::new(&window, FORMAT);
            let slot = Slot::new();
            assert!(slot.lend(&captured), "an empty slot takes the picture");
            drop(captured);
            let mut current = Current::default();
            current
                .show(&window, &queue, &painter, &slot, slot.take().expect("the picture waits"))
                .expect("a picture this device holds");
            assert!(slot.takes_imports(), "a capture the device can import is not refused");

            let (drawn, handed) = std::thread::scope(|threads| {
                let handed = threads.spawn(|| {
                    window.poll(wgpu::PollType::wait_indefinitely()).expect("the hand-over ends");
                    Instant::now()
                });
                (drawing.end(), handed.join().expect("the hand-over's wait"))
            });
            drop(current);
            window.poll(wgpu::PollType::wait_indefinitely()).expect("the window lets the buffer go");
            let (drawn, handed) = (drawn - started, handed - started);
            println!("the drawing ended after {drawn:?}, the window's hand-over after {handed:?}");
            assert!(drawn >= DRAWING / 2, "the stand-in compositor drew for long enough to tell: {drawn:?}");
            // A thread's wake-up after its device's end may come a little late: well inside the drawing's length.
            assert!(handed + DRAWING / 10 >= drawn, "the hand-over waited for the drawing: {handed:?} of {drawn:?}");
            drop(compositor);
        }
    }
}
