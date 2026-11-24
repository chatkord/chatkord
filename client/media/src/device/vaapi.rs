//! A picture on a VA-API device run through the device's own video processor (`scale_vaapi`): one FFmpeg graph per
//! source and size, from a frames context of the source's surfaces to NV12 in frames of the graph's own, which an
//! encoder on the same device is opened on. The share converts its screen through one, the camera its pictures.

use std::ffi::{CString, c_int, c_void};

use crate::av::{Frame, Graph, GraphSource, HwDevice, HwFrames};
use crate::encode::EncodeError;

// What a graph is fed: surfaces of `frames`, of this size and rate, in this colour.
pub(crate) use crate::av::GraphSource as Source;

/// A configured graph: the source, its `scale_vaapi` steps, the sink.
pub(crate) struct Scale {
    graph: Graph,
}

// SAFETY: a scale owns the graph and everything it holds and shares none; FFmpeg lets a graph move between threads
// while one thread uses them at a time, which `&mut self` ensures.
unsafe impl Send for Scale {}

impl Scale {
    /// A graph on `device` from `source` through one `scale_vaapi` per entry of `steps` (each its options, the last
    /// naming the output's format).
    pub(crate) fn open(device: &HwDevice, source: &GraphSource<'_>, steps: &[CString]) -> Result<Self, EncodeError> {
        Graph::scale_vaapi(device, source, steps).map(|graph| Self { graph }).map_err(|_| EncodeError::Frames)
    }

    /// The frames the converted pictures are drawn from: an encoder is opened on these.
    pub(crate) fn frames(&self) -> Result<HwFrames, EncodeError> {
        self.graph.output_frames().map_err(|_| EncodeError::Frames)
    }

    /// Runs `picture`, a surface of the source's frames, through the device and returns the converted one once the
    /// GPU has finished it, so whatever the source surface held may be reused as soon as this returns.
    pub(crate) fn run(&mut self, picture: Frame) -> Result<Frame, EncodeError> {
        let converted = self.graph.run(picture).map_err(|_| EncodeError::Convert)?;
        wait_for_gpu(&converted)?;
        Ok(converted)
    }
}

/// Blocks until the GPU has finished with `picture`, a converted frame. Asked of the picture's own VA display:
/// another display's answer is not valid for its surface.
fn wait_for_gpu(picture: &Frame) -> Result<(), EncodeError> {
    let (Some(display), Some(surface)) = (picture.vaapi_display(), picture.vaapi_surface()) else {
        return Err(EncodeError::Convert);
    };
    // SAFETY: `picture` came out of a graph's sink, so it is a live VA-API frame, the display is its own and the
    // surface is one of that display.
    if unsafe { va_sync_surface(display.as_ptr(), surface) } == 0 { Ok(()) } else { Err(EncodeError::Convert) }
}

// The declaration is va.h's `VAStatus vaSyncSurface(VADisplay, VASurfaceID)`, an int, a pointer and a u32.
unsafe extern "C" {
    #[link_name = "vaSyncSurface"]
    fn va_sync_surface(display: *mut c_void, surface: u32) -> c_int;
}
