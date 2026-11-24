//! Linux's cameras through the camera portal, as libwebrtc's own capture (Chromium's and Telegram's) reads them: the
//! portal grants the cameras and hands over a PipeWire connection on which only they are seen, each camera a node
//! named by its `node.name`. A camera is opened at one of its own modes, so nothing is scaled; its pictures come in
//! memory as NV12, YUY2 or JPEG, the newest waits in one slot for the reader, and an older one is dropped.

mod directory;
mod formats;

use super::hub::hub;
use super::{CameraError, Device, Mode, Shot};
use directory::{Kept, directory, remote};
use formats::{Offered, Settled};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

/// The cameras the portal shows now, in the order PipeWire told them.
pub fn list() -> Result<Vec<Device>, CameraError> {
    let cameras = directory()?.cameras();
    Ok(cameras.into_iter().map(|camera| Device { id: camera.name, label: camera.description }).collect())
}

/// The camera named `id`, and every mode it offers and how.
fn offered(id: &str) -> Result<(pw::Camera, Vec<Offered>), CameraError> {
    let camera = directory()?.cameras().into_iter().find(|camera| camera.name == id).ok_or(CameraError::Gone)?;
    let offered = camera.formats.iter().flat_map(|format| formats::offered(format)).collect();
    Ok((camera, offered))
}

/// The modes the camera `id` captures at, read without streaming, each once.
pub fn modes(id: &str) -> Result<Vec<Mode>, CameraError> {
    let (_, offered) = offered(id)?;
    let mut modes: Vec<Mode> = Vec::new();
    for each in offered {
        if !modes.contains(&each.mode) {
            modes.push(each.mode);
        }
    }
    Ok(modes)
}

/// The cameras coming and going, told to a listener until this is dropped.
pub struct Watch {
    _kept: Kept,
    id: u64,
}

impl Drop for Watch {
    fn drop(&mut self) {
        hub().unlisten(self.id);
    }
}

/// Calls `changed`, from PipeWire's thread, whenever a camera arrives, is removed or changes what it offers.
pub fn watch(changed: impl Fn() + Send + Sync + 'static) -> Result<Watch, CameraError> {
    Ok(Watch { _kept: Kept::new()?, id: hub().listen(changed) })
}

/// What a read waits for: the newest picture, or the end.
#[derive(Default)]
struct Slot {
    /// The format settled on, once the camera has said.
    settled: Option<Settled>,
    picture: Option<(Shot, Instant)>,
    /// A picture's memory, given back by the reader for the next.
    spare: Option<Vec<u8>>,
    /// Whether a picture has come, so a stream that fails after one is a camera gone, not one never had.
    began: bool,
    /// Every picture the camera handed over, the ones a newer replaced before a read included.
    delivered: u64,
    ended: Option<CameraError>,
}

/// The slot shared by PipeWire's thread, the reader and whoever stops the camera.
#[derive(Default)]
struct Latest {
    slot: Mutex<Slot>,
    told: Condvar,
}

impl Latest {
    fn slot(&self) -> MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Ends the camera for why, unless it ended already; a read waiting returns.
    fn end(&self, why: CameraError) {
        self.slot().ended.get_or_insert(why);
        self.told.notify_all();
    }
}

/// Ends a camera from any thread: a read waiting on it returns at once as gone.
#[derive(Clone)]
pub struct Stopper(Arc<Latest>);

impl Stopper {
    /// Ends the camera, whatever it is doing.
    pub fn stop(&self) {
        self.0.end(CameraError::Gone);
    }
}

/// What hears the camera's stream, on PipeWire's thread.
struct Intake {
    latest: Arc<Latest>,
    buffers: [Vec<u8>; 2],
}

impl pw::Handler for Intake {
    fn state_changed(&mut self, state: pw::State) {
        if state == pw::State::Error {
            // A camera that never gave a picture is held elsewhere or refused this format; one that did is gone.
            let began = self.latest.slot().began;
            self.latest.end(if began { CameraError::Gone } else { CameraError::Busy });
        }
    }

    fn param_changed(&mut self, stream: &pw::StreamRef<'_>, id: u32, param: Option<&[u8]>) {
        if id != pw::FORMAT {
            return;
        }
        let Some(param) = param else { return };
        match formats::settled(param) {
            Some(settled) => {
                self.latest.slot().settled = Some(settled);
                let [buffers, header] = &self.buffers;
                if stream.update_params(&[buffers, header]).is_err() {
                    self.latest.end(CameraError::Format);
                }
            }
            None => self.latest.end(CameraError::Format),
        }
    }

    fn process(&mut self, stream: &pw::StreamRef<'_>) {
        // The newest picture only: each one dequeued before it goes back as the next is taken. A buffer holding none,
        // which a camera hands over between pictures, is no picture sent.
        let (mut newest, mut handed) = (None, 0);
        while let Some(buffer) = stream.dequeue() {
            if buffer.mapped().is_some() {
                newest = Some(buffer);
                handed += 1;
            }
        }
        self.latest.slot().delivered += handed;
        let Some(buffer) = newest else { return };
        let Some(mapped) = buffer.mapped() else { return };
        let mut slot = self.latest.slot();
        let spare = slot.spare.take().or_else(|| slot.picture.take().map(|(old, _)| old.into_memory()));
        let Some(settled) = slot.settled else { return };
        let size = (settled.width, settled.height);
        slot.picture = Some((Shot::new(mapped.bytes, mapped.stride, settled.pixel, size, spare), Instant::now()));
        slot.began = true;
        drop(slot);
        self.latest.told.notify_all();
    }
}

