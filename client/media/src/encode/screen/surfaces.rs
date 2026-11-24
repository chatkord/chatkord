//! The compositor's buffers as surfaces of the encoder's VA-API display, each made once from its DMA-BUF and kept by
//! the buffer, as FFmpeg's `hwmap` would make and destroy one a picture.

use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::Arc;

use super::kept::{Kept, Key};
use super::va::{
    Attribute, EXTERNAL_BUFFER_DESCRIPTOR, INTEGER, MEM_TYPE_DRM_PRIME_2, MEMORY_TYPE, POINTER, PrimeDescriptor,
    PrimeLayer, PrimeObject, RT_FORMAT_RGB32, SETTABLE, Value, va_create_surfaces, va_destroy_surfaces,
};
use crate::av::{ColourSpace, Frame, HwDevice, HwFrames, PixelFormat, VaDisplay};
use crate::encode::EncodeError;
use crate::probe::Backend;
use capture::{Frame as Captured, Layout, Pixels};

/// One lent buffer as a surface of the display, destroyed when the last picture drawn from it is let go.
struct Surface {
    id: u32,
    display: VaDisplay,
    /// The device the display belongs to, kept for as long as the surface is.
    _device: HwDevice,
    /// The buffer's own descriptor, kept for as long as the surface reads it.
    _fd: OwnedFd,
}

// SAFETY: a VA display may be called from any thread; the surface is only destroyed, once, when the last reference
// goes, and read meanwhile by the driver alone.
unsafe impl Send for Surface {}
// SAFETY: as above; `&Surface` gives out only its id.
unsafe impl Sync for Surface {}

impl Drop for Surface {
    fn drop(&mut self) {
        // SAFETY: the surface was made on this display, which `_device` keeps alive, and is destroyed only here.
        unsafe { va_destroy_surfaces(self.display.as_ptr(), &raw mut self.id, 1) };
    }
}

/// The surfaces made so far, and the frames context the pictures drawn from them belong to.
pub(super) struct Surfaces {
    // Declared first: the frames go before the device they were made on.
    frames: HwFrames,
    device: HwDevice,
    display: VaDisplay,
    layout: Layout,
    /// What of each picture is read, from its top left.
    read: (u32, u32),
    kept: Kept<Arc<Surface>>,
}

// SAFETY: the surfaces and contexts held are FFmpeg's and the driver's, which may move between threads, and a path is
// used by one thread at a time (`&mut self`).
unsafe impl Send for Surfaces {}

impl Surfaces {
    /// What makes the buffers of a stream laid out as `layout` into surfaces of `device`, a VA-API device context,
    /// each picture's top-left `read` the part read.
    pub(super) fn new(device: &HwDevice, layout: Layout, read: (u32, u32)) -> Result<Self, EncodeError> {
        if read.0 == 0 || read.1 == 0 || read.0 > layout.width || read.1 > layout.height {
            return Err(EncodeError::Settings);
        }
        let device = device.again().ok_or(EncodeError::Device)?;
        let display = device.vaapi_display().ok_or(EncodeError::Device)?;
        let in_memory = match layout.pixels {
            Pixels::Bgrx => PixelFormat::Bgr0,
            Pixels::Bgra => PixelFormat::Bgra,
        };
        // With no pool size it makes no surfaces to keep.
        let frames = HwFrames::new(&device, Backend::VaApi, in_memory, (layout.width, layout.height), 0)
            .ok_or(EncodeError::Frames)?;
        Ok(Self { frames, device, display, layout, read, kept: Kept::default() })
    }

    /// The frames context a filter graph's source is told its pictures are drawn from.
    pub(super) fn frames(&self) -> &HwFrames {
        &self.frames
    }

    /// `captured` as a VA-API picture stamped `pts`, on the surface its buffer was made into the first time the stream
    /// lent it. The picture holds the surface; the compositor's lend stays with `captured`.
    pub(super) fn picture(&mut self, captured: &Captured, pts: i64) -> Result<Frame, EncodeError> {
        if captured.layout() != self.layout {
            return Err(EncodeError::FrameSize);
        }
        let Self { frames, device, display, layout, read, kept } = self;
        let surface = kept.of(captured, |key, fd| make(device, *display, *layout, key, fd).map(Arc::new), drop)?;
        let surface = Arc::clone(surface);
        let id = surface.id;
        let mut picture = Frame::vaapi(frames, id, surface, (layout.width, layout.height), *read)
            .map_err(|_| EncodeError::Convert)?;
        picture.set_colour_space(ColourSpace::Bt709);
        picture.set_pts(pts);
        Ok(picture)
    }
}

