//! Encode and decode through the pinned FFmpeg, and what this machine can do: encode is hardware first, software after
//! it, so a hardware encoder that fails leaves a successor, and the probe gives the reason for every encoder it cannot
//! open. Capture is the `capture` crate's; FFmpeg is reached only through its `av` layer.

// The screen tests' painted DMA-BUF, shared with the crate's own tests, names the crate as its users do.
#[cfg(all(test, target_os = "linux"))]
extern crate self as media;
/// The one safe layer over FFmpeg, public only for the tests' painted DMA-BUF.
#[cfg(feature = "testing")]
pub mod av;
#[cfg(not(feature = "testing"))]
mod av;
pub mod camera;
mod coded;
mod colours;
mod decode;
mod decoders;
mod device;
mod drivers;
mod encode;
#[cfg(any(test, fuzzing))]
pub mod fuzz;
mod loader;
mod log;
mod multiple;
mod picture;
mod planes;
mod probe;
mod size;
#[cfg(target_os = "linux")]
pub mod startup;
mod survey;
mod tuning;

pub use av::AvError;
#[cfg(any(target_os = "linux", windows))]
pub use av::can_make_hardware_devices;
#[cfg(windows)]
pub use capture::{AdapterId, Capture, D3d11, Gpu, Paced, Preview, PreviewPicture, Still, still};
#[cfg(windows)]
pub use capture::{CaptureError, Frame, Layout, Native, Pixels};
pub use capture::{Card, Reported, Validation, card};
pub use coded::{Coded, CodedError};
pub use colours::{Colours, Matrix, Range};
pub use decode::{DecodeError, Decoded, Decoder, Shown};
#[cfg(windows)]
pub use device::d3d12::{D3d12Device, D3d12Surface, Lender};
#[cfg(target_os = "linux")]
pub use device::surface::{Layer, Object, Plane, Surface};
#[cfg(target_os = "linux")]
pub use device::{importable, vulkan_image};
#[cfg(target_os = "linux")]
pub use drivers::pci_ids;
pub use drivers::{Drivers, order_va_drivers, run_hidden};
#[cfg(any(target_os = "linux", windows))]
pub use encode::Convert;
pub use encode::{Changes, EncodeError, Encoded, FrameError, Nv12, Rate, Reopening, Session, Settings, Timed};
#[cfg(target_os = "linux")]
pub use encode::{Lift, Lifted};
pub use loader::EndsAlone;
#[cfg(target_os = "linux")]
pub use loader::{Alone, alone};
pub use multiple::Multiple;
pub use picture::{Picture, PictureLayout, Samples};
pub use probe::{
    Backend, Codec, Device, Encoder, Path, PciVendor, Rank, Report, Runs, SCREEN_ORDER, Sources, Unavailable, Why,
    survey, survey_on,
};
pub use survey::{Devices, Survey, Surveyor};
pub use tuning::{Family, KEY_FRAME_BUDGET_MS, Measured, Name, h264_level};
