//! A share's or a camera's health as one note every ten looks: what it is sending and what happened to it since
//! the line before. Pure: the looks come in, a note goes out, and nothing here reads a clock or names a person.

use crate::lines::{Coded, LOOKS_A_LINE};
use crate::send::screen::{KeyAsks, KeyFrames, Reopens};
use proto::video::Video;
use report::{Fact, word};
use std::time::Duration;

/// What the sender's counters said at one look, the counts over the share so far.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SentLook {
    /// How the picture is coded and encoded.
    pub(crate) coded: Coded,
    /// The stream's width and height.
    pub(crate) size: (u32, u32),
    /// The pictures a second over the last look.
    pub(crate) frame_rate: Option<u32>,
    /// The bits a second sent over the last look.
    pub(super) bits_per_second: u64,
    /// What the engine last told the encoder to aim at, and its estimate of the link, in bits a second.
    pub(crate) target_bits_per_second: u64,
    pub(crate) estimate_bits_per_second: u64,
    /// The most the picture's envelope allows the encoder, the person's own choice in it.
    pub(super) allowance: u64,
    /// Key frames the encoder made.
    pub(crate) key_frames_made: u64,
    /// Key frames the viewers asked for, by PLI and FIR.
    pub(crate) key_frames_asked: u64,
    /// Packets the viewers asked for again.
    pub(crate) nacks: u64,
    /// The picture's bytes sent again.
    pub(crate) resent_bytes: u64,
    /// Why the encoder was reopened.
    pub(crate) reopens: Reopens,
    /// Pictures the share's own rate held back, replaced by a newer.
    pub(crate) held: u64,
    /// Key frames the encoder made, by what asked for each.
    pub(crate) key_frames_by: KeyFrames,
    /// The asks for a key frame the share deferred, and those a key frame made for another answered.
    pub(crate) key_asks: KeyAsks,
    /// The packets sent, and the time they waited in the engine's pacer before they left, summed.
    pub(crate) packets_sent: u64,
    pub(crate) send_delay: Duration,
}

impl SentLook {
    /// The time a packet waited in the pacer on average, over the packets this look counts.
    fn mean_send_delay(&self) -> Duration {
        u64::try_from(self.send_delay.as_micros())
            .ok()
            .and_then(|micros| micros.checked_div(self.packets_sent))
            .map_or(Duration::ZERO, Duration::from_micros)
    }
}

/// The looks of one video sent since the last line, and the counts it was written at.
#[derive(Debug)]
pub(crate) struct SentLog {
    of: Video,
    looks: u32,
    at_last_line: SentLook,
}

/// One line about a video sent: what it sends now, and what changed over the looks it covers.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SentLine {
    of: Video,
    now: SentLook,
    since: SentLook,
}

impl SentLog {
    /// The log of `of`, the share or the camera.
    pub(crate) fn new(of: Video) -> Self {
        Self { of, looks: 0, at_last_line: SentLook::default() }
    }

    /// Takes in a look; every [`LOOKS_A_LINE`] looks, the line about them.
    pub(crate) fn look(&mut self, now: SentLook) -> Option<SentLine> {
        self.looks += 1;
        if self.looks < LOOKS_A_LINE {
            return None;
        }
        self.looks = 0;
        let before = &self.at_last_line;
        let since = SentLook {
            key_frames_made: now.key_frames_made.saturating_sub(before.key_frames_made),
            key_frames_asked: now.key_frames_asked.saturating_sub(before.key_frames_asked),
            nacks: now.nacks.saturating_sub(before.nacks),
            resent_bytes: now.resent_bytes.saturating_sub(before.resent_bytes),
            reopens: now.reopens.since(&before.reopens),
            held: now.held.saturating_sub(before.held),
            key_frames_by: KeyFrames {
                started: now.key_frames_by.started.saturating_sub(before.key_frames_by.started),
                asked: now.key_frames_by.asked.saturating_sub(before.key_frames_by.asked),
                reopened: now.key_frames_by.reopened.saturating_sub(before.key_frames_by.reopened),
                other: now.key_frames_by.other.saturating_sub(before.key_frames_by.other),
            },
            key_asks: KeyAsks {
                deferred: now.key_asks.deferred.saturating_sub(before.key_asks.deferred),
                merged: now.key_asks.merged.saturating_sub(before.key_asks.merged),
            },
            packets_sent: now.packets_sent.saturating_sub(before.packets_sent),
            send_delay: now.send_delay.saturating_sub(before.send_delay),
            ..SentLook::default()
        };
        self.at_last_line = now.clone();
        Some(SentLine { of: self.of, now, since })
    }
}

