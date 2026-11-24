//! One member's picture, a shared screen or a camera, shown from inside the call over its own receive transport: each
//! encoded frame goes to a decoder thread, which puts the newest picture in the window's slot.

use super::log::{WatchedLog, WatchedLook};
use super::receive::{Intake, Pipeline};
use crate::lines::Coded;
use link::Link;
use picture::{Painting, Slot};
use proto::chatkord_v1::TrackKind;
use proto::video::Video;
use screens::{ShareCard, ShareState};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;
use terms::MemberId;
use webrtc::{Hearing, KeyFrames, Reach};
use words::Failure;

/// How long a watched screen goes unseen before its stream is held (the maintainer's call, 2026-11-09).
pub const UNSEEN: Duration = Duration::from_secs(10);

/// Whose picture of which kind: what a call watches, shows and holds.
pub(crate) type Viewed = (MemberId, Video);

/// One picture sent in the call, decoded here: the decoder and what it counts. What the call decides of it, whether the
/// window shows it and when its sender is asked for a key frame, is the call's state.
pub(crate) struct Viewing {
    pub user_id: MemberId,
    pub video: Video,
    pipeline: Arc<Pipeline>,
    pub(crate) slot: Slot,
    /// How its sender is asked for a key frame.
    key_frames: KeyFrames,
    /// The pictures decoded at the last look, for the rate over the next.
    decoded: u64,
    /// How many key frames this viewer asked for.
    asks: u64,
    /// Which screen this call watched it as, for its health line.
    number: u64,
    log: WatchedLog,
    hidden: Hidden,
    /// Whether it was taken held, the person having paused it.
    pub(crate) started_held: bool,
}

/// What a decoder says of its stream, for the call to decide whether to ask its sender for a key frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Waiting {
    /// The stream waits for a key frame: before its first, and after a frame was lost or did not decode.
    pub(crate) awaiting_key: bool,
    /// This computer cannot keep up with the stream.
    pub(crate) behind: bool,
}

/// Holds `viewed`'s stream at the server, and a screen's sound with it: a viewer who has paused the picture hears no
/// more of it. Whether the picture is held.
pub(crate) async fn hold(link: &Link, (user_id, video): &Viewed) -> bool {
    let held = webrtc::pause(link, user_id.as_str(), video.kind()).await;
    if held && *video == Video::Screen {
        // A screen with no sound has no consumer to hold.
        webrtc::pause(link, user_id.as_str(), TrackKind::ScreenAudio).await;
    }
    held
}

/// Starts `viewed`'s held stream again, and a screen's sound with it. Whether the picture is started.
pub(crate) async fn unhold(link: &Link, (user_id, video): &Viewed) -> bool {
    let started = webrtc::resume(link, user_id.as_str(), video.kind()).await;
    if *video == Video::Screen {
        webrtc::resume(link, user_id.as_str(), TrackKind::ScreenAudio).await;
    }
    started
}

/// The pictures replaced unpainted while the window showed the screen nowhere, which no paint was owed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Hidden {
    /// The slot's count of replaced pictures when the card was last hidden; none while it is shown.
    since: Option<u64>,
    /// Those replaced while it was hidden before.
    before: u64,
}

impl Hidden {
    /// The card is shown or not, the slot having replaced `dropped` pictures so far.
    fn shown(&mut self, shown: bool, dropped: u64) {
        match (shown, self.since) {
            (false, None) => self.since = Some(dropped),
            (true, Some(since)) => {
                self.before += dropped.saturating_sub(since);
                self.since = None;
            }
            (false, Some(_)) | (true, None) => {}
        }
    }

    /// Of the `dropped` the slot replaced, those the window owed a paint: replaced while the card was shown.
    fn unpainted(self, dropped: u64) -> u64 {
        let now = self.since.map_or(0, |since| dropped.saturating_sub(since));
        dropped.saturating_sub(self.before + now)
    }
}

