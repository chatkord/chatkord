//! A camera's pictures as the device hands them over, in memory: the bytes of a JPEG, or YUY2 or NV12 rows, with the
//! size and how they are laid out. Turning them into pictures is `media`'s, so no codec is here.

#[cfg(any(test, feature = "testing"))]
pub mod fixtures;
#[cfg(any(target_os = "linux", windows))]
mod hub;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(not(any(target_os = "linux", windows)))]
mod none;
#[cfg(windows)]
mod windows;

use std::fmt;

#[cfg(all(test, windows))]
use self::windows::removed;
#[cfg(windows)]
pub use self::windows::{Camera, Stopper, Watch, list, modes, watch};
#[cfg(all(test, target_os = "linux"))]
use linux::removed;
#[cfg(target_os = "linux")]
pub use linux::{Camera, Stopper, Watch, list, modes, watch};
#[cfg(not(any(target_os = "linux", windows)))]
pub use none::{Camera, Stopper, Watch, list, modes, watch};

/// A camera this system offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    /// The system's own name for it, opaque and stable while it stays plugged in: what [`Camera::open`] takes.
    pub id: String,
    /// The name a person knows it by.
    pub label: String,
}

/// A size and rate a camera captures at, or is asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mode {
    /// Pixels across.
    pub width: u32,
    /// Pixels down.
    pub height: u32,
    /// Pictures a second, whole.
    pub frames_per_second: u32,
}

impl Mode {
    /// The mode of `offered` nearest `asked`: the largest picture no larger than asked, at the rate nearest the one
    /// asked and at least it where one is; the smallest picture when every one is larger. Sizes are even, which NV12
    /// needs; none when no mode is.
    pub fn nearest(offered: &[Mode], asked: Mode) -> Option<Mode> {
        let even = offered.iter().filter(|mode| mode.width.is_multiple_of(2) && mode.height.is_multiple_of(2));
        let fits = |mode: &&Mode| mode.width <= asked.width && mode.height <= asked.height;
        let size = even
            .clone()
            .filter(fits)
            .map(|mode| (mode.width, mode.height))
            .max_by_key(|(width, height)| u64::from(*width) * u64::from(*height))
            .or_else(|| {
                even.clone()
                    .map(|mode| (mode.width, mode.height))
                    .min_by_key(|(width, height)| u64::from(*width) * u64::from(*height))
            })?;
        even.filter(|mode| (mode.width, mode.height) == size).copied().min_by_key(|mode| {
            let short = mode.frames_per_second < asked.frames_per_second;
            (short, mode.frames_per_second.abs_diff(asked.frames_per_second))
        })
    }
}

/// How a camera hands a picture over, cheapest to make NV12 first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Pixel {
    /// NV12 itself: copied by rows.
    Nv12,
    /// Packed 4:2:2, converted.
    Yuy2,
    /// A JPEG each picture, decoded: the only way most USB cameras send 720p30.
    Mjpeg,
}

/// The zeros kept after a picture's bytes, for a decoder that reads past them (FFmpeg's `AV_INPUT_BUFFER_PADDING_SIZE`,
/// which `media` checks it does not outgrow).
pub const PADDING: usize = 64;

/// A camera's picture as it came, copied out of its buffer: its bytes and how they are laid out, to be checked against
/// the size settled on before a byte of them is read.
#[derive(Debug)]
pub struct Shot {
    bytes: Vec<u8>,
    length: usize,
    stride: Option<u32>,
    pixel: Pixel,
    size: (u32, u32),
}

impl Shot {
    /// The picture `picture` holds, `size` big and laid out as `pixel`, put in `spare`'s memory where it is large
    /// enough; `stride` is the bytes from one row to the next where the camera said.
    pub fn new(picture: &[u8], stride: Option<u32>, pixel: Pixel, size: (u32, u32), spare: Option<Vec<u8>>) -> Self {
        let mut bytes = spare.unwrap_or_default();
        bytes.clear();
        bytes.reserve(picture.len() + PADDING);
        bytes.extend_from_slice(picture);
        bytes.resize(picture.len() + PADDING, 0);
        Self { bytes, length: picture.len(), stride, pixel, size }
    }

    /// The picture's bytes, and no padding.
    pub fn picture(&self) -> &[u8] {
        &self.bytes[..self.length]
    }

    /// The bytes with [`PADDING`] zeros after them.
    pub fn padded(&self) -> &[u8] {
        &self.bytes
    }

    /// How many bytes are the picture's.
    pub fn length(&self) -> usize {
        self.length
    }

    /// The bytes from one row to the next, where the camera said.
    pub fn stride(&self) -> Option<u32> {
        self.stride
    }

    /// How the picture is laid out.
    pub fn pixel(&self) -> Pixel {
        self.pixel
    }

