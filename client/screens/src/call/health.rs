//! How a call's connection is, for the call panel's bars and its stats popover: what the engine had counted at two
//! looks, as rates and totals, and the verdict on them. Nothing here reads a clock; the time between looks comes in.

use crate::call::quality::{Counts, Looks, Quality};
use proto::video::Video;
use std::collections::VecDeque;
use std::time::Duration;
use words::{Phrase, Words};

/// How often the call looks at its connection; each look is one sample of the round-trip graph.
pub const LOOK_EVERY: Duration = Duration::from_secs(1);

/// How many looks the round-trip graph keeps: five minutes of them, as Discord's.
pub const KEPT_LOOKS: usize = 300;

/// The looks in a minute, where the graph marks one.
#[expect(clippy::cast_possible_truncation, reason = "at most sixty looks a minute")]
const MINUTE_LOOKS: usize = (60 / LOOK_EVERY.as_secs()) as usize;

/// How far back the round-trip graph reaches.
pub const KEPT: Duration = Duration::from_secs(LOOK_EVERY.as_secs() * KEPT_LOOKS as u64);

/// The tops the graph's scale may take, the least one at or above the highest sample chosen, so the scale moves in
/// steps rather than with every sample. A round trip past the last is drawn at the top.
const TOPS: [Duration; 6] = [
    Duration::from_millis(50),
    Duration::from_millis(100),
    Duration::from_millis(200),
    Duration::from_millis(400),
    Duration::from_millis(800),
    Duration::from_millis(1600),
];

/// Where a round trip was measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasuredOn {
    /// The media route the call's sound and screens take.
    Route,
    /// The link's heartbeat to the server, while the call has no media route measured.
    Heartbeat,
}

/// One round trip, and where it was measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundTrip {
    /// How long the trip took.
    pub took: Duration,
    /// The path it was measured on.
    pub on: MeasuredOn,
}

/// What the engine had counted over the call so far, at one look. A stream that closed takes its share out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reading {
    /// The round trip; none until one was measured.
    pub round_trip: Option<RoundTrip>,
    /// Audio packets received; with `lost`, what the verdict is judged on.
    pub heard: u64,
    /// Audio packets lost.
    pub lost: u64,
    /// Packets of every stream sent, audio and video.
    pub sent: u64,
    /// Payload bytes of every stream sent.
    pub sent_bytes: u64,
    /// Packets of every stream received, audio and video.
    pub received: u64,
    /// Payload bytes of every stream received.
    pub received_bytes: u64,
    /// Everything the transports sent, headers and control included, in bytes.
    pub up_bytes: u64,
    /// Everything the transports received, headers and control included, in bytes.
    pub down_bytes: u64,
}

/// What the looks so far kept: the loss window, and each direction's packets added up look by look, so a stream
/// that closed takes nothing back from a total.
#[derive(Clone, Debug, Default)]
pub struct Tally {
    looks: Looks,
    sent: u64,
    received: u64,
}

/// One direction of every stream: the packets so far and the rate since the last look.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flow {
    /// Packets over the call so far, a closed stream's kept.
    pub packets: u64,
    /// Payload bits a second since the last look; zero at a first look.
    pub bits_per_second: u64,
}

/// How this account's own screen share or camera is going, for the popover's section about each.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outgoing {
    /// How the picture is coded ("H.264").
    pub codec: String,
    /// How the picture is encoded: in hardware through an interface, or in software where none opens.
    pub encoder: crate::Route,
    /// The stream's width in pixels.
    pub width: u32,
    /// The stream's height in pixels.
    pub height: u32,
    /// The screen's size, where the stream is scaled down from it; never for a camera.
    pub scaled_from: Option<(u32, u32)>,
    /// The pictures a second sent over the last look; none until there is one.
    pub frame_rate: Option<u32>,
    /// The bits a second the share sent over the last look.
    pub bits_per_second: u64,
    /// Pictures sent since the share began.
    pub frames: u64,
    /// Key frames its encoder made since the share began.
    pub key_frames_made: u64,
    /// Key frames its viewers asked for since the share began.
    pub key_frames_asked: u64,
    /// The picture's bytes sent again for lost packets since the share began.
    pub resent_bytes: u64,
    /// How often its encoder was reopened: for a new size, rate or encoder.
    pub restarts: u64,
    /// The connection has kept the encoder below what the picture is allowed, which the person did not choose.
    pub limited: bool,
    /// Why the share sends less than was chosen, while it does; none when it sends what was chosen.
    pub reduced: Option<Reduced>,
}

