//! One's own video, a share or a camera, as the call looks at it once a second: the rates since the last look from the
//! encoder's counters and the engine's, said to the model and, every few looks, to the log.

use crate::lines::Coded;
use crate::screen::Progress;
use crate::send::log::{SentLog, SentLook};
use crate::send::stage_log::StagesLog;
use choices::Codec;
use proto::video::Video;
use screens::{Outgoing, Route};
use std::sync::PoisonError;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use webrtc::SendStats;

/// What the sender knows of its stream now, apart from its counters.
pub(crate) struct Sending<'a> {
    /// The codec it is sent in.
    pub(super) codec: Codec,
    /// How it is encoded.
    pub(crate) encoder: &'a Route,
    /// The stream's width.
    pub(super) width: u32,
    /// The stream's height.
    pub(super) height: u32,
    /// The screen's size, where the stream is scaled down from it.
    pub(super) scaled_from: Option<(u32, u32)>,
    /// The most the picture's envelope allows the encoder, in bits per second: the row's cap under the person's own
    /// choice, so a target at it is no loss.
    pub(super) allowance: u64,
}

/// How long the link must hold the picture below the allowance, and then let it be again, before the person is told.
pub(crate) const HELD_FOR: Duration = Duration::from_secs(3);

/// libwebrtc's own line for a sender held by what it has to send (`AlrDetector`'s usage ratio): under this share of
/// the estimate the estimate does not grow, so it says nothing of the link.
const APPLICATION_LIMITED_BELOW: f64 = 0.65;

/// Whether the connection holds the picture below what the person allowed, with since when the turn is under way.
#[derive(Clone, Copy, Debug)]
enum Held {
    /// Not told; the target has been below the allowance since `Some`.
    Free(Option<Instant>),
    /// Told; the target has been back at the allowance since `Some`.
    Held(Option<Instant>),
}

impl Held {
    /// The standing after a look at `now` when the link holds the picture `below` the allowance or not.
    fn after(self, below: bool, now: Instant) -> Self {
        let lasted = |since: Instant| now.saturating_duration_since(since) >= HELD_FOR;
        match self {
            Self::Free(since) if below => {
                let since = since.unwrap_or(now);
                if lasted(since) { Self::Held(None) } else { Self::Free(Some(since)) }
            }
            Self::Free(_) => Self::Free(None),
            Self::Held(since) if !below => {
                let since = since.unwrap_or(now);
                if lasted(since) { Self::Free(None) } else { Self::Held(Some(since)) }
            }
            Self::Held(_) => Self::Held(None),
        }
    }

    fn told(self) -> bool {
        matches!(self, Self::Held(_))
    }
}

/// The counts at the last look and when, for the rates between looks, and the looks since the last log line; and
/// which video sent they are of, so an answer asked of one is never taken as another's.
#[derive(Debug)]
pub(crate) struct Looks {
    id: u64,
    last: Option<(u64, u64, Instant)>,
    log: SentLog,
    stages: StagesLog,
    held: Held,
}

