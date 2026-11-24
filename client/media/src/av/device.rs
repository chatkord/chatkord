//! A hardware device FFmpeg made, and the handles of the APIs under it that the screen path and the probe read.

use super::dict::Dict;
use super::format::PixelFormat;
use crate::probe::Backend;
use capture::Validation;
use ffmpeg_sys_next as ff;
use std::ffi::CString;
use std::ptr;
#[cfg(target_os = "linux")]
use std::sync::{Mutex, PoisonError};

/// A VA-API display, valid while the device it came from is held.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VaDisplay(pub(super) *mut std::ffi::c_void);

#[cfg(target_os = "linux")]
impl VaDisplay {
    /// The `VADisplay` for a call into libva.
    pub fn as_ptr(self) -> *mut std::ffi::c_void {
        self.0
    }
}

/// An FFmpeg hardware device context, released when dropped.
pub struct HwDevice(pub(super) *mut ff::AVBufferRef);

/// The device type FFmpeg names `backend` by.
pub(super) const fn device_type(backend: Backend) -> ff::AVHWDeviceType {
    match backend {
        Backend::VaApi => ff::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
        Backend::Vulkan => ff::AVHWDeviceType::AV_HWDEVICE_TYPE_VULKAN,
        #[cfg(windows)]
        Backend::D3d11 => ff::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
        #[cfg(windows)]
        Backend::D3d12 => ff::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D12VA,
    }
}

/// The format of a frame on `backend`'s device.
pub const fn device_format(backend: Backend) -> PixelFormat {
    match backend {
        Backend::VaApi => PixelFormat::Vaapi,
        Backend::Vulkan => PixelFormat::Vulkan,
        #[cfg(windows)]
        Backend::D3d11 => PixelFormat::D3d11,
        #[cfg(windows)]
        Backend::D3d12 => PixelFormat::D3d12,
    }
}

/// The backend a device type of FFmpeg's is, none for one this client does not use.
fn backend_of(kind: ff::AVHWDeviceType) -> Option<Backend> {
    [
        Backend::VaApi,
        Backend::Vulkan,
        #[cfg(windows)]
        Backend::D3d11,
        #[cfg(windows)]
        Backend::D3d12,
    ]
    .into_iter()
    .find(|backend| device_type(*backend) == kind)
}

/// Whether the FFmpeg this program calls is the one this crate was built against and was built with the hardware
/// device type of this system's own backend. libwebrtc carries another FFmpeg under the same symbol names, without
/// it.
#[cfg(any(target_os = "linux", windows))]
pub fn can_make_hardware_devices() -> bool {
    use ff::AVHWDeviceType::AV_HWDEVICE_TYPE_NONE;
    #[cfg(target_os = "linux")]
    const SYSTEMS: ff::AVHWDeviceType = device_type(Backend::VaApi);
    #[cfg(windows)]
    const SYSTEMS: ff::AVHWDeviceType = device_type(Backend::D3d11);
    let built =
        (ff::LIBAVCODEC_VERSION_MAJOR << 16) | (ff::LIBAVCODEC_VERSION_MINOR << 8) | ff::LIBAVCODEC_VERSION_MICRO;
    // SAFETY: a constant of the library, with no arguments and no state.
    if u32::try_from(built).ok() != Some(unsafe { ff::avcodec_version() }) {
        return false;
    }
    // The iteration lists the types the library was built with; looking one up by name would find it by its name alone.
    let mut kind = AV_HWDEVICE_TYPE_NONE;
    loop {
        // SAFETY: the iterator takes the previous type, starting from NONE, and has no other state.
        kind = unsafe { ff::av_hwdevice_iterate_types(kind) };
        match kind {
            AV_HWDEVICE_TYPE_NONE => return false,
            found if found == SYSTEMS => return true,
            _ => {}
        }
    }
}