/// What a share was chosen to send, and why it sends less.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reduced {
    /// The screen's size that was chosen to send, only where the size stepped down too.
    pub from_size: Option<(u32, u32)>,
    /// The pictures a second that were chosen to send.
    pub from_frame_rate: u32,
    /// What holds it below.
    pub cause: Reduction,
}

/// Why a share sends less than was chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reduction {
    /// It started lower to fit a ceiling too low for what was chosen.
    Ceiling {
        /// The ceiling, in bits a second.
        bits_per_second: u64,
    },
    /// The screen's content needs more than the rate.
    Busy {
        /// The rate the person chose, in bits a second; none under Automatic.
        ceiling: Option<u64>,
    },
    /// The encoder could not keep up.
    Encoder,
}

/// Whether anyone takes this account's own share or camera now, as the server last told. Nobody watching it, it rests:
/// no picture is made or sent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OwnWatch {
    /// Someone watches it, or nobody has said otherwise yet.
    #[default]
    Watched,
    /// Nobody does.
    Unwatched,
}

impl OwnWatch {
    /// What the call reported: whether anyone `watched`.
    pub fn of(watched: bool) -> OwnWatch {
        if watched { OwnWatch::Watched } else { OwnWatch::Unwatched }
    }

    /// The figure of the stats' Frame rate row of `outgoing`: "Not sending" while unwatched, else its rate once measured.
    pub fn frame_rate(self, outgoing: &Outgoing, words: &Words) -> Option<String> {
        match self {
            OwnWatch::Unwatched => Some(words.phrase(Phrase::StatsNotSending).into_owned()),
            OwnWatch::Watched => outgoing.frame_rate.map(|rate| words.fps(rate)),
        }
    }

    /// The figure of the stats' Rate row while it is not a rate: a dash while unwatched; none, so the measured rate is
    /// shown, while watched.
    pub fn rate_instead(self, words: &Words) -> Option<String> {
        match self {
            OwnWatch::Unwatched => Some(words.phrase(Phrase::StatsRateNone).into_owned()),
            OwnWatch::Watched => None,
        }
    }
}

/// What the connection popover shows of this account's own video, while one is sent and once the call has looked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnVideo<'a> {
    /// One is sent: its section, under its own heading, and whether anyone watches it.
    Alone(Video, &'a Outgoing, OwnWatch),
    /// Both are: a tab each, and the section of the one `shown` under them.
    Tabbed {
        /// The video whose tab is picked.
        shown: Video,
        /// How it is going.
        outgoing: &'a Outgoing,
        /// Whether anyone watches it.
        watch: OwnWatch,
    },
}

/// What the call panel shows of the connection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Health {
    /// The round trip; none until one was measured.
    pub round_trip: Option<RoundTrip>,
    /// The verdict on the round trip and the loss over the window; none until a round trip was measured.
    pub quality: Option<Quality>,
    /// What every stream sent.
    pub sending: Flow,
    /// What every stream received.
    pub receiving: Flow,
    /// Audio packets lost over the call.
    pub lost: u64,
    /// Everything the transports sent over the call, headers and control included, in bytes.
    pub up_bytes: u64,
    /// Everything the transports received over the call, headers and control included, in bytes.
    pub down_bytes: u64,
}

