//! What the chain's thread and its handles share: the asks, the answers and the counts.

use super::devices::Sound;
use super::mixer::Track;
use super::report::{Sounding, Standing, Turn};
use super::{IDLE, Mono, RATE_HZ, TEST_LATE};
use crate::keeper::Stage;
use crate::processing::Processing;
use libwebrtc::audio_source::AudioSourceOptions;
use libwebrtc::audio_source::native::NativeAudioSource;
use libwebrtc::native::audio_mixer::AudioMixer;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;
use tokio::sync::watch;

/// What the thread counts as it goes, read without waiting on it.
#[derive(Default)]
pub(crate) struct Counts {
    pub(super) turns: AtomicU64,
    pub(super) captured: AtomicU64,
    pub(super) played: AtomicU64,
    pub(super) received: AtomicU64,
    pub(super) restarts: AtomicU32,
    pub(crate) microphone_dropped: AtomicU64,
    pub(crate) speaker_dropped: AtomicU64,
    pub(crate) speaker_underruns: AtomicU64,
    /// How long a frame handed to the speaker takes to be heard, in milliseconds, as the last turn read it.
    pub(crate) speaker_delay_ms: AtomicU32,
    /// The most of it any turn read since the health was last read.
    pub(crate) speaker_delay_most_ms: AtomicU32,
}

/// How a thread of the chain sleeps between its turns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cadence {
    /// Nothing is asked of it and nothing of a device is open: it sleeps until an ask or a stop wakes it.
    Parked,
    /// A device is asked or open: it turns at least every [`IDLE`], so the device's keeping runs even in silence.
    Turning,
}

impl Cadence {
    /// Sleeps the calling thread as this says, or less when it is woken.
    pub(crate) fn sleep(self) {
        match self {
            Self::Parked => thread::park(),
            Self::Turning => thread::park_timeout(IDLE),
        }
    }
}

/// One of the chain's two devices.
#[derive(Clone, Copy)]
pub(crate) enum Which {
    Microphone,
    Speaker,
}

impl Which {
    pub(crate) fn asked(self, state: &State) -> &Asked {
        match self {
            Self::Microphone => &state.microphone,
            Self::Speaker => &state.speaker,
        }
    }

    pub(super) fn asked_mut(self, state: &mut State) -> &mut Asked {
        match self {
            Self::Microphone => &mut state.microphone,
            Self::Speaker => &mut state.speaker,
        }
    }

    pub(super) fn standing(self, sounding: &mut Sounding) -> &mut Standing {
        match self {
            Self::Microphone => &mut sounding.microphone,
            Self::Speaker => &mut sounding.speaker,
        }
    }

    pub(crate) fn flowing(self, shared: &Shared) -> &AtomicBool {
        match self {
            Self::Microphone => &shared.capturing,
            Self::Speaker => &shared.playing,
        }
    }

    pub(crate) fn stage(self, shared: &Shared) -> &Stage {
        match self {
            Self::Microphone => &shared.microphone,
            Self::Speaker => &shared.speaker,
        }
    }

    /// The name of the thread that keeps this device.
    pub(crate) fn keeper(self) -> &'static str {
        match self {
            Self::Microphone => "chatkord-microphone-keeper",
            Self::Speaker => "chatkord-speaker-keeper",
        }
    }
}

/// What the chain's thread and its owners share.
pub(crate) struct Shared {
    pub(crate) sound: Arc<dyn Sound>,
    pub(crate) state: Mutex<State>,
    pub(crate) stop: AtomicBool,
    /// The mix thread is gone, so the keepers have nothing left to keep a device for.
    pub(crate) halted: AtomicBool,
    /// Counts the times the mix thread dropped its ends after a panic: a keeper that sees it change opens its device
    /// anew, since the end it made is gone.
    pub(crate) epoch: AtomicU32,
    /// The [`Turn`] the mix thread is in, by its code.
    pub(super) doing: AtomicU8,
    /// What each keeper is doing.
    pub(crate) microphone: Stage,
    pub(crate) speaker: Stage,
    /// How long a keeper may be in one call before the warden gives up on it for a fresh one.
    pub(crate) give_up_after: Duration,
    /// What became of each device; written only with `state` held, so it never contradicts what is asked.
    pub(super) sounding: watch::Sender<Sounding>,
    /// Whether the microphone its keeper holds is open, as it last looked.
    pub(super) capturing: AtomicBool,
    /// Whether the speaker its keeper holds is open, as it last looked.
    pub(super) playing: AtomicBool,
    /// The loudest processed microphone sample since the level was last read.
    pub(crate) level: AtomicI32,
    /// The loudest frame's loudness before the gate since it was last read, in dB below full scale.
    pub(crate) loudness: Mutex<Option<f32>>,
    /// Processing asked for and not yet taken by the mix thread.
    pub(super) asked: Mutex<Option<Processing>>,
    pub(super) source: NativeAudioSource,
    pub(crate) counts: Counts,
}