impl Viewing {
    /// Starts decoding `user_id`'s picture of `video` into `slot`, on `painting`: the GPU the window paints with, so
    /// its pictures can be imported where they are. It starts held when `paused`: its stream is never resumed.
    pub(crate) async fn start(
        hearing: &mut Hearing,
        link: &Link,
        (user_id, video): Viewed,
        number: u64,
        slot: Slot,
        painting: Painting,
        paused: bool,
    ) -> Result<Self, Failure> {
        // A picture starts from none, whatever an earlier one left.
        slot.empty();
        let intake = Intake::new();
        let feeding = intake.clone();
        let sink = move |frame: webrtc::Encoded<'_>| feeding.feed(frame.data, frame.render_time_us, frame.key_frame);
        let start = if paused { webrtc::Start::Held } else { webrtc::Start::Playing };
        let key_frames = hearing
            .watch_encoded_from(link, user_id.as_str(), video, start, sink)
            .await
            .map_err(crate::failure::watch)?;
        // The codec is known only once the screen is taken; until its decoder opens, the intake holds what arrives.
        let (codec, into) = (crate::codec::arrived(key_frames.codec()), slot.clone());
        let opened = tokio::task::spawn_blocking(move || Pipeline::start(&intake, codec, &painting, into)).await;
        let Some(pipeline) = opened.ok().and_then(Result::ok).map(Arc::new) else {
            hearing.release(link, user_id.as_str(), video.kind()).await;
            return Err(Failure::CannotDecode);
        };
        if paused && video == Video::Screen {
            // The sound of a screen has a consumer of its own, which plays unless held too; a screen with no
            // sound has none to hold.
            webrtc::pause(link, user_id.as_str(), TrackKind::ScreenAudio).await;
        }
        if paused {
            // Taken held, nothing arrives to decode until the person resumes it.
            pipeline.hold();
        }
        Ok(Self {
            user_id,
            video,
            pipeline,
            slot,
            key_frames,
            decoded: 0,
            asks: 0,
            number,
            log: WatchedLog::default(),
            hidden: Hidden::default(),
            started_held: paused,
        })
    }

    /// The window shows it (`shown`) or not.
    pub(crate) fn shown(&mut self, shown: bool) {
        self.hidden.shown(shown, self.slot.stats().dropped);
    }

    /// Its stream is held at the server: its decoder is closed, so it holds nothing on the GPU.
    pub(crate) fn close(&self) {
        self.pipeline.hold();
    }

    /// Its held stream is to be started again: a decoder opens again, told by [`Viewing::reopened`], and the gap the
    /// hold leaves is no freeze.
    pub(crate) fn resumed(&self) {
        self.pipeline.resume();
    }

