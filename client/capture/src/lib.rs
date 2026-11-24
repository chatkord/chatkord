//! Capture: a screen or window's pictures as buffers the GPU owns, a DMA-BUF on Linux and a D3D11 texture on Windows,
//! never read or copied here, and a camera's pictures as its device hands them over. The compositor lends few buffers,
//! so only the newest frame is kept. There is no codec here: decoding a camera's JPEG and encoding anything belong to `media`.

pub mod camera;
mod card;
#[cfg(target_os = "linux")]
mod linux;
#[cfg_attr(
    not(any(target_os = "linux", windows)),
    allow(
        dead_code,
        clippy::drop_non_drop,
        reason = "the hand-off every capture shares; the first on another system uses it, and its frames drop a buffer"
    )
)]
mod slot;
pub mod validation;
#[cfg(windows)]
mod windows;

use std::fmt;
use std::time::Instant;

#[cfg(windows)]
pub use self::windows::{
    AdapterId, Capture, D3d11, Gpu, Locked, Output, Paced, Preview, PreviewPicture, Processor, Still, still,
};
pub use card::{Card, card};
#[cfg(target_os = "linux")]
pub use linux::{Capture, DmaBuf, Modifier};
#[cfg(any(target_os = "linux", windows))]
pub(crate) use slot::Shared;
pub use validation::{Reported, Validation};

/// The most buffers a stream lends at once (PipeWire's are counted in single figures): the stream asks for no more, and
/// whatever keeps something per buffer keeps no more than this many.
pub const LENT_AT_MOST: usize = 16;

/// The channel order of a captured picture; the compositor's alpha is not shown to the call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pixels {
    /// Four bytes a pixel, blue first, the fourth unused.
    Bgrx,
    /// Four bytes a pixel, blue first, the fourth alpha.
    Bgra,
}

/// What the compositor agreed to send.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// The channel order of each pixel.
    pub pixels: Pixels,
    /// Pixels across, which may be odd.
    pub width: u32,
    /// Pixels down, which may be odd.
    pub height: u32,
}

/// One captured picture, lent by the system that made it. A DMA-BUF may still be being written when it arrives: a
/// reader waits on `DmaBuf::written` (Linux) first. Dropping it gives the buffer back.
#[derive(Debug)]
pub struct Frame {
    layout: Layout,
    native: Native,
    captured: Instant,
}

impl Frame {
    /// A picture whose memory holds `layout`, which the system drew at `captured`.
    pub fn new(layout: Layout, native: Native, captured: Instant) -> Self {
        Self { layout, native, captured }
    }

    /// What the compositor agreed to send for this picture.
    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// The system's time for this picture, which its stream's clock follows rather than when it was encoded.
    pub fn captured(&self) -> Instant {
        self.captured
    }

    /// The picture's memory, in the form this system keeps it.
    pub fn native(&self) -> &Native {
        &self.native
    }

    /// The same picture lent again, for a second reader beside the encoder; the buffer goes back once every copy has
    /// dropped, so a copy kept is a buffer the compositor cannot draw into. `None` when it cannot be lent again.
    #[cfg_attr(
        not(any(target_os = "linux", windows)),
        allow(unreachable_code, reason = "a system with no capture has no picture to lend")
    )]
    pub fn lend_again(&self) -> Option<Self> {
        // On the place, not a reference: a system with no capture has no variant, and a reference is never empty.
        let native = match self.native {
            #[cfg(target_os = "linux")]
            Native::DmaBuf(ref dmabuf) => Native::DmaBuf(dmabuf.again()?),
            #[cfg(windows)]
            Native::D3d11(ref texture) => Native::D3d11(texture.again()),
        };
        Some(Self { layout: self.layout, native, captured: self.captured })
    }
}

/// Where a picture lives, one variant per system whose capture exists; a match over it has no wildcard arm, so a
/// system added here stops every consumer compiling until it has a path.
#[derive(Debug)]
pub enum Native {
    /// A buffer the compositor shares through the kernel.
    #[cfg(target_os = "linux")]
    DmaBuf(DmaBuf),
    /// A BGRA texture on the encoder's D3D11 device.
    #[cfg(windows)]
    D3d11(D3d11),
}

/// Why a capture did not start or stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureError {
    /// The capture would not connect: the portal's PipeWire connection.
    Connect,
    /// The compositor offers no buffer the GPU can import.
    NoDmaBuf,
    /// The compositor agreed to a picture of a size or layout that cannot be encoded.
    Format,
    /// The stream failed.
    Stream,
    /// This system has no screen capture: no libpipewire on Linux.
    Unavailable,
    /// The cast ended: the person stopped it, or the window went away.
    Ended,
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect => f.write_str("the screen capture did not connect"),
            Self::NoDmaBuf => f.write_str("the compositor offered no DMA-BUF format the GPU can import"),
            Self::Format => f.write_str("the negotiated picture cannot be encoded"),
            Self::Stream => f.write_str("the screen capture's stream failed"),
            Self::Unavailable => f.write_str("this system has no screen capture"),
            Self::Ended => f.write_str("the cast ended"),
        }
    }
}

impl std::error::Error for CaptureError {}
