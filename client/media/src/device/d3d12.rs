//! Decoding onto the window's own D3D12 device (Windows): the device comes in from the painter, and a decoded picture
//! goes back out as the resource the decoder wrote, with the fence that says when it is written. Nothing is copied.

use crate::av::{Frame, HwFrames, PixelFormat};
use crate::colours::Colours;
use std::sync::Arc;
use windows::Win32::Graphics::Direct3D12::{ID3D12CommandQueue, ID3D12Device, ID3D12Fence, ID3D12Resource};

/// The D3D12 device a window paints with, for a decoder to decode onto.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct D3d12Device(pub ID3D12Device);

// SAFETY: a D3D12 device is free-threaded: Microsoft documents every `ID3D12Device` method as callable from any thread.
unsafe impl Send for D3d12Device {}
// SAFETY: as above.
unsafe impl Sync for D3d12Device {}

impl D3d12Device {
    /// The adapter the device is on, which the encoder, the capture and a share's card are made on too.
    pub fn adapter(&self) -> capture::AdapterId {
        // SAFETY: a live device; the call reads its adapter's id.
        capture::AdapterId::of(unsafe { self.0.GetAdapterLuid() })
    }

    /// The errors the D3D12 debug layer found in what was asked of the device; none where it is not under it.
    pub fn reported(&self) -> Vec<capture::Reported> {
        capture::validation::d3d12_errors(&self.0)
    }
}

/// The pool a decoder lends its pictures from, one for each `hw_frames_ctx` it decodes into. Every surface of the pool
/// carries the same `Arc`, so whatever a window keeps about the pool's resources can be dropped, and a resource's
/// address never read for another pool's, once the decoder and every surface it lent are gone.
pub struct Lender {
    pool: HwFrames,
}

// SAFETY: the buffer is a counted reference to the pool, which FFmpeg counts atomically; nothing here reads through it
// except to compare its address, and it is only dropped.
unsafe impl Send for Lender {}
// SAFETY: as above; no `&self` method touches the pool.
unsafe impl Sync for Lender {}

impl std::fmt::Debug for Lender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lender").finish_non_exhaustive()
    }
}

impl Lender {
    /// The lender of `frame`'s pool: `kept` when it lends from that pool still, else a new one, which `kept` becomes.
    pub(crate) fn of(frame: &Frame, kept: &mut Option<Arc<Self>>) -> Result<Arc<Self>, crate::DecodeError> {
        if let Some(lender) = kept.as_ref().filter(|lender| lender.pool.draws(frame)) {
            return Ok(Arc::clone(lender));
        }
        let pool = HwFrames::of_frame(frame).ok_or(crate::DecodeError::Picture)?;
        let lender = Arc::new(Self { pool });
        *kept = Some(Arc::clone(&lender));
        Ok(lender)
    }
}

/// A picture the decoder left on the window's device: NV12, in a resource of its own (`AV_PIX_FMT_D3D12`). Holding it
/// keeps the decoder from writing that resource again; dropping it gives the resource back.
pub struct D3d12Surface {
    frame: Frame,
    width: u32,
    height: u32,
    colours: Colours,
    lender: Arc<Lender>,
}

// SAFETY: the surface owns its own reference to the frame, FFmpeg counts references atomically, and nothing here writes
// the frame: moving it to the thread that paints, or dropping it there, is what it is for.
unsafe impl Send for D3d12Surface {}
// SAFETY: every `&self` method only reads the frame (its size, and references to its resource and fence); the one that
// writes, `release_after`, takes the surface by value.
unsafe impl Sync for D3d12Surface {}

impl std::fmt::Debug for D3d12Surface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("D3d12Surface").field("width", &self.width).field("height", &self.height).finish_non_exhaustive()
    }
}

impl D3d12Surface {
    /// Takes another reference to a decoded D3D12 frame, lent by `kept`'s pool or a new one. `Picture` when it is not
    /// one.
    pub(crate) fn of(frame: &Frame, kept: &mut Option<Arc<Lender>>) -> Result<Self, crate::DecodeError> {
        let (width, height) = frame.size();
        let colours = Colours::of(frame);
        if frame.format() != Some(PixelFormat::D3d12) || width == 0 || height == 0 {
            return Err(crate::DecodeError::Picture);
        }
        let lender = Lender::of(frame, kept)?;
        let frame = frame.reference().ok_or(crate::DecodeError::Picture)?;
        Ok(Self { frame, width, height, colours, lender })
    }

    /// The pool this picture is lent from, shared by every picture of it.
    pub fn lender(&self) -> &Arc<Lender> {
        &self.lender
    }

    /// The matrix and range its samples are in, as the decoder's frame said.
    pub fn colours(&self) -> Colours {
        self.colours
    }

    /// Never zero.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Never zero.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The resource the decoder wrote, referenced again.
    pub fn resource(&self) -> Option<ID3D12Resource> {
        self.frame.d3d12_resource()
    }

    /// The fence the decoder signals when the picture is written, and the value it signals.
    pub fn written(&self) -> Option<(ID3D12Fence, u64)> {
        self.frame.d3d12_fence()
    }

    /// Makes `queue` wait, on the GPU, until the decoder has written the picture, as FFmpeg's frame fence asks.
    pub fn wait_written(&self, queue: &ID3D12CommandQueue) -> bool {
        // SAFETY: a queue may wait on any device's fence.
        self.written().is_some_and(|(fence, value)| unsafe { queue.Wait(&fence, value) }.is_ok())
    }

    /// Gives the picture back to the decoder once `queue` has done what was submitted to it so far, its reads
    /// included.
    pub fn release_after(mut self, queue: &ID3D12CommandQueue) {
        self.frame.d3d12_release_after(queue);
    }
}
