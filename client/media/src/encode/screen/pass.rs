//! Our compute pass (`rgb2yuv.comp`) on a Vulkan device FFmpeg made: the compositor's DMA-BUF, imported, to the two
//! planes of NV12, a scaled pixel the mean of what it covers, written into a frame of FFmpeg's Vulkan pool or into
//! another API's surface imported as two images. A run into another API's surface is waited on, so what it wrote is
//! that API's when it returns; one into FFmpeg's frame is ordered by the frame's own semaphore, which the encoder
//! waits on, and the compositor's buffer it reads is held until its fence is waited, when its run is reused, the pass
//! settles or drops.

use std::collections::HashMap;
use std::mem;

use ash::vk;

use crate::av::{Frame, HwDevice, QueueLocks, VulkanImage};
use crate::encode::{EncodeError, Settings};
use capture::Frame as Captured;

/// The longest the GPU gets to convert one picture before it is called failed.
const GPU_WAIT_NANOSECONDS: u64 = 1_000_000_000;

/// Invocations along each side of a group, which `rgb2yuv.comp` fixes; each takes one pixel of the stream.
const GROUP: u32 = 8;

/// How many runs may be in flight at once: the one just submitted, which the encoder waits for on the GPU, and the
/// one before, which a GPU busy with a game's draws may not have finished when the next picture comes. Each more holds
/// one more of the compositor's few buffers, which capture counts on the encoder holding one of.
const IN_FLIGHT: usize = 2;

/// The id of the shader's SCALED, a 32-bit boolean: the exact read compiles without the averaging.
const SCALED_ID: u32 = 0;

/// The shader as SPIR-V words, as `scripts/shaders.sh` wrote them.
const SHADER: &[u32] = &include!("shader/rgb2yuv.spv.inc");

/// One plane of NV12 the pass writes: an image of its device, and the storage view it is written through.
#[derive(Clone, Copy)]
pub(super) struct Plane {
    /// The plane's image, in `R8_UNORM` for luma or `R8G8_UNORM` for chroma.
    pub(super) image: vk::Image,
    /// A storage view of all of it.
    pub(super) view: vk::ImageView,
}

/// What a run writes its NV12 into.
#[derive(Clone, Copy)]
pub(super) enum Target<'a> {
    /// A frame of FFmpeg's Vulkan pool (`AVVkFrame`): one image of two planes, ordered by its own semaphore, which
    /// the encoder waits on after.
    Frame(&'a Frame),
    /// Another API's NV12 surface as two images, held by that API between runs: every pixel is written, so what it
    /// held before is not kept.
    Planes {
        /// The luma plane, `size` big.
        luma: Plane,
        /// The chroma plane, half `size` each way.
        chroma: Plane,
        /// The picture's width and height, both even.
        size: [u32; 2],
    },
}

/// How the pass reads the screen: each pixel once, or averaged by area into a smaller stream.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Read {
    Exact,
    Scaled,
}

/// What one run records and submits, and the buffer it reads until its submission is waited.
struct Run {
    commands: vk::CommandBuffer,
    /// Signalled when the lent buffer's writers are done: the run waits on it before reading.
    written: vk::Semaphore,
    /// Signalled when the run's submission is done.
    done: vk::Fence,
    /// The compositor's buffer the submission in flight reads, given back once `done` is waited; none while nothing
    /// is in flight.
    reading: Option<Captured>,
}

/// The compute pass: its pipeline, the runs it submits in turn, and the plane views of the pool frames it writes.
pub(super) struct Pass {
    device: ash::Device,
    push: ash::khr::push_descriptor::Device,
    queue: vk::Queue,
    family: u32,
    lock: QueueLocks,
    /// Set when the GPU did not finish in time: what it still holds cannot be reused.
    broken: bool,
    /// Set when it did not finish and the device could not be idled either: its work may still be in flight, so
    /// nothing it touched may be freed.
    pub(super) lost: bool,
    pool: vk::CommandPool,
    /// The next to submit first, the last submitted last.
    runs: [Run; IN_FLIGHT],
    layout: vk::DescriptorSetLayout,
    pipeline_layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    /// What of the screen is read, from its top left: the shader's push constant.
    extent: [i32; 2],
    /// The luma and chroma views of each frame of a Vulkan pool, made the first time the frame is written.
    planes: HashMap<vk::Image, (vk::ImageView, vk::ImageView)>,
}

