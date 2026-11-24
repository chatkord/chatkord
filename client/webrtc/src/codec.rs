//! The video codecs a screen is sent and watched in, as the engine names them.

use libwebrtc::video_frame::EncodedVideoCodec;
pub use rtc::Codec;

/// The engine's name for `codec`.
pub(crate) const fn encoded(codec: Codec) -> EncodedVideoCodec {
    match codec {
        Codec::H264 => EncodedVideoCodec::H264,
        Codec::Av1 => EncodedVideoCodec::AV1,
    }
}