impl Health {
    /// `now` against `earlier`, taken `elapsed` before it; the rates are zero at a first look. `tally` takes this
    /// look in: the verdict counts loss over its window.
    pub fn between(earlier: Option<&Reading>, now: &Reading, elapsed: Duration, tally: &mut Tally) -> Self {
        let grown =
            |now: u64, then: fn(&Reading) -> u64| earlier.map_or(now, |earlier| now.saturating_sub(then(earlier)));
        let rate =
            |now: u64, then: fn(&Reading) -> u64| earlier.map_or(0, |_| bits_per_second(grown(now, then), elapsed));
        tally.sent = tally.sent.saturating_add(grown(now.sent, |r| r.sent));
        tally.received = tally.received.saturating_add(grown(now.received, |r| r.received));
        Self {
            round_trip: now.round_trip,
            quality: Quality::of(
                now.round_trip.map(|trip| trip.took),
                tally.looks.look(Counts { received: now.heard, lost: now.lost }),
            ),
            sending: Flow { packets: tally.sent, bits_per_second: rate(now.sent_bytes, |r| r.sent_bytes) },
            receiving: Flow {
                packets: tally.received,
                bits_per_second: rate(now.received_bytes, |r| r.received_bytes),
            },
            lost: now.lost,
            up_bytes: now.up_bytes,
            down_bytes: now.down_bytes,
        }
    }
}

/// The round trips of the last [`KEPT_LOOKS`] looks, the oldest dropped as a new one comes; a look that measured none
/// is kept as a gap.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoundTrips {
    looks: VecDeque<Option<RoundTrip>>,
}

/// A point of the graph: `x` from 0, the oldest look kept, to 1, the latest; `y` from 0 to 1, the scale's top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    /// Across, by the look's age.
    pub x: f32,
    /// Up, by the round trip against the top.
    pub y: f32,
}

/// The round trips as a graph: each line runs over looks measured on one path, broken where a look measured none or
/// the path changed, so no slope joins two different measures.
#[derive(Clone, Debug, PartialEq)]
pub struct Graph {
    /// The lines, oldest first, each one point or more.
    pub lines: Vec<Vec<Point>>,
    /// The round trip at the graph's top; its scale reads this, half of it and nothing.
    pub top: Duration,
    /// A mark at each whole minute back that the graph reaches, oldest first, then the latest look's.
    pub minutes: Vec<Minute>,
}

/// A mark across the graph: how many minutes back it is, at the place of the look it marks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Minute {
    /// Across, as a point's `x`.
    pub x: f32,
    /// Minutes before the latest look; 0 is the latest itself.
    pub ago: u64,
}

impl Graph {
    /// The middle of the graph's scale.
    pub fn half(&self) -> Duration {
        self.top / 2
    }
}

impl RoundTrips {
    /// Takes in a look's round trip, or its gap.
    pub fn push(&mut self, look: Option<RoundTrip>) {
        if self.looks.len() == KEPT_LOOKS {
            self.looks.pop_front();
        }
        self.looks.push_back(look);
    }

    /// The looks kept as a graph, the latest at the right edge.
    pub fn graph(&self) -> Graph {
        let highest = self.looks.iter().flatten().map(|trip| trip.took).max().unwrap_or_default();
        let top = TOPS.into_iter().find(|top| *top >= highest).unwrap_or(TOPS[TOPS.len() - 1]);
        let empty = KEPT_LOOKS - self.looks.len();
        let last = (KEPT_LOOKS - 1) as f32;
        let mut lines: Vec<Vec<Point>> = Vec::new();
        let mut path = None;
        for (index, look) in self.looks.iter().enumerate() {
            let Some(trip) = look else {
                path = None;
                continue;
            };
            let point =
                Point { x: (empty + index) as f32 / last, y: (trip.took.as_secs_f32() / top.as_secs_f32()).min(1.0) };
            match lines.last_mut() {
                Some(line) if path == Some(trip.on) => line.push(point),
                _ => lines.push(vec![point]),
            }
            path = Some(trip.on);
        }
        let latest = KEPT_LOOKS - 1;
        let minutes = (0..=latest / MINUTE_LOOKS)
            .rev()
            .map(|ago| Minute { x: (latest - ago * MINUTE_LOOKS) as f32 / last, ago: ago as u64 })
            .collect();
        Graph { lines, top, minutes }
    }
}

/// The most calls a window keeps the figures of.
pub const KEPT_CALLS: usize = 5;

/// A figure's least, middle and greatest over a call's looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spread<T> {
    /// The smallest.
    pub least: T,
    /// The middle one; of an even number of looks, the upper of the two.
    pub middle: T,
    /// The greatest.
    pub most: T,
}

