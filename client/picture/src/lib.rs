//! The pictures the window paints: the slot a picture waits in, the paint of its planes, and the import of a picture
//! already on the GPU as a texture. Decoding is `media`'s; this crate never opens a socket or touches the engine.

mod choose;
mod conversion;
mod current;
#[cfg(windows)]
mod d3d12;
#[cfg(target_os = "linux")]
mod dmabuf;
mod drivers;
#[cfg(not(target_os = "linux"))]
mod elsewhere;
mod frame;
#[cfg(any(target_os = "linux", windows))]
mod painting;
#[cfg(target_os = "linux")]
mod release;
#[cfg(target_os = "linux")]
mod vulkan;
mod yuv;

pub use current::Current;
#[cfg(windows)]
pub use d3d12::{Held, Imports, OnWindow, Refused as ImportRefused, import};
#[cfg(target_os = "linux")]
pub use dmabuf::{Asleep, FEATURES as IMPORT_FEATURES, Refused as ImportRefused, painting_that_presents, render_node};
pub use drivers::{Drivers, leave_out_sleeping, run_hidden};
#[cfg(not(target_os = "linux"))]
pub use elsewhere::{Asleep, Display, IMPORT_FEATURES, adapters, device, instance, painting_that_presents};
pub use frame::{Frame, Malformed, Planes, Slot, Stats};
pub use media::EndsAlone;
#[cfg(any(target_os = "linux", windows))]
pub use painting::Painting;
#[cfg(target_os = "linux")]
pub use vulkan::{DeviceError, Display, adapters, device, instance};
pub use yuv::{Crop, Framing, Gpu, Imported, Painter, Shape, TooLarge};