    /// The card of the screen now, with the pictures a second over the last look, `paused` as the person has it;
    /// reconnecting while the receive connection, which carries it, is not up. `taken` is the engine's last word on it.
    pub(crate) fn card(
        &mut self,
        every: Duration,
        reach: Reach,
        paused: bool,
        taken: Option<&webrtc::WatchStats>,
    ) -> Option<ShareCard> {
        let now = self.pipeline.report();
        let seconds = every.as_secs_f64();
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a count over positive seconds; a float cast saturates"
        )]
        let rate = (seconds > 0.0).then(|| (now.decoded.saturating_sub(self.decoded) as f64 / seconds).round() as u32);
        self.decoded = now.decoded;
        let state = state(reach, now.behind, paused);
        let look = WatchedLook {
            coded: Coded::of(now.codec, &crate::codec::route(now.path)),
            frame_rate: rate,
            key_frames_asked: self.asks,
            failed: now.failed,
            dropped: now.dropped,
            freezes: (now.freezes, now.longest_freeze),
            painted_gap: self.slot.take_painted_gap(),
            held: taken.map_or((0.0, 0), |taken| (taken.held_seconds, taken.held)),
        };
        if let Some(line) = self.log.look(self.number, look) {
            line.log();
        }
        now.card(&self.user_id, rate.filter(|rate| *rate > 0), self.hidden.unpainted(self.slot.stats().dropped), state)
    }

    /// Resolves with whose first picture was decoded and whose card `reported` not yet; never, while none waits.
    pub(crate) async fn first_picture(viewings: &HashMap<Viewed, Self>, reported: impl Fn(&Viewed) -> bool) -> Viewed {
        let waits = viewings.iter().filter(|(viewed, _)| !reported(viewed));
        first_of(waits.map(|(viewed, viewing)| (viewed, Box::pin(viewing.pipeline.first_picture())))).await
    }

    /// Resolves with whose held stream's decoder opened again, or did not; never, while none opens.
    pub(crate) async fn reopened(viewings: &HashMap<Viewed, Self>) -> Viewed {
        first_of(viewings.iter().map(|(viewed, viewing)| (viewed, Box::pin(viewing.pipeline.reopened())))).await
    }

    /// Its card at its first picture, so the window shows it then and not at the next look; no rate yet.
    pub(crate) fn first_card(&self, reach: Reach, paused: bool) -> Option<ShareCard> {
        let now = self.pipeline.report();
        let state = state(reach, now.behind, paused);
        now.card(&self.user_id, None, self.hidden.unpainted(self.slot.stats().dropped), state)
    }

    /// The way it decodes now, once its decoder broke after showing pictures and handed over: told once.
    pub(crate) fn handed_over(&self) -> Option<screens::Route> {
        self.pipeline.handed_over().map(crate::codec::route)
    }

    /// Whether its decoder did not open again after a hold, once.
    pub(crate) fn unopened(&self) -> bool {
        self.pipeline.unopened()
    }

    /// Resolves with whose decoder needs its sender to send a key frame; never, while nothing is decoded.
    pub(crate) async fn needs_key_frame(viewings: &HashMap<Viewed, Self>) -> Viewed {
        first_of(viewings.iter().map(|(viewed, viewing)| (viewed, Box::pin(viewing.pipeline.wants_key_frame())))).await
    }

    /// What its decoder says of its stream now.
    pub(crate) fn waiting(&self) -> Waiting {
        Waiting { awaiting_key: self.pipeline.awaiting_key(), behind: self.pipeline.behind() }
    }

    /// Asks its sender for a key frame.
    pub(crate) fn ask_key_frame(&mut self) {
        self.asks += 1;
        self.key_frames.ask();
    }
}

/// Resolves with whose of `waits` resolved first; never, while there is none.
async fn first_of<W: Future>(waits: impl Iterator<Item = (&Viewed, Pin<Box<W>>)>) -> Viewed {
    let mut waits: Vec<_> = waits.collect();
    std::future::poll_fn(|cx| {
        for (viewed, wait) in &mut waits {
            if wait.as_mut().poll(cx).is_ready() {
                return Poll::Ready((*viewed).clone());
            }
        }
        Poll::Pending
    })
    .await
}

/// What a card says of its stream: that the person paused it, else of the connection that carries it, and, while that is
/// up, whether this computer keeps up with it.
fn state(reach: Reach, behind: bool, paused: bool) -> ShareState {
    if paused {
        return ShareState::Paused;
    }
    match reach {
        // A picture that arrives says more of the route than a connection the engine has not called up yet.
        Reach::Up | Reach::Starting if behind => ShareState::Behind,
        Reach::Up | Reach::Starting => ShareState::Live,
        Reach::Down | Reach::Lost => ShareState::Reconnecting,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_replaced_while_the_card_is_hidden_are_not_counted_unpainted() {
        let mut hidden = Hidden::default();
        hidden.shown(true, 0);
        assert_eq!(hidden.unpainted(3), 3, "shown: each replaced picture was owed a paint");
        hidden.shown(false, 3);
        assert_eq!(hidden.unpainted(603), 3, "hidden 10 s at 60 a second: none of those");
        hidden.shown(false, 603);
        hidden.shown(true, 607);
        assert_eq!(hidden.unpainted(607), 3, "shown again: the hidden ones stay out");
        assert_eq!(hidden.unpainted(610), 6, "and the new ones count");
    }
}
