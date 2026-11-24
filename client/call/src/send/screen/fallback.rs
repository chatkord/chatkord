//! Which encoder a share or a camera is on after one stops, and when it goes back: a decision that is a function of the
//! encoders' state alone, so it is tested without a device. A lost device is rebuilt once before the encoder is given
//! up; an encoder that fails any other way (a stall included) is given up at once, but a hardware one is remembered, so
//! the next reopen tries it first and, if it falls again, moves on to the encoder that took over.

use media::{EncodeError, Encoder, Path, Reopening, Session};
use std::collections::VecDeque;

/// How the running encoder stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failing {
    /// Its device was lost under it.
    Lost,
    /// It took more pictures than any encoder holds and made no unit for them.
    Stalled,
    /// Any other failure of it or its device.
    Failed,
}

impl From<EncodeError> for Failing {
    fn from(error: EncodeError) -> Self {
        match error {
            EncodeError::DeviceLost => Self::Lost,
            EncodeError::Stalled => Self::Stalled,
            EncodeError::Settings
            | EncodeError::Device
            | EncodeError::Frames
            | EncodeError::Open
            | EncodeError::FrameSize
            | EncodeError::Upload
            | EncodeError::Planar
            | EncodeError::Screen
            | EncodeError::Convert
            | EncodeError::Input
            | EncodeError::OtherCodec
            | EncodeError::Encoder(_) => Self::Failed,
        }
    }
}

/// What to do about an encoder that stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    /// Open the same encoder on a new device.
    Rebuild,
    /// Give the encoder up for the one after it.
    Fall,
}

/// What to do about an encoder that stopped as `failing`, when its device `rebuilt` already: a lost device is rebuilt
/// once, anything else falls at once.
pub(crate) fn after_failure(failing: Failing, rebuilt: bool) -> Move {
    match failing {
        Failing::Lost if !rebuilt => Move::Rebuild,
        Failing::Lost | Failing::Stalled | Failing::Failed => Move::Fall,
    }
}

/// The encoder a reopen goes back to first: the hardware one the share fell from, unless it is the one running, or this
/// reopen `tried` it already, when it moves on to the encoder that took over.
pub(crate) fn returning<'a>(running: &Encoder, fallen: Option<&'a Encoder>, tried: bool) -> Option<&'a Encoder> {
    fallen.filter(|fallen| !tried && *fallen != running)
}

/// The encoders of a codec behind the running one, and the hardware one the stream fell from.
#[derive(Clone, Debug, Default)]
pub struct Ladder {
    /// The encoders after the running one, in the survey's order: each takes over when the one before fails.
    pub(crate) later: VecDeque<Encoder>,
    /// The hardware encoder the stream fell from, which the next reopen tries first.
    fallen: Option<Encoder>,
    /// Whether the running encoder's device was rebuilt already.
    rebuilt: bool,
}

impl Ladder {
    /// A ladder under a running encoder whose successors are `later`.
    pub(crate) fn new(later: VecDeque<Encoder>) -> Self {
        Self { later, ..Self::default() }
    }

    /// The running encoder `running` stopped as `error` says: whether its device is rebuilt or it is given up, which
    /// is remembered, a hardware encoder as the one to go back to.
    pub(crate) fn failed(&mut self, running: &Encoder, error: EncodeError) -> Move {
        let taken = after_failure(error.into(), self.rebuilt);
        match taken {
            Move::Rebuild => self.rebuilt = true,
            Move::Fall => {
                if running.path() != Path::Software && self.fallen.is_none() {
                    self.fallen = Some(running.clone());
                }
                self.rebuilt = false;
            }
        }
        taken
    }

    /// The stream is back on `fallen`, from `was`, which is now first among those after it.
    fn returned(&mut self, was: Encoder) {
        self.later.push_front(was);
        self.fallen = None;
        self.rebuilt = false;
    }