impl Shared {
    /// The state of a chain nobody holds yet and no thread runs.
    pub(super) fn new(sound: Arc<dyn Sound>, give_up_after: Duration) -> Self {
        Self {
            sound,
            state: Mutex::new(State { mixer: Some(AudioMixer::new()), ..State::default() }),
            stop: AtomicBool::new(false),
            halted: AtomicBool::new(false),
            epoch: AtomicU32::new(0),
            doing: AtomicU8::new(Turn::Idle.code()),
            microphone: Stage::default(),
            speaker: Stage::default(),
            give_up_after,
            sounding: watch::Sender::new(Sounding::default()),
            capturing: AtomicBool::new(false),
            playing: AtomicBool::new(false),
            level: AtomicI32::new(0),
            loudness: Mutex::new(None),
            asked: Mutex::new(None),
            source: NativeAudioSource::new(AudioSourceOptions::default(), RATE_HZ, 1, 0),
            counts: Counts::default(),
        }
    }

    /// How loud the processed microphone was since this was last asked, from 0 to 100.
    pub(super) fn take_level(&self) -> Option<u8> {
        let peak = self.level.swap(0, Ordering::AcqRel);
        u8::try_from(peak * 100 / i32::from(i16::MAX)).ok()
    }

    pub(super) fn doing(&self, turn: Turn) {
        self.doing.store(turn.code(), Ordering::Relaxed);
    }

    /// Says what the thread made of `which` as asked at `asks`, unless what is asked has moved on since: then the
    /// one who asked has already said it is opening, and the next turn answers that.
    pub(crate) fn tell(&self, which: Which, asks: u64, standing: Standing) {
        let state = lock(&self.state);
        if state.ended {
            return;
        }
        let asked = which.asked(&state);
        let current = match standing {
            Standing::Closed => asked.holds == 0,
            Standing::Opening | Standing::Open | Standing::Refused(_) | Standing::Unanswered | Standing::Hung => {
                asked.holds > 0 && asked.asks == asks
            }
        };
        if current {
            self.sounding.send_if_modified(|sounding| {
                let was = which.standing(sounding);
                // A fresh keeper's try of the same ask is no answer from the device.
                if standing == Standing::Opening && matches!(*was, Standing::Unanswered | Standing::Hung) {
                    return false;
                }
                std::mem::replace(was, standing) != standing
            });
        }
    }
}

/// What is asked of one device: its keeper opens, moves and closes its device to match.
#[derive(Default)]
pub(crate) struct Asked {
    pub(crate) holds: usize,
    pub(crate) device: Option<String>,
    /// Counts each ask the keeper must answer by trying the host: a first hold, a choice.
    pub(crate) asks: u64,
}

#[derive(Default)]
pub(crate) struct State {
    pub(super) microphone: Asked,
    pub(super) speaker: Asked,
    pub(super) tracks: Vec<Track>,
    pub(super) mixer: Option<AudioMixer>,
    pub(super) next_id: i32,
    pub(super) testing: Testing,
    /// The mix thread is gone, so nothing asked from now on is ever answered.
    pub(crate) ended: bool,
}

/// The microphone tests held. While there is one, the processed frames wait here to be played back and the call's
/// mix is not played.
#[derive(Default)]
pub(super) struct Testing {
    pub(super) holds: usize,
    pub(super) late: VecDeque<Mono>,
}

impl Testing {
    /// Keeps `frame` and hands back the one now due to be played, if a test is held.
    pub(super) fn late(&mut self, frame: Mono) -> Option<Mono> {
        if self.holds == 0 {
            return None;
        }
        self.late.push_back(frame);
        if self.late.len() > TEST_LATE { self.late.pop_front() } else { None }
    }
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
