//! FFmpeg's Vulkan device and frames, as typed handles for the code that records its own commands on them.

use super::device::HwDevice;
use super::format::PixelFormat;
use super::frame::Frame;
use crate::probe::Backend;
use ash::vk::{self, Handle};
use ffmpeg_sys_next as ff;
use std::ffi::{CStr, c_char};

/// One queue family FFmpeg made the device with.
#[derive(Clone, Copy)]
pub struct QueueFamily {
    /// Its index on the device.
    pub index: u32,
    /// How many queues it has.
    pub count: u32,
    /// What its queues do.
    pub flags: vk::QueueFlags,
}

/// FFmpeg's own lock for a queue: it installs one on every device, and the encoder and its filters submit to the same
/// queues. Holds the device, so the lock outlives nothing it locks.
pub struct QueueLocks {
    device: HwDevice,
    lock: unsafe extern "C" fn(*mut ff::AVHWDeviceContext, u32, u32),
    unlock: unsafe extern "C" fn(*mut ff::AVHWDeviceContext, u32, u32),
    families: Vec<(u32, u32)>,
}

impl QueueLocks {
    fn context(&self) -> *mut ff::AVHWDeviceContext {
        self.device.context().cast_mut()
    }

    /// Runs `work` holding queue `index` of `family`, as submitting to it requires; none when the device has no such
    /// queue.
    pub fn with_queue<R>(&self, family: u32, index: u32, work: impl FnOnce() -> R) -> Option<R> {
        let known = self.families.iter().any(|&(each, count)| each == family && index < count);
        if !known {
            return None;
        }
        // SAFETY: FFmpeg's lock for a queue it made, on the device context this value holds.
        unsafe { (self.lock)(self.context(), family, index) };
        let done = work();
        // SAFETY: the lock taken above.
        unsafe { (self.unlock)(self.context(), family, index) };
        Some(done)
    }

    /// Runs `work` holding every queue of the device, as `vkDeviceWaitIdle` requires of each queue it waits on.
    pub fn with_all<R>(&self, work: impl FnOnce() -> R) -> R {
        let queues = || self.families.iter().flat_map(|&(family, count)| (0..count).map(move |index| (family, index)));
        for (family, index) in queues() {
            // SAFETY: FFmpeg's lock for a queue it made, on the device context this value holds.
            unsafe { (self.lock)(self.context(), family, index) };
        }
        let done = work();
        for (family, index) in queues() {
            // SAFETY: the lock taken above.
            unsafe { (self.unlock)(self.context(), family, index) };
        }
        done
    }
}

/// FFmpeg's Vulkan device as the pass and the imports call it.
pub struct VulkanDevice {
    /// `vkGetInstanceProcAddr`, from which the instance's functions load.
    pub get_instance_proc_addr: vk::PFN_vkGetInstanceProcAddr,
    /// The instance FFmpeg made the device from.
    pub instance: vk::Instance,
    /// The device's GPU.
    pub physical: vk::PhysicalDevice,
    /// The device.
    pub device: vk::Device,
    /// The flags its queues were created with.
    pub queue_flags: vk::DeviceQueueCreateFlags,
    /// Its queue families.
    pub families: Vec<QueueFamily>,
    /// The lock for its queues.
    pub locks: QueueLocks,
}

impl VulkanDevice {
    /// Whether FFmpeg enabled the device extension `name`.
    pub fn enabled(&self, name: &CStr) -> bool {
        self.locks.device.vulkan_enabled(name)
    }
}

impl HwDevice {
    /// The handles of a Vulkan device, valid while the value returned holds the device; none for another kind of
    /// device or one FFmpeg gave no loader or queue lock.
    pub fn vulkan(&self) -> Option<VulkanDevice> {
        if self.backend() != Some(Backend::Vulkan) {
            return None;
        }
        // SAFETY: a Vulkan device context's hwctx is its `AVVulkanDeviceContext`, alive with the context.
        let vulkan = unsafe { &*self.hwctx().cast::<ff::AVVulkanDeviceContext>() };
        let get = vulkan.get_proc_addr?;
        // SAFETY: both types are `vkGetInstanceProcAddr` as FFmpeg's headers and ash declare it.
        let get_instance_proc_addr = unsafe {
            std::mem::transmute::<
                unsafe extern "C" fn(ff::VkInstance, *const c_char) -> Option<unsafe extern "C" fn()>,
                vk::PFN_vkGetInstanceProcAddr,
            >(get)
        };
        let (lock, unlock) = (vulkan.lock_queue?, vulkan.unlock_queue?);
        let listed = &vulkan.qf[..usize::try_from(vulkan.nb_qf).unwrap_or(0)];
        let families: Vec<QueueFamily> = listed
            .iter()
            .filter_map(|q| {
                Some(QueueFamily {
                    index: u32::try_from(q.idx).ok()?,
                    count: u32::try_from(q.num).ok()?,
                    flags: vk::QueueFlags::from_raw(q.flags),
                })
            })
            .collect();
        let counted = families.iter().map(|family| (family.index, family.count)).collect();
        Some(VulkanDevice {
            get_instance_proc_addr,
            instance: vk::Instance::from_raw(vulkan.inst as u64),
            physical: vk::PhysicalDevice::from_raw(vulkan.phys_dev as u64),
            device: vk::Device::from_raw(vulkan.act_dev as u64),
            queue_flags: vk::DeviceQueueCreateFlags::from_raw(vulkan.queue_flags),
            families,
            locks: QueueLocks { device: self.again()?, lock, unlock, families: counted },
        })
    }
}

