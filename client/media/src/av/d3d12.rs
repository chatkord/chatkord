//! Windows' D3D12: a device context on the window's own device, and the frames a decoder writes on it, each a resource
//! and the fence that says when it is written.

use super::device::{HwDevice, device_type};
use super::format::PixelFormat;
use super::frame::Frame;
use super::frames::HwFrames;
use crate::probe::Backend;
use ffmpeg_sys_next as ff;
use std::ffi::c_void;
use windows::Win32::Graphics::Direct3D12::{ID3D12CommandQueue, ID3D12Device, ID3D12Fence, ID3D12Resource};
use windows::core::Interface;

impl HwDevice {
    /// A D3D12VA device context on `device`, a device somebody else made (the window's), which it references again.
    pub(crate) fn on_d3d12(device: &ID3D12Device) -> Option<Self> {
        // SAFETY: allocation has no preconditions; null is handled.
        let context = Self(unsafe { ff::av_hwdevice_ctx_alloc(device_type(Backend::D3d12)) });
        if context.0.is_null() {
            return None;
        }
        // SAFETY: a fresh device context, checked for null above.
        let data = unsafe { (*context.0).data };
        // SAFETY: a device context's data is its `AVHWDeviceContext`, not yet initialised.
        let shared = unsafe { &mut *data.cast::<ff::AVHWDeviceContext>() };
        // SAFETY: a D3D12VA context's hwctx is its `AVD3D12VADeviceContext`, allocated with it.
        let hwctx = unsafe { &mut *shared.hwctx.cast::<ff::AVD3D12VADeviceContext>() };
        // FFmpeg never releases a device it did not make, so the context holds a reference its `free` gives back,
        // which freeing even an uninitialised context calls.
        let held = device.clone().into_raw();
        hwctx.device = held.cast();
        shared.user_opaque = held;
        shared.free = Some(release_d3d12);
        // SAFETY: a fresh D3D12VA context with its device set.
        (unsafe { ff::av_hwdevice_ctx_init(context.0) } >= 0).then_some(context)
    }

    /// The D3D12 device of a D3D12VA device context, referenced again; none for a device of another kind.
    pub(crate) fn d3d12(&self) -> Option<ID3D12Device> {
        if self.backend() != Some(Backend::D3d12) {
            return None;
        }
        // SAFETY: a D3D12VA device context's hwctx is its `AVD3D12VADeviceContext`, alive with the context.
        let raw: *mut c_void = unsafe { &*self.hwctx().cast::<ff::AVD3D12VADeviceContext>() }.device.cast();
        // SAFETY: the context holds a live device while it lives, referenced again before this returns.
        unsafe { ID3D12Device::from_raw_borrowed(&raw) }.cloned()
    }
}

/// Gives back the reference a D3D12VA device context made by [`HwDevice::on_d3d12`] holds on its device.
unsafe extern "C" fn release_d3d12(context: *mut ff::AVHWDeviceContext) {
    // SAFETY: FFmpeg calls this once, as the context is freed, with that live context.
    let held = unsafe { (*context).user_opaque };
    // SAFETY: `user_opaque` is the reference `on_d3d12` took, given back once.
    drop(unsafe { ID3D12Device::from_raw(held) });
}

impl HwFrames {
    /// The pool `frame` is drawn from, referenced again; none for a frame in memory.
    pub fn of_frame(frame: &Frame) -> Option<Self> {
        // SAFETY: the frame is live; only a plain field is read.
        let pool = unsafe { (*frame.0).hw_frames_ctx };
        if pool.is_null() {
            return None;
        }
        // SAFETY: `pool` is a live reference, referenced again; null is handled.
        let again = unsafe { ff::av_buffer_ref(pool) };
        (!again.is_null()).then_some(Self(again))
    }

    /// Whether `frame` is drawn from this pool, by the pool's identity and not by what it holds.
    pub fn draws(&self, frame: &Frame) -> bool {
        // SAFETY: the frame is live; only a plain field is read.
        let pool = unsafe { (*frame.0).hw_frames_ctx };
        if pool.is_null() {
            return false;
        }
        // SAFETY: a live reference of the frame, whose buffer is compared by address only.
        let theirs = unsafe { (*pool).buffer };
        // SAFETY: this pool's own live reference, read likewise.
        theirs == unsafe { (*self.0).buffer }
    }
}

