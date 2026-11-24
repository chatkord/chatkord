//! The voice chain's health as one note every ten seconds, whoever looks. Pure: the looks come in, a note goes
//! out, and nothing here reads a clock or names a person.

use crate::lines::LOOKS_A_LINE;
use std::time::Duration;
use tokio::time::Instant;
use webrtc::{ChainHealth, Keeper, Keeping, Sounding, Standing, Turn};

/// How long a voice line covers: as many seconds as the other lines' looks.
const LINE_EVERY: Duration = screens::LOOK_EVERY.saturating_mul(LOOKS_A_LINE);

pub(crate) use webrtc::chain::STILL_AFTER;

/// Whether a thread of the chain turns, as the looks so far saw it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Turning {
    /// Nothing is asked of it, or it has not been looked at.
    #[default]
    Unwatched,
    /// It had made `turns` turns at `since`, and no look since saw more.
    At { turns: u64, since: Instant },
    /// It stood still at `turns` for [`STILL_AFTER`], which was said.
    Still { turns: u64 },
}

/// The looks at one engine's voice chain, by whoever looks: a line is due by the time given, never by how many
/// looked, so two lookers in one second count once.
#[derive(Debug, Default)]
pub(crate) struct VoiceLog {
    /// When the last line was written, or the first look made, and the counts then.
    at_last_line: Option<(Instant, ChainHealth)>,
    /// The most the speaker's delay read at any look since the last line, in milliseconds.
    delay_most_ms: u32,
    turning: Turning,
    /// The microphone's keeper and the speaker's.
    keepers: [Turning; 2],
}

/// One line about the voice chain: what it did over the time the line covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VoiceLine {
    over: Duration,
    turns: u64,
    captured: u64,
    played: u64,
    received: u64,
    tracks: usize,
    speaker_holds: usize,
    microphone_dropped: u64,
    speaker_dropped: u64,
    speaker_underruns: u64,
    tests: usize,
    restarts: u32,
    /// How long a frame handed to the speaker takes to be heard, at the look that made the line.
    delay_ms: u32,
    /// The most that was, at any look since the last line.
    delay_most_ms: u32,
}

impl VoiceLog {
    /// Takes in a look at `now`; once [`LINE_EVERY`] has passed since the last line, the line about that time. The
    /// first look only starts the time. A device's own counts start over with a new device: a count that fell is
    /// read as nothing lost.
    pub(crate) fn look(&mut self, health: ChainHealth, now: Instant) -> Option<VoiceLine> {
        let Some((then, before)) = self.at_last_line else {
            self.at_last_line = Some((now, health));
            return None;
        };
        self.delay_most_ms = self.delay_most_ms.max(health.speaker_delay_most_ms).max(health.speaker_delay_ms);
        let over = now.saturating_duration_since(then);
        if over < LINE_EVERY {
            return None;
        }
        self.at_last_line = Some((now, health));
        Some(VoiceLine {
            over,
            turns: health.turns.saturating_sub(before.turns),
            captured: health.captured.saturating_sub(before.captured),
            played: health.played.saturating_sub(before.played),
            received: health.received.saturating_sub(before.received),
            tracks: health.tracks,
            speaker_holds: health.speaker_holds,
            microphone_dropped: health.microphone_dropped.saturating_sub(before.microphone_dropped),
            speaker_dropped: health.speaker_dropped.saturating_sub(before.speaker_dropped),
            speaker_underruns: health.speaker_underruns.saturating_sub(before.speaker_underruns),
            tests: health.tests,
            restarts: health.restarts.saturating_sub(before.restarts),
            delay_ms: health.speaker_delay_ms,
            delay_most_ms: std::mem::take(&mut self.delay_most_ms),
        })
    }

    /// What the chain's mix thread is held in, once, when a look at `now` finds it has made no turn for
    /// [`STILL_AFTER`] while a device is asked of it (`sounding`); said again only after it has turned.
    pub(crate) fn stalled(&mut self, health: ChainHealth, sounding: Sounding, now: Instant) -> Option<Turn> {
        let asked = [sounding.microphone, sounding.speaker].iter().any(is_asked);
        still(&mut self.turning, asked && health.alive, health.turns, health.doing, now)
    }

    /// What the microphone's keeper is held in, once, when a look at `now` finds it has finished no look for
    /// [`STILL_AFTER`] while the microphone is asked (`sounding`); said again only after it has looked.
    pub(crate) fn microphone_stalled(
        &mut self,
        health: ChainHealth,
        sounding: Sounding,
        now: Instant,
    ) -> Option<Keeping> {
        self.keeper_stalled(0, health.microphone, is_asked(&sounding.microphone) && health.alive, now)
    }

    /// What the speaker's keeper is held in, once, as [`VoiceLog::microphone_stalled`] says of the microphone's.
    pub(crate) fn speaker_stalled(&mut self, health: ChainHealth, sounding: Sounding, now: Instant) -> Option<Keeping> {
        self.keeper_stalled(1, health.speaker, is_asked(&sounding.speaker) && health.alive, now)
    }

