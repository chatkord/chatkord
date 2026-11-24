//! One's own camera in the call: opened and read on a thread of its own, and encoded by the loop a share's pictures are
//! (`screen::Started::run`), on the first encoder that takes them, sent as its own track at its own rates. Nothing is
//! decoded, converted or shown for a camera nobody watches whose card is hidden. A preview opens it the same way and
//! only puts its pictures in one's own camera slot.

use crate::failure;
use crate::screen::{Fail, Ladder, Move, Prepared, Preview as Lent, Progress, Stages, first_open};
use crate::send::sent_look::{Looks, Sending};
use crate::send::source::{Source, hand_over};
use capture::camera::{Camera, CameraError, Device as Listed, Mode, Stopper};
use choices::{CameraAsk, Resolution};
use link::Link;
use media::camera::{Decoder, NotReady, Picture, Ready, Route as Way, ToNv12};
use media::{Changes, EncodeError, Encoded, Reopening, Session, Settings};
use net::{Envelope, Ladder as Steps, Size, Step};
use picture::Slot;
use proto::video::Video;
use screens::{Outgoing, Route};
use std::collections::VecDeque;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use webrtc::{Feeder, SendStats, SentVideo, Voice};
use words::Failure;

/// A camera is sent in H.264 whatever a share is: every GPU here decodes it in hardware, and each viewer decodes every
/// camera in the call at once.
const CODEC: media::Codec = media::Codec::H264;

/// How long the loop waits for a picture at a time, so a stop is noticed while the camera is silent.
const POLL: Duration = Duration::from_millis(100);

/// A camera's pictures, as the call reads them: a device's, or a test's own.
pub trait Pictures {
    /// The mode it gives pictures at.
    fn mode(&self) -> Mode;
    /// The next picture and when it arrived, waiting for it.
    fn read(&mut self) -> Result<(Picture, Instant), CameraError>;
    /// The next picture's arrival alone, waiting for it, for a picture nobody takes: a device skips what it would
    /// have decoded.
    fn skip(&mut self) -> Result<Instant, CameraError> {
        self.read().map(|(_, at)| at)
    }
    /// What ends a read under way from another thread; none where a read always returns.
    fn stopper(&self) -> Option<Stopper> {
        None
    }
}

/// A camera of this device and the decoder that makes pictures of what it hands over.
struct Device {
    camera: Camera,
    decoder: Decoder,
}

impl Pictures for Device {
    fn mode(&self) -> Mode {
        self.camera.mode()
    }

    fn read(&mut self) -> Result<(Picture, Instant), CameraError> {
        let (shot, at) = self.camera.read()?;
        let decoded = self.decoder.decode(&shot);
        self.camera.recycle(shot);
        Ok((Picture::Decoded(decoded?), at))
    }

    fn skip(&mut self) -> Result<Instant, CameraError> {
        let (shot, at) = self.camera.read()?;
        self.camera.recycle(shot);
        Ok(at)
    }

    fn stopper(&self) -> Option<Stopper> {
        Some(self.camera.stopper())
    }
}

/// A camera to send: one the person chose, opened on the thread that reads it, or a test's own pictures.
pub enum Pick {
    /// A camera of this device.
    Device(CameraAsk),
    /// A test's own pictures.
    Pictures(Box<dyn Pictures + Send>),
}

impl std::fmt::Debug for Pick {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Device(ask) => f.debug_tuple("Pick::Device").field(ask).finish(),
            Self::Pictures(_) => f.write_str("Pick::Pictures"),
        }
    }
}

/// A camera opened and its encoder, waiting to be sent.
pub(crate) struct Begun {
    /// The mode the camera captures at.
    mode: Mode,
    /// How it is encoded.
    encoder: Route,
    envelope: Envelope,
    worker: Worker,
    progress: Arc<Progress>,
}

/// One's camera being sent.
pub(crate) struct Live {
    sent: SentVideo,
    worker: Worker,
    /// The mode the camera captures at.
    mode: Mode,
    /// The stream's size now, which a step down changes.
    size: (u32, u32),
    scaled_from: Option<(u32, u32)>,
    encoder: Route,
    progress: Arc<Progress>,
    looks: Looks,
    allowance: u64,
}

/// The thread that runs the camera's loop, and how it ended.
type Worker = crate::send::worker::Worker<Result<(), Failure>, Stops>;

/// How a worker is ended: the flag it and the reader read between pictures, the camera's stopper once open, so ending
/// it never waits on a device that stopped answering, and the feeder's way in, so ending it ends its wait for one too.
#[derive(Default)]
struct Stops {
    asked: Arc<AtomicBool>,
    camera: OnceLock<Stopper>,
    feed: Mutex<Option<oneshot::Sender<Feeder>>>,
}

impl Stops {
    fn asked(&self) -> bool {
        self.asked.load(Ordering::Relaxed)
    }

    /// Keeps the camera's `stopper`, and stops it at once if the end was asked meanwhile.
    fn opened(&self, stopper: Option<Stopper>) {
        if let Some(stopper) = stopper {
            let _ = self.camera.set(stopper);
            if self.asked() {
                crate::send::worker::Halt::end(self);
            }
        }
    }

    /// Gives the worker its feeder; false when it was ended or fed before, or has stopped.
    fn fed(&self, feeder: Feeder) -> bool {
        let feed = self.feed().take();
        feed.is_some_and(|feed| feed.send(feeder).is_ok())
    }