    /// Reopens `session`, going back first to the hardware encoder it fell from when the reopen makes a new stream
    /// anyway; if that falls again at this reopen the reopen moves on without it.
    pub(crate) fn reopen(&mut self, session: &mut Session) -> Result<Reopening, EncodeError> {
        let running = session.running().2.clone();
        let back = session.will_reopen().then(|| returning(&running, self.fallen.as_ref(), false).cloned()).flatten();
        let Some(back) = back else { return session.reopen() };
        if session.set_encoder(&back).is_ok() {
            match session.reopen() {
                Ok(made) => {
                    self.returned(running);
                    return Ok(made);
                }
                Err(why) if why.is_the_encoders() => session.withdraw_encoder(),
                Err(why) => return Err(why),
            }
        }
        session.reopen()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use media::{Device, Name, Runs, Validation};

    fn hardware(name: Name) -> Encoder {
        let device = Device { backend: media::Backend::Vulkan, node: None, vendor: None, validation: Validation::Off };
        Encoder { name, runs: Runs::Gpu(device), multiple: media::Multiple::EVEN }
    }

    fn software() -> Encoder {
        Encoder { name: Name::X264, runs: Runs::Cpu { convert: None }, multiple: media::Multiple::EVEN }
    }

    #[test]
    fn a_lost_device_is_rebuilt_once_and_then_the_encoder_falls() {
        assert_eq!(after_failure(Failing::Lost, false), Move::Rebuild);
        assert_eq!(after_failure(Failing::Lost, true), Move::Fall);
    }

    #[test]
    fn a_stall_and_any_other_failure_fall_at_once() {
        for rebuilt in [false, true] {
            assert_eq!(after_failure(Failing::Stalled, rebuilt), Move::Fall);
            assert_eq!(after_failure(Failing::Failed, rebuilt), Move::Fall);
        }
        assert_eq!(Failing::from(EncodeError::DeviceLost), Failing::Lost);
        assert_eq!(Failing::from(EncodeError::Stalled), Failing::Stalled);
        assert_eq!(Failing::from(EncodeError::Open), Failing::Failed);
        let vulkan = hardware(Name::H264Vulkan);
        let mut ladder = Ladder::new(VecDeque::from([software()]));
        assert_eq!(ladder.failed(&vulkan, EncodeError::Stalled), Move::Fall, "a stall is not rebuilt");
        assert_eq!(ladder.fallen, Some(vulkan), "and its encoder is gone back to at the next reopen");
    }

    #[test]
    fn a_fall_then_a_reopen_returns_to_the_hardware_encoder_and_a_second_fall_moves_on() {
        let (vulkan, x264) = (hardware(Name::H264Vulkan), software());
        let mut ladder = Ladder::new(VecDeque::from([x264.clone()]));
        assert_eq!(ladder.failed(&vulkan, EncodeError::Open), Move::Fall);
        // The share is on x264 now; the next reopen goes back first.
        assert_eq!(returning(&x264, ladder.fallen.as_ref(), false), Some(&vulkan));
        // It fell again at this reopen: the reopen moves on to x264.
        assert_eq!(returning(&x264, ladder.fallen.as_ref(), true), None);
        // Back on the hardware encoder, the software one is the first behind it.
        ladder.later.clear();
        ladder.returned(x264.clone());
        assert_eq!(ladder.fallen, None);
        assert_eq!(ladder.later, VecDeque::from([x264]));
    }

    #[test]
    fn a_software_encoder_is_never_the_one_gone_back_to_and_the_running_one_is_not_either() {
        let (vulkan, x264) = (hardware(Name::H264Vulkan), software());
        let mut ladder = Ladder::new(VecDeque::new());
        ladder.failed(&x264, EncodeError::Open);
        assert_eq!(ladder.fallen, None);
        assert_eq!(returning(&vulkan, Some(&vulkan), false), None, "already running");
    }

    #[test]
    fn the_first_hardware_encoder_fallen_from_is_the_one_kept() {
        let (vulkan, vaapi) = (hardware(Name::H264Vulkan), hardware(Name::H264Vaapi));
        let mut ladder = Ladder::new(VecDeque::from([vaapi.clone()]));
        ladder.failed(&vulkan, EncodeError::Open);
        ladder.failed(&vaapi, EncodeError::Open);
        assert_eq!(ladder.fallen, Some(vulkan));
    }

    #[test]
    fn a_rebuilt_device_that_is_lost_again_falls() {
        let vulkan = hardware(Name::H264Vulkan);
        let mut ladder = Ladder::new(VecDeque::new());
        assert_eq!(ladder.failed(&vulkan, EncodeError::DeviceLost), Move::Rebuild);
        assert_eq!(ladder.failed(&vulkan, EncodeError::DeviceLost), Move::Fall);
        assert_eq!(ladder.fallen, Some(vulkan));
    }
}
