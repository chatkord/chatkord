//! Windows' D3D11: a device context on the device capture and the encoders share, and frames that are a captured or
//! converted texture.

use super::device::{HwDevice, device_type};
use super::error::AvError;
use super::format::PixelFormat;
use super::frame::Frame;
use super::frames::HwFrames;
use crate::probe::Backend;
use capture::Gpu;
use ffmpeg_sys_next as ff;
use std::ffi::{c_int, c_void};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::core::Interface;

impl HwDevice {
    /// A D3D11VA device context on `gpu`'s device, which it references again: FFmpeg, the encoders and the capture
    /// take the one lock the device's multithread guard is. The context holds the `Gpu` too, which [`HwDevice::gpu`]
    /// gives back.
    pub(crate) fn on_d3d11(gpu: &Gpu) -> Option<Self> {
        // SAFETY: allocation has no preconditions; null is handled.
        let context = Self(unsafe { ff::av_hwdevice_ctx_alloc(device_type(Backend::D3d11)) });
        if context.0.is_null() {
            return None;
        }
        // SAFETY: a fresh device context, checked for null above.
        let data = unsafe { (*context.0).data };
        // SAFETY: a device context's data is its `AVHWDeviceContext`, not yet initialised.
        let shared = unsafe { &mut *data.cast::<ff::AVHWDeviceContext>() };
        // SAFETY: a D3D11VA context's hwctx is its `AVD3D11VADeviceContext`, allocated with it.
        let hwctx = unsafe { &mut *shared.hwctx.cast::<ff::AVD3D11VADeviceContext>() };
        // The context's `free`, which freeing even an uninitialised one calls, gives this `Gpu` back; both locks read
        // it until then.
        let held = Box::into_raw(Box::new(gpu.clone())).cast::<c_void>();
        shared.user_opaque = held;
        shared.free = Some(release_gpu);
        // FFmpeg's own teardown releases the device and its context, each a reference taken here.
        hwctx.device = gpu.device().clone().into_raw().cast();
        hwctx.device_context = gpu.context().clone().into_raw().cast();
        hwctx.lock = Some(enter);
        hwctx.unlock = Some(leave);
        hwctx.lock_ctx = held;
        // SAFETY: a fresh D3D11VA context with its device, context and lock set.
        (unsafe { ff::av_hwdevice_ctx_init(context.0) } >= 0).then_some(context)
    }

    /// The `Gpu` a D3D11VA device context was made on, referenced again; none for a device of another kind.
    pub fn gpu(&self) -> Option<Gpu> {
        if self.backend() != Some(Backend::D3d11) {
            return None;
        }
        // SAFETY: the context is live as long as `self`.
        let held = unsafe { (*self.context()).user_opaque };
        // SAFETY: every D3D11VA context here is made by `on_d3d11`, whose `user_opaque` is a `Gpu` it keeps until the
        // context is freed.
        unsafe { held.cast::<Gpu>().as_ref() }.cloned()
    }
}

/// Takes the guard the `Gpu` of a D3D11VA device context made by [`HwDevice::on_d3d11`] holds.
unsafe extern "C" fn enter(held: *mut c_void) {
    // SAFETY: `held` is the `Gpu` `on_d3d11` keeps for the context's life, which FFmpeg's calls are within.
    if let Some(gpu) = unsafe { held.cast::<Gpu>().as_ref() } {
        // SAFETY: the guard is the device's own, entered here and left by `leave`.
        unsafe { gpu.multithread().Enter() };
    }
}

/// Leaves the guard [`enter`] took.
unsafe extern "C" fn leave(held: *mut c_void) {
    // SAFETY: as in `enter`.
    if let Some(gpu) = unsafe { held.cast::<Gpu>().as_ref() } {
        // SAFETY: left once for each time it was entered.
        unsafe { gpu.multithread().Leave() };
    }
}

/// Gives back the `Gpu` a D3D11VA device context made by [`HwDevice::on_d3d11`] holds.
unsafe extern "C" fn release_gpu(context: *mut ff::AVHWDeviceContext) {
    // SAFETY: FFmpeg calls this once, as the context is freed, after its own teardown, with that live context.
    let held = unsafe { (*context).user_opaque };
    // SAFETY: `user_opaque` is the box `on_d3d11` made, given back once.
    drop(unsafe { Box::from_raw(held.cast::<Gpu>()) });
}

impl HwFrames {
    /// A pool of BGRA frames of `size` on `device`, a D3D11 one, with none made: each frame is a texture a capture drew
    /// and [`Frame::d3d11`] wraps, which an encoder that converts it reads, so the textures need only be rendered to.
    pub fn captured(device: &HwDevice, size: (u32, u32)) -> Option<Self> {
        let pool = Self::alloc(device, Backend::D3d11, PixelFormat::Bgra, size, 0)?;
        pool.bind_d3d11(windows::Win32::Graphics::Direct3D11::D3D11_BIND_RENDER_TARGET.0 as u32);
        pool.init()
    }
}

