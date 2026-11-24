//! What each stage of a share's pipeline has counted, from the capture to libwebrtc: one worker writes, a look once a
//! second reads. Every count only grows, so a reader differences two looks; the two worst times are taken, and start
//! over, by the reader.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// The stages' counts, written by the share's worker and read from another thread.
#[derive(Default)]
pub struct Stages {
    delivered: AtomicU64,
    slot_dropped: AtomicU64,
    paced: AtomicU64,
    held: AtomicU64,
    key_frames: [AtomicU64; KeyCause::ALL.len()],
    asks_deferred: AtomicU64,
    asks_merged: AtomicU64,
    discarded: AtomicU64,
    encodes: AtomicU64,
    encode_micros: AtomicU64,
    convert_micros: AtomicU64,
    over_budget: AtomicU64,
    sent: AtomicU64,
    budget_micros: AtomicU64,
    slowest_encode_micros: AtomicU64,
    widest_capture_gap_micros: AtomicU64,
    longest_freeze_micros: AtomicU64,
}

/// Why a key frame was made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyCause {
    /// It is the first of the share.
    Started,
    /// A viewer, or the engine for one, asked for it.
    Asked,
    /// The encoder was reopened: a new size, step or rate, or another encoder.
    Reopened,
    /// Nothing the share knows of asked for it.
    Other,
}

impl KeyCause {
    const ALL: [KeyCause; 4] = [KeyCause::Started, KeyCause::Asked, KeyCause::Reopened, KeyCause::Other];

    /// Its place in [`KeyCause::ALL`], which lists the causes as they are declared.
    fn index(self) -> usize {
        self as usize
    }
}

/// The key frames made, by what asked for each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyFrames {
    /// The share's first.
    pub started: u64,
    /// Asked for by a viewer or the engine.
    pub asked: u64,
    /// Made by a reopened encoder.
    pub reopened: u64,
    /// Of no cause the share knows.
    pub other: u64,
}

/// The asks for a key frame the share did not answer with one of their own: none is made within a second of the last.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyAsks {
    /// Asks within a second of the last key frame, made at its end unless a key frame came before.
    pub deferred: u64,
    /// Asks a key frame made for another cause, or for an earlier ask, answered.
    pub merged: u64,
}

/// What the stages had counted at one look.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counted {
    /// Pictures the system made, those nobody took included.
    pub delivered: u64,
    /// Pictures the capture's slot replaced before the share took them.
    pub slot_dropped: u64,
    /// Pictures lost to the pace: left out for being over the chosen rate, and replaced by a newer.
    pub paced: u64,
    /// Pictures lost to the share's own rate: replaced by a newer while more bits than the rate allows were owed.
    pub held: u64,
    /// The key frames made, by cause.
    pub key_frames: KeyFrames,
    /// The asks for a key frame that made none at once.
    pub key_asks: KeyAsks,
    /// Pictures lost before the encoder otherwise: replaced while a new size settled, or left behind by a rebuilt
    /// device.
    pub discarded: u64,
    /// Pictures encoded.
    pub encodes: u64,
    /// The time all the encodes took, the encoder's own.
    pub encode_time: Duration,
    /// The time making the pictures the encoder's took, apart from their encodes: a screen's convert submitted and,
    /// for a software encoder, brought to memory.
    pub convert_time: Duration,
    /// Encodes that took longer than the budget a picture has at the rate the share runs.
    pub over_budget: u64,
    /// Encoded units handed to libwebrtc.
    pub sent: u64,
    /// What one picture may take at the rate the share runs: the interval between pictures.
    pub budget: Duration,
}

/// The worst times since they were last taken.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Worst {
    /// The slowest encode.
    pub slowest_encode: Duration,
    /// The widest gap between two pictures the system made.
    pub capture_gap: Duration,
    /// The longest freeze in the pictures the share took, by WebRTC's bound: a gap longer than three recent intervals
    /// and than one plus 150 ms, with frames lost or refused in it; zero for none.
    pub freeze: Duration,
}

impl Worst {
    /// The worse of each of `self` and `other`.
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        Self {
            slowest_encode: self.slowest_encode.max(other.slowest_encode),
            capture_gap: self.capture_gap.max(other.capture_gap),
            freeze: self.freeze.max(other.freeze),
        }
    }
}

fn micros(time: Duration) -> u64 {
    u64::try_from(time.as_micros()).unwrap_or(u64::MAX)
}