impl SentLine {
    /// Writes the line to the log.
    pub(crate) fn log(&self) {
        let (now, since) = (&self.now, &self.since);
        let of = match self.of {
            Video::Screen => Fact::Word(word!("share")),
            Video::Camera => Fact::Word(word!("camera")),
        };
        report::note!(
            info,
            "a video sent",
            of = of,
            codec = now.coded.codec,
            route = now.coded.route,
            width = now.size.0,
            height = now.size.1,
            frame_rate = now.frame_rate,
            bits_per_second = now.bits_per_second,
            target_bits_per_second = now.target_bits_per_second,
            estimate_bits_per_second = now.estimate_bits_per_second,
            allowance = now.allowance,
            over_s = LOOKS_A_LINE,
            key_frames_made = since.key_frames_made,
            key_frames_asked = since.key_frames_asked,
            nacks = since.nacks,
            resent_bytes = since.resent_bytes,
            reopens = since.reopens.total(),
            held = since.held,
            send_delay_us = u64::try_from(since.mean_send_delay().as_micros()).unwrap_or(u64::MAX),
            key_frames_from_start = since.key_frames_by.started,
            key_frames_from_ask = since.key_frames_by.asked,
            key_frames_from_reopen = since.key_frames_by.reopened,
            key_frames_from_other = since.key_frames_by.other,
            key_asks_deferred = since.key_asks.deferred,
            key_asks_merged = since.key_asks.merged,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::send::screen::Cause;
    use choices::Codec;
    use screens::Route;

    fn look(made: u64, asked: u64, nacks: u64) -> SentLook {
        SentLook {
            coded: Coded::of(Codec::H264, &Route::Hardware { via: String::new() }),
            size: (1920, 1080),
            frame_rate: Some(60),
            bits_per_second: 8_100_000,
            key_frames_made: made,
            key_frames_asked: asked,
            nacks,
            resent_bytes: nacks * 1200,
            reopens: Reopens::default(),
            ..SentLook::default()
        }
    }

    #[test]
    fn a_line_comes_every_ten_looks_with_the_deltas() {
        let mut log = SentLog::new(Video::Screen);
        for n in 1..LOOKS_A_LINE {
            assert_eq!(log.look(look(1, 0, u64::from(n))), None, "look {n}");
        }
        let first = log.look(look(1, 0, 10)).expect("the tenth look");
        assert_eq!(first.of, Video::Screen);
        assert_eq!(
            (first.now.size, first.now.frame_rate, first.now.bits_per_second),
            ((1920, 1080), Some(60), 8_100_000)
        );
        assert_eq!(first.now.coded, Coded::of(Codec::H264, &Route::Hardware { via: String::new() }));
        assert_eq!((first.since.key_frames_made, first.since.key_frames_asked, first.since.nacks), (1, 0, 10));
        assert_eq!((first.since.resent_bytes, first.since.reopens.total()), (12_000, 0));
        let mut later = look(3, 2, 14);
        later.reopens.record(Cause::LinkRate, media::Changes::default());
        for _ in 1..LOOKS_A_LINE {
            log.look(later.clone());
        }
        let second = log.look(later).expect("the twentieth look");
        let counted = (second.since.key_frames_made, second.since.key_frames_asked, second.since.nacks);
        assert_eq!(counted, (2, 2, 4), "since the last line");
        assert_eq!(second.since.reopens.total(), 1);
    }

    #[test]
    fn a_line_says_the_mean_wait_of_a_packet_in_the_pacer_the_held_pictures_and_the_key_frames_by_cause_since_the_last()
    {
        let mut log = SentLog::new(Video::Screen);
        let mut at = |held, by: KeyFrames, packets, delay_ms| {
            let sent = SentLook {
                held,
                key_frames_by: by,
                packets_sent: packets,
                send_delay: Duration::from_millis(delay_ms),
                ..look(0, 0, 0)
            };
            (0..LOOKS_A_LINE).find_map(|_| log.look(sent.clone()))
        };
        let first = KeyFrames { started: 1, asked: 2, reopened: 0, other: 0 };
        let line = at(4, first, 1000, 500).expect("the tenth look");
        assert_eq!(line.since.held, 4);
        assert_eq!(line.since.mean_send_delay(), Duration::from_micros(500), "500 ms over 1000 packets");
        assert_eq!(line.since.key_frames_by, first);
        let then = KeyFrames { started: 1, asked: 5, reopened: 1, other: 0 };
        let line = at(10, then, 1600, 1100).expect("the twentieth look");
        assert_eq!((line.since.held, line.since.packets_sent), (6, 600));
        assert_eq!(line.since.mean_send_delay(), Duration::from_millis(1), "600 ms over 600 packets");
        assert_eq!(line.since.key_frames_by, KeyFrames { started: 0, asked: 3, reopened: 1, other: 0 });
    }

    #[test]
    fn a_line_says_the_asks_deferred_and_merged_since_the_last() {
        let mut log = SentLog::new(Video::Screen);
        let mut at = |deferred, merged| {
            let sent = SentLook { key_asks: KeyAsks { deferred, merged }, ..look(0, 0, 0) };
            (0..LOOKS_A_LINE).find_map(|_| log.look(sent.clone()))
        };
        assert_eq!(at(2, 3).expect("the tenth look").since.key_asks, KeyAsks { deferred: 2, merged: 3 });
        assert_eq!(at(5, 3).expect("the twentieth look").since.key_asks, KeyAsks { deferred: 3, merged: 0 });
    }

    #[test]
    fn a_line_of_no_packets_says_no_wait_not_a_division_by_zero() {
        assert_eq!(SentLook::default().mean_send_delay(), Duration::ZERO);
    }

    #[test]
    fn a_camera_s_line_says_it_is_the_camera() {
        let mut log = SentLog::new(Video::Camera);
        let line = (0..LOOKS_A_LINE).find_map(|_| log.look(look(1, 0, 0))).expect("the tenth look");
        assert_eq!(line.of, Video::Camera);
    }
}
