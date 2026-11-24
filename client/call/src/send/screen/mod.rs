//! The screen, from the picker to encoded units, counted or sent. This reads no pixel: the picture stays on the video
//! device, or `media` brings it to memory for a software encoder; each system opens its capture its own way and runs
//! it the same way.

mod fallback;
mod key_gate;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(any(target_os = "linux", windows))]
mod reopens;
mod run;
mod stages;
#[cfg(windows)]
mod windows;

use capture::{CaptureError, Frame};
use choices::EncoderOption;
pub use fallback::{Ladder, Move};
#[cfg(target_os = "linux")]
pub use linux::{begin, encode};
use media::EncodeError;
#[cfg(any(target_os = "linux", windows))]
pub use reopens::{Cause, Reopens};
pub use run::{
    CAPTURE_FOR, Lender, Preview, Progress, Report, Resized, SETTLE, Screen, Started, begin_into, begin_with,
};
pub(crate) use run::{Prepared, first_open};
pub use stages::{Counted, KeyAsks, KeyCause, KeyFrames, Stages, Worst};
use std::sync::Arc;
use std::time::Duration;
#[cfg(windows)]
pub use windows::{begin, encode};

/// Why the run ended before it had a report.
#[derive(Debug)]
pub enum Fail {
    /// The capture would not open or ended.
    Capture(CaptureError),
    /// The encoder would not open or encode.
    Encode(EncodeError),
    /// The screen sent no picture in time.
    Silent,
    /// The camera would not give a picture or went.
    Camera(capture::camera::CameraError),
}

impl From<CaptureError> for Fail {
    fn from(why: CaptureError) -> Self {
        Self::Capture(why)
    }
}

impl From<capture::camera::CameraError> for Fail {
    fn from(why: capture::camera::CameraError) -> Self {
        Self::Camera(why)
    }
}

impl From<EncodeError> for Fail {
    fn from(why: EncodeError) -> Self {
        Self::Encode(why)
    }
}

/// Where a screen's pictures come from: the portal's capture, or a test's own.
pub trait Frames: Send {
    /// The newest picture, waiting up to `wait` for one; none is a screen with nothing new.
    fn next(&self, wait: Duration) -> Result<Option<Frame>, CaptureError>;
    /// Pictures the source made that nobody took before the next one came.
    fn dropped(&self) -> u64;
    /// Pictures the source made, those nobody took included; none from a source that does not count them.
    fn delivered(&self) -> u64 {
        0
    }
    /// The widest gap between two pictures the source made since this was last asked, which starts it over; none from a
    /// source that does not time them.
    fn take_widest_gap(&self) -> Duration {
        Duration::ZERO
    }
    /// What the source calls itself, where it says: KWin names a window's after its app.
    fn source_name(&self) -> Option<String> {
        None
    }
}

/// This device's encoders for a screen, from `survey`, each codec's in the order a share tries them; none is a device
/// with no encoder, which cannot share. A survey may ask the drivers, so it runs on the blocking pool.
pub async fn encoders(survey: Arc<media::Survey>) -> Vec<EncoderOption> {
    tokio::task::spawn_blocking(move || options(&survey.report())).await.unwrap_or_default()
}

/// The codecs `report` offers a screen and what encodes each, each pair once, in the order a share tries them.
fn options(report: &media::Report) -> Vec<EncoderOption> {
    let mut options: Vec<EncoderOption> = Vec::new();
    for codec in choices::Codec::ALL {
        for encoder in report.screen_order(crate::codec::encoded(codec)) {
            let option = crate::codec::encoder_option(codec, encoder);
            if !options.contains(&option) {
                options.push(option);
            }
        }
    }
    options
}

/// A screen to send: the one the portal gave, or a test's own pictures.
pub enum Pick {
    /// What the system's picker gave.
    Portal(::share::Selection),
    /// A test's own pictures.
    Frames(Box<dyn Frames>),
}

impl std::fmt::Debug for Pick {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Portal(_) => "Pick::Portal",
            Self::Frames(_) => "Pick::Frames",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use media::{Device, Encoder, Name, Runs};

    #[test]
    fn each_codec_is_listed_once_with_what_encodes_it() {
        let device = |backend| Device {
            backend,
            node: None,
            vendor: Some(media::PciVendor::AMD),
            validation: media::Validation::Off,
        };
        let software = |backend| Encoder {
            name: Name::SvtAv1,
            runs: Runs::Cpu { convert: Some(device(backend)) },
            multiple: media::Multiple::EVEN,
        };
        let on_gpu = Runs::Gpu(device(media::SCREEN_ORDER[0]));
        #[cfg(windows)]
        let h264 = Encoder { name: Name::H264Amf, runs: on_gpu, multiple: media::Multiple::EVEN };
        #[cfg(not(windows))]
        let h264 = Encoder { name: Name::H264Vulkan, runs: on_gpu, multiple: media::Multiple::EVEN };
        let encoders = std::iter::once(h264).chain(media::SCREEN_ORDER.map(software)).collect();
        let report = media::Report { encoders, unavailable: vec![] };
        let listed: Vec<_> = options(&report).into_iter().map(|o| (o.codec, o.on)).collect();
        assert_eq!(
            listed,
            [
                (choices::Codec::H264, choices::EncodesOn::GraphicsCard),
                (choices::Codec::Av1, choices::EncodesOn::Processor),
            ]
        );
    }
}