    fn keeper_stalled(&mut self, which: usize, keeper: Keeper, asked: bool, now: Instant) -> Option<Keeping> {
        still(&mut self.keepers[which], asked, keeper.turns, keeper.doing, now)
    }
}

fn is_asked(standing: &Standing) -> bool {
    matches!(standing, Standing::Opening | Standing::Open | Standing::Unanswered | Standing::Hung)
}

/// What `doing` is, once, when a thread that `asked` shows no more than `turns` at `now` for [`STILL_AFTER`] since
/// `turning` first saw them; the thread is watched only while something is asked of it.
fn still<T: Copy>(turning: &mut Turning, asked: bool, turns: u64, doing: T, now: Instant) -> Option<T> {
    if !asked {
        *turning = Turning::Unwatched;
        return None;
    }
    let (next, told) = match *turning {
        Turning::At { turns: seen, since } if seen == turns => {
            if now.saturating_duration_since(since) >= STILL_AFTER {
                (Turning::Still { turns }, Some(doing))
            } else {
                (*turning, None)
            }
        }
        Turning::Still { turns: seen } if seen == turns => (*turning, None),
        Turning::Unwatched | Turning::At { .. } | Turning::Still { .. } => (Turning::At { turns, since: now }, None),
    };
    *turning = next;
    told
}