impl HwDevice {
    /// A hardware device context of `backend`, on the device `device` names; none for a D3D backend, whose device is
    /// made elsewhere and adopted (`on_d3d11`, `on_d3d12`). Making a Vulkan device is where FFmpeg asks the Vulkan
    /// loader for one, so it takes the loader's lock; a VA-API one takes it too, kept until a measurement shows
    /// libva's open safe beside the loader's.
    pub fn open(backend: Backend, device: Option<&str>, validation: Validation) -> Option<Self> {
        crate::log::forward();
        #[cfg(target_os = "linux")]
        let _alone = crate::loader::alone();
        #[cfg(windows)]
        if matches!(backend, Backend::D3d11 | Backend::D3d12) {
            return None;
        }
        let device = device.map(CString::new).transpose().ok()?;
        let mut options = Dict::new();
        if validation == Validation::On {
            options.set(c"debug", c"1").ok()?;
        }
        // A screen's buffer is another API's, handed to a Vulkan device's queue and back (`screen::pass`); FFmpeg
        // leaves out an extension the device lacks, which the screen path then refuses.
        if matches!(backend, Backend::Vulkan) {
            options.set(c"device_extensions", c"VK_EXT_queue_family_foreign").ok()?;
        }
        let mut context = ptr::null_mut();
        // SAFETY: `context` is a valid out-pointer; `device` is a NUL-terminated string or null; `options` is a
        // dictionary or null, which FFmpeg only reads.
        let status = unsafe {
            ff::av_hwdevice_ctx_create(
                &raw mut context,
                device_type(backend),
                device.as_ref().map_or(ptr::null(), |d| d.as_ptr()),
                options.as_ptr(),
                0,
            )
        };
        if status < 0 || context.is_null() {
            return None;
        }
        let made = Self(context);
        #[cfg(target_os = "linux")]
        {
            // SAFETY: a device context's data is its `AVHWDeviceContext`, which nothing else holds yet.
            let shared = unsafe { &mut *made.context().cast_mut() };
            // The backend's teardown ends the device in the loader, from whichever handle lets go of it last.
            if let Some(teardown) = shared.free.take() {
                remember(shared.type_, teardown);
                shared.free = Some(end_alone);
            }
        }
        Some(made)
    }

    /// Another reference to the same device.
    pub fn again(&self) -> Option<Self> {
        // SAFETY: a live reference is referenced again; null is handled.
        let again = unsafe { ff::av_buffer_ref(self.0) };
        (!again.is_null()).then_some(Self(again))
    }

    /// The device context, alive while `self` is.
    pub(super) fn context(&self) -> *const ff::AVHWDeviceContext {
        // SAFETY: a device context's buffer is live and its data is its `AVHWDeviceContext`.
        unsafe { (*self.0).data.cast::<ff::AVHWDeviceContext>() }
    }

    /// The device's backend-specific context (`hwctx`), alive while `self` is.
    pub(super) fn hwctx(&self) -> *mut std::ffi::c_void {
        // SAFETY: the context is live as above.
        unsafe { (*self.context()).hwctx }
    }

    /// The backend the device is of, none for a type this client does not use.
    pub fn backend(&self) -> Option<Backend> {
        // SAFETY: the context is live as above.
        backend_of(unsafe { (*self.context()).type_ })
    }

    /// The VA-API display of a VA-API device; none for any other.
    #[cfg(target_os = "linux")]
    pub fn vaapi_display(&self) -> Option<VaDisplay> {
        if self.backend() != Some(Backend::VaApi) {
            return None;
        }
        // SAFETY: a VA-API device context's hwctx is its `AVVAAPIDeviceContext`, alive with the context.
        let display = unsafe { (*self.hwctx().cast::<ff::AVVAAPIDeviceContext>()).display };
        Some(VaDisplay(display))
    }

    /// The layouts in memory the device's frames take.
    #[cfg(target_os = "linux")]
    pub fn uploads(&self) -> Vec<PixelFormat> {
        // SAFETY: `self` is a live device context; null constraints are handled, and freed below.
        let mut constraints = unsafe { ff::av_hwdevice_get_hwframe_constraints(self.0, ptr::null()) };
        let mut found = Vec::new();
        // SAFETY: as above; a non-null list is terminated by `AV_PIX_FMT_NONE`.
        let list = unsafe { constraints.as_ref() }.map_or(ptr::null(), |c| c.valid_sw_formats.cast_const());
        let mut at = list;
        while !at.is_null() {
            // SAFETY: `at` lies inside the terminated list.
            let format = unsafe { *at };
            if format == ff::AVPixelFormat::AV_PIX_FMT_NONE {
                break;
            }
            found.extend(PixelFormat::from_c(format));
            // SAFETY: the list goes on to its terminator, not yet met.
            at = unsafe { at.add(1) };
        }
        // SAFETY: the constraints were made above for this caller, and are freed once.
        unsafe { ff::av_hwframe_constraints_free(&raw mut constraints) };
        found
    }