    /// Its width and height as the camera settled on.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// The memory, to be given back to the camera for its next picture.
    pub fn into_memory(self) -> Vec<u8> {
        self.bytes
    }
}

/// Why a camera does not give pictures, each a sentence a person reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CameraError {
    /// The system's privacy settings keep this application from the camera.
    Denied,
    /// Another application holds the camera.
    Busy,
    /// The camera was unplugged, or is no longer there.
    Gone,
    /// The camera offers no picture this build can encode.
    Format,
    /// The system offers no camera capture here.
    Unavailable,
}

impl fmt::Display for CameraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Denied => f.write_str("the system's privacy settings do not allow the camera"),
            Self::Busy => f.write_str("another application is using the camera"),
            Self::Gone => f.write_str("the camera was disconnected"),
            Self::Format => f.write_str("the camera offers no picture that can be encoded"),
            Self::Unavailable => f.write_str("camera capture is not available on this system"),
        }
    }
}

impl std::error::Error for CameraError {}

#[cfg(test)]
mod tests {
    use super::*;

    const fn mode(width: u32, height: u32, frames_per_second: u32) -> Mode {
        Mode { width, height, frames_per_second }
    }

    const OFFERED: [Mode; 6] = [
        mode(1920, 1080, 30),
        mode(1920, 1080, 60),
        mode(1280, 720, 30),
        mode(1280, 720, 60),
        mode(640, 480, 30),
        mode(640, 480, 15),
    ];

    #[test]
    fn the_largest_picture_no_larger_than_asked_at_the_rate_asked() {
        assert_eq!(Mode::nearest(&OFFERED, mode(1280, 720, 30)), Some(mode(1280, 720, 30)));
        assert_eq!(Mode::nearest(&OFFERED, mode(1600, 900, 30)), Some(mode(1280, 720, 30)), "never scaled up");
        assert_eq!(Mode::nearest(&OFFERED, mode(640, 480, 20)), Some(mode(640, 480, 30)), "at least the rate asked");
        assert_eq!(Mode::nearest(&OFFERED, mode(640, 480, 60)), Some(mode(640, 480, 30)), "else the nearest under it");
    }

    #[test]
    fn a_camera_that_only_captures_larger_gives_its_smallest() {
        assert_eq!(Mode::nearest(&OFFERED, mode(320, 240, 30)), Some(mode(640, 480, 30)));
    }

    #[test]
    fn an_odd_picture_is_never_chosen() {
        assert_eq!(Mode::nearest(&[mode(1279, 719, 30)], mode(1280, 720, 30)), None);
        assert_eq!(Mode::nearest(&[mode(1279, 719, 30), mode(640, 480, 30)], mode(1280, 720, 30)), Some(OFFERED[4]));
        assert_eq!(Mode::nearest(&[], mode(1280, 720, 30)), None);
    }

    #[cfg(any(target_os = "linux", windows))]
    mod lab {
        use super::super::*;
        use std::time::{Duration, Instant};
        use tier::skipped;

        /// Within this a read ended from another thread returns: one picture's time at 15 a second, and room.
        const PROMPTLY: Duration = Duration::from_millis(500);

        /// The machine's first camera, and this test's turn at it.
        fn first() -> Option<(Device, tier::Turn)> {
            let turn = tier::turn("camera")?;
            Some((list().ok()?.into_iter().next()?, turn))
        }

        /// Opens `device` and reads one picture, then has `end` done to it from another thread while the next read
        /// waits: the read returns as gone, promptly.
        fn ended_while_reading(device: &Device, end: impl FnOnce(&Camera) -> Box<dyn FnOnce() + Send>) {
            let mut camera = Camera::open(&device.id, Mode { width: 640, height: 480, frames_per_second: 15 })
                .expect("the camera opens");
            camera.read().expect("a first picture");
            let ending = end(&camera);
            let ended = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(20));
                let at = Instant::now();
                ending();
                at
            });
            let read = loop {
                match camera.read() {
                    Ok(_) => {}
                    failed @ Err(_) => break failed.map(|_| ()),
                }
            };
            let returned = Instant::now();
            let asked = ended.join().expect("the ending thread");
            assert_eq!(read, Err(CameraError::Gone));
            assert!(returned.duration_since(asked) < PROMPTLY, "returned {:?} after", returned.duration_since(asked));
        }

        #[test]
        fn a_camera_removed_while_read_returns_as_gone_at_once() {
            let Some((device, _turn)) = first() else { return skipped("no camera plugged in") };
            let id = device.id.clone();
            ended_while_reading(&device, move |_| Box::new(move || removed(&id)));
        }

        #[test]
        fn a_camera_stopped_while_read_returns_at_once() {
            let Some((device, _turn)) = first() else { return skipped("no camera plugged in") };
            ended_while_reading(&device, |camera| {
                let stopper = camera.stopper();
                Box::new(move || stopper.stop())
            });
        }
    }
}
