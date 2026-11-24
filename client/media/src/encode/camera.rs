//! A camera's pictures on their way to one encoder and to one's own card. Where the encoder is on a VA-API device
//! that converts them, each picture is lifted onto it (`encode::lift`): the encoder and the card take the same
//! surface, and the processor's work past the decode is one upload. Elsewhere each is made NV12 in memory, which the
//! encoder uploads and the card copies.

use super::to_nv12;
use super::{EncodeError, Encoded, Nv12, Reopening, Session, Settings};
use crate::colours::Colours;
use crate::probe::Encoder;
use capture::camera::CameraError;
use std::borrow::Cow;

#[cfg(target_os = "linux")]
use super::{Changes, Inputs, Lift, Lifted};
use crate::decode::mjpeg::Decoded;

/// A camera's picture, as the system's capture hands it over.
#[derive(Debug)]
pub enum Picture {
    /// Packed NV12 already, taken as it is: what a stand-in camera hands over, as the tests make it.
    Nv12(Nv12),
    /// The picture as the camera sent it, in memory: a JPEG decoded to its own planes, or YUY2 or NV12, with its
    /// range. A device lifts it (`Lift`, on Linux), or [`ToNv12`] makes it NV12 on the processor.
    Decoded(Decoded),
}

impl Picture {
    /// Pixels across and down.
    pub fn size(&self) -> (usize, usize) {
        match self {
            Picture::Nv12(nv12) => (nv12.width(), nv12.height()),
            Picture::Decoded(decoded) => (decoded.width() as usize, decoded.height() as usize),
        }
    }
}

/// Makes a camera's pictures NV12 in memory, keeping what it needs from one picture to the next.
#[derive(Default)]
pub struct ToNv12 {
    converter: to_nv12::ToNv12,
}

impl ToNv12 {
    /// What the pictures it makes carry, and so what their stream is tagged: a camera's BT.601 in limited range, as
    /// the processor converts them.
    pub const COLOURS: Colours = to_nv12::COLOURS;

    /// `picture` as packed NV12: itself, or converted on the processor.
    pub fn nv12<'a>(&mut self, picture: &'a Picture) -> Result<Cow<'a, Nv12>, CameraError> {
        self.nv12_at(picture, None)
    }

    /// `picture` as packed NV12 of `size` where one is given and it is not the picture's own: scaled in the same pass
    /// that converts it.
    pub fn nv12_at<'a>(
        &mut self,
        picture: &'a Picture,
        size: Option<(u32, u32)>,
    ) -> Result<Cow<'a, Nv12>, CameraError> {
        let own = picture.size();
        let wanted = size.filter(|(width, height)| (*width as usize, *height as usize) != own);
        match (picture, wanted) {
            (Picture::Nv12(nv12), None) => Ok(Cow::Borrowed(nv12)),
            (Picture::Nv12(nv12), Some(size)) => self.converter.scaled(nv12, size).map(Cow::Owned),
            (Picture::Decoded(decoded), None) => self.converter.nv12(decoded).map(Cow::Owned),
            (Picture::Decoded(decoded), Some(size)) => self.converter.nv12_at(decoded, size).map(Cow::Owned),
        }
    }
}

/// How the pictures go.
enum Way {
    /// Lifted onto the encoder's device.
    #[cfg(target_os = "linux")]
    Lifted(Lift),
    /// Made NV12 in memory.
    Memory(ToNv12),
}

/// How a camera's pictures are made ready for an encoder and one's own card: lifted onto the encoder's device, or
/// NV12 in memory, at the camera's own size or scaled down as they are converted. The encoder's [`Session`] is the
/// caller's, so one loop runs a camera as it runs a share; a route that changes the stream's size or kind says what the
/// old stream still held through [`Reopening`].
pub struct Route {
    way: Way,
    /// The size the pictures are made, where it is not the camera's own: a step down scales them as they are
    /// converted, and the camera's mode is never reopened.
    size: Option<(u32, u32)>,
    /// What a lift is made of again after the route went through memory.
    #[cfg(target_os = "linux")]
    inputs: Option<Inputs>,
}

/// A picture made ready for the encoder and the card.
pub enum Ready<'a> {
    /// NV12 in memory: the camera's own where it is that already, else converted.
    Nv12(Cow<'a, Nv12>),
    /// NV12 on the encoder's device.
    #[cfg(target_os = "linux")]
    Lifted(Lifted),
}

/// Why a picture could not be made ready.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotReady {
    /// The picture is not one the camera's format allows: the camera's own fault.
    Camera(CameraError),
    /// The device would not lift it: the pictures can still go through memory.
    Lift(EncodeError),
}

impl Ready<'_> {
    /// Encodes it on `session`, which opened for pictures of its kind, and returns whatever the encoder has finished.
    pub fn encode(&self, session: &mut Session) -> Result<Vec<Encoded>, EncodeError> {
        match self {
            #[cfg(target_os = "linux")]
            Ready::Lifted(lifted) => session.encode_lifted(lifted),
            Ready::Nv12(nv12) => session.encode(nv12),
        }
    }
}

impl Route {
    /// Pictures like `first` lifted onto `encoder`'s own VA-API device, at `settings`, and the session of the encoder
    /// on it; refused where the encoder or its device cannot take them.
    #[cfg(target_os = "linux")]
    pub fn lifted(encoder: &Encoder, settings: Settings, first: &Decoded) -> Result<(Self, Session), EncodeError> {
        let lift = Lift::open(encoder, first, settings.frames_per_second)?;
        let session = Session::open_lifted(encoder, settings, &lift)?;
        Ok((Self { way: Way::Lifted(lift), size: None, inputs: Inputs::of(first) }, session))
    }

