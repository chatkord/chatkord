//! A decoded VA-API surface left where the decoder made it, as the DMA-BUF objects and planes it is made of. The map
//! to DRM PRIME waits for the decode, so a surface is complete when returned; nothing here copies a pixel.

use crate::av::{DrmMapping, Frame};
use crate::colours::Colours;
use crate::decode::DecodeError;
use capture::Modifier;
use std::num::TryFromIntError;
use std::os::fd::{BorrowedFd, RawFd};

/// One DMA-BUF the picture lives in.
#[derive(Clone, Copy, Debug)]
pub struct Object<'a> {
    /// Valid while the [`Surface`] is. A consumer that keeps it longer, or hands it to an API that takes ownership,
    /// duplicates it first (`try_clone_to_owned`).
    pub fd: BorrowedFd<'a>,
    /// Bytes in the buffer, including any that hold no picture.
    pub size: u64,
    /// How the driver laid the planes out in this buffer.
    pub modifier: Modifier,
}

/// Where one plane of a layer is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plane {
    /// Index into [`Surface::objects`].
    pub object: usize,
    /// Bytes from the start of that object to the plane's first row.
    pub offset: u64,
    /// Bytes from one row of the plane to the next.
    pub pitch: u64,
}

/// A run of planes that form one DRM format: NV12 as one layer of two planes, or as two layers of one (`R8` for luma,
/// `GR88` for chroma), as the driver exports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    /// A `DRM_FORMAT_*` fourcc.
    pub fourcc: u32,
    /// In the format's order, each in an object of the surface.
    pub planes: Vec<Plane>,
}

/// A decoded picture on the GPU, exported as DMA-BUF objects. Dropping it gives the surface back to the decoder.
pub struct Surface {
    /// The DRM-PRIME mapping; its buffer owns the file descriptors and holds the decoder's surface, and freeing it
    /// closes the first and gives the second back.
    _mapped: DrmMapping,
    width: u32,
    height: u32,
    pts: i64,
    colours: Colours,
    objects: Vec<(RawFd, u64, Modifier)>,
    layers: Vec<Layer>,
}

// SAFETY: the mapping is a reference-counted FFmpeg buffer nobody mutates after `export`, and the file descriptors it
// owns are plain integers, so it may move to the thread that imports it.
unsafe impl Send for Surface {}
// SAFETY: every method takes `&self` and only reads plain data, so sharing a reference is as safe as moving the value.
unsafe impl Sync for Surface {}

impl Surface {
    /// Maps a frame a VA-API decoder returned to DRM PRIME. A frame that is not on a VA-API surface, or a driver that
    /// will not export it, is [`DecodeError::Picture`].
    pub(crate) fn export(frame: &Frame) -> Result<Self, DecodeError> {
        let mapped = frame.map_drm(false).map_err(|_| DecodeError::Picture)?;
        let (width, height) = frame.size();
        if width == 0 || height == 0 {
            return Err(DecodeError::Picture);
        }
        let objects: Vec<_> = mapped.objects.iter().map(|o| (o.fd, o.size, Modifier::from_raw(o.modifier))).collect();
        let layers = mapped
            .layers
            .iter()
            .map(|l| {
                let planes = l
                    .planes
                    .iter()
                    .map(|p| {
                        Ok(Plane { object: p.object, offset: u64::try_from(p.offset)?, pitch: u64::try_from(p.pitch)? })
                    })
                    .collect::<Result<_, TryFromIntError>>()?;
                Ok(Layer { fourcc: l.format, planes })
            })
            .collect::<Result<Vec<_>, TryFromIntError>>()
            .map_err(|_| DecodeError::Picture)?;
        let whole = !objects.is_empty()
            && !layers.is_empty()
            && layers.iter().all(|l| l.planes.iter().all(|p| p.object < objects.len()));
        if !whole || objects.iter().any(|(fd, ..)| *fd < 0) {
            return Err(DecodeError::Picture);
        }
        Ok(Self { _mapped: mapped, width, height, pts: frame.pts(), colours: Colours::of(frame), objects, layers })
    }

    /// The same surface, its samples said to be in `colours`: for one made here, whose frame was never tagged.
    #[must_use]
    pub(crate) fn in_colours(self, colours: Colours) -> Self {
        Self { colours, ..self }
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

    /// The timestamp of the unit this picture came from.
    pub fn pts(&self) -> i64 {
        self.pts
    }

    /// The DMA-BUFs the picture is in, usually one.
    pub fn objects(&self) -> impl ExactSizeIterator<Item = Object<'_>> {
        self.objects.iter().map(|&(fd, size, modifier)| Object {
            // SAFETY: the descriptor was checked non-negative in `export` and the mapping owns it until `self` drops,
            // so it stays open for the whole life of the returned iterator.
            fd: unsafe { BorrowedFd::borrow_raw(fd) },
            size,
            modifier,
        })
    }

    /// How the objects divide into planes.
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }
}

impl std::fmt::Debug for Surface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Surface")
            .field("size", &(self.width, self.height))
            .field("layers", &self.layers)
            .field("pts", &self.pts)
            .finish()
    }
}
