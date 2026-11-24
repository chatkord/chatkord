//! The screen sent from the picker's pick: opened each system's own way, encoded on a thread of its own, handed to the
//! engine over the call's send connection, and said to the model once a second.

use super::system;
use crate::Slot;
use crate::failure;
use crate::room::Paints;
use crate::screen::Preview;
use crate::screen::{self, Fail, Pick, Progress};
use crate::send::sent_look::{Looks, Sending};
use crate::send::share_sound::{self, Heard, Hears, SoundAsked};
use choices::ShareQuality;
use link::Link;
use picture::Painting;
use proto::video::Video;
use screens::{Outgoing, SoundSaid};
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use webrtc::{Feeder, SendStats, Sharing, Voice};
use words::Failure;

/// A screen whose first picture has come and whose encoder is open, not yet sent: the wait for the picture is long,
/// so it is done before the call's engine is asked for anything.
pub struct Begun {
    started: screen::Started,
    selection: Option<::share::Selection>,
    hears: Hears,
    sound_said: Option<SoundSaid>,
}

/// The thread the screen is encoded on, and how it ended.
type Worker = crate::send::worker::Worker<Result<screen::Report, Fail>, Stopped>;

/// How the encoding thread is ended: a flag it reads between pictures.
#[derive(Default)]
struct Stopped(AtomicBool);