    /// A session on `encoder` for the pictures [`Route::memory`] makes, its stream tagged as they are.
    pub fn memory_session(encoder: &Encoder, settings: Settings) -> Result<Session, EncodeError> {
        Session::open_in(encoder, settings, ToNv12::COLOURS)
    }

    /// Pictures like `first` made NV12 in memory, for a session [`Route::memory_session`] opens.
    pub fn memory(first: &Picture) -> Self {
        #[cfg(not(target_os = "linux"))]
        let _ = first;
        Self {
            way: Way::Memory(ToNv12::default()),
            size: None,
            #[cfg(target_os = "linux")]
            inputs: match first {
                Picture::Decoded(decoded) => Inputs::of(decoded),
                Picture::Nv12(_) => None,
            },
        }
    }

    /// Whether the pictures are lifted onto the encoder's device.
    pub fn lifts(&self) -> bool {
        match self.way {
            #[cfg(target_os = "linux")]
            Way::Lifted(_) => true,
            Way::Memory(_) => false,
        }
    }

    /// The size the pictures are made when it is not the camera's own.
    pub fn scaled_to(&self) -> Option<(u32, u32)> {
        self.size
    }

    /// Makes the pictures `size`, scaled as they are converted, for `session`, which is the route's: in memory the
    /// session asks for the new size and the next encode makes it; lifted, a new video processor and a new session
    /// replace both, and what the old stream still held and what changed is told. A lifted route that cannot is
    /// refused, and the pictures can still go through memory ([`Route::leave_lift`]).
    pub fn resize(&mut self, session: &mut Session, size: (u32, u32)) -> Result<Option<Reopening>, EncodeError> {
        let to_usize = |value: u32| usize::try_from(value).map_err(|_| EncodeError::Settings);
        let told = match &mut self.way {
            Way::Memory(_) => {
                session.set_size(to_usize(size.0)?, to_usize(size.1)?)?;
                None
            }
            #[cfg(target_os = "linux")]
            Way::Lifted(lift) => {
                let sized = Lift::open_as(session.running().2, lift.inputs(), size, session.rate().frames_per_second)?;
                let told = Self::replaced(session, &sized, size)?;
                *lift = sized;
                Some(told)
            }
        };
        self.size = Some(size);
        Ok(told)
    }

    /// Replaces `session` by one of its encoder on `lift`'s device, at `size` and the rate it ran at.
    #[cfg(target_os = "linux")]
    fn replaced(session: &mut Session, lift: &Lift, size: (u32, u32)) -> Result<Reopening, EncodeError> {
        let to_usize = |value: u32| usize::try_from(value).map_err(|_| EncodeError::Settings);
        let (width, height, encoder) = session.running();
        let (before, encoder) = ((width, height), encoder.clone());
        let rate = session.rate();
        let settings = Settings {
            width: to_usize(size.0)?,
            height: to_usize(size.1)?,
            frames_per_second: rate.frames_per_second,
            bits_per_second: rate.bits_per_second,
            key_frame_every: None,
        };
        let opened = Session::open_lifted(&encoder, settings, lift)?;
        let old = std::mem::replace(session, opened);
        let after = (settings.width, settings.height);
        let changes = Changes { size: (before != after).then_some((before, after)), ..Changes::default() };
        Ok(Reopening { held: old.finish().unwrap_or_default(), changes, in_place: false })
    }

    /// The pictures go through memory from now on, as a device that stopped lifting them leaves them; the caller opens
    /// the session for them with [`Route::memory_session`].
    pub fn leave_lift(&mut self) {
        self.way = Way::Memory(ToNv12::default());
    }

    /// A memory route made lifted again onto `session`'s own encoder, for the kind of picture it began with and at the
    /// size it sends: a lift refused earlier may be taken now. None where it was not made, whatever refused; the
    /// route is then as it was.
    #[cfg(target_os = "linux")]
    pub fn relift(&mut self, session: &mut Session) -> Option<Reopening> {
        let (Way::Memory(_), Some(inputs)) = (&self.way, self.inputs) else { return None };
        let size = self.size.unwrap_or(inputs.size());
        let lift = Lift::open_as(session.running().2, inputs, size, session.rate().frames_per_second).ok()?;
        let told = Self::replaced(session, &lift, size).ok()?;
        self.way = Way::Lifted(lift);
        Some(told)
    }

    /// `picture` made ready for the encoder and the card.
    pub fn ready<'a>(&mut self, picture: &'a Picture) -> Result<Ready<'a>, NotReady> {
        match (&mut self.way, picture) {
            #[cfg(target_os = "linux")]
            (Way::Lifted(lift), Picture::Decoded(decoded)) => {
                lift.lift(decoded).map(Ready::Lifted).map_err(NotReady::Lift)
            }
            #[cfg(target_os = "linux")]
            (Way::Lifted(_), Picture::Nv12(_)) => Err(NotReady::Lift(EncodeError::Input)),
            (Way::Memory(to_nv12), picture) => {
                to_nv12.nv12_at(picture, self.size).map(Ready::Nv12).map_err(NotReady::Camera)
            }
        }
    }
}