impl HwDevice {
    /// Whether the device says it is lost: a Vulkan device asked to finish its work answers `DEVICE_LOST` from a
    /// reset or a removal on, which FFmpeg's own statuses do not tell apart from any other failure. Any other kind of
    /// device has no such question, and never says so.
    pub fn is_lost(&self) -> bool {
        let Some(vulkan) = self.vulkan() else { return false };
        let table = ash::StaticFn { get_instance_proc_addr: vulkan.get_instance_proc_addr };
        // SAFETY: `instance` is the live instance FFmpeg made, and `table` its loader.
        let instance = unsafe { ash::Instance::load(&table, vulkan.instance) };
        // SAFETY: `device` is the live device FFmpeg made from this instance.
        let device = unsafe { ash::Device::load(instance.fp_v1_0(), vulkan.device) };
        // SAFETY: the device is live, and `with_all` holds every one of its queues while this runs.
        let idle = vulkan.locks.with_all(|| unsafe { device.device_wait_idle() });
        idle == Err(vk::Result::ERROR_DEVICE_LOST)
    }
}

/// The image record FFmpeg keeps for a frame of its Vulkan pool, which FFmpeg's own lock for the frame guards.
pub struct VulkanImage<'a>(&'a mut ff::AVVkFrame);

impl VulkanImage<'_> {
    /// Whether the frame is one image, whose planes are views of it, as NV12 is.
    pub fn is_single_image(&self) -> bool {
        self.0.img[0] != 0 && self.0.img[1] == 0
    }

    /// The image.
    pub fn image(&self) -> vk::Image {
        vk::Image::from_raw(self.0.img[0])
    }

    /// The layout FFmpeg left the image in.
    pub fn layout(&self) -> vk::ImageLayout {
        vk::ImageLayout::from_raw(self.0.layout[0])
    }

    /// The accesses FFmpeg left pending on the image.
    pub fn access(&self) -> vk::AccessFlags2 {
        vk::AccessFlags2::from_raw(self.0.access[0])
    }

    /// The timeline semaphore that orders the image, and the value it stands at.
    pub fn timeline(&self) -> (vk::Semaphore, u64) {
        (vk::Semaphore::from_raw(self.0.sem[0]), self.0.sem_value[0])
    }

    /// Tells FFmpeg where the image now stands, for the encoder's next wait and barrier: the timeline one on, the
    /// layout and the accesses as given.
    pub fn advance(&mut self, layout: vk::ImageLayout, access: vk::AccessFlags2) {
        self.0.sem_value[0] += 1;
        self.0.layout[0] = layout.as_raw();
        self.0.access[0] = access.as_raw();
    }
}

impl Frame {
    /// Runs `work` on the image record of a frame of FFmpeg's Vulkan pool, holding the frame's lock; none for any
    /// other frame.
    pub fn with_vulkan_image<R>(&self, work: impl FnOnce(&mut VulkanImage<'_>) -> R) -> Option<R> {
        // SAFETY: the frame is owned and live; plain fields are read.
        let fields = unsafe { &*self.0 };
        if fields.format != PixelFormat::Vulkan.to_c() as i32 || fields.hw_frames_ctx.is_null() {
            return None;
        }
        // A Vulkan frame's first data pointer is its image record.
        let record = fields.data[0].cast::<ff::AVVkFrame>();
        if record.is_null() {
            return None;
        }
        // SAFETY: the frame holds its frames context's buffer, whose data is its `AVHWFramesContext`.
        let frames = unsafe { (*fields.hw_frames_ctx).data.cast::<ff::AVHWFramesContext>() };
        // SAFETY: `frames` is the live frames context read above.
        let hwctx = unsafe { (*frames).hwctx };
        // SAFETY: a Vulkan frames context's hwctx is its `AVVulkanFramesContext`, alive as long as `frames`.
        let vulkan = unsafe { &*hwctx.cast::<ff::AVVulkanFramesContext>() };
        if let Some(lock) = vulkan.lock_frame {
            // SAFETY: FFmpeg's lock for a frame of this context; it is released below.
            unsafe { lock(frames, record) };
        }
        // SAFETY: the record is the frame's own, reached through this one reference while it is locked.
        let done = work(&mut VulkanImage(unsafe { &mut *record }));
        if let Some(unlock) = vulkan.unlock_frame {
            // SAFETY: the lock taken above.
            unsafe { unlock(frames, record) };
        }
        Some(done)
    }
}

#[cfg(test)]
mod tests {
    mod lab {
        use crate::av::HwDevice;
        use crate::probe::Backend;

        #[test]
        fn a_device_that_works_does_not_say_it_is_lost() {
            tier::lab();
            let mut asked = 0;
            for backend in [Backend::Vulkan, Backend::VaApi] {
                if let Some(device) = HwDevice::open(backend, None, crate::Validation::Off) {
                    assert!(!device.is_lost(), "{backend:?}");
                    asked += 1;
                }
            }
            if asked == 0 {
                tier::skipped("no Vulkan or VA-API device opens here");
            }
        }
    }
}