impl HwFrames {
    /// A pool of NV12 frames of `size` on `device`, a D3D11 one like [`HwFrames::new`]'s, whose textures another device
    /// can open by NT handle: a D3D12 pass writes them.
    pub fn shared_nv12(device: &HwDevice, size: (u32, u32)) -> Option<Self> {
        use windows::Win32::Graphics::Direct3D11::{
            D3D11_BIND_RENDER_TARGET, D3D11_BIND_VIDEO_ENCODER, D3D11_RESOURCE_MISC_SHARED,
            D3D11_RESOURCE_MISC_SHARED_NTHANDLE,
        };
        let pool = Self::alloc(device, Backend::D3d11, PixelFormat::Nv12, size, 0)?;
        pool.bind_d3d11((D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_VIDEO_ENCODER.0) as u32);
        // SAFETY: the context is not yet initialised, and nothing else reaches it while this borrow lives.
        let asked = unsafe { &mut *pool.context().cast_mut() };
        // SAFETY: a D3D11 frames context's hwctx is its `AVD3D11VAFramesContext`, allocated with it.
        unsafe {
            (*asked.hwctx.cast::<ff::AVD3D11VAFramesContext>()).MiscFlags =
                (D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0 | D3D11_RESOURCE_MISC_SHARED.0) as u32;
        }
        pool.init()
    }
}

impl Frame {
    /// A frame of `frames`, a D3D11 pool, that is `texture` itself, not an array's slice, of `size`: it holds `owner`
    /// until the last reference to the frame lets go, which keeps `texture` drawn into by nobody else.
    pub fn d3d11<T: Send + 'static>(
        frames: &HwFrames,
        texture: &ID3D11Texture2D,
        owner: T,
        (width, height): (u32, u32),
    ) -> Result<Self, AvError> {
        let (width, height) = (c_int::try_from(width), c_int::try_from(height));
        let (Ok(width), Ok(height)) = (width, height) else { return Err(AvError::Invalid) };
        let frame = Self::new().ok_or(AvError::NoMemory)?;
        let raw = texture.as_raw();
        let held = Box::into_raw(Box::new(owner)).cast::<c_void>();
        // SAFETY: `raw` is the texture `held` keeps alive until `release` drops it, which FFmpeg calls once with
        // `held` when the last reference to this buffer goes.
        let buffer = unsafe { ff::av_buffer_create(raw.cast(), 1, Some(release::<T>), held, 0) };
        if buffer.is_null() {
            // SAFETY: `held` was just made from a box and given to nothing.
            drop(unsafe { Box::from_raw(held.cast::<T>()) });
            return Err(AvError::NoMemory);
        }
        // SAFETY: the frames context is live; the new reference is the frame's, which frees it.
        let context = unsafe { ff::av_buffer_ref(frames.0) };
        // SAFETY: `frame` is owned and empty, and nothing else reaches it while this borrow lives.
        let fields = unsafe { &mut *frame.0 };
        fields.buf[0] = buffer;
        fields.hw_frames_ctx = context;
        // A D3D11 frame is its texture and its index in an array; a texture on its own is no array.
        fields.data[0] = raw.cast();
        fields.data[1] = std::ptr::null_mut();
        fields.format = PixelFormat::D3d11.to_c() as c_int;
        (fields.width, fields.height) = (width, height);
        if context.is_null() {
            return Err(AvError::NoMemory);
        }
        Ok(frame)
    }

    /// The texture of a D3D11 frame, referenced again; none for any other frame.
    pub fn d3d11_texture(&self) -> Option<ID3D11Texture2D> {
        if self.format() != Some(PixelFormat::D3d11) {
            return None;
        }
        // SAFETY: the frame is owned and live; only a plain field is read.
        let raw: *mut c_void = unsafe { (*self.0).data[0].cast() };
        // SAFETY: a D3D11 frame's first data pointer is its texture, which the frame holds a reference to.
        unsafe { ID3D11Texture2D::from_raw_borrowed(&raw) }.cloned()
    }
}

/// Drops the owner a frame made by [`Frame::d3d11`] holds.
unsafe extern "C" fn release<T>(held: *mut c_void, _: *mut u8) {
    // SAFETY: `held` is the box `d3d11` gave this buffer, dropped here once, as FFmpeg frees a buffer once.
    drop(unsafe { Box::from_raw(held.cast::<T>()) });
}

#[cfg(test)]
mod tests {
    use super::*;

    mod lab {
        use super::*;

        #[test]
        fn a_d3d11_device_that_works_does_not_say_it_is_lost() {
            tier::lab();
            let Some(gpu) = Gpu::open(crate::Validation::Off) else { return tier::skipped("no D3D11 device") };
            let device = HwDevice::on_d3d11(&gpu).expect("a device context");
            assert!(!device.is_lost());
        }
    }
}