    fn feed(&self) -> MutexGuard<'_, Option<oneshot::Sender<Feeder>>> {
        self.feed.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl crate::send::worker::Halt for Stops {
    fn end(&self) {
        self.asked.store(true, Ordering::Relaxed);
        drop(self.feed().take());
        if let Some(camera) = self.camera.get() {
            camera.stop();
        }
    }
}

/// Opens the camera `pick` names and its encoder, one of those `survey` lists; whether anyone watches is read from
/// `watched` once it is sent, and its pictures are put in `own` while that window is shown.
pub(crate) async fn begin(
    pick: Pick,
    own: Option<Slot>,
    watched: Arc<AtomicBool>,
    survey: Arc<media::Survey>,
) -> Result<Begun, Failure> {
    let (opened, told) = oneshot::channel();
    let (feed, fed) = oneshot::channel();
    let progress = Arc::new(Progress::default());
    let counted = Arc::clone(&progress);
    let halt = Stops { feed: Mutex::new(Some(feed)), ..Stops::default() };
    let worker = Worker::spawn(Arc::new(halt), move |halt, (): &mut ()| {
        work(pick, own, &watched, &survey, halt, Counted { opened, fed, progress: counted })
    });
    match told.await {
        Ok((mode, encoder, envelope)) => Ok(Begun { mode, encoder, envelope, worker, progress }),
        Err(_) => Err(worker.end().await.and_then(Result::err).unwrap_or(Failure::EncodeStopped)),
    }
}

impl Begun {
    /// Sends it over the call's voice at its own rates; the worker encodes from then on.
    pub(crate) fn send(self, voice: &Voice, link: Link) -> impl Future<Output = Result<Live, Failure>> + Send + use<> {
        let started = voice.camera(&link, webrtc::Codec::H264, self.mode.width, self.mode.height);
        async move {
            let sent = match started.await {
                Ok((sent, feeder)) => {
                    let rated = sent.set_rates(self.envelope.min, self.envelope.start, self.envelope.max);
                    match rated {
                        Ok(()) if self.worker.halt().fed(feeder) => Ok(sent),
                        Ok(()) => Err((sent, Failure::EncodeStopped)),
                        Err(refused) => Err((sent, failure::voice(refused))),
                    }
                }
                Err(refused) => {
                    let _ = self.worker.end().await;
                    return Err(failure::voice(refused));
                }
            };
            match sent {
                Ok(sent) => Ok(Live {
                    sent,
                    worker: self.worker,
                    mode: self.mode,
                    size: (self.mode.width, self.mode.height),
                    scaled_from: None,
                    encoder: self.encoder,
                    progress: self.progress,
                    looks: Looks::new(Video::Camera),
                    allowance: self.envelope.max,
                }),
                Err((sent, failed)) => {
                    let _ = self.worker.end().await;
                    let _ = sent.stop(&link).await;
                    Err(failed)
                }
            }
        }
    }

    /// Lets go of it unsent.
    pub(crate) async fn abandon(self) {
        let _ = self.worker.end().await;
    }
}

impl Live {
    /// The mode the camera captures at.
    pub(crate) fn mode(&self) -> Mode {
        self.mode
    }

    /// How it is encoded.
    pub(crate) fn encoder(&self) -> &Route {
        &self.encoder
    }

    /// Resolves with why it ended by itself, the camera gone or the encoder failed; never while it runs. Once it has,
    /// only [`Live::close`] lets go of what is left.
    pub(crate) async fn ended(&mut self) -> Failure {
        self.worker.ended().await.and_then(|ended| ended.clone().err()).unwrap_or(Failure::EncodeStopped)
    }

    /// Ends its track at the server, once [`Live::ended`] said it ended by itself.
    pub(crate) async fn close(self, link: &Link) {
        let _ = self.sent.stop(link).await;
    }

    /// Ends it, at the server too.
    pub(crate) async fn stop(self, link: &Link) {
        let _ = self.worker.end().await;
        let _ = self.sent.stop(link).await;
    }

    /// The engine's report a look at this camera takes, asked on handles of its own so the call goes on while it is
    /// answered, and which camera it is of.
    pub(crate) fn look_ask(&self) -> (u64, impl Future<Output = SendStats> + Send + use<>) {
        let ask = self.sent.stats_ask();
        (self.looks.id(), async move { ask.await.unwrap_or_default() })
    }

