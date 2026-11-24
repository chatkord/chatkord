//! A pool of frames on a hardware device.

#[cfg(target_os = "linux")]
use super::device::VaDisplay;
use super::device::{HwDevice, device_format};
use super::error::{AvError, check};
use super::format::PixelFormat;
use super::frame::Frame;
use super::scale::View;
use crate::probe::Backend;
use ffmpeg_sys_next as ff;
use std::ffi::c_int;

/// An FFmpeg hardware frames context, released when dropped.
pub struct HwFrames(pub(super) *mut ff::AVBufferRef);

impl HwFrames {
    /// A pool of frames of `size` on `device` of `backend` that hold pictures laid out as `layout` in memory, with
    /// `pool_size` made at once (none: they are made as asked). On D3D11 the textures are single and can be rendered
    /// to: AMD's driver refuses an NV12 texture array that can be (measured on the RX 9060 XT), and the screen path's
    /// video processor writes them.
    pub fn new(
        device: &HwDevice,
        backend: Backend,
        layout: PixelFormat,
        size: (u32, u32),
        pool_size: u32,
    ) -> Option<Self> {
        let pool = Self::alloc(device, backend, layout, size, pool_size)?;
        #[cfg(windows)]
        if backend == Backend::D3d11 {
            use windows::Win32::Graphics::Direct3D11::{D3D11_BIND_RENDER_TARGET, D3D11_BIND_VIDEO_ENCODER};
            pool.bind_d3d11((D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_VIDEO_ENCODER.0) as u32);
        }
        pool.init()
    }

    /// A pool of `size` on `device`, filled in and not yet initialised.
    pub(super) fn alloc(
        device: &HwDevice,
        backend: Backend,
        layout: PixelFormat,
        (width, height): (u32, u32),
        pool_size: u32,
    ) -> Option<Self> {
        let (width, height) = (c_int::try_from(width).ok()?, c_int::try_from(height).ok()?);
        if width == 0 || height == 0 {
            return None;
        }
        // SAFETY: `device.0` is a live device context of `backend`; the pool is owned by its `HwFrames` from here.
        let pool = Self(unsafe { ff::av_hwframe_ctx_alloc(device.0) });
        if pool.0.is_null() {
            return None;
        }
        // SAFETY: the context is not yet initialised, and nothing else reaches it while this borrow lives.
        let asked = unsafe { &mut *pool.context().cast_mut() };
        asked.format = device_format(backend).to_c();
        asked.sw_format = layout.to_c();
        asked.width = width;
        asked.height = height;
        asked.initial_pool_size = c_int::try_from(pool_size).ok()?;
        Some(pool)
    }

    /// Makes a pool not yet initialised of D3D11 textures that are single, not an array, with `flags` as their bind
    /// flags.
    #[cfg(windows)]
    pub(super) fn bind_d3d11(&self, flags: u32) {
        // SAFETY: the context is not yet initialised, and nothing else reaches it while this borrow lives.
        let asked = unsafe { &mut *self.context().cast_mut() };
        // With no initial size FFmpeg makes single textures, not an array.
        asked.initial_pool_size = 0;
        // SAFETY: a D3D11 frames context's hwctx is its `AVD3D11VAFramesContext`, allocated with it.
        unsafe { (*asked.hwctx.cast::<ff::AVD3D11VAFramesContext>()).BindFlags = flags };
    }

    /// Initialises a pool filled in by [`HwFrames::alloc`].
    pub(super) fn init(self) -> Option<Self> {
        // SAFETY: the pool is a live frames context, filled in.
        (unsafe { ff::av_hwframe_ctx_init(self.0) } >= 0).then_some(self)
    }

    /// Another reference to the same pool.
    pub fn again(&self) -> Option<Self> {
        // SAFETY: a live reference is referenced again; null is handled.
        let again = unsafe { ff::av_buffer_ref(self.0) };
        (!again.is_null()).then_some(Self(again))
    }

    /// The frames context, alive while `self` is.
    pub(super) fn context(&self) -> *const ff::AVHWFramesContext {
        // SAFETY: a frames context's buffer is live and its data is its `AVHWFramesContext`.
        unsafe { (*self.0).data.cast::<ff::AVHWFramesContext>() }
    }

    /// A free frame of the pool, written into `into`, which is emptied first.
    pub fn fill(&self, into: &mut Frame) -> Result<(), AvError> {
        into.unref();
        // SAFETY: the pool is a live frames context and `into` is owned and empty.
        check(unsafe { ff::av_hwframe_get_buffer(self.0, into.0, 0) }).map(drop)
    }

    /// A free frame of the pool.
    pub fn take(&self) -> Result<Frame, AvError> {
        let mut frame = Frame::new().ok_or(AvError::NoMemory)?;
        self.fill(&mut frame)?;
        Ok(frame)
    }

