//! Capture on Linux: a PipeWire node (the portal's screen cast) whose buffers are DMA-BUFs.

mod clock;
mod pods;
mod stream;
mod written;

use std::fmt;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::sync::{Arc, Mutex};

pub use stream::Capture;

/// A DRM format modifier: how the compositor laid the pixels out in the buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Modifier(u64);

impl Modifier {
    /// Rows one after the other; every importer can read it, and the floor when the GPU lists none.
    pub const LINEAR: Self = Self(0);

    /// The modifier DRM names by this value; any value, as a compositor may name one no GPU here reads.
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// The value DRM and Vulkan name it by.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// A picture in a single-plane DMA-BUF the compositor lent. Dropping it gives the buffer back. One plane because a
/// layout with compression metadata beside the pixels needs its planes imported together, and none is offered.
pub struct DmaBuf {
    fd: OwnedFd,
    offset: u32,
    stride: u32,
    modifier: Modifier,
    /// Whatever keeps the buffer out of the compositor's pool, released when the last copy of the picture drops.
    lend: Arc<Mutex<dyn Send>>,
}

impl DmaBuf {
    /// A picture in `fd`, held until `lend` drops; `stride` is the bytes from one row to the next, and `None` when it
    /// is zero.
    pub fn new(fd: OwnedFd, offset: u32, stride: u32, modifier: Modifier, lend: impl Send + 'static) -> Option<Self> {
        (stride > 0).then(|| Self { fd, offset, stride, modifier, lend: Arc::new(Mutex::new(lend)) })
    }

    /// A second copy of the picture, holding the same lend: the buffer goes back once both have dropped. `None` when
    /// the descriptor cannot be copied.
    pub fn again(&self) -> Option<Self> {
        let fd = self.fd.try_clone().ok()?;
        Some(Self {
            fd,
            offset: self.offset,
            stride: self.stride,
            modifier: self.modifier,
            lend: Arc::clone(&self.lend),
        })
    }

    /// The DMA-BUF. Valid while the frame is.
    pub fn fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }

    /// Bytes from the start of the buffer to the first row.
    pub fn offset(&self) -> u32 {
        self.offset
    }

    /// Bytes from one row to the next; never zero.
    pub fn stride(&self) -> u32 {
        self.stride
    }

    /// How the pixels are laid out in the buffer.
    pub fn modifier(&self) -> Modifier {
        self.modifier
    }
}

impl fmt::Debug for DmaBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DmaBuf")
            .field("offset", &self.offset)
            .field("stride", &self.stride)
            .field("modifier", &self.modifier)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Frame, Layout, Native, Pixels};
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Counts the times it was sent home.
    struct Home(Arc<AtomicU32>);

    impl Drop for Home {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn a_picture_lent_again_goes_home_once_and_only_after_both_copies_drop() {
        let home = Arc::new(AtomicU32::new(0));
        let fd = OwnedFd::from(std::fs::File::open("/dev/null").unwrap());
        let dmabuf = DmaBuf::new(fd, 0, 4, Modifier::LINEAR, Home(home.clone())).unwrap();
        let layout = Layout { pixels: Pixels::Bgrx, width: 2, height: 2 };
        let first = Frame::new(layout, Native::DmaBuf(dmabuf), std::time::Instant::now());
        let second = first.lend_again().unwrap();
        assert_eq!(second.layout(), layout);
        drop(first);
        assert_eq!(home.load(Ordering::SeqCst), 0, "the second copy still holds the buffer");
        drop(second);
        assert_eq!(home.load(Ordering::SeqCst), 1);
    }
}