    /// How it is going, from the encoder's counters and `engine`, the answer to [`Live::look_ask`] of camera `of`: the
    /// rates are over the time since the last look, and none is told while nobody `watched`. None when `of` is another
    /// camera.
    pub(crate) fn looked(&mut self, of: u64, engine: SendStats, watched: bool) -> Option<Outgoing> {
        if of != self.looks.id() {
            return None;
        }
        // A step down or up: what the person sees and what the engine is told follow it.
        let resized = self.progress.resized.lock().ok().and_then(|mut resized| resized.take());
        if let Some(crate::screen::Resized { width, height, scaled_from, envelope }) = resized {
            (self.size, self.scaled_from) = ((width, height), scaled_from);
            self.allowance = envelope.max;
            let _ = self.sent.bound_rates(envelope.min, envelope.max);
        }
        if let Some(route) = self.progress.handed_over.lock().unwrap_or_else(PoisonError::into_inner).take() {
            self.encoder = route;
        }
        let sending = Sending {
            codec: choices::Codec::H264,
            encoder: &self.encoder,
            width: self.size.0,
            height: self.size.1,
            scaled_from: self.scaled_from,
            allowance: self.allowance,
        };
        Some(self.looks.look(&sending, &self.progress, &engine, watched, Instant::now()))
    }
}

/// A camera opened for a look at it alone: its pictures go to one's own camera slot until it ends, and nothing is
/// encoded or sent.
pub struct Preview {
    worker: Worker,
    own: Slot,
}

/// Opens the camera `pick` names and puts its pictures in `own`; resolves once it is open, or with why it would not.
pub async fn preview(pick: Pick, own: Slot) -> Result<Preview, Failure> {
    let (opened, told) = oneshot::channel();
    let slot = own.clone();
    let worker = Worker::spawn(Arc::new(Stops::default()), move |halt, held: &mut Option<Box<dyn Pictures>>| {
        let camera = held.insert(open(pick)?);
        halt.opened(camera.stopper());
        if opened.send(()).is_err() {
            return Ok(());
        }
        let mut to_nv12 = ToNv12::default();
        while !halt.asked() {
            // A hidden window is shown nothing: the picture is read and dropped.
            let read = if slot.is_shown() { camera.read().map(Some) } else { camera.skip().map(|_| None) };
            // A stop asked ends the read too, as a camera gone: it is no failure.
            if halt.asked() {
                break;
            }
            if let Some((picture, _)) = read.map_err(failure::camera)? {
                let nv12 = to_nv12.nv12(&picture).map_err(failure::camera)?;
                let _ = slot.put_nv12(sides(&nv12), nv12.luma(), nv12.chroma(), nv12.colours());
            }
        }
        Ok(())
    });
    match told.await {
        Ok(()) => Ok(Preview { worker, own }),
        Err(_) => {
            let failed = worker.end().await.and_then(Result::err).unwrap_or(Failure::CameraGone);
            own.empty();
            Err(failed)
        }
    }
}

impl Preview {
    /// Resolves with why it ended by itself, the camera gone or taken; never while it runs.
    pub async fn ended(&mut self) -> Failure {
        self.worker.ended().await.and_then(|ended| ended.clone().err()).unwrap_or(Failure::CameraGone)
    }