    /// The pictures' width and height.
    #[cfg(target_os = "linux")]
    pub fn size(&self) -> (u32, u32) {
        // SAFETY: the context is live as above.
        let frames = unsafe { &*self.context() };
        (u32::try_from(frames.width).unwrap_or(0), u32::try_from(frames.height).unwrap_or(0))
    }

    /// How many frames were made at once; none or less means the pool grows as asked.
    #[cfg(target_os = "linux")]
    pub fn pool_size(&self) -> u32 {
        // SAFETY: the context is live as above.
        u32::try_from(unsafe { (*self.context()).initial_pool_size }).unwrap_or(0)
    }

    /// The VA-API display of a VA-API pool; none for any other.
    #[cfg(target_os = "linux")]
    pub fn vaapi_display(&self) -> Option<VaDisplay> {
        // SAFETY: a frames context holds its live device context, here borrowed for the call.
        let device = unsafe { (*self.context()).device_ctx };
        if device.is_null() {
            return None;
        }
        // SAFETY: the device context is live as long as the frames context.
        if unsafe { (*device).type_ } != ff::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI {
            return None;
        }
        // SAFETY: as above.
        let hwctx = unsafe { (*device).hwctx };
        // SAFETY: a VA-API device context's hwctx is its `AVVAAPIDeviceContext`.
        Some(VaDisplay(unsafe { (*hwctx.cast::<ff::AVVAAPIDeviceContext>()).display }))
    }

    /// Whether `surface` is one of the surfaces a fixed VA-API pool made.
    #[cfg(target_os = "linux")]
    pub fn has_vaapi_surface(&self, surface: u32) -> bool {
        // SAFETY: the context is live as above.
        let hwctx = unsafe { (*self.context()).hwctx };
        // SAFETY: a VA-API frames context's hwctx is its `AVVAAPIFramesContext`, alive with it.
        let vaapi = unsafe { &*hwctx.cast::<ff::AVVAAPIFramesContext>() };
        let count = usize::try_from(vaapi.nb_surfaces).unwrap_or(0);
        if vaapi.surface_ids.is_null() {
            return false;
        }
        // SAFETY: a fixed pool lists the `nb_surfaces` surfaces it made so far at `surface_ids`, alive with it.
        unsafe { std::slice::from_raw_parts(vaapi.surface_ids, count) }.contains(&surface)
    }

    /// Software `frame` in memory uploaded to a frame of the pool, written into `into`.
    #[cfg(target_os = "linux")]
    pub fn upload(&self, frame: &Frame, into: &mut Frame) -> Result<(), AvError> {
        self.fill(into)?;
        // SAFETY: `into` is a frame of the pool and `frame` a picture in memory of its layout and size.
        check(unsafe { ff::av_hwframe_transfer_data(into.0, frame.0, 0) }).map(drop)
    }

    /// The NV12 picture `from` reads uploaded to a frame of the pool, written into `into`. `staging` is an empty frame
    /// that points at its planes for the transfer alone.
    pub fn upload_nv12(&self, staging: &mut Frame, from: &View<'_>, into: &mut Frame) -> Result<(), AvError> {
        let ((width, height), [(luma, luma_stride), (chroma, chroma_stride)]) =
            from.nv12_planes().ok_or(AvError::Invalid)?;
        self.fill(into)?;
        // SAFETY: `staging` is owned and holds no buffer, and nothing else reaches it while this borrow lives.
        let borrowed = unsafe { &mut *staging.0 };
        borrowed.format = PixelFormat::Nv12.to_c() as c_int;
        (borrowed.width, borrowed.height) = (width, height);
        borrowed.data[0] = luma.cast_mut();
        borrowed.data[1] = chroma.cast_mut();
        (borrowed.linesize[0], borrowed.linesize[1]) = (luma_stride, chroma_stride);
        // SAFETY: `into` is a frame of the pool and `staging` describes the planes `from` borrows, alive here; the
        // transfer copies them and writes through no borrowed plane.
        let sent = check(unsafe { ff::av_hwframe_transfer_data(into.0, staging.0, 0) });
        // SAFETY: as when it was filled; the staging frame owns no buffer, so the borrowed planes are cleared before
        // it is freed or reused.
        let borrowed = unsafe { &mut *staging.0 };
        borrowed.data[0] = std::ptr::null_mut();
        borrowed.data[1] = std::ptr::null_mut();
        sent.map(drop)
    }
}

impl Drop for HwFrames {
    fn drop(&mut self) {
        // SAFETY: the pointer is a reference this value owns, and `av_buffer_unref` nulls it.
        unsafe { ff::av_buffer_unref(&raw mut self.0) };
    }
}