impl Looks {
    /// The looks at `of`, the share or the camera, a video sent apart from every other this process sends.
    pub(crate) fn new(of: Video) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            last: None,
            log: SentLog::new(of),
            stages: StagesLog::new(of),
            held: Held::Free(None),
        }
    }

    /// Which video sent these are the looks of.
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    /// How `sending` is going at `now`, from the encoder's `progress` and the `engine`'s counters. No frame rate is
    /// told while nobody `watched` it: a resting encoder sends none, which is no rate of zero.
    pub(crate) fn look(
        &mut self,
        sending: &Sending<'_>,
        progress: &Progress,
        engine: &SendStats,
        watched: bool,
        now: Instant,
    ) -> Outgoing {
        let frames = progress.frames.load(Ordering::Relaxed);
        let (frame_rate, bits_per_second, rated) = match self.last.replace((frames, engine.bytes_sent, now)) {
            Some((then_frames, then_bytes, then)) => {
                let seconds = now.duration_since(then).as_secs_f64();
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a count over positive seconds; a float cast saturates"
                )]
                let per = |after: u64, before: u64| ((after.saturating_sub(before)) as f64 / seconds).round() as u64;
                if seconds > 0.0 {
                    (u32::try_from(per(frames, then_frames)).ok(), per(engine.bytes_sent, then_bytes) * 8, true)
                } else {
                    (None, 0, false)
                }
            }
            None => (None, 0, false),
        };
        let frame_rate = frame_rate.filter(|_| watched);
        // The link holds the picture when the engine's estimate, which the allowance caps, is under it while the
        // picture uses enough of it to make it grow; the target, net of overhead, is always under the estimate.
        let estimate = engine.estimate_bits_per_second;
        if rated && estimate > 0.0 {
            let using = bits_per_second as f64 >= APPLICATION_LIMITED_BELOW * estimate;
            self.held = self.held.after(estimate < sending.allowance as f64 && using, now);
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a rate the engine says in bits a second; a float cast saturates"
        )]
        let rate = |bits: f64| bits.round() as u64;
        let key_frames_made = progress.key_frames.load(Ordering::Relaxed);
        let key_frames_asked = u64::from(engine.picture_losses) + u64::from(engine.full_intra_requests);
        let reopens = *progress.reopens.lock().unwrap_or_else(PoisonError::into_inner);
        let (counted, worst) = (progress.stages.counted(), progress.stages.take_worst());
        let line = self.log.look(SentLook {
            coded: Coded::of(sending.codec, sending.encoder),
            size: (sending.width, sending.height),
            frame_rate,
            bits_per_second,
            target_bits_per_second: rate(engine.target_bits_per_second),
            estimate_bits_per_second: rate(engine.estimate_bits_per_second),
            allowance: sending.allowance,
            key_frames_made,
            key_frames_asked,
            nacks: u64::from(engine.nacks),
            resent_bytes: engine.retransmitted_bytes,
            reopens,
            held: counted.held,
            key_frames_by: counted.key_frames,
            key_asks: counted.key_asks,
            packets_sent: engine.packets_sent,
            send_delay: Duration::try_from_secs_f64(engine.send_delay_seconds).unwrap_or(Duration::ZERO),
        });
        if let Some(line) = line {
            line.log();
        }
        // The worst times are taken whoever watches, so they never carry a second the share rested through.
        if let Some(line) = self.stages.look(counted, worst, now).filter(|_| watched) {
            line.log();
        }
        Outgoing {
            codec: sending.codec.label().to_string(),
            encoder: sending.encoder.clone(),
            width: sending.width,
            height: sending.height,
            scaled_from: sending.scaled_from,
            frame_rate,
            bits_per_second,
            frames,
            key_frames_made,
            key_frames_asked,
            resent_bytes: engine.retransmitted_bytes,
            restarts: reopens.total(),
            limited: self.held.told(),
            reduced: *progress.reduced.lock().unwrap_or_else(PoisonError::into_inner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt as _;

    #[test]
    fn each_video_sent_is_looked_at_apart() {
        assert_ne!(Looks::new(Video::Camera).id(), Looks::new(Video::Camera).id());
    }

    #[test]
    fn the_rates_are_over_the_time_since_the_last_look_and_none_at_the_first() {
        let mut looks = Looks::new(Video::Camera);
        let progress = Progress::default();
        let sending = Sending {
            codec: Codec::H264,
            encoder: &Route::Software,
            width: 1280,
            height: 720,
            scaled_from: None,
            allowance: 0,
        };
        let start = Instant::now();
        let first =
            looks.look(&sending, &progress, &SendStats { bytes_sent: 1_000, ..SendStats::default() }, true, start);
        assert_eq!((first.frame_rate, first.bits_per_second), (None, 0));
        progress.frames.store(60, Ordering::Relaxed);
        progress.key_frames.store(1, Ordering::Relaxed);
        let engine = SendStats { bytes_sent: 1_000 + 375_000, picture_losses: 2, ..SendStats::default() };
        let second = looks.look(&sending, &progress, &engine, true, start + Duration::from_secs(2));
        assert_eq!((second.frame_rate, second.bits_per_second), (Some(30), 1_500_000));
        assert_eq!((second.frames, second.key_frames_made, second.key_frames_asked), (60, 1, 2));
        assert_eq!((second.encoder, second.width, second.scaled_from), (Route::Software, 1280, None));
    }

    #[test]
    fn what_a_share_is_reduced_from_is_told_as_the_share_set_it() {
        let mut looks = Looks::new(Video::Screen);
        let progress = Progress::default();
        let sending = Sending {
            codec: Codec::H264,
            encoder: &Route::Software,
            width: 1280,
            height: 720,
            scaled_from: None,
            allowance: 0,
        };
        let (start, engine) = (Instant::now(), SendStats::default());
        assert_eq!(looks.look(&sending, &progress, &engine, true, start).reduced, None, "sends what was chosen");
        let reduced = screens::Reduced { from_size: None, from_frame_rate: 60, cause: screens::Reduction::Encoder };
        *progress.reduced.lock().unwrap() = Some(reduced);
        let told = looks.look(&sending, &progress, &engine, true, start + Duration::from_secs(1));
        assert_eq!(told.reduced, Some(reduced));
    }

    #[test]
    fn a_video_nobody_watches_tells_no_frame_rate_not_zero() {
        let mut looks = Looks::new(Video::Screen);
        let progress = Progress::default();
        let sending = Sending {
            codec: Codec::H264,
            encoder: &Route::Software,
            width: 1280,
            height: 720,
            scaled_from: None,
            allowance: 0,
        };
        let start = Instant::now();
        let engine = SendStats::default();
        looks.look(&sending, &progress, &engine, false, start);
        let resting = looks.look(&sending, &progress, &engine, false, start + Duration::from_secs(1));
        assert_eq!(resting.frame_rate, None);
    }

    /// The notes about a share's pictures that looks at the start and a second on write, when a picture was dropped in
    /// that second and `watched` says whether anyone takes the share.
    fn pictures_noted(watched: bool) -> usize {
        let mut looks = Looks::new(Video::Screen);
        let progress = Progress::default();
        let sending = Sending {
            codec: Codec::H264,
            encoder: &Route::Software,
            width: 1280,
            height: 720,
            scaled_from: None,
            allowance: 0,
        };
        let (start, engine) = (Instant::now(), SendStats::default());
        let ring = report::Ring::new(std::time::SystemTime::now);
        tracing::subscriber::with_default(tracing_subscriber::registry().with(ring.layer()), || {
            looks.look(&sending, &progress, &engine, watched, start);
            progress.stages.slot(60, 2);
            looks.look(&sending, &progress, &engine, watched, start + Duration::from_secs(1));
        });
        ring.lines().iter().filter(|line| line.as_str().contains("video stages went wrong")).count()
    }

    #[test]
    fn a_share_whose_second_dropped_a_picture_says_so_while_it_is_watched_and_not_while_it_rests() {
        assert_eq!(pictures_noted(true), 1);
        assert_eq!(pictures_noted(false), 0);
    }

    /// Whether each look at `(second, estimate, sent)`, the engine's estimate and the bits a second sent since the
    /// look before, is told as held back.
    fn held(allowance: u64, looked: &[(u64, f64, u64)]) -> Vec<bool> {
        let mut looks = Looks::new(Video::Screen);
        let progress = Progress::default();
        let sending = Sending {
            codec: Codec::H264,
            encoder: &Route::Software,
            width: 1280,
            height: 720,
            scaled_from: None,
            allowance,
        };
        let start = Instant::now();
        let (mut bytes, mut then) = (0, 0);
        looked
            .iter()
            .map(|&(second, estimate, sent)| {
                bytes += sent / 8 * (second - then);
                then = second;
                // The engine aims the encoder at what it sends: under the estimate by the packets' overhead.
                let target = sent as f64;
                let engine = SendStats {
                    estimate_bits_per_second: estimate,
                    target_bits_per_second: target,
                    bytes_sent: bytes,
                    ..SendStats::default()
                };
                looks.look(&sending, &progress, &engine, true, start + Duration::from_secs(second)).limited
            })
            .collect()
    }

    /// A busy picture on a link the engine estimates at `link`: it sends what the encoder is given, net of overhead.
    fn busy(second: u64, link: u64) -> (u64, f64, u64) {
        (second, link as f64, link / 10 * 9)
    }

    #[test]
    fn a_link_below_the_allowance_is_told_after_three_seconds_and_not_before() {
        let looks = [busy(0, 1_000_000), busy(1, 1_000_000), busy(3, 1_000_000), busy(4, 1_000_000)];
        assert_eq!(held(2_000_000, &looks), [false, false, false, true], "the first look has no rate yet");
    }

    #[test]
    fn a_link_back_at_the_allowance_is_told_no_more_after_three_seconds_and_not_before() {
        let (below, max) = (1_000_000, 2_000_000);
        let looks = [busy(0, below), busy(1, below), busy(4, below), busy(5, max), busy(7, max), busy(8, max)];
        assert_eq!(held(2_000_000, &looks), [false, false, true, true, true, false]);
    }

    #[test]
    fn a_dip_back_to_below_restarts_the_wait_to_be_told_or_cleared() {
        let (below, max) = (1_000_000, 2_000_000);
        let looks = [busy(0, below), busy(1, below), busy(3, max), busy(5, below), busy(7, below), busy(8, below)];
        assert_eq!(held(2_000_000, &looks), [false, false, false, false, false, true]);
    }

    #[test]
    fn a_picture_held_only_to_the_persons_own_choice_is_never_told() {
        // Machine A, a 60 fps video at a 4 Mbit/s Bitrate: the estimate at the choice, the target 9 to 13 % under it.
        let steady: Vec<_> = (0..10).map(|second| (second, 4_000_000.0, 3_600_000)).collect();
        assert!(held(4_000_000, &steady).iter().all(|limited| !limited));
    }

    #[test]
    fn a_picture_that_needs_little_is_not_held_by_an_estimate_it_never_made_grow() {
        // The estimate stays at the share's start, under the allowance, while a still screen sends a fraction of it.
        let still: Vec<_> = (0..10).map(|second| (second, 1_714_285.0, 300_000)).collect();
        assert!(held(4_000_000, &still).iter().all(|limited| !limited));
    }

    #[test]
    fn an_engine_with_no_estimate_yet_is_no_loss() {
        let looks = [(0, 0.0, 0), (5, 0.0, 900_000), (9, 0.0, 900_000)];
        assert!(held(2_000_000, &looks).iter().all(|limited| !limited));
    }
}