    /// Ends it and lets the camera go, leaving the slot empty.
    pub async fn stop(self) {
        let _ = self.worker.end().await;
        self.own.empty();
    }
}

/// `nv12`'s pixels across and down.
fn sides(nv12: &media::Nv12) -> (u32, u32) {
    let side = |length: usize| u32::try_from(length).unwrap_or(u32::MAX);
    (side(nv12.width()), side(nv12.height()))
}

/// The cameras this system lists, read once for whoever asks: the settings page's list and an open's choice.
pub(crate) fn listed() -> Result<Vec<Listed>, Failure> {
    capture::camera::list().map_err(failure::camera)
}

/// The camera `pick` names, opened on this thread: the one chosen, else the first the system lists. A chosen camera
/// that is not plugged in is gone, never swapped for another.
fn open(pick: Pick) -> Result<Box<dyn Pictures>, Failure> {
    Ok(match pick {
        Pick::Device(ask) => {
            let asked = Mode { width: ask.width, height: ask.height, frames_per_second: ask.frames_per_second };
            let mut listed = listed()?.into_iter();
            let device = match ask.device {
                Some(chosen) => listed.find(|camera| camera.id == chosen),
                None => listed.next(),
            };
            let device = device.ok_or(Failure::CameraGone)?;
            Box::new(Device {
                camera: Camera::open(&device.id, asked).map_err(failure::camera)?,
                decoder: Decoder::default(),
            })
        }
        Pick::Pictures(pictures) => pictures,
    })
}

/// What a worker tells its call: how it opened, once; and what it has encoded, as it goes.
struct Counted {
    opened: oneshot::Sender<(Mode, Route, Envelope)>,
    fed: oneshot::Receiver<Feeder>,
    progress: Arc<Progress>,
}

/// One picture of the camera, and when it arrived.
pub struct Taken {
    picture: Picture,
    at: Instant,
}

/// Whether anyone takes the camera's pictures: those it is sent to, until the first is in hand; or one's own card, while
/// its window is shown.
struct Needed {
    watched: Arc<AtomicBool>,
    own: Option<Slot>,
    /// Set once the route is chosen, from the first picture, which must be whole.
    settled: AtomicBool,
}

impl Needed {
    fn is(&self) -> bool {
        !self.settled.load(Ordering::Relaxed)
            || self.watched.load(Ordering::Relaxed)
            || self.own.as_ref().is_some_and(Slot::is_shown)
    }
}

/// The newest picture the reader made and what it counted, which the loop takes.
#[derive(Default)]
struct Mailbox {
    held: Mutex<Held>,
    arrived: Condvar,
}

#[derive(Default)]
struct Held {
    /// How the camera opened on the reader's thread: its mode and what stops its read, or why it would not.
    opened: Option<Result<(Mode, Option<Stopper>), Failure>>,
    picture: Option<Taken>,
    /// Why the camera stopped giving pictures.
    ended: Option<CameraError>,
    /// Pictures the camera gave, those nobody took included.
    delivered: u64,
    /// Pictures the next replaced before the loop took them.
    dropped: u64,
    /// The widest gap between two pictures since it was last taken.
    widest_gap: Duration,
    last_at: Option<Instant>,
}

impl Mailbox {
    fn held(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The thread that opens and reads the camera, keeping only its newest picture, so the loop waits for one with a bound
/// and a camera that blocks never holds it up. Dropping it ends the thread once its read returns, which the camera's
/// stopper ends at once.
struct Reader {
    mailbox: Arc<Mailbox>,
    asked: Arc<AtomicBool>,
}

impl Reader {
    /// Opens `pick` and reads it until `asked`, or until it fails; each picture as `needed` says: whole, or only read
    /// and let go. The camera is opened on the thread that reads it, as a system may need.
    fn start(pick: Pick, needed: Arc<Needed>, asked: Arc<AtomicBool>) -> Self {
        let mailbox = Arc::new(Mailbox::default());
        let (posted, stopping) = (Arc::clone(&mailbox), Arc::clone(&asked));
        let read = move || {
            let mut pictures = match open(pick) {
                Ok(pictures) => pictures,
                Err(why) => {
                    posted.held().opened = Some(Err(why));
                    posted.arrived.notify_all();
                    return;
                }
            };
            posted.held().opened = Some(Ok((pictures.mode(), pictures.stopper())));
            posted.arrived.notify_all();
            while !stopping.load(Ordering::Relaxed) {
                let read = if needed.is() {
                    pictures.read().map(|(picture, at)| (Some(picture), at))
                } else {
                    pictures.skip().map(|at| (None, at))
                };
                let mut held = posted.held();
                match read {
                    Ok((picture, at)) => {
                        held.delivered += 1;
                        if let Some(last) = held.last_at.replace(at) {
                            held.widest_gap = held.widest_gap.max(at.saturating_duration_since(last));
                        }
                        if let Some(picture) = picture
                            && held.picture.replace(Taken { picture, at }).is_some()
                        {
                            held.dropped += 1;
                        }
                    }
                    Err(why) => {
                        held.ended = Some(why);
                        drop(held);
                        posted.arrived.notify_all();
                        return;
                    }
                }
                drop(held);
                posted.arrived.notify_all();
            }
        };
        // A thread of its own: a camera whose release hangs holds nothing else up.
        if std::thread::Builder::new().name("camera".into()).spawn(read).is_err() {
            mailbox.held().opened = Some(Err(Failure::CameraUnavailable));
        }
        Self { mailbox, asked }
    }

    /// The camera's mode and what stops its read, once it is open; why it would not open; none when the stop was asked
    /// first.
    fn opened(&self) -> Result<Option<(Mode, Option<Stopper>)>, Failure> {
        let mut held = self.mailbox.held();
        loop {
            if let Some(opened) = held.opened.take() {
                return opened.map(Some);
            }
            if self.asked.load(Ordering::Relaxed) {
                return Ok(None);
            }
            held = self.mailbox.arrived.wait_timeout(held, POLL).unwrap_or_else(PoisonError::into_inner).0;
        }
    }

    /// The newest picture, waiting up to `wait` for one; none when none came, or the stop was asked, and the camera's
    /// own reason where it stopped giving pictures.
    fn next(&self, wait: Duration) -> Result<Option<Taken>, CameraError> {
        let mut held = self.mailbox.held();
        if held.picture.is_none() && held.ended.is_none() && !self.asked.load(Ordering::Relaxed) {
            held = self.mailbox.arrived.wait_timeout(held, wait).unwrap_or_else(PoisonError::into_inner).0;
        }
        if let Some(taken) = held.picture.take() {
            return Ok(Some(taken));
        }
        match held.ended {
            Some(why) if !self.asked.load(Ordering::Relaxed) => Err(why),
            Some(_) | None => Ok(None),
        }
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.asked.store(true, Ordering::Relaxed);
    }
}

/// The camera as a source: what its reader made, how the pictures are made ready for the encoder and one's own card,
/// and the fallbacks of a route that lifts them onto the encoder's device.
pub struct Cameras {
    reader: Reader,
    route: Way,
    own: Option<Slot>,
    progress: Arc<Progress>,
    /// What a new stream made between two encodes left: the old stream's last units and what changed.
    rebuilt: Option<Reopening>,
}

/// `then` after `first`: each change `then` made, else the one before it; both sets of units, in order.
fn merged(first: Reopening, then: Reopening) -> Reopening {
    let (before, after) = (first.changes, then.changes);
    let changes = Changes {
        bits_per_second: after.bits_per_second.or(before.bits_per_second),
        frames_per_second: after.frames_per_second.or(before.frames_per_second),
        size: after.size.or(before.size),
        encoder: after.encoder.or(before.encoder),
    };
    Reopening { held: first.held.into_iter().chain(then.held).collect(), changes, in_place: false }
}

impl Cameras {
    /// Gives one's own card `ready`, unless its window is hidden.
    fn show(&self, ready: &Ready<'_>) {
        let Some(own) = self.own.as_ref().filter(|own| own.is_shown()) else { return };
        match ready {
            Ready::Nv12(nv12) => {
                let _ = own.put_nv12(sides(nv12), nv12.luma(), nv12.chroma(), nv12.colours());
            }
            #[cfg(target_os = "linux")]
            Ready::Lifted(lifted) => {
                if let Some(surface) = lifted.surface() {
                    own.put_surface(surface);
                }
            }
        }
    }

    /// `picture` made ready, counted; a device that will not lift it leaves the route to memory for the rest of the
    /// call, on the encoder that runs, and its old stream's last units come first.
    fn ready<'p>(&mut self, session: &mut Session, picture: &'p Picture) -> Result<(Ready<'p>, Vec<Encoded>), Fail> {
        let made = match self.route.ready(picture) {
            Ok(ready) => (ready, Vec::new()),
            Err(NotReady::Camera(why)) => return Err(Fail::Camera(why)),
            Err(NotReady::Lift(why)) => {
                report::note!(warn, "the camera's pictures are no longer lifted; they go through memory", why = why);
                let held = self.through_memory(session)?;
                let ready = self.route.ready(picture).map_err(|_| Fail::Camera(CameraError::Format))?;
                (ready, held)
            }
        };
        self.progress.converts.fetch_add(1, Ordering::Relaxed);
        Ok(made)
    }

    /// The route goes through memory from now on, a new stream of the running encoder at the size and rate it ran at;
    /// the old stream's last units.
    fn through_memory(&mut self, session: &mut Session) -> Result<Vec<Encoded>, EncodeError> {
        let (width, height, encoder) = session.running();
        let (size, encoder) = ((width, height), encoder.clone());
        let rate = session.rate();
        let settings = Settings {
            width: size.0,
            height: size.1,
            frames_per_second: rate.frames_per_second,
            bits_per_second: rate.bits_per_second,
            key_frame_every: None,
        };
        let opened = Way::memory_session(&encoder, settings)?;
        let old = std::mem::replace(session, opened);
        self.route.leave_lift();
        Ok(old.finish().unwrap_or_default())
    }
}

impl Source for Cameras {
    type Picture = Taken;
    type Shape = (u32, u32);
    type Lending = ();