impl crate::send::worker::Halt for Stopped {
    fn end(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// A screen being sent.
pub struct Broadcast {
    sharing: Sharing,
    heard: Option<Heard>,
    /// Why the share's sound did not start, where it did not: the picture goes on without it.
    pub sound_refused: Option<Failure>,
    /// What the share's sound is, for the person, where it follows what was picked and did not fail.
    pub sound_said: Option<SoundSaid>,
    worker: Worker,
    progress: Arc<Progress>,
    selection: Option<::share::Selection>,
    /// Where the screen is lent for its card, and whether the card's refusal of it was told.
    preview: Option<Slot>,
    told_unshown: bool,
    /// Whether the sound's stop was told, so it is told once each time it stops.
    told_sound_stopped: bool,
    /// The stream's width, which follows the screen's.
    pub width: u32,
    /// The stream's height, which follows the screen's.
    pub height: u32,
    /// The screen's size, where the stream is scaled down from it.
    pub scaled_from: Option<(u32, u32)>,
    /// The codec the screen is sent in.
    pub codec: choices::Codec,
    /// How the screen is encoded.
    pub encoder: screens::Route,
    /// How the encoder that took over from a failed one codes, until it is told.
    handed_over: Option<screens::Route>,
    /// The most the encoder is allowed, in bits per second, which follows the screen's size.
    allowance: u64,
    /// The counts at the last look, for the rates between looks and the health line.
    looks: Looks,
}

/// A capture being opened on the blocking pool, with what its share hears and how that is said.
pub(super) type Opening = tokio::task::JoinHandle<Result<(screen::Started, (Hears, Option<SoundSaid>)), Fail>>;

/// Opens the capture of `pick` and waits for its first picture; a pick that cannot be opened is closed with the picker.
/// What the share hears is decided beside it, from what was picked and `sound`, and heard once the share is sent.
/// Its encoder is one of those `encoders` lists.
pub async fn begin(
    pick: Pick,
    quality: ShareQuality,
    sound: SoundAsked,
    encoders: Arc<media::Survey>,
) -> Result<Begun, Failure> {
    let (opening, selection) = match pick {
        Pick::Portal(selection) => match system::open(&selection, quality, sound, encoders) {
            Ok(opening) => (opening, Some(selection)),
            Err(failed) => {
                selection.close().await;
                return Err(failed);
            }
        },
        Pick::Frames(frames) => (system::open_frames(frames, quality, sound, encoders), None),
    };
    let failed = match opening.await {
        Ok(Ok((started, (hears, sound_said)))) => return Ok(Begun { started, selection, hears, sound_said }),
        Ok(Err(why)) => failure::screen(&why),
        Err(_) => failure::capture(capture::CaptureError::Stream),
    };
    if let Some(selection) = selection {
        selection.close().await;
    }
    Err(failed)
}

impl Begun {
    /// Gives the picker's permission back without sending anything.
    pub async fn abandon(self) {
        if let Some(selection) = self.selection {
            selection.close().await;
        }
    }

    /// Sends the screen over `voice`'s send connection, encoding on a thread of its own until it is stopped, and lends
    /// what it sends to `preview`, which the window paints on `painting`; the sound chosen goes beside it. It encodes
    /// while `watched` says someone takes it. The send borrows nothing of `voice`.
    pub fn send(
        self,
        voice: &Voice,
        link: Link,
        preview: Option<Slot>,
        painting: Option<Paints>,
        watched: Arc<AtomicBool>,
    ) -> impl Future<Output = Result<Broadcast, Failure>> + Send + use<> {
        let width = self.started.layout.width;
        let height = self.started.layout.height;
        let shared = voice.share(&link, crate::codec::sent(self.started.codec), width, height);
        self.sent(shared, link, preview, painting, watched)
    }

    async fn sent(
        self,
        shared: impl Future<Output = Result<(Sharing, Feeder), webrtc::VoiceRefused>>,
        link: Link,
        preview: Option<Slot>,
        painting: Option<Paints>,
        watched: Arc<AtomicBool>,
    ) -> Result<Broadcast, Failure> {
        let link = &link;
        let Self { started, selection, hears, sound_said } = self;
        let (width, height, scaled_from, codec, encoder, envelope) = (
            started.layout.width,
            started.layout.height,
            started.scaled_from,
            started.codec,
            crate::codec::encoder_route(started.encoder()),
            started.envelope,
        );
        let shared = async {
            let (sharing, feeder) = shared.await.map_err(failure::voice)?;
            sharing.set_rates(envelope.min, envelope.start, envelope.max).map_err(failure::voice)?;
            Ok::<_, Failure>((sharing, feeder))
        };
        let (sharing, feeder) = match shared.await {
            Ok(sent) => sent,
            Err(failed) => {
                if let Some(selection) = selection {
                    selection.close().await;
                }
                return Err(failed);
            }
        };
        let (heard, sound_refused, sound_said) = match share_sound::start(&sharing, link, hears).await {
            Ok(heard) => (heard, None, sound_said),
            Err(failed) => (None, Some(failed), None),
        };
        let progress = Arc::new(Progress::default());
        let worker = {
            let (progress, preview) = (Arc::clone(&progress), preview.clone());
            Worker::spawn(Arc::default(), move |stopped: &Stopped, (): &mut ()| {
                let current = painting.as_ref().map(|paints| || paints.now());
                let lent = preview
                    .as_ref()
                    .map(|slot| Preview { slot, painting: current.as_ref().map(|now| now as &dyn Fn() -> Painting) });
                let (stop, watched) = (|| stopped.0.load(Ordering::Relaxed), || watched.load(Ordering::Relaxed));
                started.run(Some(&feeder), lent.as_ref(), &progress, stop, watched)
            })
        };
        Ok(Broadcast {
            sharing,
            heard,
            sound_refused,
            sound_said,
            worker,
            progress,
            selection,
            preview,
            told_unshown: false,
            told_sound_stopped: false,
            width,
            height,
            scaled_from,
            codec,
            encoder,
            handed_over: None,
            allowance: envelope.max,
            looks: Looks::new(Video::Screen),
        })
    }
}

/// A share whose encoding thread ended by itself: what is left to close.
pub struct Ended {
    sharing: Sharing,
    heard: Option<Heard>,
    selection: Option<::share::Selection>,
}

impl Ended {
    /// Ends the track and gives back the portal's permission.
    pub async fn close(self, link: &Link) {
        close(self.sharing, self.heard, self.selection, link).await;
    }
}

impl Broadcast {
    /// Resolves with why the encoding thread ended without being stopped (the screen went or could not be encoded).
    /// Cancel-safe; once it has resolved, the share is only [`Broadcast::ended`].
    pub async fn finished(&mut self) -> Failure {
        match self.worker.ended().await {
            Some(Ok(_)) => failure::capture(capture::CaptureError::Ended),
            Some(Err(why)) => failure::screen(why),
            None => failure::capture(capture::CaptureError::Stream),
        }
    }

    /// What is left to close of a share whose encoding [`Broadcast::finished`].
    pub fn ended(self) -> Ended {
        let Self { sharing, heard, selection, .. } = self;
        Ended { sharing, heard, selection }
    }

    /// The engine's report a look at this share takes, asked on handles of its own so the call goes on while it is
    /// answered, and which share it is of.
    pub fn look_ask(&self) -> (u64, impl Future<Output = SendStats> + Send + use<>) {
        let ask = self.sharing.stats_ask();
        (self.looks.id(), async move { ask.await.unwrap_or_default() })
    }

    /// How the share is going, from the encoder's counters and `engine`, the answer to [`Broadcast::look_ask`] of
    /// share `of`: the rates are over the time since the last look, and none is told while nobody `watched`. None when `of`
    /// is another share.
    pub fn looked(&mut self, of: u64, engine: SendStats, watched: bool) -> Option<Outgoing> {
        if of != self.looks.id() {
            return None;
        }
        // A screen that changed size: what the person sees and what the engine is told follow it.
        let resized = self.progress.resized.lock().ok().and_then(|mut resized| resized.take());
        if let Some(screen::Resized { width, height, scaled_from, envelope }) = resized {
            (self.width, self.height, self.scaled_from) = (width, height, scaled_from);
            // A refusal keeps the bounds the share started with, inside which the encoder's own envelope still holds
            // it.
            self.allowance = envelope.max;
            let _ = self.sharing.bound_rates(envelope.min, envelope.max);
        }
        let handed_over = self.progress.handed_over.lock().ok().and_then(|mut handed| handed.take());
        if let Some(route) = handed_over {
            self.encoder = route;
            self.handed_over = Some(self.encoder.clone());
        }
        let sending = Sending {
            codec: self.codec,
            encoder: &self.encoder,
            width: self.width,
            height: self.height,
            scaled_from: self.scaled_from,
            allowance: self.allowance,
        };
        Some(self.looks.look(&sending, &self.progress, &engine, watched, Instant::now()))
    }

    /// How the encoder that took over from a failed one codes, once, since the last look found it.
    pub fn handed_over(&mut self) -> Option<screens::Route> {
        self.handed_over.take()
    }

    /// Whether the card was found, since the last ask and for the first time, unable to show the screen.
    pub fn first_unshown(&mut self) -> bool {
        let first = !self.told_unshown && self.preview.as_ref().is_some_and(|preview| !preview.takes_imports());
        self.told_unshown |= first;
        first
    }

    /// Hears this application from now on: for a window whose owner could not be found, picked by the sharer.
    #[cfg(target_os = "linux")]
    pub fn hear(&mut self, app: ::sound::share::App) {
        if let Some(heard) = &self.heard {
            self.sound_said = Some(screens::SoundSaid::App(app.name.clone()));
            heard.choose(::sound::share::Hears::Apps([app].into()));
        }
    }

    /// Whether the share's sound was found, since the last ask, stopped past its wait: told once each time it stops.
    pub fn first_sound_stopped(&mut self) -> bool {
        let stopped = self.heard.as_ref().is_some_and(Heard::stopped);
        let first = stopped && !self.told_sound_stopped;
        self.told_sound_stopped = stopped;
        first
    }

    /// Ends the share: the encoding thread, waited for at most `worker::STOP_BOUND`, the track and the
    /// portal's permission.
    pub async fn stop(self, link: &Link) {
        let _ = self.worker.end().await;
        close(self.sharing, self.heard, self.selection, link).await;
    }
}

async fn close(sharing: Sharing, heard: Option<Heard>, selection: Option<::share::Selection>, link: &Link) {
    drop(heard);
    let _ = sharing.stop(link).await;
    if let Some(selection) = selection {
        selection.close().await;
    }
}