impl Frame {
    /// The `AVD3D12VAFrame` of a decoded D3D12 frame, none for any other.
    fn d3d12_held(&self) -> Option<*mut ff::AVD3D12VAFrame> {
        // SAFETY: the frame is live; only plain fields are read.
        let fields = unsafe { &*self.0 };
        (fields.format == PixelFormat::D3d12.to_c() as i32 && !fields.data[0].is_null())
            .then(|| fields.data[0].cast::<ff::AVD3D12VAFrame>())
    }

    /// The resource the decoder wrote, referenced again; none for a frame that is not a D3D12 one.
    pub fn d3d12_resource(&self) -> Option<ID3D12Resource> {
        // SAFETY: a D3D12 frame's `data[0]` is its `AVD3D12VAFrame`, alive while the frame is.
        let texture: *mut c_void = unsafe { (*self.d3d12_held()?).texture.cast() };
        // SAFETY: the frame's `AVD3D12VAFrame` holds a live resource while the frame does.
        unsafe { ID3D12Resource::from_raw_borrowed(&texture) }.cloned()
    }

    /// The fence the decoder signals when the frame is written, and the value it signals.
    pub fn d3d12_fence(&self) -> Option<(ID3D12Fence, u64)> {
        // SAFETY: as in `d3d12_resource`.
        let sync = unsafe { &(*self.d3d12_held()?).sync_ctx };
        let raw: *mut c_void = sync.fence.cast();
        // SAFETY: the frame's sync context holds a live fence and the value of its last signal.
        let fence = unsafe { ID3D12Fence::from_raw_borrowed(&raw) }?.clone();
        Some((fence, sync.fence_value))
    }

    /// Makes the decoder wait, once `queue` has done what was submitted to it so far, its reads included: the frame's
    /// fence is signalled on `queue` with the next value. A queue that cannot signal leaves the old value, which the
    /// decoder waits for no longer than its own decode.
    pub fn d3d12_release_after(&mut self, queue: &ID3D12CommandQueue) {
        let Some(held) = self.d3d12_held() else { return };
        // SAFETY: as in `d3d12_resource`. FFmpeg's decoder writes the value only on a frame fresh from its pool, never
        // on one held, and the frame reaches the pool again through its lock, so the decode thread reads this value.
        let sync = unsafe { &mut (*held).sync_ctx };
        let raw: *mut c_void = sync.fence.cast();
        // SAFETY: as in `d3d12_fence`.
        let Some(fence) = (unsafe { ID3D12Fence::from_raw_borrowed(&raw) }) else { return };
        sync.fence_value += 1;
        // SAFETY: a queue may signal any device's fence.
        if unsafe { queue.Signal(fence, sync.fence_value) }.is_err() {
            sync.fence_value -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Direct3D12::{D3D12CreateDevice, ID3D12Device5};

    mod lab {
        use super::*;

        #[test]
        fn a_d3d12_device_removed_by_remove_device_says_it_is_lost() {
            tier::lab();
            let mut made: Option<ID3D12Device5> = None;
            // SAFETY: the default adapter, and an out-pointer that outlives the call.
            let created =
                unsafe { D3D12CreateDevice(None::<&windows::core::IUnknown>, D3D_FEATURE_LEVEL_11_0, &raw mut made) };
            let (Ok(()), Some(device)) = (created, made) else { return tier::skipped("no D3D12 device") };
            let Ok(plain) = device.cast::<ID3D12Device>() else { return tier::skipped("no D3D12 device") };
            let Some(context) = HwDevice::on_d3d12(&plain) else {
                return tier::skipped("FFmpeg has no D3D12 here");
            };
            assert!(!context.is_lost(), "a device that works");
            // SAFETY: a live device, which this test owns and uses no more after.
            unsafe { device.RemoveDevice() };
            assert!(context.is_lost(), "a device that was removed");
        }
    }
}
