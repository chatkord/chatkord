//! How many devices of each kind the host has listed so far this run, which the debug report counts and never names.

use crate::window::Message;
use crate::{CallMessage, PlaceMessage, ProbeHeard, SourceKind};

/// The counts of the last listing of each kind; none until that kind was listed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Counted {
    pub(crate) microphones: Option<usize>,
    pub(crate) speakers: Option<usize>,
    pub(crate) cameras: Option<usize>,
    pub(crate) monitors: Option<usize>,
}

impl Counted {
    /// Takes in the listing `message` carries, if it carries one. The system's default alias is not a device.
    pub(crate) fn see(&mut self, message: &Message) {
        match message {
            Message::Probed(ProbeHeard::Sound(sound)) => {
                let real = |listed: &[crate::Listed]| listed.iter().filter(|each| each.asked.is_some()).count();
                self.microphones = Some(real(&sound.microphones));
                self.speakers = Some(real(&sound.speakers));
            }
            Message::Probed(ProbeHeard::Cameras(Ok(cameras))) => self.cameras = Some(cameras.len()),
            Message::At(_, PlaceMessage::Call(CallMessage::Sources(sources))) => {
                self.monitors = Some(sources.iter().filter(|source| source.kind == SourceKind::Screen).count());
            }
            _ => {}
        }
    }
}
