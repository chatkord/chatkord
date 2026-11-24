//! What a screen path made of each buffer the compositor lends, kept by the buffer for as long as the stream lends it
//! again, so a buffer is imported once and not once a picture.

use std::fs::File;
use std::os::fd::OwnedFd;
use std::os::unix::fs::MetadataExt;

use crate::encode::EncodeError;
use capture::{Frame as Captured, Native};

/// A stream lends a handful of buffers in turn, no more than [`capture::LENT_AT_MOST`]; a buffer past this many is
/// made again in the place of the one least recently read.
const KEPT: usize = capture::LENT_AT_MOST;

/// What tells one lent buffer from another: the DMA-BUF's file, and where its picture is in it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Key {
    device: u64,
    inode: u64,
    /// Where the picture starts in the buffer, in bytes.
    pub(super) offset: u32,
    /// The picture's bytes from one row to the next.
    pub(super) stride: u32,
    /// The DRM format modifier the buffer is laid out with.
    pub(super) modifier: u64,
}

/// What each lent buffer was made into, the most recently read last.
pub(super) struct Kept<T>(Vec<(Key, T)>);

impl<T> Default for Kept<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> Kept<T> {
    /// What `captured`'s buffer was made into, made by `make` from its key and a duplicate of its descriptor the first
    /// time the stream lends it; the least recently read is handed to `evict` when one more would be too many.
    pub(super) fn of(
        &mut self,
        captured: &Captured,
        make: impl FnOnce(Key, OwnedFd) -> Result<T, EncodeError>,
        evict: impl FnOnce(T),
    ) -> Result<&T, EncodeError> {
        let Native::DmaBuf(dmabuf) = captured.native();
        let file = File::from(dmabuf.fd().try_clone_to_owned().map_err(|_| EncodeError::Convert)?);
        let identity = file.metadata().map_err(|_| EncodeError::Convert)?;
        let key = Key {
            device: identity.dev(),
            inode: identity.ino(),
            offset: dmabuf.offset(),
            stride: dmabuf.stride(),
            modifier: dmabuf.modifier().raw(),
        };
        if let Some(at) = self.0.iter().position(|(kept, _)| *kept == key) {
            let hit = self.0.remove(at);
            self.0.push(hit);
        } else {
            if self.0.len() == KEPT {
                let (_, oldest) = self.0.remove(0);
                evict(oldest);
            }
            let made = make(key, OwnedFd::from(file))?;
            self.0.push((key, made));
        }
        self.0.last().map(|(_, made)| made).ok_or(EncodeError::Convert)
    }

    /// Everything kept, the most recently read last, leaving none.
    pub(super) fn take(&mut self) -> impl Iterator<Item = T> {
        std::mem::take(&mut self.0).into_iter().map(|(_, made)| made)
    }
}