/// FFmpeg's Vulkan device as the pass and the imports call it.
pub(super) struct Loaded {
    /// The instance FFmpeg made the device from.
    pub(super) instance: ash::Instance,
    /// The device's GPU.
    pub(super) physical: vk::PhysicalDevice,
    /// The device, alive while the `HwDevice` it was loaded from is.
    pub(super) device: ash::Device,
    family: u32,
    queue: vk::Queue,
    lock: QueueLocks,
}

impl Loaded {
    /// `device`, a Vulkan device context, as the pass calls it: refused (`Screen`) unless FFmpeg enabled every
    /// extension the pass and the imports use, and the GPU writes both planes as the shader does.
    pub(super) fn of(device: &HwDevice) -> Result<Self, EncodeError> {
        let vulkan = device.vulkan().ok_or(EncodeError::Screen)?;
        let table = ash::StaticFn { get_instance_proc_addr: vulkan.get_instance_proc_addr };
        // SAFETY: `instance` is the live instance FFmpeg made, and `table` its loader.
        let instance = unsafe { ash::Instance::load(&table, vulkan.instance) };
        let physical = vulkan.physical;
        // SAFETY: `device` is the live device FFmpeg made from this instance.
        let dev = unsafe { ash::Device::load(instance.fp_v1_0(), vulkan.device) };
        let needed = [
            ash::khr::push_descriptor::NAME,
            ash::khr::external_memory_fd::NAME,
            ash::ext::external_memory_dma_buf::NAME,
            ash::khr::external_semaphore_fd::NAME,
            ash::ext::image_drm_format_modifier::NAME,
            ash::ext::queue_family_foreign::NAME,
        ];
        if !can_write_planes(&instance, physical) || !needed.iter().all(|name| vulkan.enabled(name)) {
            return Err(EncodeError::Screen);
        }
        let family = vulkan
            .families
            .iter()
            .find(|q| q.flags.contains(vk::QueueFlags::COMPUTE))
            .map(|q| q.index)
            .ok_or(EncodeError::Screen)?;
        // SAFETY: the family and its flags are ones FFmpeg created the device's queues with.
        let queue = unsafe {
            dev.get_device_queue2(
                &vk::DeviceQueueInfo2::default().flags(vulkan.queue_flags).queue_family_index(family).queue_index(0),
            )
        };
        Ok(Self { instance, physical, device: dev, family, queue, lock: vulkan.locks })
    }
}

/// What one submission writes, and how its target is handed to the pass and back.
struct Writes<'a> {
    luma: vk::ImageView,
    chroma: vk::ImageView,
    /// The picture's width and height.
    size: [u32; 2],
    /// The target's barriers before the pass writes it, and after.
    before: &'a [vk::ImageMemoryBarrier2<'a>],
    after: &'a [vk::ImageMemoryBarrier2<'a>],
    /// The target's timeline semaphore and the value it stands at, where FFmpeg orders it by one: the run waits for
    /// that value and leaves it one on.
    timeline: Option<(vk::Semaphore, u64)>,
}

impl Pass {
    /// The pass on `loaded` that reads the top-left `region` of screens into pictures of the settings' size,
    /// averaging by area where those are smaller.
    pub(super) fn new(loaded: Loaded, settings: Settings, region: (u32, u32)) -> Result<Self, EncodeError> {
        let side = |value: u32| i32::try_from(value).map_err(|_| EncodeError::Settings);
        let extent = [side(region.0)?, side(region.1)?];
        let read = if [settings.width, settings.height] == [region.0 as usize, region.1 as usize] {
            Read::Exact
        } else {
            Read::Scaled
        };
        let Loaded { instance, device, family, queue, lock, .. } = loaded;
        let push = ash::khr::push_descriptor::Device::new(&instance, &device);
        Self::build(device, push, queue, family, lock, read, extent)
    }