impl<T: Copy + Ord> Spread<T> {
    fn of(figures: impl Iterator<Item = T>) -> Option<Self> {
        let mut sorted: Vec<T> = figures.collect();
        sorted.sort_unstable();
        Some(Self { least: *sorted.first()?, middle: *sorted.get(sorted.len() / 2)?, most: *sorted.last()? })
    }
}

/// One look at a call's connection, as the figures of a finished call keep it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CallLook {
    round_trip: Option<Duration>,
    /// Audio packets lost since the look before.
    lost: u64,
    /// Bits a second sent over the look.
    sent: u64,
}

/// The last [`KEPT_LOOKS`] looks of a call, and how many it made in all: what [`CallFigures`] are computed from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CallLooks {
    kept: VecDeque<CallLook>,
    total: u64,
    /// Audio packets lost over the call at the last look, which the next look's loss is the growth from.
    lost_so_far: u64,
}

impl CallLooks {
    /// Takes in a look: its round trip, the audio lost since the last look and the rate sent.
    pub fn push(&mut self, health: &Health) {
        if self.kept.len() == KEPT_LOOKS {
            self.kept.pop_front();
        }
        self.kept.push_back(CallLook {
            round_trip: health.round_trip.map(|trip| trip.took),
            lost: health.lost.saturating_sub(self.lost_so_far),
            sent: health.sending.bits_per_second,
        });
        self.lost_so_far = health.lost;
        self.total = self.total.saturating_add(1);
    }
}

/// What a finished call came to, as the debug report states it: how long it ran, the spread of its connection over the
/// last [`KEPT_LOOKS`] looks, and how often the round trip went unmeasured after it had been.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallFigures {
    /// How many looks it made, one each [`LOOK_EVERY`]: its length.
    pub looks: u64,
    /// The round trip; none when no look measured one.
    pub round_trip: Option<Spread<Duration>>,
    /// Audio packets lost between two looks; none when there was no look.
    pub lost: Option<Spread<u64>>,
    /// Bits a second sent; none when there was no look.
    pub sent: Option<Spread<u64>>,
    /// Stretches of looks that measured no round trip after one had been measured.
    pub stalls: u64,
}

impl CallFigures {
    /// The figures of `looks`, which are a call's all of them.
    pub fn of(looks: &CallLooks) -> Self {
        let mut stalls = 0;
        let (mut measured, mut gap) = (false, false);
        for look in &looks.kept {
            match look.round_trip {
                Some(_) => (measured, gap) = (true, false),
                None if measured && !gap => (stalls, gap) = (stalls + 1, true),
                None => {}
            }
        }
        Self {
            looks: looks.total,
            round_trip: Spread::of(looks.kept.iter().filter_map(|look| look.round_trip)),
            lost: Spread::of(looks.kept.iter().map(|look| look.lost)),
            sent: Spread::of(looks.kept.iter().map(|look| look.sent)),
            stalls,
        }
    }

    /// How long the call ran.
    pub fn length(&self) -> Duration {
        LOOK_EVERY.saturating_mul(u32::try_from(self.looks).unwrap_or(u32::MAX))
    }
}