impl Stages {
    /// What the capture has made and what its slot dropped, in all, from the capture's own counts.
    pub(crate) fn slot(&self, delivered: u64, dropped: u64) {
        self.delivered.store(delivered, Ordering::Relaxed);
        self.slot_dropped.store(dropped, Ordering::Relaxed);
    }

    /// The widest gap the capture saw between two pictures since it was last read.
    pub(crate) fn capture_gap(&self, gap: Duration) {
        self.widest_capture_gap_micros.fetch_max(micros(gap), Ordering::Relaxed);
    }

    /// The pictures the share took froze for `gap`.
    pub(crate) fn froze(&self, gap: Duration) {
        self.longest_freeze_micros.fetch_max(micros(gap), Ordering::Relaxed);
    }

    /// A picture was lost to the pace.
    pub(crate) fn paced(&self) {
        self.paced.fetch_add(1, Ordering::Relaxed);
    }

    /// A picture was lost to the share's own rate.
    pub(crate) fn held(&self) {
        self.held.fetch_add(1, Ordering::Relaxed);
    }

    /// A key frame was made, for `cause`.
    pub(crate) fn key_frame(&self, cause: KeyCause) {
        self.key_frames[cause.index()].fetch_add(1, Ordering::Relaxed);
    }

    /// An ask for a key frame came within a second of the last and waits for its time.
    pub(crate) fn ask_deferred(&self) {
        self.asks_deferred.fetch_add(1, Ordering::Relaxed);
    }

    /// An ask for a key frame was answered by one made for another.
    pub(crate) fn ask_merged(&self) {
        self.asks_merged.fetch_add(1, Ordering::Relaxed);
    }

    /// `pictures` were lost before the encoder, other than to the pace.
    pub(crate) fn discarded(&self, pictures: u64) {
        self.discarded.fetch_add(pictures, Ordering::Relaxed);
    }

    /// What a picture may take at the rate the share runs now.
    pub(crate) fn budget(&self, budget: Duration) {
        self.budget_micros.store(micros(budget), Ordering::Relaxed);
    }

    /// A picture took `took` to encode, which is over the budget when it is longer than what the share's rate allows.
    /// The time is the encoder's own, its convert apart ([`Stages::converted`]).
    pub(crate) fn encoded(&self, took: Duration) {
        let took = micros(took);
        self.encodes.fetch_add(1, Ordering::Relaxed);
        self.encode_micros.fetch_add(took, Ordering::Relaxed);
        let budget = self.budget_micros.load(Ordering::Relaxed);
        if budget > 0 && took > budget {
            self.over_budget.fetch_add(1, Ordering::Relaxed);
        }
        self.slowest_encode_micros.fetch_max(took, Ordering::Relaxed);
    }

    /// Making a picture the encoder's took `took`, apart from its encode.
    pub(crate) fn converted(&self, took: Duration) {
        self.convert_micros.fetch_add(micros(took), Ordering::Relaxed);
    }

    /// `units` encoded units were handed to libwebrtc.
    pub(crate) fn sent(&self, units: u64) {
        self.sent.fetch_add(units, Ordering::Relaxed);
    }

    /// What the stages have counted so far.
    pub fn counted(&self) -> Counted {
        let load = |count: &AtomicU64| count.load(Ordering::Relaxed);
        Counted {
            delivered: load(&self.delivered),
            slot_dropped: load(&self.slot_dropped),
            paced: load(&self.paced),
            held: load(&self.held),
            key_frames: KeyFrames {
                started: load(&self.key_frames[KeyCause::Started.index()]),
                asked: load(&self.key_frames[KeyCause::Asked.index()]),
                reopened: load(&self.key_frames[KeyCause::Reopened.index()]),
                other: load(&self.key_frames[KeyCause::Other.index()]),
            },
            key_asks: KeyAsks { deferred: load(&self.asks_deferred), merged: load(&self.asks_merged) },
            discarded: load(&self.discarded),
            encodes: load(&self.encodes),
            encode_time: Duration::from_micros(load(&self.encode_micros)),
            convert_time: Duration::from_micros(load(&self.convert_micros)),
            over_budget: load(&self.over_budget),
            sent: load(&self.sent),
            budget: Duration::from_micros(load(&self.budget_micros)),
        }
    }

