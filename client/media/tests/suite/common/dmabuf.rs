//! A DMA-BUF the test paints, standing in for the compositor's buffer: a VA-API surface exported as a DRM PRIME
//! frame. Painting it again changes the same memory the exported descriptor names.

#![allow(dead_code, reason = "each test binary that includes this fixture uses a different part of it")]

use std::os::fd::{BorrowedFd, OwnedFd};

use capture::{DmaBuf, Frame, Layout, Modifier, Native, Pixels};
use media::av::{DrmMapping, Frame as Av, HwDevice, HwFrames, PixelFormat};
use media::{Backend, Validation};

pub(crate) struct Surface {
    // Declared in the order they are freed: the mapping goes before the surface it maps, the frames before the device.
    exported: DrmMapping,
    surface: Av,
    _frames: HwFrames,
    _device: HwDevice,
    width: usize,
    height: usize,
}

impl Surface {
    /// A `width`x`height` BGRx surface on `render_node`, or none when that device makes none.
    pub(crate) fn new(render_node: &str, width: usize, height: usize) -> Option<Self> {
        let device = HwDevice::open(Backend::VaApi, Some(render_node), Validation::Off)?;
        let size = (u32::try_from(width).ok()?, u32::try_from(height).ok()?);
        let frames = HwFrames::new(&device, Backend::VaApi, PixelFormat::Bgr0, size, 1)?;
        let surface = frames.take().ok()?;
        let exported = surface.map_drm(true).ok()?;
        Some(Self { exported, surface, _frames: frames, _device: device, width, height })
    }

    /// Paints the surface with `gray`, one byte for each pixel.
    pub(crate) fn paint(&mut self, gray: &[u8]) {
        let rgb: Vec<[u8; 3]> = gray.iter().map(|g| [*g; 3]).collect();
        self.paint_rgb(&rgb);
    }

    /// Paints the surface with `rgb`, one triple for each pixel, by the GPU's own upload, finished before this
    /// returns; the host's copy is dropped then.
    pub(crate) fn paint_rgb(&mut self, rgb: &[[u8; 3]]) {
        assert_eq!(rgb.len(), self.width * self.height);
        let size = (self.width as u32, self.height as u32);
        let mut software = Av::software(PixelFormat::Bgr0, size).expect("a frame");
        let picture = software.plane_mut(0).expect("a plane");
        let stride = picture.stride;
        for (y, row) in picture.bytes.chunks_mut(stride).enumerate() {
            for (x, pixel) in row[..self.width * 4].as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let [r, g, b] = rgb[y * self.width + x];
                *pixel = [b, g, r, 255];
            }
        }
        software.upload_into(&mut self.surface).expect("upload to the surface");
        // The upload is queued on the device and `vaSyncSurface` does not wait for it (Renoir, Mesa); reading the
        // surface back does. A compositor's buffer is finished before it is lent, so this one is too.
        let mut back = Av::new().expect("a frame");
        back.fetch(&self.surface).expect("the upload finished");
    }

    /// The surface as a frame the way a capture would lend it: a new descriptor of the same buffer.
    pub(crate) fn frame(&self, pixels: Pixels) -> Frame {
        self.frame_lent(pixels, ())
    }

    /// The same, held out until `lend` drops, as a compositor's buffer is.
    pub(crate) fn frame_lent(&self, pixels: Pixels, lend: impl Send + 'static) -> Frame {
        self.lent_as(pixels, lend, None)
    }

    /// The surface as a frame that says its buffer is laid out as `modifier`, as a compositor's lie would.
    pub(crate) fn frame_claiming(&self, pixels: Pixels, modifier: u64) -> Frame {
        self.lent_as(pixels, (), Some(modifier))
    }

    fn lent_as(&self, pixels: Pixels, lend: impl Send + 'static, claimed: Option<u64>) -> Frame {
        let (objects, layers) = (&self.exported.objects, &self.exported.layers);
        assert_eq!((objects.len(), layers.len(), layers[0].planes.len()), (1, 1, 1), "a single-plane surface");
        let plane = layers[0].planes[0];
        // SAFETY: the mapping owns the descriptor's file, open until `Drop`, and the borrow is cloned at once.
        let fd = unsafe { BorrowedFd::borrow_raw(objects[0].fd) };
        let (offset, stride, modifier) = (plane.offset as u32, plane.pitch as u32, objects[0].modifier);
        let fd: OwnedFd = fd.try_clone_to_owned().expect("duplicate the descriptor");
        let modifier = Modifier::from_raw(claimed.unwrap_or(modifier));
        let dmabuf = DmaBuf::new(fd, offset, stride, modifier, lend).expect("a stride");
        let layout = Layout { pixels, width: self.width as u32, height: self.height as u32 };
        Frame::new(layout, Native::DmaBuf(dmabuf), std::time::Instant::now())
    }
}