    fn build(
        device: ash::Device,
        push: ash::khr::push_descriptor::Device,
        queue: vk::Queue,
        family: u32,
        lock: QueueLocks,
        read: Read,
        extent: [i32; 2],
    ) -> Result<Self, EncodeError> {
        let mut pass = Self {
            device,
            push,
            queue,
            family,
            lock,
            broken: false,
            lost: false,
            pool: vk::CommandPool::null(),
            runs: std::array::from_fn(|_| Run {
                commands: vk::CommandBuffer::null(),
                written: vk::Semaphore::null(),
                done: vk::Fence::null(),
                reading: None,
            }),
            layout: vk::DescriptorSetLayout::null(),
            pipeline_layout: vk::PipelineLayout::null(),
            pipeline: vk::Pipeline::null(),
            extent,
            planes: HashMap::new(),
        };
        // Each object is stored as soon as it is made, so `Drop` frees what was made if a later step fails.
        let binding = |index, kind| {
            vk::DescriptorSetLayoutBinding::default()
                .binding(index)
                .descriptor_type(kind)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
        };
        let bindings = [
            binding(0, vk::DescriptorType::SAMPLED_IMAGE),
            binding(1, vk::DescriptorType::STORAGE_IMAGE),
            binding(2, vk::DescriptorType::STORAGE_IMAGE),
        ];
        // SAFETY: the device is live and the push-descriptor extension, which the flag needs, is enabled on it.
        pass.layout = unsafe {
            pass.device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default()
                    .flags(vk::DescriptorSetLayoutCreateFlags::PUSH_DESCRIPTOR_KHR)
                    .bindings(&bindings),
                None,
            )
        }
        .map_err(|_| EncodeError::Screen)?;
        let layouts = [pass.layout];
        #[expect(clippy::cast_possible_truncation, reason = "eight bytes")]
        const PUSHED: u32 = mem::size_of::<[i32; 2]>() as u32;
        let ranges = [vk::PushConstantRange::default().stage_flags(vk::ShaderStageFlags::COMPUTE).size(PUSHED)];
        // SAFETY: the device is live and the set layout was made from it.
        pass.pipeline_layout = unsafe {
            pass.device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default().set_layouts(&layouts).push_constant_ranges(&ranges),
                None,
            )
        }
        .map_err(|_| EncodeError::Screen)?;
        // SAFETY: the device is live and `SHADER` is SPIR-V; the module is destroyed once the pipeline is made.
        let module =
            unsafe { pass.device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(SHADER), None) }
                .map_err(|_| EncodeError::Screen)?;
        let scaled = vk::Bool32::from(read == Read::Scaled).to_ne_bytes();
        let entries = [vk::SpecializationMapEntry::default().constant_id(SCALED_ID).size(scaled.len())];
        let specialization = vk::SpecializationInfo::default().map_entries(&entries).data(&scaled);
        let stage = vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(module)
            .name(c"main")
            .specialization_info(&specialization);
        // SAFETY: the module and the pipeline layout were made from this live device.
        let made = unsafe {
            pass.device.create_compute_pipelines(
                vk::PipelineCache::null(),
                &[vk::ComputePipelineCreateInfo::default().stage(stage).layout(pass.pipeline_layout)],
                None,
            )
        };
        // SAFETY: the module was made from this device, and only the pipeline, now made or failed, needed it.
        unsafe { pass.device.destroy_shader_module(module, None) };
        pass.pipeline = made.map_err(|_| EncodeError::Screen)?[0];
        // SAFETY: the device is live and `family` one of its queue families.
        pass.pool = unsafe {
            pass.device.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
                    .queue_family_index(family),
                None,
            )
        }
        .map_err(|_| EncodeError::Screen)?;
        #[expect(clippy::cast_possible_truncation, reason = "a handful of runs")]
        const RUNS: u32 = IN_FLIGHT as u32;
        // SAFETY: the pool was made from this live device; its buffers go with it.
        let commands = unsafe {
            pass.device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(pass.pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(RUNS),
            )
        }
        .map_err(|_| EncodeError::Screen)?;
        for (run, commands) in pass.runs.iter_mut().zip(commands) {
            run.commands = commands;
            // SAFETY: the device is live; a binary semaphore, into which each of the run's turns imports the buffer's
            // fence.
            run.written = unsafe { pass.device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) }
                .map_err(|_| EncodeError::Screen)?;
            // SAFETY: the device is live; an unsignalled fence, which each of the run's submissions signals.
            run.done = unsafe { pass.device.create_fence(&vk::FenceCreateInfo::default(), None) }
                .map_err(|_| EncodeError::Screen)?;
        }
        Ok(pass)
    }

    /// Converts `screen` (`captured`'s buffer, read through `view`) into `target` once its writers are done, which
    /// `written` imports into the semaphore it is given. A frame of FFmpeg's pool is handed on at once, ordered by its
    /// own semaphore, and `captured` held until the run is waited; another API's planes are waited on before this
    /// returns. Buffers of earlier runs the GPU has finished go back first, and the oldest run is waited for when none
    /// is free. A GPU that does not finish in time ends the pass; `lost` says its work may still run.
    ///
    /// # Safety
    /// `screen` and `view` are an image of the device and its view, kept until the pass has settled; `target` is a
    /// live Vulkan frame of the pool the path was opened with, or two images of the device that nothing else uses
    /// until this returns.
    pub(super) unsafe fn run(
        &mut self,
        screen: vk::Image,
        view: vk::ImageView,
        target: Target<'_>,
        captured: Captured,
        written: impl FnOnce(&Captured, vk::Semaphore) -> Result<(), EncodeError>,
    ) -> Result<(), EncodeError> {
        if self.broken {
            return Err(EncodeError::Convert);
        }
        self.collect();
        self.wait(0)?;
        let [next, ..] = &self.runs;
        written(&captured, next.written)?;
        match target {
            // SAFETY: the caller's promise covers the screen and the frame.
            Target::Frame(frame) => unsafe { self.write_frame(screen, view, frame) },
            // SAFETY: the caller's promise covers the screen and the planes.
            Target::Planes { luma, chroma, size } => unsafe { self.write_planes(screen, view, [luma, chroma], size) },
        }?;
        let [submitted, ..] = &mut self.runs;
        submitted.reading = Some(captured);
        self.runs.rotate_left(1);
        match target {
            Target::Frame(_) => Ok(()),
            Target::Planes { .. } => self.wait(IN_FLIGHT - 1),
        }
    }

    /// Waits for every run in flight and gives its buffer back, so each buffer is its owner's again and what the runs
    /// read may be freed. Refused once the GPU did not finish in time, when nothing it read may be freed if `lost`.
    pub(super) fn settle(&mut self) -> Result<(), EncodeError> {
        if self.broken {
            return if self.lost { Err(EncodeError::Convert) } else { Ok(()) };
        }
        (0..IN_FLIGHT).try_for_each(|at| self.wait(at))
    }

    /// Gives back the buffers of the runs the GPU has finished, without waiting.
    fn collect(&mut self) {
        for run in &mut self.runs {
            // SAFETY: the run's own fence, live; a fence is only read.
            let finished = run.reading.is_some() && unsafe { self.device.get_fence_status(run.done) } == Ok(true);
            if finished {
                run.reading = None;
            }
        }
    }

    /// Waits for run `at`'s submission if one is in flight and gives its buffer back.
    fn wait(&mut self, at: usize) -> Result<(), EncodeError> {
        let Some(run) = self.runs.get_mut(at).filter(|run| run.reading.is_some()) else { return Ok(()) };
        // SAFETY: the run's own fence, which its submission in flight signals.
        let waited = unsafe { self.device.wait_for_fences(&[run.done], true, GPU_WAIT_NANOSECONDS) };
        if waited.is_ok() {
            run.reading = None;
            return Ok(());
        }
        self.broken = true;
        // Freeing what a submission uses is only sound once it is done: wait for all of it.
        let device = &self.device;
        // SAFETY: the device is live, and `with_all` holds every one of its queues while this runs.
        let idle = self.lock.with_all(|| unsafe { device.device_wait_idle() });
        self.lost = idle.is_err();
        if !self.lost {
            self.runs.iter_mut().for_each(|run| run.reading = None);
        }
        Err(if idle == Err(vk::Result::ERROR_DEVICE_LOST) { EncodeError::DeviceLost } else { EncodeError::Convert })
    }

    /// Submits the conversion into a frame of FFmpeg's Vulkan pool, locked meanwhile, and tells FFmpeg where the frame
    /// now stands.
    ///
    /// # Safety
    /// As [`Pass::run`], for a frame.
    unsafe fn write_frame(
        &mut self,
        screen: vk::Image,
        view: vk::ImageView,
        target: &Frame,
    ) -> Result<(), EncodeError> {
        let (width, height) = target.size();
        if width == 0 || height == 0 {
            return Err(EncodeError::Convert);
        }
        target
            .with_vulkan_image(|to| {
                // NV12 is two planes of one multi-planar image.
                if !to.is_single_image() {
                    return Err(EncodeError::Convert);
                }
                // SAFETY: the caller's promise covers the screen and the frame, which is locked.
                unsafe { self.submit_to_frame(screen, view, to, [width, height]) }
            })
            .ok_or(EncodeError::Convert)?
    }

    /// Submits the conversion into the frame's image, its semaphore waited at and left one on.
    ///
    /// # Safety
    /// As [`Pass::run`], with the target frame locked.
    unsafe fn submit_to_frame(
        &mut self,
        screen: vk::Image,
        view: vk::ImageView,
        to: &mut VulkanImage<'_>,
        size: [u32; 2],
    ) -> Result<(), EncodeError> {
        let image = to.image();
        // SAFETY: the caller's promise: the target's image is a live NV12 image of the device.
        let (luma, chroma) = unsafe { self.planes_of(image) }?;
        let general = vk::ImageLayout::GENERAL;
        let before = [vk::ImageMemoryBarrier2::default()
            .src_stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)
            .src_access_mask(to.access())
            .dst_stage_mask(vk::PipelineStageFlags2::COMPUTE_SHADER)
            .dst_access_mask(vk::AccessFlags2::SHADER_STORAGE_WRITE)
            .old_layout(to.layout())
            .new_layout(general)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(image)
            .subresource_range(color_range())];
        let writes = Writes { luma, chroma, size, before: &before, after: &[], timeline: Some(to.timeline()) };
        // SAFETY: the caller's promise; the barrier names the frame's image in the layout FFmpeg left.
        unsafe { self.submit(screen, view, &writes) }?;
        // What FFmpeg is told the frame now is, for the encoder's next wait and barrier.
        to.advance(general, vk::AccessFlags2::SHADER_STORAGE_WRITE);
        Ok(())
    }

    /// Submits the conversion into another API's planes, each taken from it for the write and handed back after.
    ///
    /// # Safety
    /// As [`Pass::run`], for planes.
    unsafe fn write_planes(
        &mut self,
        screen: vk::Image,
        view: vk::ImageView,
        [luma, chroma]: [Plane; 2],
        size: [u32; 2],
    ) -> Result<(), EncodeError> {
        let (foreign, general) = (vk::QUEUE_FAMILY_FOREIGN_EXT, vk::ImageLayout::GENERAL);
        let write = vk::AccessFlags2::SHADER_STORAGE_WRITE;
        let handed = |plane: Plane| {
            vk::ImageMemoryBarrier2::default().new_layout(general).image(plane.image).subresource_range(color_range())
        };
        // What the other API left is not read: every pixel is written, so it is taken as undefined.
        let taken = |plane| {
            handed(plane)
                .dst_stage_mask(vk::PipelineStageFlags2::COMPUTE_SHADER)
                .dst_access_mask(write)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .src_queue_family_index(foreign)
                .dst_queue_family_index(self.family)
        };
        let given = |plane| {
            handed(plane)
                .src_stage_mask(vk::PipelineStageFlags2::COMPUTE_SHADER)
                .src_access_mask(write)
                .old_layout(general)
                .src_queue_family_index(self.family)
                .dst_queue_family_index(foreign)
        };
        let (before, after) = ([taken(luma), taken(chroma)], [given(luma), given(chroma)]);
        let writes =
            Writes { luma: luma.view, chroma: chroma.view, size, before: &before, after: &after, timeline: None };
        // SAFETY: the caller's promise; the barriers name the two planes, which nothing else uses meanwhile.
        unsafe { self.submit(screen, view, &writes) }
    }

    /// Records the conversion into the next run and submits it, signalling the run's `done` when the GPU has finished.
    ///
    /// # Safety
    /// As [`Pass::run`], with the next run waited; `writes` names views of the target and barriers that bring it to the
    /// pass's queue and back.
    unsafe fn submit(
        &mut self,
        screen: vk::Image,
        view: vk::ImageView,
        writes: &Writes<'_>,
    ) -> Result<(), EncodeError> {
        let general = vk::ImageLayout::GENERAL;
        // The lent buffer is another API's between reads: handed to this queue for the read and back after it.
        let handed = |from: u32, to: u32, src: vk::AccessFlags2, dst: vk::AccessFlags2| {
            vk::ImageMemoryBarrier2::default()
                .src_stage_mask(vk::PipelineStageFlags2::COMPUTE_SHADER)
                .src_access_mask(src)
                .dst_stage_mask(vk::PipelineStageFlags2::COMPUTE_SHADER)
                .dst_access_mask(dst)
                .old_layout(general)
                .new_layout(general)
                .src_queue_family_index(from)
                .dst_queue_family_index(to)
                .image(screen)
                .subresource_range(color_range())
        };
        let read = vk::AccessFlags2::SHADER_SAMPLED_READ;
        let foreign = vk::QUEUE_FAMILY_FOREIGN_EXT;
        let before: Vec<_> = [handed(foreign, self.family, vk::AccessFlags2::NONE, read)]
            .into_iter()
            .chain(writes.before.iter().copied())
            .collect();
        let after: Vec<_> = [handed(self.family, foreign, read, vk::AccessFlags2::NONE)]
            .into_iter()
            .chain(writes.after.iter().copied())
            .collect();
        let [next, ..] = &self.runs;
        let (cmd, written, done) = (next.commands, next.written, next.done);
        // SAFETY: the run's command buffer, of a pool made resettable; `run` only records into it once its last
        // submission finished, and never again once one did not.
        unsafe { self.device.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty()) }
            .map_err(|_| EncodeError::Convert)?;
        // SAFETY: the buffer was just reset.
        unsafe {
            self.device.begin_command_buffer(
                cmd,
                &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )
        }
        .map_err(|_| EncodeError::Convert)?;
        // SAFETY: the buffer is recording; the barriers name live images, by the caller's promise for the target's.
        unsafe {
            self.device.cmd_pipeline_barrier2(cmd, &vk::DependencyInfo::default().image_memory_barriers(&before))
        };
        // SAFETY: the buffer is recording and the pipeline is the pass's own compute one.
        unsafe { self.device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, self.pipeline) };
        let extent: Vec<u8> = self.extent.iter().flat_map(|side| side.to_ne_bytes()).collect();
        // SAFETY: the range is the layout's one push constant range, the size of the extent.
        unsafe { self.device.cmd_push_constants(cmd, self.pipeline_layout, vk::ShaderStageFlags::COMPUTE, 0, &extent) };
        let image_info = |view| [vk::DescriptorImageInfo::default().image_view(view).image_layout(general)];
        let (screen_info, luma_info, chroma_info) =
            (image_info(view), image_info(writes.luma), image_info(writes.chroma));
        // SAFETY: the writes match the push-descriptor set layout's three bindings, each a live view of this device.
        unsafe {
            self.push.cmd_push_descriptor_set(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                self.pipeline_layout,
                0,
                &[
                    write_image(0, vk::DescriptorType::SAMPLED_IMAGE, &screen_info),
                    write_image(1, vk::DescriptorType::STORAGE_IMAGE, &luma_info),
                    write_image(2, vk::DescriptorType::STORAGE_IMAGE, &chroma_info),
                ],
            )
        };
        let [width, height] = writes.size;
        // SAFETY: the buffer is recording with the pipeline bound and its descriptors and constants pushed.
        unsafe { self.device.cmd_dispatch(cmd, width.div_ceil(GROUP), height.div_ceil(GROUP), 1) };
        // SAFETY: the buffer is recording; the barriers name the lent buffer's image and, by the caller's promise, the
        // target's.
        unsafe { self.device.cmd_pipeline_barrier2(cmd, &vk::DependencyInfo::default().image_memory_barriers(&after)) };
        // SAFETY: the buffer is recording.
        unsafe { self.device.end_command_buffer(cmd) }.map_err(|_| EncodeError::Convert)?;

        // Wait for the buffer's writers and the frame where it stands, and leave the frame one step further on.
        let stage = vk::PipelineStageFlags2::ALL_COMMANDS;
        let mut waits = vec![vk::SemaphoreSubmitInfo::default().semaphore(written).stage_mask(stage)];
        let mut signals = Vec::new();
        if let Some((semaphore, at)) = writes.timeline {
            waits.push(vk::SemaphoreSubmitInfo::default().semaphore(semaphore).value(at).stage_mask(stage));
            signals.push(vk::SemaphoreSubmitInfo::default().semaphore(semaphore).value(at + 1).stage_mask(stage));
        }
        let commands = [vk::CommandBufferSubmitInfo::default().command_buffer(cmd)];
        let submit = vk::SubmitInfo2::default()
            .wait_semaphore_infos(&waits)
            .command_buffer_infos(&commands)
            .signal_semaphore_infos(&signals);
        // SAFETY: the run's own fence, unused or signalled by its last submission, which the CPU waited for.
        unsafe { self.device.reset_fences(&[done]) }.map_err(|_| EncodeError::Convert)?;
        // SAFETY: the queue is held by FFmpeg's lock for it, which `with_queue` takes; the buffer is recorded and the
        // semaphores and fence are live.
        let submitted =
            self.lock.with_queue(self.family, 0, || unsafe { self.device.queue_submit2(self.queue, &[submit], done) });
        submitted.ok_or(EncodeError::Convert)?.map_err(|_| EncodeError::Convert)
    }

    /// The luma and chroma views of an NV12 image, made once for each image the pool holds.
    ///
    /// # Safety
    /// `image` is a live NV12 image of the device.
    unsafe fn planes_of(&mut self, image: vk::Image) -> Result<(vk::ImageView, vk::ImageView), EncodeError> {
        if let Some(found) = self.planes.get(&image) {
            return Ok(*found);
        }
        let mut usage = vk::ImageViewUsageCreateInfo::default().usage(vk::ImageUsageFlags::STORAGE);
        let mut plane = |aspect, format| {
            let range = vk::ImageSubresourceRange::default().aspect_mask(aspect).level_count(1).layer_count(1);
            // SAFETY: the caller's promise covers the image; the views are destroyed with the pass.
            unsafe {
                self.device.create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .push_next(&mut usage)
                        .image(image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(format)
                        .subresource_range(range),
                    None,
                )
            }
            .map_err(|_| EncodeError::Convert)
        };
        let luma = plane(vk::ImageAspectFlags::PLANE_0, vk::Format::R8_UNORM)?;
        let chroma = match plane(vk::ImageAspectFlags::PLANE_1, vk::Format::R8G8_UNORM) {
            Ok(chroma) => chroma,
            Err(why) => {
                // SAFETY: the luma view was just made from this device and nothing has used it.
                unsafe { self.device.destroy_image_view(luma, None) };
                return Err(why);
            }
        };
        self.planes.insert(image, (luma, chroma));
        Ok((luma, chroma))
    }
}

