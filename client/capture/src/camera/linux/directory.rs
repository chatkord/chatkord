//! The camera portal and the cameras it shows: access asked once a process, then a PipeWire connection of the portal's
//! on which only cameras are seen, one shared while any watch or open camera holds it, and made again when the daemon
//! ended the last. What it hears goes to the hub: a camera gone stops its opener, and every change wakes the listeners.

use crate::camera::CameraError;
use crate::camera::hub::hub;
use ashpd::desktop::camera::{Camera, CameraAccessOptions, OpenPipeWireRemoteOptions};
use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// How long a list waits for the daemon to tell every camera and its formats: a few round trips on this machine.
const SETTLE: Duration = Duration::from_secs(2);

/// The portal's answer to whether this process may use the cameras, asked once and kept once granted.
fn access() -> Result<(), CameraError> {
    static GRANTED: AtomicBool = AtomicBool::new(false);
    if GRANTED.load(Ordering::Acquire) {
        return Ok(());
    }
    async_io::block_on(async {
        let camera = Camera::new().await.map_err(|_| CameraError::Unavailable)?;
        let asked = camera.request_access(CameraAccessOptions::default()).await.map_err(|_| CameraError::Denied)?;
        asked.response().map_err(|_| CameraError::Denied)
    })?;
    GRANTED.store(true, Ordering::Release);
    Ok(())
}

/// Whether the portal says a camera is plugged in; a portal that does not answer says none.
fn present() -> bool {
    async_io::block_on(async { Camera::new().await.ok()?.is_present().await.ok() }).unwrap_or(false)
}

/// A new connection of the portal's to PipeWire, on which only the cameras are seen.
pub(super) fn remote() -> Result<OwnedFd, CameraError> {
    access()?;
    async_io::block_on(async {
        let camera = Camera::new().await.map_err(|_| CameraError::Unavailable)?;
        camera.open_pipe_wire_remote(OpenPipeWireRemoteOptions::default()).await.map_err(|_| CameraError::Unavailable)
    })
}

/// The cameras on one connection, and whether the daemon ended it.
pub(super) struct Directory {
    cameras: pw::Cameras,
    lost: Arc<AtomicBool>,
}

impl Directory {
    fn open() -> Result<Self, CameraError> {
        let lost = Arc::new(AtomicBool::new(false));
        let told = Arc::clone(&lost);
        let cameras = pw::Cameras::open(remote()?, move |change| match change {
            pw::Change::Listed => hub().changed(),
            pw::Change::Went(name) => hub().removed(&name),
            pw::Change::Lost => {
                told.store(true, Ordering::Release);
                hub().changed();
            }
        })
        .map_err(|why| match why {
            pw::Error::Unavailable(_) => CameraError::Unavailable,
            pw::Error::Refused(_) | pw::Error::Pod => CameraError::Gone,
        })?;
        Ok(Self { cameras, lost })
    }

    /// The cameras, once the daemon has told them all and, while the portal says one is present, at least one
    /// (within [`SETTLE`]).
    pub(super) fn cameras(&self) -> Vec<pw::Camera> {
        let present = present();
        self.cameras.settled(SETTLE, |cameras| !present || !cameras.is_empty())
    }
}

/// The shared directory, kept while anything holds it, and how many do.
static SHARED: Mutex<Option<Arc<Directory>>> = Mutex::new(None);
static HOLDERS: AtomicUsize = AtomicUsize::new(0);

/// The shared directory, made (again, when the daemon ended the last) if there is none.
pub(super) fn directory() -> Result<Arc<Directory>, CameraError> {
    let mut shared = SHARED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(running) = shared.as_ref().filter(|running| !running.lost.load(Ordering::Acquire)) {
        return Ok(Arc::clone(running));
    }
    let made = Arc::new(Directory::open()?);
    *shared = (HOLDERS.load(Ordering::Acquire) > 0).then(|| Arc::clone(&made));
    Ok(made)
}

/// Keeps the shared directory, so cameras coming and going are heard, until this is dropped.
pub(super) struct Kept;

impl Kept {
    /// Keeps it, made first if there is none.
    pub(super) fn new() -> Result<Self, CameraError> {
        let mut shared = SHARED.lock().unwrap_or_else(PoisonError::into_inner);
        HOLDERS.fetch_add(1, Ordering::AcqRel);
        if shared.as_ref().is_none_or(|running| running.lost.load(Ordering::Acquire)) {
            match Directory::open() {
                Ok(made) => *shared = Some(Arc::new(made)),
                Err(refused) => {
                    HOLDERS.fetch_sub(1, Ordering::AcqRel);
                    return Err(refused);
                }
            }
        }
        Ok(Self)
    }
}

impl Drop for Kept {
    fn drop(&mut self) {
        let mut shared = SHARED.lock().unwrap_or_else(PoisonError::into_inner);
        if HOLDERS.fetch_sub(1, Ordering::AcqRel) == 1 {
            *shared = None;
        }
    }
}
