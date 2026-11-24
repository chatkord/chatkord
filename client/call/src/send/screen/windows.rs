//! Windows' screen: an encoder opened for the picked item's size first, then Windows Graphics Capture on that
//! encoder's own D3D11 device, so its pictures are read where they are drawn. A first picture of another size is
//! followed as any change of size is.

use super::Fail;
use super::run::{Report, Started, begin_reopening, count, frames_per_second};
use choices::ShareQuality;
use media::{Capture, CaptureError, Layout, Paced, Pixels, Survey};
use std::num::NonZeroU32;
use std::sync::Arc;
use windows::Graphics::Capture::GraphicsCaptureItem;

/// Opens an encoder for `item`, one of those `encoders` lists, and its capture into that encoder's device (again on
/// the new one whenever that device is rebuilt), the pointer drawn in when `cursor` is set, taken at the share's rate
/// from a monitor refreshing `refresh` times a second (every refresh where that is unknown), and waits for its first
/// picture. The item of a window `picked` names ends its capture once that window is gone.
pub fn begin(
    item: &GraphicsCaptureItem,
    picked: Option<::share::Id>,
    refresh: Option<NonZeroU32>,
    cursor: bool,
    quality: &ShareQuality,
    encoders: &Arc<Survey>,
) -> Result<Started, Fail> {
    let size = item.Size().map_err(|_| CaptureError::Connect)?;
    let (Ok(width), Ok(height)) = (u32::try_from(size.Width), u32::try_from(size.Height)) else {
        return Err(Fail::Capture(CaptureError::Format));
    };
    let item = item.clone();
    let paced = refresh.map(|refresh| Paced { rate: frames_per_second(quality), refresh });
    begin_reopening(Layout { pixels: Pixels::Bgra, width, height }, quality, encoders, move |session| {
        let gpu = session.gpu().ok_or(CaptureError::Connect)?;
        let capture = Capture::open(&item, &gpu, cursor, paced)?;
        Ok(Box::new(match picked {
            Some(::share::Id::Window(window)) => capture.of_window(window),
            Some(::share::Id::Monitor(_)) | None => capture,
        }))
    })
}

/// Captures `item` and encodes every picture for a while, sending none, as a share would by default.
pub fn encode(item: &GraphicsCaptureItem, refresh: Option<NonZeroU32>) -> Result<Report, Fail> {
    count(begin(item, None, refresh, true, &ShareQuality::default(), &Arc::new(Survey::system()))?)
}
