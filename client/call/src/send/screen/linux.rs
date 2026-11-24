//! Linux's screen: the portal's PipeWire capture, opened on the picked node, then joined to an encoder of the size of
//! its first picture.

use super::Fail;
use super::run::{Report, Started, begin_with, count, frames_per_second};
use capture::Capture;
use choices::ShareQuality;
use media::Survey;
use std::os::fd::OwnedFd;
use std::sync::Arc;

/// Opens the capture on the picked `node` and an encoder for whatever size the screen sends, one of those `encoders`
/// lists, as `quality` says.
pub fn begin(remote: OwnedFd, node: u32, quality: &ShareQuality, encoders: &Arc<Survey>) -> Result<Started, Fail> {
    let capture = Capture::open(remote, node, &media::importable(), frames_per_second(quality))?;
    begin_with(Box::new(capture), quality, encoders)
}

/// Captures `node` and encodes every picture for a while, sending none, as a share would by default.
pub fn encode(remote: OwnedFd, node: u32) -> Result<Report, Fail> {
    count(begin(remote, node, &ShareQuality::default(), &Arc::new(Survey::system()))?)
}
