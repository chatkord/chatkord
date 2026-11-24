//! The engine's names for a picture's track and stream.

use proto::video::Video;

/// The name the engine gives `video`'s track and the stream it is sent in.
pub(crate) const fn label(video: Video) -> &'static str {
    match video {
        Video::Screen => "screen",
        Video::Camera => "camera",
    }
}
