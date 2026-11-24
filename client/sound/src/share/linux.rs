//! A share's sound on Linux: PipeWire's mix of the chosen applications' outputs (`pw::Mix`), linked into a capture
//! stream of this client's own; never a sink's monitor, so the call is never heard in it.

use super::{CHANNELS, RATE, SoundError};
use crate::voice::Health;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

pub use pw::{App, Hears};

// The mix links each application's ports to a left and a right input.
const _: () = assert!(CHANNELS == 2, "pw's mix is stereo");

/// Whether the tree of any of `pids` holds this process. Linux's owner of a window lists the processes of its own
/// application, so this process is held when it is one of them.
pub fn holds_us(pids: &[u32]) -> bool {
    pids.contains(&std::process::id())
}

/// The mix being heard. Dropping it unlinks every application.
pub(super) struct Stream(pw::Mix);

impl Stream {
    /// Hears what `hears` names, each quantum handed to `hear` on PipeWire's thread.
    pub(super) fn open(hears: &Hears, hear: impl FnMut(&[u8]) + Send + 'static) -> Result<Self, SoundError> {
        pw::Mix::open(hears.clone(), RATE, hear).map(Self).map_err(|why| match why {
            pw::Error::Unavailable(_) => SoundError::Unavailable,
            pw::Error::Refused(_) | pw::Error::Pod => SoundError::Stream,
        })
    }

    /// Paused is a mix no application is linked to yet: waiting, not failed. A server that went ends the stream.
    pub(super) fn health(&self) -> Health {
        match self.0.state() {
            pw::State::Streaming => Health::Running,
            pw::State::Connecting | pw::State::Paused => Health::Starting,
            pw::State::Error | pw::State::Unconnected => Health::Stopped,
        }
    }

    /// How long the applications' sound took through the graph to reach the stream, by PipeWire's own clock.
    pub(super) fn delay(&self) -> Duration {
        self.0.timing(RATE).map_or(Duration::ZERO, |timing| timing.delay)
    }
}

/// How long the sound server is listened to for what plays: the graph arrives in pieces, with no word that it is whole.
const LISTEN: Duration = Duration::from_secs(1);

/// A listen of the sound server for the applications playing, whose sound a share can carry: begun by an ask and
/// read once it has heard the graph for [`LISTEN`], so the owner thread answers its other asks meanwhile.
pub(crate) struct Listen {
    _watch: pw::Apps,
    lists: Receiver<Vec<App>>,
    until: Instant,
}

impl Listen {
    /// Starts listening at `now`.
    pub(crate) fn start(now: Instant) -> Result<Self, SoundError> {
        let (tell, lists) = mpsc::channel();
        let watch = pw::Apps::watch(move |apps| {
            let _ = tell.send(apps);
        })
        .map_err(|_| SoundError::Unavailable)?;
        Ok(Self { _watch: watch, lists, until: now + LISTEN })
    }

    /// When it has heard enough to be read.
    pub(crate) fn until(&self) -> Instant {
        self.until
    }

    /// The applications heard, in name order.
    pub(crate) fn heard(self) -> Vec<App> {
        self.lists.try_iter().last().unwrap_or_default()
    }
}