impl VoiceLine {
    /// Writes the line to the log.
    pub(crate) fn log(&self) {
        report::note!(
            info,
            "the voice chain",
            over_s = self.over.as_secs(),
            turns = self.turns,
            captured = self.captured,
            played = self.played,
            received = self.received,
            tracks = self.tracks,
            speaker_holds = self.speaker_holds,
            microphone_dropped = self.microphone_dropped,
            speaker_dropped = self.speaker_dropped,
            ran_out = self.speaker_underruns,
            tests = self.tests,
            restarts = self.restarts,
            delay_ms = self.delay_ms,
            delay_most_ms = self.delay_most_ms,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn health(turns: u64) -> ChainHealth {
        ChainHealth {
            turns,
            captured: turns.saturating_sub(2),
            played: turns,
            received: turns,
            tracks: 1,
            speaker_holds: 1,
            tests: 1,
            restarts: 0,
            doing: Turn::Idle,
            microphone: Keeper { turns, doing: Keeping::Idle },
            speaker: Keeper { turns, doing: Keeping::Idle },
            microphone_dropped: 0,
            speaker_dropped: 0,
            speaker_underruns: 0,
            speaker_delay_ms: 0,
            speaker_delay_most_ms: 0,
            alive: true,
        }
    }

    fn open() -> Sounding {
        Sounding { microphone: Standing::Open, speaker: Standing::Closed }
    }

    #[tokio::test(start_paused = true)]
    async fn a_voice_line_says_the_speakers_delay() {
        let start = Instant::now();
        let mut log = VoiceLog::default();
        assert_eq!(log.look(health(2), start), None);
        let spike = ChainHealth { speaker_delay_ms: 30, speaker_delay_most_ms: 90, ..health(500) };
        assert_eq!(log.look(spike, start + Duration::from_secs(5)), None);
        let line = log
            .look(
                ChainHealth { speaker_delay_ms: 25, speaker_delay_most_ms: 40, ..health(1002) },
                start + Duration::from_secs(10),
            )
            .expect("a line");
        assert_eq!(
            (line.delay_ms, line.delay_most_ms),
            (25, 90),
            "the latest, and the most of any look since the last line"
        );
        let next = log.look(health(2002), start + Duration::from_secs(20)).expect("the next line");
        assert_eq!(next.delay_most_ms, 0, "each line starts its most over");
    }

    #[tokio::test(start_paused = true)]
    async fn a_voice_line_comes_every_ten_seconds_with_what_happened_since_the_last() {
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        let mut log = VoiceLog::default();
        for second in 0..10 {
            assert_eq!(log.look(health(2 + second * 100), at(second)), None, "second {second}");
        }
        let first = log.look(health(1002), at(10)).expect("ten seconds after the first look");
        assert_eq!(
            (first.over, first.turns, first.captured, first.played, first.received),
            (Duration::from_secs(10), 1000, 1000, 1000, 1000)
        );
        assert_eq!((first.tracks, first.speaker_holds), (1, 1), "as they are now");
        assert_eq!((first.microphone_dropped, first.speaker_dropped, first.speaker_underruns), (0, 0, 0));
        assert_eq!((first.tests, first.restarts), (1, 0));
        let later = ChainHealth {
            microphone_dropped: 3,
            speaker_dropped: 480,
            speaker_underruns: 2,
            tests: 0,
            restarts: 1,
            ..health(2002)
        };
        assert_eq!(log.look(later, at(19)), None, "nine seconds after the line");
        let second = log.look(later, at(20)).expect("ten seconds after the line");
        assert_eq!((second.turns, second.captured, second.played), (1000, 1000, 1000));
        assert_eq!((second.microphone_dropped, second.speaker_dropped, second.speaker_underruns), (3, 480, 2));
        assert_eq!((second.tests, second.restarts), (0, 1));
        let reopened = ChainHealth { restarts: 1, ..health(3002) };
        let third = log.look(reopened, at(30)).expect("a line");
        let dropped = (third.microphone_dropped, third.speaker_dropped, third.speaker_underruns);
        assert_eq!(dropped, (0, 0, 0), "a new device counts anew");
        assert_eq!(third.restarts, 0, "since the last line");
    }

    #[tokio::test(start_paused = true)]
    async fn two_who_look_in_the_same_second_count_once() {
        let start = Instant::now();
        let (mut alone, mut shared) = (VoiceLog::default(), VoiceLog::default());
        let (mut lines_alone, mut lines_shared) = (Vec::new(), Vec::new());
        for second in 0..=30_u64 {
            let (now, counts) = (start + Duration::from_secs(second), health(second * 100));
            lines_alone.extend(alone.look(counts, now).map(|line| (second, line)));
            // A call and the probe, each once a second, six tenths apart.
            lines_shared.extend(shared.look(counts, now).map(|line| (second, line)));
            let between = health(second * 100 + 60);
            assert_eq!(shared.look(between, now + Duration::from_millis(600)), None, "second {second}");
        }
        assert_eq!(lines_alone.len(), 3, "a line every ten seconds");
        assert_eq!(lines_shared, lines_alone, "the same lines, at the same seconds, with a second looker");
    }

    #[tokio::test(start_paused = true)]
    async fn a_thread_that_stands_still_while_a_device_is_asked_is_said_once_by_what_it_is_held_in() {
        let start = Instant::now();
        let at = |millis: u64| start + Duration::from_millis(millis);
        let mut log = VoiceLog::default();
        let held = ChainHealth { doing: Turn::Processing, ..health(500) };
        assert_eq!(log.stalled(health(400), open(), at(0)), None);
        assert_eq!(log.stalled(held, open(), at(500)), None, "it turned since the last look");
        assert_eq!(log.stalled(held, open(), at(1400)), None, "still for less than a second");
        assert_eq!(log.stalled(held, open(), at(1500)), Some(Turn::Processing), "named by its step");
        assert_eq!(log.stalled(held, open(), at(2500)), None, "said once");
        assert_eq!(log.stalled(health(501), open(), at(3000)), None, "it turns again");
        assert_eq!(log.stalled(health(501), open(), at(4000)), Some(Turn::Idle), "and a new stall is said");
    }

    #[tokio::test(start_paused = true)]
    async fn a_thread_with_nothing_asked_of_it_or_one_that_stopped_is_never_said_to_stand_still() {
        let start = Instant::now();
        let closed = Sounding { microphone: Standing::Closed, speaker: Standing::Closed };
        let mut log = VoiceLog::default();
        for second in 0..5 {
            let now = start + Duration::from_secs(second);
            assert_eq!(log.stalled(health(7), closed, now), None, "nothing is asked: it may rest");
        }
        assert_eq!(log.stalled(health(7), open(), start + Duration::from_secs(5)), None, "watched from the ask on");
        let stopped = ChainHealth { alive: false, ..health(7) };
        assert_eq!(log.stalled(stopped, open(), start + Duration::from_secs(9)), None, "a stopped chain is said apart");
    }

    #[tokio::test(start_paused = true)]
    async fn a_keeper_that_finishes_no_look_while_its_device_is_asked_is_said_once_by_what_it_is_held_in() {
        let start = Instant::now();
        let at = |millis: u64| start + Duration::from_millis(millis);
        let mut log = VoiceLog::default();
        let held = |turns: u64| ChainHealth {
            microphone: Keeper { turns, doing: Keeping::Opening },
            speaker: Keeper { turns: turns + 100, doing: Keeping::Idle },
            ..health(turns + 100)
        };
        assert_eq!(log.microphone_stalled(held(40), open(), at(0)), None);
        assert_eq!(log.microphone_stalled(held(50), open(), at(500)), None, "it looked since the last look");
        assert_eq!(log.microphone_stalled(held(50), open(), at(1400)), None, "still for less than a second");
        assert_eq!(log.microphone_stalled(held(50), open(), at(1500)), Some(Keeping::Opening), "named by its step");
        assert_eq!(log.microphone_stalled(held(50), open(), at(2500)), None, "said once");
        assert_eq!(log.speaker_stalled(held(50), open(), at(2500)), None, "the speaker is not asked, nor stuck");
        assert_eq!(log.microphone_stalled(held(51), open(), at(3000)), None, "it looks again");
        let closed = Sounding { microphone: Standing::Closed, speaker: Standing::Closed };
        assert_eq!(log.microphone_stalled(held(51), closed, at(9000)), None, "a device nobody asks for may rest");
    }
}
