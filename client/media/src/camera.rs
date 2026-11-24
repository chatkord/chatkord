//! A camera's pictures on the processor's side of the device: the decoder that makes a picture of what a camera handed
//! over, and the route that takes it to an encoder and to one's own card. The camera itself is `capture::camera`.

pub use crate::decode::mjpeg::{Decoded, Decoder};
#[cfg(target_os = "linux")]
pub use crate::encode::Inputs;
pub use crate::encode::camera::{NotReady, Picture, Ready, Route, ToNv12};