impl Drop for Pass {
    fn drop(&mut self) {
        // A run may still be in flight: what it reads and writes is freed only once it is done.
        if self.settle().is_err() && self.lost {
            // Its command buffers may still be pending on a device that would not idle: leaked, not destroyed, and
            // the buffers they read never given back under them.
            for reading in self.runs.iter_mut().filter_map(|run| run.reading.take()) {
                #[expect(clippy::mem_forget, reason = "the GPU may still be reading it: leaked, never freed under it")]
                mem::forget(reading);
            }
            return;
        }
        // Each handle was made from this device and is destroyed once; every run was waited or the device went idle,
        // and `lost` is unset, so nothing is in flight.
        for run in &self.runs {
            // SAFETY: made from this device for the pass (null if its making failed), and no longer in use, as above.
            unsafe { self.device.destroy_fence(run.done, None) };
            // SAFETY: as for the fence.
            unsafe { self.device.destroy_semaphore(run.written, None) };
        }
        for (luma, chroma) in self.planes.values() {
            // SAFETY: made from this device for the pass, and no longer in use, as above.
            unsafe { self.device.destroy_image_view(*luma, None) };
            // SAFETY: as for the luma view.
            unsafe { self.device.destroy_image_view(*chroma, None) };
        }
        // SAFETY: the pool and its one buffer are the pass's and no longer in use, as above.
        unsafe { self.device.destroy_command_pool(self.pool, None) };
        // SAFETY: the pass's pipeline, no longer in use, as above.
        unsafe { self.device.destroy_pipeline(self.pipeline, None) };
        // SAFETY: its layout, freed after the pipeline made from it.
        unsafe { self.device.destroy_pipeline_layout(self.pipeline_layout, None) };
        // SAFETY: the set layout, freed after the pipeline layout made from it.
        unsafe { self.device.destroy_descriptor_set_layout(self.layout, None) };
    }
}

fn write_image<'a>(
    binding: u32,
    kind: vk::DescriptorType,
    info: &'a [vk::DescriptorImageInfo; 1],
) -> vk::WriteDescriptorSet<'a> {
    vk::WriteDescriptorSet::default().dst_binding(binding).descriptor_type(kind).image_info(info)
}

fn color_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange::default().aspect_mask(vk::ImageAspectFlags::COLOR).level_count(1).layer_count(1)
}

/// Whether the device can write an R8 and an R8G8 storage image without naming a format, which the shader does to
/// the two planes of NV12.
fn can_write_planes(instance: &ash::Instance, physical: vk::PhysicalDevice) -> bool {
    [vk::Format::R8_UNORM, vk::Format::R8G8_UNORM].into_iter().all(|format| {
        let mut extra = vk::FormatProperties3::default();
        let mut properties = vk::FormatProperties2::default().push_next(&mut extra);
        // SAFETY: the physical device belongs to this instance and the chain is well formed.
        unsafe { instance.get_physical_device_format_properties2(physical, format, &mut properties) };
        extra.optimal_tiling_features.contains(vk::FormatFeatureFlags2::STORAGE_WRITE_WITHOUT_FORMAT)
    })
}