    fn next(&mut self, wait: Duration) -> Result<Option<Taken>, Fail> {
        Ok(self.reader.next(wait)?)
    }

    fn shape(picture: &Taken) -> (u32, u32) {
        let (width, height) = picture.picture.size();
        (u32::try_from(width).unwrap_or(u32::MAX), u32::try_from(height).unwrap_or(u32::MAX))
    }

    fn captured(picture: &Taken) -> Instant {
        picture.at
    }

    fn count_into(&self, stages: &Stages) {
        let mut held = self.reader.mailbox.held();
        stages.slot(held.delivered, held.dropped);
        stages.capture_gap(std::mem::take(&mut held.widest_gap));
    }

    fn envelope((width, height): (u32, u32), frames_per_second: u32, ceiling: Option<u64>) -> Envelope {
        Envelope::for_camera(width, height, frames_per_second, ceiling)
    }

    fn lend(&mut self, (): &mut (), _: Option<&Lent<'_>>, picture: &Taken, _: &Session, _: u32) {
        // Nothing encodes it: it is made ready for one's own card alone, and not at all where that window is hidden.
        if self.own.as_ref().is_some_and(Slot::is_shown)
            && let Ok(ready) = self.route.ready(&picture.picture)
        {
            self.progress.converts.fetch_add(1, Ordering::Relaxed);
            self.show(&ready);
        }
    }

    fn encode(
        &mut self,
        (): &mut (),
        session: &mut Session,
        _: Option<&Lent<'_>>,
        picture: Taken,
        _: u32,
    ) -> Result<Vec<Encoded>, Fail> {
        let (ready, mut out) = self.ready(session, &picture.picture)?;
        self.show(&ready);
        out.extend(ready.encode(session)?);
        Ok(out)
    }

    fn follow(&mut self, session: &mut Session, _: (u32, u32), stream: (u32, u32)) -> Result<(), EncodeError> {
        let told = match self.route.resize(session, stream) {
            Ok(told) => told,
            Err(why) if self.route.lifts() => {
                // A device that cannot make this size leaves the pictures to memory, as one that stops lifting them.
                report::note!(
                    warn,
                    "the camera's pictures are not scaled on the device; they go through memory",
                    why = why
                );
                let held = self.through_memory(session)?;
                self.route.resize(session, stream)?;
                Some(Reopening { held, changes: Changes::default(), in_place: false })
            }
            Err(why) => return Err(why),
        };
        if let Some(told) = told {
            self.rebuilt = Some(match self.rebuilt.take() {
                Some(earlier) => merged(earlier, told),
                None => told,
            });
        }
        Ok(())
    }

    fn withdrawn(&mut self, session: &mut Session, stream: (u32, u32)) -> Result<(), EncodeError> {
        self.follow(session, stream, stream)
    }

    fn reopen(&mut self, session: &mut Session, ladder: &mut Ladder) -> Result<Reopening, EncodeError> {
        let rebuilt = self.rebuilt.take();
        let made = match (ladder.reopen(session), rebuilt) {
            (Ok(made), Some(earlier)) => merged(earlier, made),
            (Ok(made), None) => made,
            // The stream was replaced already: what is still asked of the new one is asked again at its next encode.
            (Err(_), Some(earlier)) => return Ok(earlier),
            (Err(why), None) => return Err(why),
        };
        // A stream made anew, at a step or back on the hardware encoder it fell from, lifts the camera onto its device
        // again where that can now.
        #[cfg(target_os = "linux")]
        if !made.in_place
            && !made.changes.is_empty()
            && !self.route.lifts()
            && let Some(lifted) = self.route.relift(session)
        {
            return Ok(merged(made, lifted));
        }
        Ok(made)
    }

