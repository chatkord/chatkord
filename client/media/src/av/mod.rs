//! The one safe layer over the pinned FFmpeg: every `unsafe` call into it is here, a wrapper's handle is private, and no
//! raw pointer leaves. A device, its frames, a codec context, a frame, a packet, a filter and a converter each own what
//! they hold and free it once; a status is an [`AvError`], a format an enum with FFmpeg's value in one place.
#![cfg_attr(
    not(feature = "testing"),
    expect(unreachable_pub, reason = "public only for the tests' painted DMA-BUF, which the `testing` feature builds")
)]

mod bsf;
mod codec;
#[cfg(windows)]
mod d3d11;
#[cfg(windows)]
mod d3d12;
mod device;
mod dict;
#[cfg(target_os = "linux")]
mod drm;
mod error;
mod format;
mod frame;
mod frames;
#[cfg(target_os = "linux")]
mod graph;
mod log;
mod packet;
mod scale;
#[cfg(target_os = "linux")]
mod vulkan;

pub use bsf::UnitFilter;
pub use codec::{CodecContext, CodecRef, INPUT_PADDING};
#[cfg(test)]
pub use codec::{Rates, StreamCodec, unwanted_codec_names};
#[cfg(target_os = "linux")]
pub use device::VaDisplay;
#[cfg(any(target_os = "linux", windows))]
pub use device::can_make_hardware_devices;
pub use device::{HwDevice, device_format};
#[cfg(target_os = "linux")]
pub use drm::DrmMapping;
// What the public methods return, named only where the layer is public: for the tests' painted DMA-BUF.
#[cfg(all(target_os = "linux", feature = "testing"))]
pub use drm::{DrmLayer, DrmObject, DrmPlane};
pub use error::AvError;
pub use format::{ColourRange, ColourSpace, PixelFormat};
#[cfg(feature = "testing")]
pub use frame::PlaneMut;
#[cfg(test)]
pub(crate) use frame::counted;
pub use frame::{Frame, Plane};
pub use frames::HwFrames;
#[cfg(target_os = "linux")]
pub use graph::{Graph, GraphSource};
pub use log::forward_log;
#[cfg(test)]
pub use log::{ERROR_LEVEL, log_error_line};
pub use packet::Packet;
pub use scale::{Conversion, Scaler, View, ViewMut};
#[cfg(all(target_os = "linux", feature = "testing"))]
pub use vulkan::{QueueFamily, VulkanDevice};
#[cfg(target_os = "linux")]
pub use vulkan::{QueueLocks, VulkanImage};
