//! A watched screen's health as one note every ten looks: what its decoder did since the line before. Pure: the
//! looks come in, a note goes out, and nothing here reads a clock or names a person.

use crate::lines::{Coded, LOOKS_A_LINE};
use report::{count16, millis};
use std::time::Duration;

/// What a watched screen's decoder said at one look, the counts over the watch so far.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct WatchedLook {
    /// How the picture is coded and decoded.
    pub(crate) coded: Coded,
    /// The pictures a second decoded over the last look.
    pub(crate) frame_rate: Option<u32>,
    /// Key frames this viewer asked its sender for.
    pub(crate) key_frames_asked: u64,
    /// Frames the decoder refused.
    pub(crate) failed: u64,
    /// Frames that never reached the decoder.
    pub(super) dropped: u64,
    /// How often the picture froze, and the longest.
    pub(crate) freezes: (u64, std::time::Duration),
    /// The longest the window waited between two pictures it took to paint, since the look before.
    pub(crate) painted_gap: Duration,
    /// Seconds the engine held pictures for jitter and sound, summed, and over how many pictures, as it reports.
    pub(crate) held: (f64, u64),
}

/// The looks of one watched screen since its last line, numbered for this call rather than named.
#[derive(Debug, Default)]
pub(crate) struct WatchedLog {
    looks: u32,
    at_last_line: WatchedLook,
    /// The longest painted gap of the looks since the last line.
    longest_gap: Duration,
}

/// One line about a watched screen.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WatchedLine {
    number: u64,
    now: WatchedLook,
    since: WatchedLook,
}

impl WatchedLog {
    /// Takes in a look at the screen this call watched `number`th; every [`LOOKS_A_LINE`] looks, the line.
    pub(crate) fn look(&mut self, number: u64, now: WatchedLook) -> Option<WatchedLine> {
        self.looks += 1;
        self.longest_gap = self.longest_gap.max(now.painted_gap);
        if self.looks < LOOKS_A_LINE {
            return None;
        }
        self.looks = 0;
        let before = &self.at_last_line;
        let since = WatchedLook {
            key_frames_asked: now.key_frames_asked.saturating_sub(before.key_frames_asked),
            failed: now.failed.saturating_sub(before.failed),
            dropped: now.dropped.saturating_sub(before.dropped),
            freezes: (now.freezes.0.saturating_sub(before.freezes.0), now.freezes.1),
            painted_gap: std::mem::take(&mut self.longest_gap),
            held: (now.held.0 - before.held.0, now.held.1.saturating_sub(before.held.1)),
            ..WatchedLook::default()
        };
        self.at_last_line = now.clone();
        Some(WatchedLine { number, now, since })
    }
}

impl WatchedLine {
    /// Writes the line to the log.
    pub(crate) fn log(&self) {
        let (now, since) = (&self.now, &self.since);
        report::note!(
            info,
            "a watched screen",
            number = count16(self.number),
            codec = now.coded.codec,
            route = now.coded.route,
            frame_rate = now.frame_rate.map(|rate| count16(rate.into())),
            over_s = LOOKS_A_LINE,
            key_frames_asked = count16(since.key_frames_asked),
            refused = count16(since.failed),
            dropped = count16(since.dropped),
            freezes = count16(since.freezes.0),
            freeze_ms = count16(millis(now.freezes.1)),
            painted_gap_ms = count16(millis(since.painted_gap)),
            held_ms = webrtc::WatchStats::mean_held(since.held.0, since.held.1).map(|held| count16(millis(held))),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use choices::Codec;
    use screens::Route;

    #[test]
    fn a_watched_screens_line_is_numbered_and_says_its_losses_since_the_last() {
        let mut log = WatchedLog::default();
        let look = |asked: u64, freezes: u64| WatchedLook {
            coded: Coded::of(Codec::Av1, &Route::Software),
            frame_rate: Some(30),
            key_frames_asked: asked,
            failed: 1,
            dropped: 4,
            freezes: (freezes, std::time::Duration::from_millis(900)),
            painted_gap: Duration::from_millis(30),
            held: (0.0, 0),
        };
        for _ in 1..LOOKS_A_LINE {
            assert_eq!(log.look(2, look(1, 1)), None);
        }
        let first = log.look(2, look(1, 1)).expect("a line");
        assert_eq!((first.number, first.now.coded.clone()), (2, Coded::of(Codec::Av1, &Route::Software)));
        assert_eq!((first.now.frame_rate, first.since.key_frames_asked), (Some(30), 1));
        assert_eq!((first.since.failed, first.since.dropped, first.since.freezes.0), (1, 4, 1));
        assert_eq!(first.now.freezes.1, std::time::Duration::from_millis(900));
        for _ in 1..LOOKS_A_LINE {
            log.look(2, look(3, 1));
        }
        let later = log.look(2, look(3, 1)).expect("a line");
        let counted = (later.since.key_frames_asked, later.since.failed, later.since.dropped, later.since.freezes.0);
        assert_eq!(counted, (2, 0, 0, 0));
    }

    #[test]
    fn a_watched_screens_line_says_the_longest_painted_gap_of_its_looks_and_starts_each_line_over() {
        let mut log = WatchedLog::default();
        let gap = |millis: u64| WatchedLook { painted_gap: Duration::from_millis(millis), ..WatchedLook::default() };
        assert_eq!(log.look(1, gap(250)), None);
        for _ in 2..LOOKS_A_LINE {
            assert_eq!(log.look(1, gap(20)), None);
        }
        let first = log.look(1, gap(40)).expect("the tenth look");
        assert_eq!(first.since.painted_gap, Duration::from_millis(250), "the largest of the ten, not the last");
        for _ in 1..LOOKS_A_LINE {
            log.look(1, gap(20));
        }
        let second = log.look(1, gap(30)).expect("the twentieth look");
        assert_eq!(second.since.painted_gap, Duration::from_millis(30), "the line before is not said again");
    }

    #[test]
    fn a_watched_screens_line_is_written_whole_and_fits_the_reports_line_whatever_the_counts() {
        use tracing_subscriber::layer::SubscriberExt as _;
        let most = WatchedLook {
            coded: Coded::of(Codec::Av1, &Route::UntestedHardware { via: String::new() }),
            frame_rate: Some(u32::MAX),
            key_frames_asked: u64::MAX,
            failed: u64::MAX,
            dropped: u64::MAX,
            freezes: (u64::MAX, Duration::MAX),
            painted_gap: Duration::MAX,
            held: (f64::MAX, u64::MAX),
        };
        let line = WatchedLine { number: u64::MAX, now: most.clone(), since: most };
        let ring = report::Ring::new(std::time::SystemTime::now);
        tracing::subscriber::with_default(tracing_subscriber::registry().with(ring.layer()), || line.log());
        let lines = ring.lines();
        let [said] = &lines[..] else { panic!("one line, {} refused as too long: {lines:?}", ring.recent().dropped()) };
        assert!(said.as_str().len() <= report::MAX_LINE, "{} bytes: {said}", said.as_str().len());
        assert!(said.as_str().contains("painted_gap_ms=65535"), "{said}");
    }
}