fn bits_per_second(bytes: u64, elapsed: Duration) -> u64 {
    let micros = elapsed.as_micros();
    if micros == 0 {
        return 0;
    }
    u64::try_from(u128::from(bytes) * 8_000_000 / micros).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call::quality::Quality;
    use crate::door::Opening;
    use crate::harness::{ADA, Clock, accepted_as, ada_in_call, flow, id, moved, owls, tell, token};
    use crate::place::{self, Over, Place};
    use crate::window::Window;
    use std::time::{Duration, Instant};

    const SECOND: Duration = Duration::from_secs(1);

    pub(crate) fn reading(sent_bytes: u64, received_bytes: u64) -> Reading {
        Reading {
            round_trip: Some(RoundTrip { took: Duration::from_millis(38), on: MeasuredOn::Route }),
            sent: sent_bytes / 100,
            sent_bytes,
            received: received_bytes / 100,
            received_bytes,
            ..Reading::default()
        }
    }

    #[test]
    fn a_rate_is_the_bytes_since_the_last_look_over_the_time_between() {
        let mut tally = Tally::default();
        Health::between(None, &reading(1_000, 5_000), SECOND, &mut tally);
        let health = Health::between(Some(&reading(1_000, 5_000)), &reading(2_000, 9_000), 2 * SECOND, &mut tally);
        assert_eq!(health.sending.bits_per_second, 4_000);
        assert_eq!(health.receiving.bits_per_second, 16_000);
        assert_eq!((health.sending.packets, health.receiving.packets), (20, 90), "packets are the totals");
    }

    #[test]
    fn the_first_look_has_totals_and_no_rate() {
        let health = Health::between(None, &reading(2_000, 9_000), SECOND, &mut Tally::default());
        assert_eq!((health.sending.bits_per_second, health.receiving.bits_per_second), (0, 0));
        assert_eq!(health.sending.packets, 20);
    }

    #[test]
    fn a_total_that_went_back_or_no_time_between_looks_is_no_rate_not_a_negative_or_a_panic() {
        let mut tally = Tally::default();
        let back = Health::between(Some(&reading(5_000, 5_000)), &reading(1_000, 1_000), SECOND, &mut tally);
        assert_eq!(back.sending.bits_per_second, 0);
        let none = Health::between(Some(&reading(0, 0)), &reading(1_000, 1_000), Duration::ZERO, &mut tally);
        assert_eq!(none.receiving.bits_per_second, 0);
    }

    #[test]
    fn a_stream_that_closed_takes_nothing_back_from_a_total() {
        let mut tally = Tally::default();
        let watching = reading(1_000, 90_000);
        Health::between(None, &watching, SECOND, &mut tally);
        let closed = reading(2_000, 1_000);
        let after = Health::between(Some(&watching), &closed, SECOND, &mut tally);
        assert_eq!(after.receiving.packets, 900, "the watched screen's packets stay counted");
        let later = Health::between(Some(&closed), &reading(3_000, 3_000), SECOND, &mut tally);
        assert_eq!((later.sending.packets, later.receiving.packets), (30, 920), "and what came after adds to them");
    }

    #[test]
    fn the_verdict_is_the_round_trip_and_the_audio_loss_over_the_looks() {
        let mut tally = Tally::default();
        let mut now = Reading { heard: 1_000, lost: 0, received: 90_000, ..reading(0, 0) };
        assert_eq!(Health::between(None, &now, SECOND, &mut tally).quality, Some(Quality::Good));
        now.lost = 100;
        let lossy = Health::between(Some(&Reading::default()), &now, SECOND, &mut tally);
        assert_eq!(
            lossy.quality,
            Some(Quality::Poor),
            "100 lost of 100 heard since the first look, whatever video came"
        );
        assert_eq!(lossy.lost, 100, "the total is shown whole");
        now.round_trip = None;
        assert_eq!(Health::between(Some(&now), &now, SECOND, &mut tally).quality, None);
    }

    fn look_of(millis: Option<u64>, lost: u64, bits: u64) -> Health {
        Health {
            round_trip: millis.map(|millis| RoundTrip { took: Duration::from_millis(millis), on: MeasuredOn::Route }),
            lost,
            sending: Flow { packets: 0, bits_per_second: bits },
            ..Health::default()
        }
    }

    #[test]
    fn a_calls_figures_are_the_spread_of_its_looks_and_its_length_the_count_of_them() {
        let mut looks = CallLooks::default();
        for (millis, lost, bits) in [(Some(40), 0, 1_000), (Some(20), 3, 3_000), (Some(30), 5, 2_000)] {
            looks.push(&look_of(millis, lost, bits));
        }
        let figures = CallFigures::of(&looks);
        assert_eq!(figures.length(), 3 * LOOK_EVERY);
        let ms = |millis| Duration::from_millis(millis);
        assert_eq!(figures.round_trip, Some(Spread { least: ms(20), middle: ms(30), most: ms(40) }));
        assert_eq!(figures.lost, Some(Spread { least: 0, middle: 2, most: 3 }), "loss is the growth between looks");
        assert_eq!(figures.sent, Some(Spread { least: 1_000, middle: 2_000, most: 3_000 }));
        assert_eq!(figures.stalls, 0);
    }

    #[test]
    fn a_stall_is_a_stretch_without_a_round_trip_after_one_was_measured() {
        let mut looks = CallLooks::default();
        for millis in [None, Some(20), None, None, Some(20), None, Some(20)] {
            looks.push(&look_of(millis, 0, 0));
        }
        let figures = CallFigures::of(&looks);
        assert_eq!(figures.stalls, 2, "the lead-in before the first measure is no stall; a stretch counts once");
        let none = CallFigures::of(&CallLooks::default());
        assert_eq!((none.looks, none.round_trip, none.lost, none.sent, none.stalls), (0, None, None, None, 0));
    }

    #[test]
    fn a_call_keeps_the_last_looks_but_counts_all_of_them() {
        let mut looks = CallLooks::default();
        let made = u64::try_from(KEPT_LOOKS).unwrap() + 20;
        for at in 0..made {
            looks.push(&look_of(Some(at + 1), 0, 0));
        }
        let figures = CallFigures::of(&looks);
        assert_eq!(figures.looks, made);
        assert_eq!(figures.round_trip.map(|spread| spread.least), Some(Duration::from_millis(21)), "the oldest went");
    }

    fn route(millis: u64) -> Option<RoundTrip> {
        Some(RoundTrip { took: Duration::from_millis(millis), on: MeasuredOn::Route })
    }

    #[test]
    fn the_ring_keeps_three_minutes_and_drops_the_oldest() {
        let mut trips = RoundTrips::default();
        for millis in 0..u64::try_from(KEPT_LOOKS + 20).unwrap() {
            trips.push(route(millis));
        }
        assert_eq!(trips.looks.len(), KEPT_LOOKS);
        assert_eq!(trips.looks.front().copied().flatten().map(|trip| trip.took), Some(Duration::from_millis(20)));
        assert_eq!(KEPT, Duration::from_secs(300), "five minutes");
        let graph = trips.graph();
        assert_eq!(graph.lines.len(), 1);
        assert_eq!(
            (graph.lines[0][0].x, graph.lines[0][KEPT_LOOKS - 1].x),
            (0.0, 1.0),
            "the oldest left, the latest right"
        );
    }

    #[test]
    fn the_latest_look_is_at_the_right_edge_before_the_ring_is_full() {
        let mut trips = RoundTrips::default();
        trips.push(route(25));
        trips.push(route(50));
        let graph = trips.graph();
        assert_eq!(graph.lines, [vec![Point { x: 298.0 / 299.0, y: 0.5 }, Point { x: 1.0, y: 1.0 }]]);
    }

    #[test]
    fn a_change_of_path_or_a_missing_look_breaks_the_line() {
        let mut trips = RoundTrips::default();
        trips.push(Some(RoundTrip { took: Duration::from_millis(40), on: MeasuredOn::Heartbeat }));
        trips.push(Some(RoundTrip { took: Duration::from_millis(40), on: MeasuredOn::Heartbeat }));
        trips.push(route(30));
        trips.push(None);
        trips.push(route(30));
        trips.push(route(30));
        let lengths: Vec<usize> = trips.graph().lines.iter().map(Vec::len).collect();
        assert_eq!(lengths, [2, 1, 2]);
    }

    #[test]
    fn the_top_is_the_least_step_at_or_above_the_highest_sample() {
        let top = |millis: &[u64]| {
            let mut trips = RoundTrips::default();
            millis.iter().for_each(|&millis| trips.push(route(millis)));
            trips.graph().top
        };
        assert_eq!(top(&[]), Duration::from_millis(50), "nothing measured yet");
        assert_eq!(top(&[12, 50]), Duration::from_millis(50), "a step is inclusive");
        assert_eq!(top(&[12, 51]), Duration::from_millis(100));
        assert_eq!(top(&[30, 900]), Duration::from_millis(1600));
        let mut trips = RoundTrips::default();
        trips.push(route(5_000));
        assert_eq!(trips.graph().lines[0][0].y, 1.0, "past the last step, at the top");
    }

    #[test]
    fn a_mark_comes_at_each_whole_minute_back_and_at_the_latest_look() {
        let graph = RoundTrips::default().graph();
        let ago: Vec<u64> = graph.minutes.iter().map(|minute| minute.ago).collect();
        assert_eq!(ago, [4, 3, 2, 1, 0], "five minutes of looks reach back four whole minutes");
        assert_eq!(graph.minutes[4].x, 1.0, "the latest look at the right edge");
        assert_eq!(graph.minutes[3].x, 239.0 / 299.0, "a minute is sixty looks before it");
        assert_eq!(graph.half(), Duration::from_millis(25));
    }

    fn health(quality: Quality) -> Health {
        Health {
            quality: Some(quality),
            round_trip: Some(crate::RoundTrip { took: Duration::from_millis(38), on: crate::MeasuredOn::Route }),
            ..Health::default()
        }
    }

    #[test]
    fn the_connection_popover_opens_in_a_joined_call_swaps_with_the_menu_and_shows_the_last_look() {
        let clock = Clock(Instant::now());
        let (mut window, place) = ada_in_call(&clock);
        fn call(window: &Window) -> &place::Call {
            window.open().and_then(Place::call).unwrap()
        }
        assert_eq!(call(&window).health(), None, "nothing is known before the call's first look");
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Health(health(Quality::Good))));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Stats));
        assert_eq!(call(&window).open(), Some(Over::Connection));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Devices));
        assert_eq!(call(&window).open(), Some(Over::Devices), "opening the menu shuts the popover: one at a time");
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Stats));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Stats));
        assert_eq!(call(&window).open(), None, "a second press on the bars shuts it");
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Stats));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Health(health(Quality::Fair))));
        assert_eq!(call(&window).health().and_then(|seen| seen.quality), Some(Quality::Fair), "it follows the looks");
        assert_eq!(call(&window).open(), Some(Over::Connection), "and stays open through them");
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::CloseOver));
        assert_eq!(call(&window).open(), None);
    }

    #[test]
    fn the_connection_popover_is_not_open_without_a_joined_call() {
        let clock = Clock(Instant::now());
        let (mut window, _) = Window::launch(vec![Opening::Account(owls(), ADA.into())], None, clock.0);
        let place = id(&window, 0);
        tell(&mut window, &clock, place, flow(token()));
        tell(&mut window, &clock, place, flow(accepted_as("ada")));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Stats));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Health(health(Quality::Good))));
        assert!(window.open().is_some_and(|open| open.call().is_none()), "no call to hold either");
        let (mut window, place) = ada_in_call(&clock);
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Stats));
        tell(&mut window, &clock, place, moved());
        assert_eq!(
            window.open().and_then(Place::call).and_then(place::Call::open),
            None,
            "a call that is over shows nothing over it"
        );
    }

    #[test]
    fn an_unwatched_stream_says_it_sends_nothing_where_a_watched_one_says_its_rate() {
        let words = words::Words::english();
        let outgoing = Outgoing {
            codec: "H.264".into(),
            encoder: crate::Route::Software,
            width: 1280,
            height: 720,
            scaled_from: None,
            frame_rate: Some(30),
            bits_per_second: 1_500_000,
            frames: 300,
            key_frames_made: 1,
            key_frames_asked: 0,
            resent_bytes: 0,
            restarts: 0,
            limited: false,
            reduced: None,
        };
        assert_eq!(OwnWatch::default(), OwnWatch::Watched, "taken until the server says otherwise");
        assert_eq!(OwnWatch::of(false).frame_rate(&outgoing, &words).as_deref(), Some("Not sending"));
        assert_eq!(OwnWatch::of(false).rate_instead(&words).as_deref(), Some("\u{2014}"));
        assert_eq!(OwnWatch::of(true).frame_rate(&outgoing, &words).as_deref(), Some("30 fps"));
        assert_eq!(OwnWatch::of(true).rate_instead(&words), None, "its rate is measured");
        let unmeasured = Outgoing { frame_rate: None, ..outgoing };
        assert_eq!(OwnWatch::of(true).frame_rate(&unmeasured, &words), None, "no row before a rate is measured");
    }
}
