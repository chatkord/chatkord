//! A second decoder tried on another path beside the one that paints: offered every unit the stream brings, from its
//! next key frame, on a thread of its own, so its time is its own; its pictures are judged by [`Behind`] and dropped.
//! One that keeps up hands its decoder over in step with the stream, so taking it costs no key frame.

use crate::watch::behind::Behind;
use crate::watch::fallback::Verdict;
use crate::watch::receive::QUEUED;
use media::{Coded, Decoder};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Instant;

/// A unit offered to the trial: its bytes, its time on the stream's clock, whether it is a key frame.
type Offered = (Coded, i64, bool);

/// The end of a trial the stream's units are offered to, kept with the queue of the decoder that paints, so a unit
/// reaches both or, once the feed is dropped, only that one.
pub(crate) struct Feed {
    units: SyncSender<Offered>,
    verdict: Arc<Mutex<Verdict>>,
    /// A key frame was offered: the trial's stream has begun.
    keyed: bool,
}

impl Feed {
    /// Offers a unit, never waiting; one the trial has no room for is one it did not keep up with.
    pub(crate) fn offer(&mut self, data: &Coded, pts: i64, key: bool) {
        self.keyed |= key;
        if self.units.try_send((data.clone(), pts, key)).is_err() {
            lag(&self.verdict);
        }
    }

    /// A unit of the stream was lost before it could be offered: what the trial decodes after it is broken.
    pub(crate) fn lost(&self) {
        lag(&self.verdict);
    }

    /// Whether the trial still waits for its first key frame.
    pub(crate) fn awaits_key(&self) -> bool {
        !self.keyed
    }
}

/// A trial running on its thread, which gives its decoder back once its [`Feed`] is dropped and it has decoded every
/// unit it was offered.
pub(crate) struct Trial {
    thread: JoinHandle<Decoder>,
    verdict: Arc<Mutex<Verdict>>,
}

impl Trial {
    /// Starts the trial of `decoder` on a thread of its own, decoding as a window that takes pictures where they are
    /// decoded would (`imports`); none when the thread does not start.
    pub(crate) fn start(decoder: Decoder, imports: bool) -> Option<(Self, Feed)> {
        let (units, inbox) = mpsc::sync_channel(QUEUED);
        let verdict = Arc::new(Mutex::new(Verdict::Unjudged));
        let judged = Arc::clone(&verdict);
        let thread = std::thread::Builder::new()
            .name("decode trial".to_string())
            .spawn(move || run(decoder, &inbox, &judged, imports))
            .ok()?;
        let feed = Feed { units, verdict: Arc::clone(&verdict), keyed: false };
        Some((Self { thread, verdict }, feed))
    }

    /// What its pictures say so far.
    pub(crate) fn verdict(&self) -> Verdict {
        *held(&self.verdict)
    }

    /// Its decoder, in step with every unit it was offered; its [`Feed`] must have been dropped first. None when its
    /// thread panicked.
    pub(crate) fn finish(self) -> Option<Decoder> {
        self.thread.join().ok()
    }
}

/// The value a lock holds, whether or not a thread panicked holding it.
fn held<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The trial lags, and stays so.
fn lag(verdict: &Mutex<Verdict>) {
    *held(verdict) = Verdict::Lags;
}

/// Decodes what it is offered from the first key frame until the feed is dropped, its pictures judged and dropped, and
/// gives its decoder back.
fn run(mut decoder: Decoder, inbox: &Receiver<Offered>, verdict: &Mutex<Verdict>, imports: bool) -> Decoder {
    let path = decoder.path();
    let (mut behind, mut begun) = (Behind::default(), false);
    while let Ok((data, pts, key)) = inbox.recv() {
        begun |= key;
        if !begun {
            continue;
        }
        let started = Instant::now();
        let pictures = if imports {
            decoder.decode_shown(&data, pts).map(|shown| shown.len())
        } else {
            decoder.decode(&data, pts).map(|decoded| decoded.len())
        };
        let judged = match pictures {
            Ok(_) if decoder.path() != path => Verdict::Lags,
            Ok(0) => continue,
            Ok(_) => {
                let late = behind.decoded(pts, started.elapsed());
                match (behind.judged(), late) {
                    (false, _) => Verdict::Unjudged,
                    (true, false) => Verdict::KeepsUp,
                    (true, true) => Verdict::Lags,
                }
            }
            Err(_) => Verdict::Lags,
        };
        let mut said = held(verdict);
        if *said != Verdict::Lags {
            *said = judged;
        }
    }
    decoder
}
