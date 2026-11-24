//! The fence a DMA-BUF's writers leave on it, which a read waits on: the compositor may lend a buffer before its GPU
//! has finished drawing into it.

use std::os::fd::{AsFd, FromRawFd, OwnedFd};

use super::DmaBuf;

/// The kernel's `struct dma_buf_export_sync_file`.
#[repr(C)]
struct ExportSyncFile {
    flags: u32,
    fd: i32,
}

/// `DMA_BUF_SYNC_READ`: the fences a read waits on, its writers'.
const SYNC_READ: u32 = 1;
/// `DMA_BUF_IOCTL_EXPORT_SYNC_FILE`, `_IOWR('b', 2, struct dma_buf_export_sync_file)`.
const EXPORT_SYNC_FILE: rustix::ioctl::Opcode = rustix::ioctl::opcode::read_write::<ExportSyncFile>(b'b', 2);

impl DmaBuf {
    /// A sync file that signals once whoever wrote the picture is done, for a reader to wait on before it reads; one
    /// already signalled when nothing is writing. Fails on a kernel older than 5.20, which has no such export.
    pub fn written(&self) -> std::io::Result<OwnedFd> {
        let mut asked = ExportSyncFile { flags: SYNC_READ, fd: -1 };
        // SAFETY: the opcode is the kernel's for this struct, which it reads `flags` from and writes `fd` into.
        let updater = unsafe { rustix::ioctl::Updater::<EXPORT_SYNC_FILE, _>::new(&mut asked) };
        // SAFETY: the descriptor is a DMA-BUF, which answers this ioctl.
        unsafe { rustix::ioctl::ioctl(self.fd().as_fd(), updater) }?;
        if asked.fd < 0 {
            return Err(rustix::io::Errno::BADF.into());
        }
        // SAFETY: the kernel made this sync file for the caller, who owns it now.
        Ok(unsafe { OwnedFd::from_raw_fd(asked.fd) })
    }
}