    /// How many references to the device there are, this one included.
    #[cfg(test)]
    pub fn references(&self) -> i32 {
        // SAFETY: a live reference.
        unsafe { ff::av_buffer_get_ref_count(self.0) }
    }

    /// Whether FFmpeg enabled the device extension `name` on a Vulkan device; false for any other device.
    #[cfg(target_os = "linux")]
    pub fn vulkan_enabled(&self, name: &std::ffi::CStr) -> bool {
        if self.backend() != Some(Backend::Vulkan) {
            return false;
        }
        // SAFETY: a Vulkan device context's hwctx is its `AVVulkanDeviceContext`, alive with the context.
        let vulkan = unsafe { &*self.hwctx().cast::<ff::AVVulkanDeviceContext>() };
        enabled(vulkan, name)
    }
}

/// Whether FFmpeg enabled the device extension `name` on a Vulkan device.
#[cfg(target_os = "linux")]
pub(super) fn enabled(vulkan: &ff::AVVulkanDeviceContext, name: &std::ffi::CStr) -> bool {
    let count = usize::try_from(vulkan.nb_enabled_dev_extensions).unwrap_or(0);
    if vulkan.enabled_dev_extensions.is_null() {
        return false;
    }
    // SAFETY: FFmpeg's device context lists `count` names at this pointer, alive with the context.
    let names = unsafe { std::slice::from_raw_parts(vulkan.enabled_dev_extensions, count) };
    // SAFETY: each listed name is NUL-terminated.
    names.iter().any(|&listed| unsafe { std::ffi::CStr::from_ptr(listed) } == name)
}

impl Drop for HwDevice {
    fn drop(&mut self) {
        // SAFETY: the pointer is a reference this value owns, and `av_buffer_unref` nulls it.
        unsafe { ff::av_buffer_unref(&raw mut self.0) };
    }
}

/// How FFmpeg's backend ends a device of its type, which [`end_alone`] runs under the loader's lock.
#[cfg(target_os = "linux")]
type Teardown = unsafe extern "C" fn(*mut ff::AVHWDeviceContext);

/// Each device type's teardown, as its backend set it: one per type, the same function for every device.
#[cfg(target_os = "linux")]
static TEARDOWNS: Mutex<Vec<(ff::AVHWDeviceType, Teardown)>> = Mutex::new(Vec::new());

#[cfg(target_os = "linux")]
fn remember(kind: ff::AVHWDeviceType, teardown: Teardown) {
    let mut known = TEARDOWNS.lock().unwrap_or_else(PoisonError::into_inner);
    if !known.iter().any(|(each, _)| *each == kind) {
        known.push((kind, teardown));
    }
}

/// Ends a device made by [`HwDevice::open`] under the loader's lock, as its backend would have.
#[cfg(target_os = "linux")]
unsafe extern "C" fn end_alone(context: *mut ff::AVHWDeviceContext) {
    let _alone = crate::loader::alone();
    // SAFETY: FFmpeg calls this once, as the context is freed, with that live context.
    let kind = unsafe { (*context).type_ };
    let teardown = TEARDOWNS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .find(|(each, _)| *each == kind)
        .map(|(_, teardown)| *teardown);
    if let Some(teardown) = teardown {
        // SAFETY: the backend's own teardown for a context of its type, called once, where FFmpeg would have.
        unsafe { teardown(context) };
    }
}

#[cfg(windows)]
impl HwDevice {
    /// Whether the device says it is lost: a D3D11 or D3D12 device whose removal reason is an error, which FFmpeg's
    /// own statuses do not tell apart from any other failure. Any other kind of device here never says so.
    pub fn is_lost(&self) -> bool {
        match self.backend() {
            Some(Backend::D3d11) => self.gpu().is_some_and(|gpu| gpu.is_lost()),
            Some(Backend::D3d12) => self.d3d12().is_some_and(|device| {
                // SAFETY: a live device, asked for a plain status.
                unsafe { device.GetDeviceRemovedReason() }.is_err()
            }),
            _ => false,
        }
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
impl HwDevice {
    /// Whether the device says it is lost: only a Vulkan device on Linux and a D3D device on Windows are asked, so
    /// this one never says so.
    pub fn is_lost(&self) -> bool {
        false
    }
}
