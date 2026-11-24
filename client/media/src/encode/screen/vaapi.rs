//! VA-API's screen path: one filter graph per size and device, from the compositor's buffers as surfaces made once
//! each (`surfaces`) through the device's `scale_vaapi` to the encoder's frames.
//!
//! The driver's scaler drops one-pixel lines at a ratio far from one, so a picture over twice the stream is halved
//! first; a 4K page of such lines sent at 720p still measured 18.3 dB against the area mean on Machine A.

use std::ffi::{CString, c_int};

use super::surfaces::Surfaces;
use crate::av::{ColourRange, ColourSpace, Frame, HwDevice, HwFrames};
use crate::device::vaapi::{Scale, Source};
use crate::encode::{EncodeError, Settings};
use capture::{Frame as Captured, Layout};

pub(crate) struct Path {
    // Declared first: the graph is freed before the frames it was built from.
    scale: Scale,
    from: Surfaces,
}

impl Path {
    /// The graph that reads the top-left `read` of DMA-BUFs laid out as `source` and leaves NV12 of the settings'
    /// size in `device`: the device's own convert scales the input frame's cropped region, which is `read`.
    pub(super) fn open(
        device: &HwDevice,
        settings: Settings,
        source: Layout,
        read: (u32, u32),
    ) -> Result<Self, EncodeError> {
        let from = Surfaces::new(device, source, read)?;
        let to_c = |value: usize| c_int::try_from(value).ok().filter(|v| *v > 0).ok_or(EncodeError::Settings);
        let (width, height) = (to_c(source.width as usize)?, to_c(source.height as usize)?);
        let sizes = halvings((read.0 as usize, read.1 as usize), (settings.width, settings.height));
        let options = |(width, height), last| {
            CString::new(format!("{}w={width}:h={height}", if last { "format=nv12:" } else { "" }))
                .map_err(|_| EncodeError::Settings)
        };
        let steps = sizes
            .iter()
            .chain([&(settings.width, settings.height)])
            .enumerate()
            .map(|(i, size)| options(*size, i == sizes.len()))
            .collect::<Result<Vec<_>, _>>()?;
        let fed = Source {
            frames: from.frames(),
            width,
            height,
            frames_per_second: to_c(settings.frames_per_second as usize)?,
            space: ColourSpace::Bt709,
            range: ColourRange::Unspecified,
        };
        let scale = Scale::open(device, &fed, &steps)?;
        Ok(Self { scale, from })
    }

    /// The frames the converted pictures are drawn from: the encoder is opened on these.
    pub(super) fn frames(&self) -> Result<HwFrames, EncodeError> {
        self.scale.frames()
    }

    /// Reads `captured` on the GPU and returns the NV12 picture, `pts` stamped. The compositor's buffer goes back as
    /// soon as the GPU has read it, before this returns.
    pub(super) fn convert(&mut self, captured: Captured, pts: i64) -> Result<Frame, EncodeError> {
        let picture = self.from.picture(&captured, pts)?;
        // The GPU has read the buffer once the graph returns: only then may the compositor have it back, or it could
        // draw the next picture over this one.
        let converted = self.scale.run(picture)?;
        drop(captured);
        Ok(converted)
    }
}

/// The sizes a picture of `from` is halved to on its way to a stream of `to`, each even: while it is at least twice
/// the stream on both sides. Empty when the device's scaler takes it in one step.
fn halvings(from: (usize, usize), to: (usize, usize)) -> Vec<(usize, usize)> {
    let mut sizes = Vec::new();
    let mut at = from;
    while at.0 >= to.0 * 2 && at.1 >= to.1 * 2 && (at.0 / 2, at.1 / 2) != to {
        at = ((at.0 / 2) & !1, (at.1 / 2) & !1);
        sizes.push(at);
    }
    sizes
}

#[cfg(test)]
mod tests {
    use super::halvings;

    #[test]
    fn a_picture_is_halved_while_it_is_twice_the_stream_and_the_last_step_is_the_scaler_s() {
        assert_eq!(halvings((3840, 2160), (1280, 720)), [(1920, 1080)]);
        assert_eq!(halvings((3840, 2160), (1920, 1080)), [], "exactly half is the scaler's one step");
        assert_eq!(halvings((7680, 4320), (1280, 720)), [(3840, 2160), (1920, 1080)]);
        assert_eq!(halvings((3840, 2160), (2560, 1440)), []);
        assert_eq!(halvings((3840, 2160), (3840, 2160)), []);
    }
}