    fn hand_over(
        &mut self,
        session: &mut Session,
        ladder: &mut Ladder,
        failed: EncodeError,
    ) -> Result<(Changes, bool), EncodeError> {
        if !self.route.lifts() {
            return hand_over(session, ladder, failed);
        }
        // A lifted stream is on the failed encoder's device and its frames: the next opens through memory, at the
        // rate this one ran at, and the failed one is dropped undrained.
        let (width, height, running) = session.running();
        let (size, running) = ((width, height), running.clone());
        let rate = session.rate();
        let settings = Settings {
            width: size.0,
            height: size.1,
            frames_per_second: rate.frames_per_second,
            bits_per_second: rate.bits_per_second,
            key_frame_every: None,
        };
        let rebuild = ladder.failed(&running, failed) == Move::Rebuild;
        let mut refused = failed;
        let rebuilt =
            if rebuild { Way::memory_session(&running, settings).inspect_err(|why| refused = *why).ok() } else { None };
        let opened = match rebuilt {
            Some(opened) => opened,
            None => first_open(&mut ladder.later, refused, |encoder| Way::memory_session(encoder, settings))?.0,
        };
        drop(std::mem::replace(session, opened));
        self.route.leave_lift();
        Ok((Changes::default(), rebuild))
    }
}

/// The camera's pictures, read and encoded until stopped: what `told.opened` is told once it is open, and the feeder it
/// is then given. The loop is a share's (`screen::Started::run`); it encodes only while `watched`, and a picture
/// nobody takes is read and dropped.
fn work(
    pick: Pick,
    own: Option<Slot>,
    watched: &Arc<AtomicBool>,
    survey: &Arc<media::Survey>,
    stop: &Stops,
    told: Counted,
) -> Result<(), Failure> {
    let Counted { opened, fed, progress } = told;
    let needed = Arc::new(Needed { watched: Arc::clone(watched), own: own.clone(), settled: AtomicBool::new(false) });
    let reader = Reader::start(pick, Arc::clone(&needed), Arc::clone(&stop.asked));
    let Some((mode, stopper)) = reader.opened()? else { return Ok(()) };
    stop.opened(stopper);
    let frames_per_second = NonZeroU32::new(mode.frames_per_second).ok_or(Failure::CameraFormat)?;
    // The route is the first picture's: whether the encoder's device can lift it depends on how the camera sends it.
    let first = loop {
        match reader.next(POLL).map_err(failure::camera)? {
            Some(first) => break first,
            None if stop.asked() => return Ok(()),
            None => {}
        }
    };
    let size = Cameras::shape(&first);
    let envelope = Envelope::for_camera(size.0, size.1, mode.frames_per_second, None);
    let settings = Settings {
        width: size.0 as usize,
        height: size.1 as usize,
        frames_per_second: mode.frames_per_second,
        bits_per_second: u32::try_from(envelope.start).unwrap_or(u32::MAX),
        // Key frames only when a viewer asks, as a share's.
        key_frame_every: None,
    };
    let (route, session, encoder, ladder) = route_for(&first.picture, settings, survey)?;
    report::note!(info, "the camera is encoded", encoder = encoder.name, lifted = route.lifts());
    if opened.send((mode, crate::codec::encoder_route(&encoder), envelope)).is_err() {
        return Ok(());
    }
    let Ok(feeder) = fed.blocking_recv() else { return Ok(()) };
    let sides: Vec<u32> = Resolution::ALL.into_iter().filter_map(Resolution::short_side).collect();
    let chosen = Step { size: Size::Source, frames_per_second };
    let (width, height, _) = session.running();
    let layout = (u32::try_from(width).unwrap_or(size.0), u32::try_from(height).unwrap_or(size.1));
    let prepared = Prepared {
        session,
        layout,
        source: size,
        steps: Steps::starting(chosen, &sides, size.0, size.1, None),
        ceiling: None,
        scaled_from: None,
        envelope,
        codec: choices::Codec::H264,
        ladder,
        survey: Arc::clone(survey),
    };
    let source = Cameras { reader, route, own, progress: Arc::clone(&progress), rebuilt: None };
    needed.settled.store(true, Ordering::Relaxed);
    let started = prepared.start(source, first);
    let (stopped, watching) = (|| stop.asked(), || watched.load(Ordering::Relaxed));
    match started.run(Some(&feeder), None, &progress, stopped, watching) {
        Ok(_) => Ok(()),
        Err(failed) => Err(failure::screen(&failed)),
    }
}

/// Tells `survey` an encoder failed, where `why` is the encoder's and not the picture's or the settings'.
fn told_if_the_encoders(survey: &media::Survey, why: EncodeError) {
    if why.is_the_encoders() {
        survey.failed();
    }
}

/// The route for pictures like `first` at `settings`, its session, encoder and those after it: lifted onto the first
/// encoder in the call's order whose VA-API device takes them, else through memory on the first encoder that opens,
/// from those `survey` lists.
fn route_for(
    first: &Picture,
    settings: Settings,
    survey: &media::Survey,
) -> Result<(Way, Session, media::Encoder, Ladder), Failure> {
    #[cfg(target_os = "linux")]
    if let Picture::Decoded(decoded) = first {
        let surveyed = survey.report();
        let lifting = surveyed.memory_order(CODEC).filter(
            |encoder| matches!(&encoder.runs, media::Runs::Gpu(device) if device.backend == media::Backend::VaApi),
        );
        for encoder in lifting {
            match Way::lifted(encoder, settings, decoded) {
                Ok((route, session)) => {
                    let after = surveyed.memory_order(CODEC).skip_while(|each| *each != encoder).skip(1);
                    return Ok((route, session, encoder.clone(), Ladder::new(after.cloned().collect())));
                }
                Err(why) => {
                    report::note!(
                        info,
                        "the camera's pictures are not lifted onto this device",
                        encoder = encoder.name,
                        why = why
                    );
                }
            }
        }
    }
    memory_route(first, settings, survey)
}

/// The route through memory on the first encoder in the call's order that opens, from those `survey` lists, its
/// session and the encoders after it.
fn memory_route(
    first: &Picture,
    settings: Settings,
    survey: &media::Survey,
) -> Result<(Way, Session, media::Encoder, Ladder), Failure> {
    let surveyed = survey.report();
    let mut order: VecDeque<media::Encoder> = surveyed.memory_order(CODEC).cloned().collect();
    let open = |encoder: &media::Encoder| Way::memory_session(encoder, settings);
    let (session, encoder) = first_open(&mut order, EncodeError::Device, open)
        .inspect_err(|why| told_if_the_encoders(survey, *why))
        .map_err(failure::encode)?;
    Ok((Way::memory(first), session, encoder, Ladder::new(order)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A camera that is gone at once and whose release hangs, as Media Foundation's can on a camera disabled under it.
    struct Stuck;

    impl Pictures for Stuck {
        fn mode(&self) -> Mode {
            Mode { width: 16, height: 16, frames_per_second: 30 }
        }
        fn read(&mut self) -> Result<(Picture, Instant), CameraError> {
            Err(CameraError::Gone)
        }
    }

    impl Drop for Stuck {
        fn drop(&mut self) {
            std::thread::sleep(Duration::from_secs(3));
        }
    }

    /// A camera that gives a picture of this width and height every few milliseconds, for as long as it is read.
    struct Endless(u32, u32);

    impl Pictures for Endless {
        fn mode(&self) -> Mode {
            Mode { width: self.0, height: self.1, frames_per_second: 30 }
        }
        fn read(&mut self) -> Result<(Picture, Instant), CameraError> {
            std::thread::sleep(Duration::from_millis(5));
            let (width, height) = (self.0 as usize, self.1 as usize);
            let nv12 = media::Nv12::new(width, height, vec![16; width * height * 3 / 2])
                .map(|picture| picture.in_colours(ToNv12::COLOURS))
                .map_err(|_| CameraError::Format)?;
            Ok((Picture::Nv12(nv12), Instant::now()))
        }
    }

    #[test]
    fn a_failed_camera_encoder_hands_over_to_the_next_that_opens_and_the_end_is_the_last_refusal() {
        let survey = media::Survey::system();
        let settings = Settings {
            width: 640,
            height: 360,
            frames_per_second: 30,
            bits_per_second: 1_000_000,
            key_frame_every: None,
        };
        let first = Picture::Nv12(media::Nv12::new(640, 360, vec![16; 640 * 360 * 3 / 2]).expect("a picture"));
        let (route, mut session, running, _) = memory_route(&first, settings, &survey).expect("an encoder opens here");
        let software: VecDeque<_> =
            survey.report().memory_order(CODEC).filter(|encoder| encoder.name != running.name).cloned().collect();
        let needed = Needed { watched: Arc::default(), own: None, settled: AtomicBool::new(true) };
        let reader = Reader::start(Pick::Pictures(Box::new(Endless(640, 360))), Arc::new(needed), Arc::default());
        let mut source = Cameras { reader, route, own: None, progress: Arc::default(), rebuilt: None };
        let mut next = Ladder::new(software.clone());
        let handed = source.hand_over(&mut session, &mut next, EncodeError::Device);
        match software.front() {
            Some(successor) => {
                handed.expect("the next encoder takes over");
                assert_eq!(session.running().2.name, successor.name);
            }
            None => assert!(handed.is_err(), "with none left the camera ends"),
        }
        let mut none = Ladder::new(VecDeque::new());
        let ended = source.hand_over(&mut session, &mut none, EncodeError::Device);
        assert_eq!(ended.err(), Some(EncodeError::Device), "the last refusal ends it");
    }

    #[test]
    fn a_camera_stepping_down_is_scaled_as_it_is_converted_and_its_encoder_takes_the_new_size() {
        let survey = media::Survey::system();
        let settings = Settings {
            width: 640,
            height: 360,
            frames_per_second: 30,
            bits_per_second: 1_000_000,
            key_frame_every: None,
        };
        let picture = || {
            let nv12 = media::Nv12::new(640, 360, vec![16; 640 * 360 * 3 / 2]).expect("a picture");
            Picture::Nv12(nv12.in_colours(ToNv12::COLOURS))
        };
        let (route, mut session, _, _) = memory_route(&picture(), settings, &survey).expect("an encoder opens here");
        let needed = Needed { watched: Arc::default(), own: None, settled: AtomicBool::new(true) };
        let reader = Reader::start(Pick::Pictures(Box::new(Endless(640, 360))), Arc::new(needed), Arc::default());
        let progress = Arc::new(Progress::default());
        let mut source = Cameras { reader, route, own: None, progress: Arc::clone(&progress), rebuilt: None };
        let taken = || Taken { picture: picture(), at: Instant::now() };
        source.encode(&mut (), &mut session, None, taken(), 0).expect("a picture at the camera's own size");
        source.follow(&mut session, (640, 360), (320, 180)).expect("a smaller stream is asked for");
        assert_eq!(source.route.scaled_to(), Some((320, 180)), "the pictures are made that size from now");
        source.encode(&mut (), &mut session, None, taken(), 0).expect("a picture scaled as it is converted");
        assert_eq!(session.running().0, 320, "the encoder runs the new size");
        assert_eq!(progress.converts.load(Ordering::Relaxed), 2, "one convert a picture");
    }

    #[test]
    fn a_camera_whose_owner_is_dropped_stops_so_its_runtime_can_end() {
        let runtime = tokio::runtime::Runtime::new().expect("a runtime");
        let shown =
            runtime.block_on(preview(Pick::Pictures(Box::new(Endless(16, 16))), Slot::new())).expect("it opens");
        let (ended, told) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            drop(shown);
            drop(runtime);
            let _ = ended.send(());
        });
        assert!(told.recv_timeout(Duration::from_secs(2)).is_ok(), "the runtime waited on a camera nobody holds");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_camera_let_go_before_it_is_sent_ends_its_worker() {
        let survey = Arc::new(media::Survey::system());
        let begun =
            begin(Pick::Pictures(Box::new(Endless(640, 360))), None, Arc::default(), survey).await.expect("it opens");
        let let_go = tokio::time::timeout(Duration::from_secs(2), begun.abandon()).await;
        assert!(let_go.is_ok(), "the worker went on waiting for a feeder nobody will give it");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_camera_whose_release_hangs_is_said_gone_at_once() {
        let mut shown = preview(Pick::Pictures(Box::new(Stuck)), Slot::new()).await.expect("it opens");
        let told = tokio::time::timeout(Duration::from_secs(1), shown.ended()).await;
        assert_eq!(told, Ok(Failure::CameraGone), "told before the camera is let go");
        tokio::time::timeout(Duration::from_secs(1), shown.stop()).await.expect("stopped without waiting on it");
    }

    #[test]
    fn a_picture_nobody_takes_with_its_card_hidden_is_read_and_let_go_unconverted() {
        let (watched, own) = (Arc::new(AtomicBool::new(false)), Slot::new());
        let needed = Needed { watched: Arc::clone(&watched), own: Some(own.clone()), settled: AtomicBool::new(true) };
        assert!(needed.is(), "a shown card is given the picture");
        own.show(false);
        assert!(!needed.is(), "unwatched and hidden, nobody is");
        watched.store(true, Ordering::Relaxed);
        assert!(needed.is(), "watched, it is");
        let unsettled = Needed { watched: Arc::default(), own: Some(own), settled: AtomicBool::new(false) };
        assert!(unsettled.is(), "the first picture, which the route is chosen from, is whole whoever takes it");
    }

    /// The loop over a camera with the `Endless` source, its card hidden: nothing is converted while nobody watches,
    /// and pictures are once somebody does.
    #[test]
    fn an_unwatched_camera_with_a_hidden_card_converts_nothing_and_a_watched_one_does() {
        let survey = media::Survey::system();
        let settings = Settings {
            width: 640,
            height: 360,
            frames_per_second: 30,
            bits_per_second: 1_000_000,
            key_frame_every: None,
        };
        let first = Picture::Nv12(media::Nv12::new(640, 360, vec![16; 640 * 360 * 3 / 2]).expect("a picture"));
        let (route, session, _, ladder) = memory_route(&first, settings, &survey).expect("an encoder opens here");
        let (watched, own) = (Arc::new(AtomicBool::new(false)), Slot::new());
        own.show(false);
        let needed = Needed { watched: Arc::clone(&watched), own: Some(own.clone()), settled: AtomicBool::new(true) };
        let asked = Arc::new(AtomicBool::new(false));
        let reader = Reader::start(Pick::Pictures(Box::new(Endless(640, 360))), Arc::new(needed), Arc::clone(&asked));
        let progress = Arc::new(Progress::default());
        let source = Cameras { reader, route, own: Some(own.clone()), progress: Arc::clone(&progress), rebuilt: None };
        let envelope = Envelope::for_camera(640, 360, 30, None);
        let prepared = Prepared {
            session,
            layout: (640, 360),
            source: (640, 360),
            steps: Steps::starting(
                Step { size: Size::Source, frames_per_second: NonZeroU32::new(30).expect("nonzero") },
                &[],
                640,
                360,
                None,
            ),
            ceiling: None,
            scaled_from: None,
            envelope,
            codec: choices::Codec::H264,
            ladder,
            survey: Arc::new(survey),
        };
        let started = prepared.start(source, Taken { picture: first, at: Instant::now() });
        let (stop, flag) = (Arc::clone(&asked), Arc::clone(&watched));
        let ran = std::thread::scope(|scope| {
            let running = scope.spawn(|| {
                started.run(None, None, &progress, || stop.load(Ordering::Relaxed), || flag.load(Ordering::Relaxed))
            });
            std::thread::sleep(Duration::from_millis(400));
            let while_unwatched = progress.converts.load(Ordering::Relaxed);
            watched.store(true, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(400));
            let while_watched = progress.converts.load(Ordering::Relaxed);
            asked.store(true, Ordering::Relaxed);
            (running.join().is_ok(), while_unwatched, while_watched)
        });
        assert!(ran.0, "the loop ended well");
        assert_eq!(ran.1, 0, "nothing converted while nobody watched and the card was hidden");
        assert!(ran.2 > 0, "pictures are converted once somebody watches");
        assert_eq!(own.stats().received, 0, "and a hidden card was shown none");
    }
}