/// Makes the surface `fd`'s buffer is, laid out as `key` says, on `display`.
fn make(device: &HwDevice, display: VaDisplay, layout: Layout, key: Key, fd: OwnedFd) -> Result<Surface, EncodeError> {
    let (fourcc, drm_format) = match layout.pixels {
        Pixels::Bgrx => (u32::from_le_bytes(*b"BGRX"), u32::from_le_bytes(*b"XR24")),
        Pixels::Bgra => (u32::from_le_bytes(*b"BGRA"), u32::from_le_bytes(*b"AR24")),
    };
    // A DMA-BUF reports its size as its file length.
    let size = std::fs::File::from(fd.try_clone().map_err(|_| EncodeError::Convert)?)
        .metadata()
        .ok()
        .and_then(|meta| u32::try_from(meta.len()).ok())
        .filter(|size| *size > 0)
        .ok_or(EncodeError::Convert)?;
    let mut descriptor = PrimeDescriptor {
        fourcc,
        width: layout.width,
        height: layout.height,
        num_objects: 1,
        objects: [PrimeObject::default(); 4],
        num_layers: 1,
        layers: [PrimeLayer::default(); 4],
    };
    descriptor.objects[0] = PrimeObject { fd: fd.as_raw_fd(), size, drm_format_modifier: key.modifier };
    descriptor.layers[0].drm_format = drm_format;
    descriptor.layers[0].num_planes = 1;
    descriptor.layers[0].offset[0] = key.offset;
    descriptor.layers[0].pitch[0] = key.stride;
    let mut attributes = [
        Attribute {
            kind: MEMORY_TYPE,
            flags: SETTABLE,
            value: Value { kind: INTEGER, value: u64::from(MEM_TYPE_DRM_PRIME_2) },
        },
        Attribute {
            kind: EXTERNAL_BUFFER_DESCRIPTOR,
            flags: SETTABLE,
            value: Value { kind: POINTER, value: (&raw mut descriptor).addr() as u64 },
        },
    ];
    let mut id = 0;
    // SAFETY: the display is live; `id` is room for one surface; the attributes and the descriptor they point at
    // are valid for the call, which copies what it keeps.
    let status = unsafe {
        va_create_surfaces(
            display.as_ptr(),
            RT_FORMAT_RGB32,
            layout.width,
            layout.height,
            &raw mut id,
            1,
            attributes.as_mut_ptr(),
            2,
        )
    };
    if status != 0 {
        return Err(EncodeError::Convert);
    }
    Ok(Surface { id, display, _device: device.again().ok_or(EncodeError::Device)?, _fd: fd })
}

#[cfg(test)]
mod lab {
    use super::*;
    use crate::encode::screen::painted;
    use crate::probe::{Backend, survey};

    /// Pictures of three buffers lent in turn are drawn from three surfaces, one each, which the pictures let go and
    /// the cache alone holds once they are done.
    #[test]
    fn each_lent_buffer_is_one_surface_however_often_it_is_lent() {
        tier::lab();
        let report = survey();
        let node = report.encoders.iter().filter_map(|e| e.device()).find(|d| d.backend == Backend::VaApi);
        let Some(node) = node.and_then(|d| d.node.clone()) else {
            return tier::skipped("no VA-API device to make a DMA-BUF on");
        };
        let Some(device) = HwDevice::open(Backend::VaApi, Some(&node), crate::Validation::Off) else {
            return tier::skipped("the VA-API device does not open");
        };
        let buffers: Option<Vec<_>> = (0..3).map(|_| painted::Surface::new(&node, 640, 360)).collect();
        let Some(buffers) = buffers else { return tier::skipped("the VA-API device makes no BGRx surface") };
        let layout = Layout { pixels: Pixels::Bgrx, width: 640, height: 360 };
        let mut surfaces = Surfaces::new(&device, layout, (640, 360)).expect("a frames context");
        let mut ids = std::collections::HashSet::new();
        let mut pictures = Vec::new();
        for n in 0..30 {
            let picture = surfaces.picture(&buffers[n % 3].frame(Pixels::Bgrx), 0).expect("a picture");
            ids.insert(picture.vaapi_surface());
            pictures.push(picture);
        }
        assert_eq!(ids.len(), 3, "a surface a buffer, not one a picture");
        drop(pictures);
        let held: Vec<usize> = surfaces.kept.take().map(|surface| Arc::strong_count(&surface)).collect();
        assert_eq!(held, [1, 1, 1], "a picture let go still holds its surface");
    }
}