/// As the directory does when the daemon removes the camera `id`: for a test, which cannot unplug one.
#[cfg(test)]
pub(super) fn removed(id: &str) {
    hub().removed(id);
}

/// A camera opened at one of its own modes.
pub struct Camera {
    mode: Mode,
    latest: Arc<Latest>,
    /// Dropped first: the stream stops before the hub lets the camera go.
    _session: pw::Session,
    _held: Held,
    _kept: Kept,
}

/// The camera held in the hub, let go when this is dropped.
struct Held(u64);

impl Drop for Held {
    fn drop(&mut self) {
        hub().release(self.0);
    }
}

impl Camera {
    /// Opens the camera `id` at its mode nearest `asked`, offered each way it hands that mode over, the cheapest to
    /// make NV12 first.
    pub fn open(id: &str, asked: Mode) -> Result<Self, CameraError> {
        let kept = Kept::new()?;
        let (camera, offered) = offered(id)?;
        let modes: Vec<Mode> = offered.iter().map(|each| each.mode).collect();
        let mode = Mode::nearest(&modes, asked).ok_or(CameraError::Format)?;
        let mut ways: Vec<&Offered> = offered.iter().filter(|each| each.mode == mode).collect();
        ways.sort_by_key(|each| each.pixel);
        ways.dedup_by_key(|each| each.pixel);
        let pods = ways.into_iter().map(formats::offer).collect::<Result<Vec<_>, _>>()?;
        let pods: Vec<&[u8]> = pods.iter().map(Vec::as_slice).collect();
        let latest = Arc::new(Latest::default());
        let intake = Intake { latest: Arc::clone(&latest), buffers: formats::buffers()? };
        let held = Held(hub().hold(&camera.name, Stopper(Arc::clone(&latest))));
        let opened = remote().and_then(|remote| {
            pw::Session::open(remote, camera.node, pw::Role::Camera, &pods, intake).map_err(|why| match why {
                pw::Error::Unavailable(_) => CameraError::Unavailable,
                pw::Error::Refused(_) | pw::Error::Pod => CameraError::Busy,
            })
        });
        let session = opened?;
        Ok(Self { mode, latest, _session: session, _held: held, _kept: kept })
    }

    /// The mode it captures at.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// How many pictures the camera has handed over since it opened, read or not: what it chose to send, which in dim
    /// light is fewer than its mode's rate.
    pub fn delivered(&self) -> u64 {
        self.latest.slot().delivered
    }

    /// What ends it from another thread.
    pub fn stopper(&self) -> Stopper {
        Stopper(Arc::clone(&self.latest))
    }

    /// The next picture and when it arrived, waiting for it; its end, once it has ended. A picture of another size than
    /// the mode is refused as [`CameraError::Format`]; its memory goes back through [`Camera::recycle`].
    pub fn read(&mut self) -> Result<(Shot, Instant), CameraError> {
        let mut slot = self.latest.slot();
        let (shot, at) = loop {
            if let Some(ended) = slot.ended {
                return Err(ended);
            }
            if let Some(picture) = slot.picture.take() {
                break picture;
            }
            slot = self.latest.told.wait(slot).unwrap_or_else(PoisonError::into_inner);
        };
        drop(slot);
        if shot.size() != (self.mode.width, self.mode.height) {
            return Err(CameraError::Format);
        }
        Ok((shot, at))
    }

    /// Gives a picture's memory back, so the next is put in it.
    pub fn recycle(&self, shot: Shot) {
        self.latest.slot().spare = Some(shot.into_memory());
    }
}

#[cfg(test)]
mod tests {
    mod lab {
        use super::super::*;
        use std::time::{Duration, Instant};
        use tier::skipped;

        /// A camera really unplugged while it is read ends as gone, and leaves the list: run by hand, the camera's USB
        /// device unbound from `uvcvideo` (as root) while the test waits, then bound again.
        #[test]
        #[ignore = "by hand: unbind the camera's USB device while it waits"]
        fn a_camera_unplugged_while_read_ends_as_gone_and_leaves_the_list() {
            let Some(_turn) = tier::turn("camera") else { return skipped("another test holds the camera") };
            let Some(device) = list().expect("listed").into_iter().next() else {
                return skipped("no camera plugged in");
            };
            let mut camera = Camera::open(&device.id, Mode { width: 640, height: 480, frames_per_second: 15 })
                .expect("the camera opens");
            println!("reading {}: unbind it now", device.label);
            let until = Instant::now() + Duration::from_secs(60);
            let ended = loop {
                match camera.read() {
                    Ok(_) if Instant::now() < until => {}
                    Ok(_) => panic!("still reading after a minute"),
                    Err(ended) => break ended,
                }
            };
            assert_eq!(ended, CameraError::Gone);
            let listed = list().expect("listed");
            assert!(listed.iter().all(|each| each.id != device.id), "left the list: {listed:?}");
        }
    }
}