    /// The worst times since they were last taken, which starts them over.
    pub fn take_worst(&self) -> Worst {
        Worst {
            slowest_encode: Duration::from_micros(self.slowest_encode_micros.swap(0, Ordering::Relaxed)),
            capture_gap: Duration::from_micros(self.widest_capture_gap_micros.swap(0, Ordering::Relaxed)),
            freeze: Duration::from_micros(self.longest_freeze_micros.swap(0, Ordering::Relaxed)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn a_stage_that_has_done_nothing_has_counted_nothing() {
        assert_eq!(Stages::default().counted(), Counted::default());
        assert_eq!(Stages::default().take_worst(), Worst::default());
    }

    #[test]
    fn the_slot_s_counts_are_the_captures_own() {
        let stages = Stages::default();
        stages.slot(120, 7);
        let counted = stages.counted();
        assert_eq!((counted.delivered, counted.slot_dropped), (120, 7));
        stages.slot(180, 9);
        assert_eq!((stages.counted().delivered, stages.counted().slot_dropped), (180, 9), "they only grow");
    }

    #[test]
    fn a_picture_lost_to_the_pace_and_one_lost_before_the_encoder_are_counted_apart() {
        let stages = Stages::default();
        stages.paced();
        stages.paced();
        stages.discarded(3);
        let counted = stages.counted();
        assert_eq!((counted.paced, counted.discarded, counted.slot_dropped), (2, 3, 0));
    }

    #[test]
    fn a_picture_held_by_the_rate_and_a_key_frame_by_its_cause_are_counted_apart() {
        let stages = Stages::default();
        stages.held();
        stages.held();
        stages.paced();
        stages.key_frame(KeyCause::Started);
        stages.key_frame(KeyCause::Asked);
        stages.key_frame(KeyCause::Asked);
        stages.key_frame(KeyCause::Reopened);
        let counted = stages.counted();
        assert_eq!((counted.held, counted.paced), (2, 1));
        assert_eq!(counted.key_frames, KeyFrames { started: 1, asked: 2, reopened: 1, other: 0 });
        stages.ask_deferred();
        stages.ask_merged();
        stages.ask_merged();
        assert_eq!(stages.counted().key_asks, KeyAsks { deferred: 1, merged: 2 });
    }

    #[test]
    fn an_encode_adds_its_time_and_counts_over_the_budget_only_when_it_took_longer() {
        let stages = Stages::default();
        stages.budget(16 * MS);
        stages.encoded(5 * MS);
        stages.encoded(16 * MS);
        stages.encoded(20 * MS);
        let counted = stages.counted();
        assert_eq!((counted.encodes, counted.encode_time, counted.over_budget), (3, 41 * MS, 1));
        assert_eq!(counted.budget, 16 * MS);
    }

    #[test]
    fn a_convert_adds_its_time_apart_and_never_counts_against_the_budget() {
        let stages = Stages::default();
        stages.budget(16 * MS);
        stages.converted(30 * MS);
        stages.encoded(5 * MS);
        let counted = stages.counted();
        assert_eq!((counted.convert_time, counted.encode_time, counted.over_budget), (30 * MS, 5 * MS, 0));
        assert_eq!(stages.take_worst().slowest_encode, 5 * MS, "the slowest encode is the encoder's own");
    }

    #[test]
    fn the_slowest_encode_the_widest_gap_and_the_longest_freeze_are_the_worst_since_taken_and_start_over() {
        let stages = Stages::default();
        stages.encoded(4 * MS);
        stages.encoded(9 * MS);
        stages.encoded(2 * MS);
        stages.capture_gap(30 * MS);
        stages.capture_gap(12 * MS);
        stages.froze(200 * MS);
        stages.froze(160 * MS);
        assert_eq!(stages.take_worst(), Worst { slowest_encode: 9 * MS, capture_gap: 30 * MS, freeze: 200 * MS });
        assert_eq!(stages.take_worst(), Worst::default(), "taken once");
        stages.encoded(3 * MS);
        assert_eq!(stages.take_worst().slowest_encode, 3 * MS);
    }

    #[test]
    fn units_handed_to_libwebrtc_are_counted_sent() {
        let stages = Stages::default();
        stages.sent(1);
        stages.sent(2);
        assert_eq!(stages.counted().sent, 3);
    }

    #[test]
    fn the_worse_of_two_is_each_time_s_larger() {
        let one = Worst { slowest_encode: 3 * MS, capture_gap: 40 * MS, freeze: 200 * MS };
        let other = Worst { slowest_encode: 9 * MS, capture_gap: 10 * MS, freeze: Duration::ZERO };
        assert_eq!(one.max(other), Worst { slowest_encode: 9 * MS, capture_gap: 40 * MS, freeze: 200 * MS });
    }
}
