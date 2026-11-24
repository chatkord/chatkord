//! Linux's parts of a session: a camera's pictures lifted onto the encoder's own VA-API device, and the Vulkan device
//! a screen's area pass runs on beside a VA-API encoder.

use super::{EncodeError, Encoded, Feed, Lift, Lifted, Session, Settings, Timed};
use crate::av::HwDevice;
use crate::probe::Encoder;
use std::time::Instant;

impl Session {
    /// Opens `encoder` for `settings` to take a camera's pictures lifted by `lift`, which is on the encoder's own
    /// device and makes [`crate::Colours::HD`], through [`Session::encode_lifted`]; refused where the encoder codes a
    /// smaller size than the lift's whole pictures.
    pub fn open_lifted(encoder: &Encoder, settings: Settings, lift: &Lift) -> Result<Self, EncodeError> {
        if super::coded(encoder, settings)? != settings {
            return Err(EncodeError::Settings);
        }
        let feed = Feed::Lifted { pool: lift.frames()?, device: lift.device().ok_or(EncodeError::Device)? };
        Self::begin(encoder, settings, (settings.width, settings.height), crate::Colours::HD, None, feed)
    }

    /// Encodes one of a camera's pictures `lifted` onto the encoder's device and returns whatever the encoder has
    /// finished. Only a session opened with [`Session::open_lifted`] takes these.
    pub fn encode_lifted(&mut self, lifted: &Lifted) -> Result<Vec<Encoded>, EncodeError> {
        if !matches!(self.parts.feed, Feed::Lifted { .. }) {
            return Err(EncodeError::Input);
        }
        let mut out = self.reopen()?.held;
        self.timed = Timed::default();
        let started = Instant::now();
        let key_frame = self.key_frame_now();
        let mut picture = lifted.reference().ok_or(EncodeError::Convert)?;
        let (width, height) = picture.size();
        let expected = self.pending.size.unwrap_or((self.settings.width, self.settings.height));
        if (usize::try_from(width).ok(), usize::try_from(height).ok()) != (Some(expected.0), Some(expected.1)) {
            return Err(EncodeError::FrameSize);
        }
        self.send(&mut picture, key_frame)?;
        self.change = Some(self.next);
        self.sent(Instant::now())?;
        out.extend(self.drain()?);
        // The lift converted the picture before it came: all of this is the encoder's.
        self.timed = Timed { convert: std::time::Duration::ZERO, encode: started.elapsed() };
        Ok(out)
    }
}

impl Feed {
    /// The Vulkan device a screen's area pass runs on beside the encoder's; none where the convert is on the
    /// encoder's own device.
    pub(super) fn pass_device(&self) -> Option<&HwDevice> {
        match self {
            Self::Screen { path, .. } | Self::ScreenPlanar { path, .. } => path.pass_device(),
            Self::Upload { .. } | Self::Planar { .. } | Self::Lifted { .. } => None,
        }
    }

    /// The same lifted feed again, for another encoder on its device: the lift's frames are its pictures' own.
    pub(super) fn lifted_again(&self) -> Option<Self> {
        match self {
            Self::Lifted { pool, device } => Some(Self::Lifted { pool: pool.again()?, device: device.again()? }),
            Self::Upload { .. } | Self::Screen { .. } | Self::Planar { .. } | Self::ScreenPlanar { .. } => None,
        }
    }
}
