//! A screen's capture, encoded: the run that counts or sends its pictures, the same on every system once each has
//! opened its capture and joined it to an encoder.

use super::fallback::Ladder;
use super::key_gate::{Asked, KeyGate};
use super::reopens::{Cause, Reopens};
use super::{Fail, Frames, KeyCause, Stages};
use crate::send::source::{Shape, Source};
use crate::send::stats::{Timings, megabits};
use crate::watch::freeze::Freezes;
use capture::{Capture, CaptureError, Frame, Layout};
use choices::{Resolution, ShareQuality};
use media::{EncodeError, Encoded, Rate, Session, Settings, Survey};
use net::{Budget, Governor, Judged, Ladder as Steps, Limit, Pace, RateGate, Size, Step, Verdict};
use std::collections::VecDeque;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};
use webrtc::Feeder;

/// How long `capture` watches the picked screen.
pub const CAPTURE_FOR: Duration = Duration::from_secs(5);

/// How long a picked screen may take to send its first picture.
const FIRST_PICTURE: Duration = Duration::from_secs(10);

/// How long one wait for a picture lasts, so a stop is noticed even when the screen is still.
const POLL: Duration = Duration::from_millis(100);

/// How often a screen that sends nothing is sent again, as a picture of nothing new: the engine notices what a receiver
/// asked for (a key frame) only when it is given a frame, so a still screen would answer none until it changed.
const STILL_EVERY: Duration = Duration::from_millis(500);

/// How long a screen's new size must hold before the stream follows it: a window being dragged changes size many
/// times a second, and each new stream costs a key frame. A first value, judged by hand.
pub const SETTLE: Duration = Duration::from_millis(300);

/// The most frames a second a share is sent at, whatever is chosen: what the capture and the envelope are built for.
const FASTEST: NonZeroU32 = NonZeroU32::new(60).unwrap();

/// What a run has done so far, readable from another thread while it runs.
#[derive(Default)]
pub struct Progress {
    /// Pictures encoded.
    pub frames: AtomicU64,
    /// Bytes the encoder made.
    pub bytes: AtomicU64,
    /// What each stage of the pipeline has counted, from the capture to libwebrtc.
    pub stages: Stages,
    /// Key frames the encoder made.
    pub key_frames: AtomicU64,
    /// The bytes of the largest key frame so far.
    pub largest_key_frame: AtomicU64,
    /// The stream's last new size, until the share takes it: the engine is told its new rates from it.
    pub resized: Mutex<Option<Resized>>,
    /// How the encoder that took over from a failed one codes, until the share takes it.
    pub handed_over: Mutex<Option<screens::Route>>,
    /// Why the encoder was reopened, and what the last reopen changed.
    pub reopens: Mutex<Reopens>,
    /// Why the share sends less than was chosen, while it does.
    pub reduced: Mutex<Option<screens::Reduced>>,
    /// Pictures made ready for the encoder or one's own card by a convert or a lift, which a source asked for pictures
    /// nobody takes makes none of.
    pub converts: AtomicU64,
}

/// What a share became when its screen changed size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resized {
    /// The stream's new width.
    pub width: u32,
    /// The stream's new height.
    pub height: u32,
    /// The screen's size, where the stream is scaled down from it.
    pub scaled_from: Option<(u32, u32)>,
    /// The rates the stream may now run between.
    pub envelope: net::Envelope,
}

/// The newest picture of a new size, held until its size has held for [`SETTLE`]; a picture of the stream's own size
/// lets it go. Generic over the picture so it is tested without one.
struct Settling<T, H> {
    held: Option<(T, H, Instant)>,
}

impl<T, H: Shape> Settling<T, H> {
    fn new() -> Self {
        Self { held: None }
    }

    /// Holds `picture`, of `layout`, at `now`; a size held before keeps the time it was first seen. Whether it replaced
    /// a picture held before, which is lost.
    fn hold(&mut self, picture: T, layout: H, now: Instant) -> bool {
        let (since, replaced) = match &self.held {
            Some((_, held, since)) if *held == layout => (*since, true),
            Some(_) => (now, true),
            None => (now, false),
        };
        self.held = Some((picture, layout, since));
        replaced
    }

    /// Lets go of what is held, the stream's own size having come back; whether a picture was, which is lost.
    fn let_go(&mut self) -> bool {
        self.held.take().is_some()
    }

    /// Whether a picture is held.
    fn holds(&self) -> bool {
        self.held.is_some()
    }

    /// The picture held, once its size has held for [`SETTLE`] at `now`.
    fn settled(&mut self, now: Instant) -> Option<(T, H)> {
        match &self.held {
            Some((_, _, since)) if now.duration_since(*since) >= SETTLE => {
                self.held.take().map(|(picture, layout, _)| (picture, layout))
            }
            _ => None,
        }
    }
}

/// How many pictures a rebuilt device costs a share: the one in hand when `in_hand`, the pace's kept picture and the
/// one a new size is settling, which are let go.
fn leave_behind<T, H: Shape>(waiting: &mut Option<T>, settling: &mut Settling<T, H>, in_hand: bool) -> u64 {
    let lost = u64::from(in_hand) + u64::from(waiting.take().is_some()) + u64::from(settling.holds());
    *settling = Settling::new();
    lost
}

/// What the captures a share replaced had counted, so the counts go on growing across a rebuild.
#[derive(Clone, Copy, Default)]
struct Carried {
    delivered: u64,
    dropped: u64,
}

/// A screen's capture, and how to open it again where it draws into the encoder's device: that device, rebuilt after it
/// was lost, is a new one, and a capture left on the lost one only ever draws there. `S` is what the capture is opened
/// from: the encoder's session.
pub(super) struct Capturing<S> {
    frames: Box<dyn Frames>,
    again: Option<Opener<S>>,
    carried: Carried,
}

/// Opens a capture from `S`.
type Opener<S> = Box<dyn FnMut(&S) -> Result<Box<dyn Frames>, Fail> + Send>;

impl<S> Capturing<S> {
    /// A capture a rebuild leaves as it is: one that is not on the encoder's device, or a test's own that is never
    /// rebuilt.
    fn fixed(frames: Box<dyn Frames>) -> Self {
        Self { frames, again: None, carried: Carried::default() }
    }

    /// `frames`, which `again` opens anew from the encoder's session when its device is rebuilt.
    #[cfg(any(windows, test))]
    fn drawing_into(frames: Box<dyn Frames>, again: Opener<S>) -> Self {
        Self { frames, again: Some(again), carried: Carried::default() }
    }

    /// The encoder's device was rebuilt: a capture that draws into it is opened anew on the new one, and the old
    /// one is dropped once the new one is open. True when it was.
    fn rebuilt(&mut self, session: &S) -> Result<bool, Fail> {
        let Some(again) = self.again.as_mut() else { return Ok(false) };
        let opened = again(session)?;
        self.carried = Carried { delivered: self.delivered(), dropped: self.dropped() };
        self.frames = opened;
        Ok(true)
    }

    /// Pictures the capture made, those of the captures it replaced included.
    fn delivered(&self) -> u64 {
        self.carried.delivered + self.frames.delivered()
    }

    /// Pictures the capture's slot replaced before they were taken, those of the captures it replaced included.
    fn dropped(&self) -> u64 {
        self.carried.dropped + self.frames.dropped()
    }

    /// Tells `stages` what the capture counted, and the widest gap between its pictures since it last did.
    fn count_into(&self, stages: &Stages) {
        stages.slot(self.delivered(), self.dropped());
        stages.capture_gap(self.frames.take_widest_gap());
    }
}

impl Frames for Capture {
    fn next(&self, wait: Duration) -> Result<Option<Frame>, CaptureError> {
        Capture::next(self, wait)
    }

    fn source_name(&self) -> Option<String> {
        Capture::source_name(self)
    }

    fn dropped(&self) -> u64 {
        Capture::dropped(self)
    }

    fn delivered(&self) -> u64 {
        Capture::delivered(self)
    }

    fn take_widest_gap(&self) -> Duration {
        Capture::take_widest_gap(self)
    }
}

/// The screen as a source: its capture, which a rebuilt encoder device may open anew.
pub struct Screen {
    capture: Capturing<Session>,
}

impl Screen {
    fn new(capture: Capturing<Session>) -> Self {
        Self { capture }
    }
}

impl Source for Screen {
    type Picture = Frame;
    type Shape = Layout;
    type Lending = Lender;

    fn next(&mut self, wait: Duration) -> Result<Option<Frame>, Fail> {
        Ok(self.capture.frames.next(wait)?)
    }

    fn shape(picture: &Frame) -> Layout {
        picture.layout()
    }

    fn captured(picture: &Frame) -> Instant {
        picture.captured()
    }

    fn count_into(&self, stages: &Stages) {
        self.capture.count_into(stages);
    }

    fn name(&self) -> Option<String> {
        self.capture.frames.source_name()
    }

    fn envelope((width, height): (u32, u32), frames_per_second: u32, ceiling: Option<u64>) -> net::Envelope {
        net::Envelope::for_screen(width, height, frames_per_second, ceiling)
    }

    fn lend(
        &mut self,
        lending: &mut Lender,
        preview: Option<&Preview<'_>>,
        picture: &Frame,
        session: &Session,
        rebuilds: u32,
    ) {
        lending.lend(preview, picture, session, rebuilds);
    }

    fn encode(
        &mut self,
        lending: &mut Lender,
        session: &mut Session,
        preview: Option<&Preview<'_>>,
        picture: Frame,
        rebuilds: u32,
    ) -> Result<Vec<Encoded>, Fail> {
        lending.lend(preview, &picture, session, rebuilds);
        Ok(session.encode_frame(picture)?)
    }

    fn follow(&mut self, session: &mut Session, shape: Layout, (width, height): (u32, u32)) -> Result<(), EncodeError> {
        session.set_source(shape, (width as usize, height as usize))
    }

    fn rebuilt(&mut self, session: &Session) -> Result<bool, Fail> {
        self.capture.rebuilt(session)
    }
}

/// A source with its first picture in hand and an encoder open for that picture's size.
pub struct Started<S: Source = Screen> {
    input: S,
    /// How many times the encoder's device was rebuilt: what a picture or a ring made on an earlier device is told by.
    rebuilds: u32,
    session: Session,
    first: Option<S::Picture>,
    /// The picture the stream is: the source's, scaled down as chosen, its right and bottom edge left out to the
    /// encoder's multiple, as the encoder runs it.
    pub layout: S::Shape,
    /// The source's own picture, which the stream is made from.
    source: S::Shape,
    /// The size and rate the stream is sent at, from how it was asked to go out down to what its encoder keeps up with,
    /// which a new size of the source keeps.
    steps: Steps,
    ceiling: Option<u64>,
    /// The source's size, where the stream is scaled down from it.
    pub scaled_from: Option<(u32, u32)>,
    /// The rates the stream may run between, which the encoder starts in and the engine is told.
    pub envelope: net::Envelope,
    /// The codec the stream is sent in.
    pub codec: choices::Codec,
    /// The codec's encoders after the running one, in the survey's order, each taking over when the one before fails,
    /// and the hardware one the stream fell from, tried first at the next reopen.
    ladder: Ladder,
    /// A new size or step asked of the encoder, which the stream takes once the encoder has made it.
    next: Option<Next<S::Shape>>,
    /// Why the change waiting for the next reopen was asked: the last asked, when several come before it.
    asked: Option<Cause>,
    /// The survey the encoders came from, told when one fails.
    survey: Arc<Survey>,
    /// What last stepped the stream down, while it is down from where it began.
    limit: Option<Limit>,
    /// Why the next key frame is made, once something asked for one.
    key_cause: Option<KeyCause>,
}

/// What a stream becomes once its encoder makes the change asked, and the steps to go back to if it is refused.
struct Next<H> {
    source: H,
    scaled_from: Option<(u32, u32)>,
    envelope: net::Envelope,
    before: Steps,
}

/// What a reopen did.
enum Reopened {
    /// The change was made, or none was asked; with what the old encoder still held.
    Made(Vec<media::Encoded>),
    /// The encoder refused a change of step or rate, which is dropped: the running stream goes on.
    Withdrawn,
    /// Another encoder took over from one that failed.
    HandedOver,
}

impl<S: Source> Started<S> {
    /// What the source calls itself, where it says.
    pub fn source_name(&self) -> Option<String> {
        self.input.name()
    }

    /// The encoder that has the stream, one of `codec`'s.
    pub fn encoder(&self) -> &media::Encoder {
        self.session.running().2
    }
}

/// The chosen frame rate, never over [`FASTEST`].
pub(super) fn frames_per_second(quality: &ShareQuality) -> NonZeroU32 {
    NonZeroU32::new(quality.frame_rate.per_second()).map_or(FASTEST, |chosen| chosen.min(FASTEST))
}

/// Waits for `capture`'s first picture and opens an encoder for the size it has from those `survey` lists, as
/// `quality` says.
pub fn begin_with(capture: Box<dyn Frames>, quality: &ShareQuality, survey: &Arc<Survey>) -> Result<Started, Fail> {
    let first = first_picture(capture.as_ref())?;
    Ok(Prepared::open(first.layout(), quality, survey)?.start(Screen::new(Capturing::fixed(capture)), first))
}

/// Opens an encoder for a screen laid out as `source` from the encoders `survey` lists, then the capture that
/// `capture` opens from its session, and waits for that capture's first picture: a capture that must draw into the
/// encoder's own device (Windows') opens after it. A rebuild of that device leaves it as it is; `begin_reopening`
/// opens it again.
pub fn begin_into(
    source: Layout,
    quality: &ShareQuality,
    survey: &Arc<Survey>,
    capture: impl FnOnce(&Session) -> Result<Box<dyn Frames>, Fail>,
) -> Result<Started, Fail> {
    let prepared = Prepared::open(source, quality, survey)?;
    let frames = capture(&prepared.session)?;
    let first = first_picture(frames.as_ref())?;
    Ok(prepared.start(Screen::new(Capturing::fixed(frames)), first))
}

/// As [`begin_into`], for a capture that draws into the encoder's own device: `capture` opens it again on the new
/// device whenever that device is rebuilt.
#[cfg(windows)]
pub(super) fn begin_reopening(
    source: Layout,
    quality: &ShareQuality,
    survey: &Arc<Survey>,
    mut capture: impl FnMut(&Session) -> Result<Box<dyn Frames>, Fail> + Send + 'static,
) -> Result<Started, Fail> {
    let prepared = Prepared::open(source, quality, survey)?;
    let frames = capture(&prepared.session)?;
    let first = first_picture(frames.as_ref())?;
    Ok(prepared.start(Screen::new(Capturing::drawing_into(frames, Box::new(capture))), first))
}

/// `capture`'s first picture, waited for up to [`FIRST_PICTURE`].
fn first_picture(capture: &dyn Frames) -> Result<Frame, Fail> {
    let until = Instant::now() + FIRST_PICTURE;
    loop {
        let left = until.checked_duration_since(Instant::now()).ok_or(Fail::Silent)?;
        if let Some(frame) = capture.next(left)? {
            return Ok(frame);
        }
    }
}

/// An encoder open for a source's size, not yet joined to it: on Windows the capture draws into the encoder's own
/// device, so it opens after.
pub(crate) struct Prepared<H> {
    pub(crate) session: Session,
    pub(crate) layout: H,
    pub(crate) source: H,
    pub(crate) steps: Steps,
    pub(crate) ceiling: Option<u64>,
    pub(crate) scaled_from: Option<(u32, u32)>,
    pub(crate) envelope: net::Envelope,
    pub(crate) codec: choices::Codec,
    pub(crate) ladder: Ladder,
    pub(crate) survey: Arc<Survey>,
}

impl Prepared<Layout> {
    /// An encoder for a screen laid out as `source`, as `quality` says, from those `survey` lists; a survey none of
    /// whose encoders opens is made again at the next ask.
    fn open(source: Layout, quality: &ShareQuality, survey: &Arc<Survey>) -> Result<Self, EncodeError> {
        let surveyed = survey.report();
        // The person's ceiling: the server states none.
        let ceiling = quality.ceiling.bits_per_second();
        let steps = starting_steps(quality, source);
        let Step { size, frames_per_second } = steps.now();
        let (width, height) = size.fit(source.width, source.height);
        let asked = Layout { width, height, ..source };
        let scaled_from =
            ((width, height) != (source.width & !1, source.height & !1)).then_some((source.width, source.height));
        let envelope = net::Envelope::for_screen(width, height, frames_per_second.get(), ceiling);
        let codec = quality.codec_on(&super::options(&surveyed));
        let (session, ladder) =
            open(source, asked, envelope, frames_per_second, codec, &surveyed).inspect_err(|failed| {
                if failed.is_the_encoders() {
                    survey.failed();
                }
            })?;
        let layout = running(&session, source);
        Ok(Self {
            session,
            layout,
            source,
            steps,
            ceiling,
            scaled_from,
            envelope,
            codec,
            ladder,
            survey: Arc::clone(survey),
        })
    }
}

impl<H: Shape> Prepared<H> {
    /// Joins `input`, whose first picture is `first`, to the encoder.
    pub(crate) fn start<S: Source<Shape = H>>(self, input: S, first: S::Picture) -> Started<S> {
        Started {
            input,
            rebuilds: 0,
            session: self.session,
            first: Some(first),
            layout: self.layout,
            source: self.source,
            steps: self.steps,
            ceiling: self.ceiling,
            scaled_from: self.scaled_from,
            envelope: self.envelope,
            codec: self.codec,
            ladder: self.ladder,
            next: None,
            asked: None,
            survey: self.survey,
            limit: None,
            key_cause: None,
        }
    }
}

/// The steps a share of `quality` takes of a screen laid out as `source`, on the first whose floor fits under the
/// person's ceiling.
fn starting_steps(quality: &ShareQuality, source: Layout) -> Steps {
    let chosen = Step { size: size_of(quality.resolution), frames_per_second: frames_per_second(quality) };
    let sides: Vec<u32> = Resolution::ALL.into_iter().filter_map(Resolution::short_side).collect();
    Steps::starting(chosen, &sides, source.width, source.height, quality.ceiling.bits_per_second())
}

/// Moves `steps` the way `judged` asks for a screen of `source`'s size; the cause of the reopen it makes, or none
/// where the steps have no step that way.
fn move_on(steps: &mut Steps, judged: Judged, (width, height): (u32, u32)) -> Option<Cause> {
    match judged {
        Judged::Over(Limit::Encoder) => steps.down(width, height).then_some(Cause::Overused),
        Judged::Over(Limit::Rate) => steps.down(width, height).then_some(Cause::Busy),
        Judged::Under => steps.up(width, height).then_some(Cause::Roomy),
    }
}

/// What a share on `steps` of a screen of `source`'s size sends less of than the person chose, and why: a ceiling that
/// held the start below the choice while the share is on its start, else what stepped it down last (`limit`) under the
/// person's `ceiling`. None when it sends what was chosen.
fn reduced_from(
    steps: &Steps,
    (width, height): (u32, u32),
    ceiling: Option<u64>,
    limit: Option<Limit>,
) -> Option<screens::Reduced> {
    let (chosen, now) = (steps.chosen(), steps.now());
    let (chosen_size, now_size) = (chosen.size.fit(width, height), now.size.fit(width, height));
    if (chosen_size, chosen.frames_per_second) == (now_size, now.frames_per_second) {
        return None;
    }
    let cause = match (steps.at_start(), ceiling, limit) {
        (true, Some(bits_per_second), _) => screens::Reduction::Ceiling { bits_per_second },
        (true, None, _) => return None,
        (false, _, Some(Limit::Rate)) => screens::Reduction::Busy { ceiling },
        (false, _, Some(Limit::Encoder) | None) => screens::Reduction::Encoder,
    };
    Some(screens::Reduced {
        from_size: (chosen_size != now_size).then_some(chosen_size),
        from_frame_rate: chosen.frames_per_second.get(),
        cause,
    })
}

/// The size a person's resolution choice is, as the share's steps say it.
fn size_of(resolution: Resolution) -> Size {
    resolution.short_side().map_or(Size::Source, Size::ShortSide)
}

/// The picture `session` streams of a source laid out as `source`.
fn running<H: Shape>(session: &Session, source: H) -> H {
    let (width, height, _) = session.running();
    let side = |value: usize| u32::try_from(value).unwrap_or(u32::MAX);
    source.sized((side(width), side(height)))
}

/// The first of `codec`'s screen order in `surveyed` that opens for a screen laid out as `source`, streaming `layout`,
/// and the rest of the order after it; the last refusal if none does.
fn open(
    source: Layout,
    layout: Layout,
    envelope: net::Envelope,
    frames_per_second: NonZeroU32,
    codec: choices::Codec,
    surveyed: &media::Report,
) -> Result<(Session, Ladder), EncodeError> {
    let settings = Settings {
        width: layout.width as usize,
        height: layout.height as usize,
        frames_per_second: frames_per_second.get(),
        // The engine's own targets move the rate from here, inside the envelope the share states.
        bits_per_second: u32::try_from(envelope.start).unwrap_or(u32::MAX),
        // Key frames only when a viewer asks: one on a timer, held to the rate, came out soft and a still
        // screen never settled before the next.
        key_frame_every: None,
    };
    let mut order: VecDeque<_> = surveyed.screen_order(crate::codec::encoded(codec)).cloned().collect();
    let (session, _) =
        first_open(&mut order, EncodeError::Device, |encoder| Session::open_screen(encoder, settings, source))?;
    Ok((session, Ladder::new(order)))
}

/// The rate a share's encoder runs at when the engine wants `bits_per_second`: a budget kept inside the share's
/// `envelope`, at the step's `frames_per_second`, which only the share's own load moves.
pub(crate) fn encoder_rate(
    bits_per_second: u64,
    envelope: net::Envelope,
    frames_per_second: NonZeroU32,
) -> Result<Rate, EncodeError> {
    Rate::new(envelope.clamp(bits_per_second), frames_per_second)
}

/// The engine's `target`, if the encoder running at `running` is to take it now: an encoder that changes rate
/// `in_place` takes every new one, as libwebrtc's own encoders do, since it costs no key frame; for every other,
/// which reopens on a key frame, the gate decides.
pub(crate) fn taken(target: Rate, running: Rate, in_place: bool, gate: &mut RateGate, now: Duration) -> Option<Rate> {
    if in_place {
        return (target != running).then_some(target);
    }
    match gate.offer(now, target.bits_per_second) {
        Verdict::Apply => Some(target),
        Verdict::Keep => None,
    }
}

/// The first of `encoders` that `open` opens, taken from their front, those after it left; the last refusal if none
/// opens, `refused` if there is none to try.
pub(crate) fn first_open<T>(
    encoders: &mut VecDeque<media::Encoder>,
    mut refused: EncodeError,
    mut open: impl FnMut(&media::Encoder) -> Result<T, EncodeError>,
) -> Result<(T, media::Encoder), EncodeError> {
    while let Some(encoder) = encoders.pop_front() {
        match open(&encoder) {
            Ok(opened) => return Ok((opened, encoder)),
            Err(why) => refused = why,
        }
    }
    Err(refused)
}

/// What a run did.
pub struct Report<H = Layout> {
    /// The encoder that took the stream.
    pub encoder: media::Encoder,
    /// The stream's picture when the run ended.
    pub layout: H,
    /// Pictures encoded.
    pub frames: u64,
    /// Pictures handed to the encoder, repeats of a still screen's last included.
    pub sends: u64,
    /// Key frames the encoder made, its last flush included.
    pub key_frames: u64,
    /// Pictures left out before the encoder: the capture's dropped, those the pace and a settling size or a rebuilt
    /// device lost.
    pub skipped: u64,
    /// Bytes the encoder made, its last flush included.
    pub bytes: u64,
    /// How long the run took, its last flush left out.
    pub span: Duration,
    /// How long the pictures took to encode: the convert and the submit, not the GPU's completion.
    pub timings: Timings,
    /// How many times the engine's targets moved the encoder's rate.
    pub rate_changes: u64,
    /// The encoder's rate when the run ended, in bits a second.
    pub final_rate: u32,
    /// The bytes of the largest key frame.
    pub largest_key_frame: u64,
    /// The most pictures a second the share was sent at when the run ended: the chosen rate, or lower where its
    /// encoder did not keep up.
    pub final_frames_per_second: NonZeroU32,
}

impl<S: Source> Started<S> {
    /// Encodes every picture until `stop` says to, while `watched` says someone takes the stream; meanwhile the capture
    /// and the preview go on and the encoder rests, and the first picture after is a key frame. A `feeder`'s asks are
    /// answered whether or not a picture came: a key frame at once, or a second after the last one, from the last
    /// picture when the screen is still, and a rate the gate finds worth one with the next picture. A `preview` is
    /// lent each picture beside the encoder, at the full rate, when the window took the last.
    pub fn run(
        mut self,
        feeder: Option<&Feeder>,
        preview: Option<&Preview<'_>>,
        progress: &Progress,
        mut stop: impl FnMut() -> bool,
        mut watched: impl FnMut() -> bool,
    ) -> Result<Report<S::Shape>, Fail> {
        let begun = Instant::now();
        let mut gate = RateGate::new(self.session.rate().bits_per_second, Duration::ZERO);
        let mut pace = Pace::new(self.steps.now().frames_per_second.get());
        let mut governor = Governor::new(Duration::ZERO);
        let allowance = Duration::from_millis(media::KEY_FRAME_BUDGET_MS.unsigned_abs());
        let mut budget = Budget::new(allowance, u64::from(self.session.rate().bits_per_second), Duration::ZERO);
        progress.stages.budget(self.steps.interval());
        self.note_reduced(progress);
        let mut timings = Timings::default();
        let (mut frames, mut key_frames, mut bytes, mut rate_changes, mut largest_key_frame) =
            (0u64, 0u64, 0u64, 0u64, 0u64);
        let mut settling = Settling::new();
        let mut lending = S::Lending::default();
        let mut last_sent = Instant::now();
        // When the encoder's device was last asked whether it is lost, while no picture is encoded to say so.
        let mut checked = Instant::now();
        // The newest picture the pace left out: the screen's last change if nothing newer comes.
        let mut waiting: Option<S::Picture> = None;
        let mut resting = false;
        let mut freezes = Freezes::default();
        let mut lost_at_last = 0;
        let mut keys = KeyGate::default();
        while !stop() {
            self.input.count_into(&progress.stages);
            let captured = match self.first.take() {
                Some(frame) => Some(frame),
                None => self.input.next(POLL)?,
            };
            if let Some(frame) = &captured {
                let counted = progress.stages.counted();
                let lost = counted.slot_dropped + counted.discarded;
                if let Some(gap) = freezes.picture(S::captured(frame), lost > lost_at_last) {
                    progress.stages.froze(gap);
                }
                lost_at_last = lost;
            }
            let nothing_new = captured.is_none();
            budget.set_rate(begun.elapsed(), u64::from(self.session.rate().bits_per_second));
            // The share owns its rate: while more bits than the rate allows are owed, the newest picture waits.
            let owing = !budget.admits(begun.elapsed());
            // Left out before the encoder, never after: an encoded picture dropped tears the stream to the next key
            // frame.
            let frame = match captured {
                Some(frame) if owing || !pace.take(Instant::now()) => {
                    if waiting.replace(frame).is_some() {
                        if owing {
                            progress.stages.held();
                            let rate = u64::from(self.session.rate().bits_per_second);
                            governor.held(begun.elapsed(), rate, self.steps.interval());
                        } else {
                            progress.stages.paced();
                        }
                    }
                    None
                }
                Some(frame) => {
                    if waiting.take().is_some() {
                        progress.stages.paced();
                    }
                    Some(frame)
                }
                None => match waiting.take() {
                    Some(frame) if !owing && pace.take(Instant::now()) => Some(frame),
                    kept => {
                        waiting = kept;
                        None
                    }
                },
            };
            let still = nothing_new && frame.is_none() && waiting.is_none();
            // A picture of a new size waits for the size to settle; one of the stream's own size lets it go.
            let frame = match frame {
                Some(frame) if S::shape(&frame) != self.source => {
                    self.input.lend(&mut lending, preview, &frame, &self.session, self.rebuilds);
                    let layout = S::shape(&frame);
                    if settling.hold(frame, layout, Instant::now()) {
                        progress.stages.discarded(1);
                    }
                    None
                }
                Some(frame) => {
                    if settling.let_go() {
                        progress.stages.discarded(1);
                    }
                    Some(frame)
                }
                None => None,
            };
            let frame = match (frame, settling.settled(Instant::now())) {
                (None, Some((frame, layout))) => {
                    self.follow(layout, self.steps.clone())?;
                    self.asked = Some(Cause::Resized);
                    Some(frame)
                }
                (frame, _) => frame,
            };
            if !watched() {
                // Its newest picture is kept, as the pace keeps one: a still screen sends no other once it is watched.
                if let Some(frame) = frame {
                    self.input.lend(&mut lending, preview, &frame, &self.session, self.rebuilds);
                    waiting = Some(frame);
                }
                // A capture has few buffers (Windows' two): resting, the share holds one, the picture kept, else the
                // capture fills none and one's own card stops.
                if waiting.is_some() {
                    self.session.superseded();
                }
                for unit in self.session.finished()? {
                    Self::send(feeder, self.layout.size(), &unit, progress);
                }
                // Resting, no convert still reads a compositor's buffer: each is the compositor's again.
                if !resting && let Err(failed) = self.session.settle() {
                    self.recover(failed, progress, &mut waiting, &mut settling)?;
                    (gate, governor) = (
                        RateGate::new(self.session.rate().bits_per_second, begun.elapsed()),
                        Governor::new(begun.elapsed()),
                    );
                }
                // A still screen nobody watches encodes nothing that could say its device is gone.
                if checked.elapsed() >= STILL_EVERY {
                    checked = Instant::now();
                    if self.session.device_lost() {
                        self.recover(EncodeError::DeviceLost, progress, &mut waiting, &mut settling)?;
                        (gate, governor) = (
                            RateGate::new(self.session.rate().bits_per_second, begun.elapsed()),
                            Governor::new(begun.elapsed()),
                        );
                    }
                }
                resting = true;
                continue;
            }
            if std::mem::take(&mut resting) {
                freezes.resumed();
                // Its load is measured anew. It asks for no key frame on waking: the server's resume of a viewer's
                // consumer (`VoiceResume`) asks its sender for one, so an ask here would be a second.
                (gate, governor) = (
                    RateGate::woken(self.session.rate().bits_per_second, begun.elapsed()),
                    Governor::new(begun.elapsed()),
                );
                budget = Budget::new(allowance, u64::from(self.session.rate().bits_per_second), begun.elapsed());
            }
            let elapsed = begun.elapsed();
            let asked = feeder.is_some_and(Feeder::key_frame_wanted).then(|| keys.ask(elapsed));
            match asked {
                Some(Asked::Deferred) => progress.stages.ask_deferred(),
                Some(Asked::Merged) => progress.stages.ask_merged(),
                Some(Asked::Now) | None => {}
            }
            let wanted = asked == Some(Asked::Now) || keys.due(elapsed);
            if wanted {
                self.session.request_key_frame();
                self.key_cause = Some(KeyCause::Asked);
            }
            let units = match frame {
                Some(frame) => {
                    if let Some(target) = feeder.and_then(Feeder::bits_wanted) {
                        let target = encoder_rate(target, self.envelope, self.steps.now().frames_per_second);
                        let in_place = self.session.takes_rate_in_place();
                        if let Ok(Some(rate)) =
                            target.map(|rate| taken(rate, self.session.rate(), in_place, &mut gate, begun.elapsed()))
                        {
                            self.session.set_rate(rate)?;
                            self.asked = Some(Cause::LinkRate);
                            rate_changes += 1;
                        }
                    }
                    // A reopen is the share's change, not its load: made before the encode is timed.
                    let rebuilds = self.rebuilds;
                    let mut held = self.reopened(progress, &mut gate, &mut governor, begun.elapsed())?;
                    if self.rebuilds != rebuilds {
                        // This picture, and any kept, are of the device that was lost.
                        progress.stages.discarded(leave_behind(&mut waiting, &mut settling, true));
                        continue;
                    }
                    let took = Instant::now();
                    let units = self.input.encode(&mut lending, &mut self.session, preview, frame, self.rebuilds).map(
                        |fresh| {
                            held.extend(fresh);
                            held
                        },
                    );
                    let took = took.elapsed();
                    timings.push(took);
                    // The encoder is judged by its own time: a convert queued behind a game's draws is not its load.
                    let timed = self.session.timed();
                    progress.stages.encoded(timed.encode);
                    progress.stages.converted(timed.convert);
                    frames += 1;
                    if let Some(judged) = governor.encoded(begun.elapsed(), timed.encode, self.steps.interval()) {
                        self.step(judged, &mut pace, progress)?;
                    }
                    units
                }
                // A screen gone still has its last change out at the next wait, not the next still repeat; a repeat is
                // load like a picture, though it has no interval of its own.
                None if still && (wanted || self.session.holds_a_change() || last_sent.elapsed() >= STILL_EVERY) => {
                    let rebuilds = self.rebuilds;
                    let mut held = self.reopened(progress, &mut gate, &mut governor, begun.elapsed())?;
                    if self.rebuilds != rebuilds {
                        progress.stages.discarded(leave_behind(&mut waiting, &mut settling, false));
                        continue;
                    }
                    let took = Instant::now();
                    let units = self.session.repeat_last().map_err(Fail::from).map(|fresh| {
                        held.extend(fresh);
                        held
                    });
                    governor.repeated(begun.elapsed(), took.elapsed());
                    units
                }
                None => continue,
            };
            let units = match units {
                Ok(units) => units,
                Err(Fail::Encode(failed)) if failed.is_the_encoders() => {
                    self.recover(failed, progress, &mut waiting, &mut settling)?;
                    (gate, governor) = (
                        RateGate::new(self.session.rate().bits_per_second, begun.elapsed()),
                        Governor::new(begun.elapsed()),
                    );
                    continue;
                }
                Err(failed) => return Err(failed),
            };
            if !units.is_empty() {
                last_sent = Instant::now();
            }
            for unit in &units {
                let made = unit.data.len() as u64;
                bytes += made;
                key_frames += u64::from(unit.key_frame);
                let at = begun.elapsed();
                budget.sent(at, made * 8);
                if unit.key_frame {
                    if keys.sent(at) {
                        progress.stages.ask_merged();
                    }
                    largest_key_frame = largest_key_frame.max(made);
                    let asked = self.key_cause.take();
                    let cause = if key_frames == 1 { KeyCause::Started } else { asked.unwrap_or(KeyCause::Other) };
                    progress.stages.key_frame(cause);
                    // Its time is read from the log's own stamp, against a viewer's first picture.
                    report::note!(info, "share: key frame sent", bytes = unit.data.len());
                } else {
                    let rate = u64::from(self.session.rate().bits_per_second);
                    if let Some(judged) = governor.made(at, made * 8, rate) {
                        self.step(judged, &mut pace, progress)?;
                    }
                }
                Self::send(feeder, self.layout.size(), unit, progress);
            }
            progress.largest_key_frame.store(largest_key_frame, Ordering::Relaxed);
            progress.frames.store(frames, Ordering::Relaxed);
            progress.bytes.store(bytes, Ordering::Relaxed);
            progress.key_frames.store(key_frames, Ordering::Relaxed);
        }
        self.input.count_into(&progress.stages);
        let left_out = progress.stages.counted();
        let skipped = left_out.slot_dropped + left_out.paced + left_out.discarded + u64::from(waiting.is_some());
        let sends = self.session.sends();
        let span = begun.elapsed();
        let final_rate = self.session.rate().bits_per_second;
        let encoder = self.encoder().clone();
        for unit in self.session.finish()? {
            bytes += unit.data.len() as u64;
            key_frames += u64::from(unit.key_frame);
            Self::send(feeder, self.layout.size(), &unit, progress);
        }
        Ok(Report {
            encoder,
            layout: self.layout,
            frames,
            sends,
            key_frames,
            skipped,
            bytes,
            span,
            timings,
            rate_changes,
            final_rate,
            largest_key_frame,
            final_frames_per_second: self.steps.now().frames_per_second,
        })
    }
}

impl<S: Source> Started<S> {
    /// Hands `unit`, of a stream of `layout`, to libwebrtc through `feeder`, when the share sends, and counts it sent:
    /// handed over, not yet on the wire. It takes no `self`, as the last units come once the session is finished.
    fn send(feeder: Option<&Feeder>, (width, height): (u32, u32), unit: &media::Encoded, progress: &Progress) {
        let Some(feeder) = feeder else { return };
        feeder.push(&unit.data, unit.key_frame, width, height, unit.captured);
        progress.stages.sent(1);
    }

    /// The screen is now `source`, or the share is on another step from `before`: the encoder is asked for the step's
    /// size and rate, a new stream on a key frame, which the share takes once it is made.
    fn follow(&mut self, source: S::Shape, before: Steps) -> Result<(), EncodeError> {
        let Step { size, frames_per_second } = self.steps.now();
        let (source_width, source_height) = source.size();
        let (width, height) = size.fit(source_width, source_height);
        self.input.follow(&mut self.session, source, (width, height))?;
        let envelope = S::envelope((width, height), frames_per_second.get(), self.ceiling);
        let rate = self.session.rate();
        let kept = u32::try_from(envelope.clamp(u64::from(rate.bits_per_second))).unwrap_or(u32::MAX);
        self.session.set_rate(Rate { bits_per_second: kept, frames_per_second: frames_per_second.get() })?;
        let scaled_from =
            ((width, height) != (source_width & !1, source_height & !1)).then_some((source_width, source_height));
        // Two changes before one reopen go back, if refused, to the steps before the first.
        let before = self.next.take().map_or(before, |earlier| earlier.before);
        self.next = Some(Next { source, scaled_from, envelope, before });
        Ok(())
    }

    /// Takes the step `judged` asks of the share, when the steps have one that way; `pace` follows its frame rate.
    fn step(&mut self, judged: Judged, pace: &mut Pace, progress: &Progress) -> Result<(), Fail> {
        let before = self.steps.clone();
        let Some(cause) = move_on(&mut self.steps, judged, self.source.size()) else {
            return Ok(());
        };
        if let Judged::Over(limit) = judged {
            self.limit = Some(limit);
        }
        self.follow(self.source, before)?;
        self.asked = Some(cause);
        *pace = Pace::new(self.steps.now().frames_per_second.get());
        progress.stages.budget(self.steps.interval());
        self.note_reduced(progress);
        Ok(())
    }

    /// What the share sends less of than was chosen, and why; none when it sends what was chosen.
    fn reduced(&self) -> Option<screens::Reduced> {
        reduced_from(&self.steps, self.source.size(), self.ceiling, self.limit)
    }

    /// Tells the share's looks what it is reduced from, now.
    fn note_reduced(&self, progress: &Progress) {
        *progress.reduced.lock().unwrap_or_else(PoisonError::into_inner) = self.reduced();
    }

    /// Makes the change asked. The encoder's own refusal of a new size, which the next picture already has, hands the
    /// share to the next encoder; of a new step or rate, it is dropped and the share stays as it runs.
    fn reopen(&mut self, progress: &Progress) -> Result<Reopened, Fail> {
        match self.input.reopen(&mut self.session, &mut self.ladder) {
            Ok(media::Reopening { held, changes, in_place }) => {
                // A rate taken in place opens nothing: it is the gate's count of rate changes, not a reopen.
                if let Some(cause) = self.asked.take().filter(|_| !changes.is_empty() && !in_place) {
                    progress.reopens.lock().unwrap_or_else(PoisonError::into_inner).record(cause, changes);
                    self.key_cause = Some(KeyCause::Reopened);
                }
                self.take_next(progress);
                Ok(Reopened::Made(held))
            }
            Err(refused) if !refused.is_the_encoders() => Err(refused.into()),
            Err(refused) if self.next.as_ref().is_some_and(|next| next.source != self.source) => {
                self.hand_over(refused, progress)?;
                Ok(Reopened::HandedOver)
            }
            Err(_) => {
                self.asked = None;
                self.session.withdraw();
                if let Some(next) = self.next.take() {
                    self.steps = next.before;
                    let (width, height) = self.source.size();
                    self.input.withdrawn(&mut self.session, self.steps.now().size.fit(width, height))?;
                }
                Ok(Reopened::Withdrawn)
            }
        }
    }

    /// [`Started::reopen`], the gate and the load measured again from `at` where the encoder changed under them; what
    /// the old encoder still held.
    fn reopened(
        &mut self,
        progress: &Progress,
        gate: &mut RateGate,
        governor: &mut Governor,
        at: Duration,
    ) -> Result<Vec<media::Encoded>, Fail> {
        Ok(match self.reopen(progress)? {
            Reopened::Made(held) => held,
            Reopened::Withdrawn => {
                *gate = RateGate::new(self.session.rate().bits_per_second, at);
                Vec::new()
            }
            Reopened::HandedOver => {
                (*gate, *governor) = (RateGate::new(self.session.rate().bits_per_second, at), Governor::new(at));
                Vec::new()
            }
        })
    }

    /// Hands the share on from the encoder that failed with `failed`, leaving behind the pictures of a device that was
    /// rebuilt.
    fn recover(
        &mut self,
        failed: EncodeError,
        progress: &Progress,
        waiting: &mut Option<S::Picture>,
        settling: &mut Settling<S::Picture, S::Shape>,
    ) -> Result<(), Fail> {
        let rebuilds = self.rebuilds;
        self.hand_over(failed, progress)?;
        if self.rebuilds != rebuilds {
            progress.stages.discarded(leave_behind(waiting, settling, false));
        }
        Ok(())
    }

    /// Hands the share, and any change asked, on from the encoder that failed: a lost device is rebuilt once, then the
    /// next of the codec's encoders that opens takes over, on a key frame; the failed one is dropped undrained, and a
    /// hardware one is gone back to at the next reopen. With none left, the last refusal ends the share.
    fn hand_over(&mut self, failed: EncodeError, progress: &Progress) -> Result<(), Fail> {
        self.survey.failed();
        let (changes, rebuilt) = self.input.hand_over(&mut self.session, &mut self.ladder, failed)?;
        self.handed(changes, progress, rebuilt)
    }

    /// The share runs on the encoder that took over, whose open made `changes`; where its device was `rebuilt`, the
    /// capture is opened anew on the new one first.
    fn handed(&mut self, changes: media::Changes, progress: &Progress, rebuilt: bool) -> Result<(), Fail> {
        if rebuilt && self.input.rebuilt(&self.session)? {
            self.rebuilds += 1;
        }
        self.asked = None;
        progress.reopens.lock().unwrap_or_else(PoisonError::into_inner).record(Cause::EncoderFailed, changes);
        self.key_cause = Some(KeyCause::Reopened);
        self.take_next(progress);
        if let Ok(mut told) = progress.handed_over.lock() {
            *told = Some(crate::codec::encoder_route(self.encoder()));
        }
        Ok(())
    }

    /// The share takes the change its encoder made: its picture, as the encoder runs it, which another encoder taking
    /// over may change too, and its rates; the engine is told them.
    fn take_next(&mut self, progress: &Progress) {
        let next = self.next.take();
        let source = next.as_ref().map_or(self.source, |next| next.source);
        let layout = running(&self.session, source);
        if next.is_none() && layout == self.layout {
            return;
        }
        if let Some(Next { scaled_from, envelope, .. }) = next {
            (self.scaled_from, self.envelope) = (scaled_from, envelope);
        }
        (self.source, self.layout) = (source, layout);
        let (scaled_from, envelope) = (self.scaled_from, self.envelope);
        let (width, height) = layout.size();
        let resized = Resized { width, height, scaled_from, envelope };
        if let Ok(mut told) = progress.resized.lock() {
            *told = Some(resized);
        }
        self.note_reduced(progress);
    }
}

impl<H: Shape> Report<H> {
    /// The one line the person reads.
    pub fn line(&self) -> String {
        let ms = |t: Option<Duration>| t.map_or(0.0, |t| t.as_secs_f64() * 1000.0);
        format!(
            "Encoded {} pictures of {}x{} with {}: {:.1} ms a picture (slowest {:.1}), {:.1} Mbit/s, {} key frames, the largest {:.0} KB; {} skipped; the engine moved the rate {} times, to {:.1} Mbit/s.",
            self.frames,
            self.layout.size().0,
            self.layout.size().1,
            self.encoder,
            ms(self.timings.mean()),
            ms(self.timings.slowest()),
            megabits(self.bytes, self.span).unwrap_or(0.0),
            self.key_frames,
            self.largest_key_frame as f64 / 1000.0,
            self.skipped,
            self.rate_changes,
            f64::from(self.final_rate) / 1e6,
        )
    }
}

/// Where this account's own picture is lent, for its card.
pub struct Preview<'a> {
    /// The card's slot.
    pub slot: &'a picture::Slot,
    /// The GPU the window paints the card with, as it is now, where it is known: asked at each lend, since a window that
    /// lost its device paints with a new one.
    pub painting: Option<&'a dyn Fn() -> picture::Painting>,
}

/// Lends each picture to the share's card, if its window took the last: Linux's captured DMA-BUF as it is, Windows'
/// drawn at the card's size into a ring the window's device opened at the first lend.
#[derive(Default)]
pub struct Lender {
    /// The ring, once tried, the count of device rebuilds and the window's device it was made under: none where the
    /// window's GPU is not the encoder's or paints through Vulkan. Either device made again is a new one, and the ring
    /// is made again with it, a size the old one refused tried afresh.
    #[cfg(windows)]
    ring: Option<(u32, Option<media::D3d12Device>, Option<media::Preview>)>,
}

impl Lender {
    #[cfg_attr(not(windows), expect(unused_variables, reason = "Linux lends the captured buffer as it is"))]
    fn lend(&mut self, preview: Option<&Preview<'_>>, frame: &Frame, session: &Session, rebuilds: u32) {
        let Some(preview) = preview else { return };
        #[cfg(target_os = "linux")]
        preview.slot.lend(frame);
        #[cfg(windows)]
        {
            let window = preview.painting.and_then(|now| now().device().cloned());
            if self.ring.as_ref().is_none_or(|(made, on, _)| *made != rebuilds || *on != window) {
                let ring = window.as_ref().and_then(|window| media::Preview::new(&session.gpu()?, &window.0));
                self.ring = Some((rebuilds, window, ring));
            }
            if let Some((_, _, Some(ring))) = &mut self.ring {
                preview.slot.lend_preview(|within| ring.lend(frame, within));
            }
        }
    }
}

/// Runs `started` for [`CAPTURE_FOR`], encoding every picture and sending none, as a share would by default.
pub(super) fn count(started: Started) -> Result<Report, Fail> {
    let until = Instant::now() + CAPTURE_FOR;
    started.run(None, None, &Progress::default(), || Instant::now() >= until, || true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use capture::Pixels;
    use media::{Backend, Path};

    fn sized(width: u32, height: u32) -> Layout {
        Layout { pixels: Pixels::Bgrx, width, height }
    }

    /// A stand-in capture that says how many pictures it dropped and made, so which one is running can be told.
    struct Counting {
        dropped: u64,
        delivered: u64,
    }

    impl Counting {
        fn dropped(dropped: u64) -> Self {
            Self { dropped, delivered: 0 }
        }
    }

    impl Frames for Counting {
        fn next(&self, _: Duration) -> Result<Option<Frame>, CaptureError> {
            Ok(None)
        }

        fn dropped(&self) -> u64 {
            self.dropped
        }

        fn delivered(&self) -> u64 {
            self.delivered
        }
    }

    #[test]
    fn a_rebuilt_device_reopens_the_capture_that_draws_into_it_and_leaves_one_that_does_not() {
        let opened = Arc::new(AtomicU64::new(0));
        let count = Arc::clone(&opened);
        let mut drawing = Capturing::<()>::drawing_into(
            Box::new(Counting::dropped(0)),
            Box::new(move |_: &()| -> Result<Box<dyn Frames>, Fail> {
                count.fetch_add(1, Ordering::Relaxed);
                Ok(Box::new(Counting::dropped(1)))
            }),
        );
        assert!(drawing.rebuilt(&()).is_ok_and(|reopened| reopened));
        assert_eq!(opened.load(Ordering::Relaxed), 1, "opened once, on the new device");
        assert_eq!(drawing.frames.dropped(), 1, "the capture now running is the new one");
        let mut apart = Capturing::<()>::fixed(Box::new(Counting::dropped(0)));
        assert!(apart.rebuilt(&()).is_ok_and(|reopened| !reopened));
        assert_eq!(apart.frames.dropped(), 0, "a capture that is not on the device is left as it is");
    }

    #[test]
    fn what_the_capture_counted_goes_on_growing_across_a_rebuilt_device() {
        let mut drawing = Capturing::<()>::drawing_into(
            Box::new(Counting { dropped: 5, delivered: 100 }),
            Box::new(|_: &()| -> Result<Box<dyn Frames>, Fail> {
                Ok(Box::new(Counting { dropped: 2, delivered: 30 }))
            }),
        );
        assert_eq!((drawing.delivered(), drawing.dropped()), (100, 5));
        assert!(drawing.rebuilt(&()).is_ok_and(|reopened| reopened));
        assert_eq!((drawing.delivered(), drawing.dropped()), (130, 7), "the new capture's, after the old one's");
        let stages = Stages::default();
        drawing.count_into(&stages);
        let counted = stages.counted();
        assert_eq!((counted.delivered, counted.slot_dropped), (130, 7), "what a look differences");
    }

    #[test]
    fn a_picture_replaced_while_a_new_size_settles_is_lost_and_one_that_settles_is_not() {
        let start = Instant::now();
        let mut settling = Settling::new();
        assert!(!settling.hold(1, sized(960, 540), start), "the first picture of a size replaces none");
        assert!(settling.hold(2, sized(960, 540), start + SETTLE / 4), "the same size again replaces the first");
        assert!(settling.hold(3, sized(1000, 560), start + SETTLE / 2), "another size replaces it too");
        assert!(settling.let_go(), "the stream's own size coming back lets a held picture go");
        assert!(!settling.let_go(), "nothing held, nothing lost");
        settling.hold(4, sized(960, 540), start);
        assert!(settling.settled(start + SETTLE).is_some());
        assert!(!settling.let_go(), "a settled picture was taken, not lost");
    }

    #[test]
    fn a_rebuilt_device_costs_the_picture_in_hand_the_one_the_pace_kept_and_the_one_a_size_was_settling() {
        let start = Instant::now();
        let (mut waiting, mut settling) = (Some(1), Settling::new());
        settling.hold(2, sized(960, 540), start);
        assert_eq!(leave_behind(&mut waiting, &mut settling, true), 3);
        assert!(waiting.is_none() && !settling.holds(), "both are let go");
        assert_eq!(leave_behind(&mut waiting, &mut settling, false), 0, "nothing left, nothing lost");
        assert_eq!(leave_behind(&mut waiting, &mut settling, true), 1, "the one in hand alone");
    }

    #[test]
    fn an_encoder_that_changes_rate_in_place_takes_every_target_and_one_that_reopens_waits_for_the_gate() {
        let rate = |bits| Rate { bits_per_second: bits, frames_per_second: 60 };
        let running = rate(20_000_000);
        let mut gate = RateGate::new(running.bits_per_second, Duration::ZERO);
        let now = Duration::from_millis(500);
        assert_eq!(taken(rate(19_000_000), running, true, &mut gate, now), Some(rate(19_000_000)), "5 % down");
        assert_eq!(taken(rate(30_000_000), running, true, &mut gate, now), Some(rate(30_000_000)), "at once up");
        assert_eq!(taken(running, running, true, &mut gate, now), None, "the running rate changes nothing");
        assert_eq!(taken(rate(19_000_000), running, false, &mut gate, now), None, "under the gate's 20 %");
        assert_eq!(taken(rate(30_000_000), running, false, &mut gate, now), None, "an increase waits 2 s");
    }

    #[test]
    fn a_new_size_is_taken_once_it_has_held_and_its_newest_picture_with_it() {
        let start = Instant::now();
        let mut settling = Settling::new();
        settling.hold(1, sized(960, 540), start);
        assert_eq!(settling.settled(start + SETTLE / 2), None, "not yet");
        settling.hold(2, sized(960, 540), start + SETTLE / 2);
        assert_eq!(
            settling.settled(start + SETTLE),
            Some((2, sized(960, 540))),
            "the newest picture, timed from the first"
        );
        assert_eq!(settling.settled(start + SETTLE * 2), None, "taken once");
    }

    #[test]
    fn a_size_still_changing_waits_and_the_streams_own_size_lets_it_go() {
        let start = Instant::now();
        let mut settling = Settling::new();
        settling.hold(1, sized(960, 540), start);
        settling.hold(2, sized(1000, 560), start + SETTLE / 2);
        assert_eq!(settling.settled(start + SETTLE), None, "a window still being dragged");
        assert_eq!(settling.settled(start + SETTLE / 2 + SETTLE), Some((2, sized(1000, 560))));
        settling.hold(3, sized(960, 540), start);
        settling.let_go();
        assert_eq!(settling.settled(start + SETTLE * 9), None, "back to the stream's size before it settled");
    }

    fn at_most(ceiling: choices::Ceiling) -> ShareQuality {
        ShareQuality { ceiling, ..ShareQuality::default() }
    }

    fn fifteen(size: Size) -> Step {
        Step { size, frames_per_second: NonZeroU32::new(15).unwrap() }
    }

    #[test]
    fn a_share_comes_down_through_the_sizes_a_person_can_choose() {
        let mut steps = starting_steps(&at_most(choices::Ceiling::Automatic), sized(7680, 4320));
        let mut sides = Vec::new();
        while steps.down(7680, 4320) {
            sides.push(steps.now().size.fit(7680, 4320).1);
        }
        sides.dedup();
        assert_eq!(sides[1..], [1440, 1080, 720], "after the frame rate, each choice below the screen's own");
    }

    #[test]
    fn a_4k_share_at_60_under_4_mbit_starts_at_15_and_goes_neither_up_nor_back_to_the_choice() {
        let four_k = sized(3840, 2160);
        let mut steps = starting_steps(&at_most(choices::Ceiling::Mbit4), four_k);
        assert_eq!(steps.now(), fifteen(Size::Source), "the first step whose floor fits under 4 Mbit/s");
        assert_eq!(steps.now().size.fit(four_k.width, four_k.height), (3840, 2160), "its size kept");
        let room = Judged::Under;
        assert_eq!(move_on(&mut steps, room, (3840, 2160)), None, "room does not lift a ceiling");
        assert_eq!(steps.now(), fifteen(Size::Source));
        assert_eq!(
            starting_steps(&at_most(choices::Ceiling::Automatic), four_k).now(),
            Step { size: Size::Source, frames_per_second: NonZeroU32::new(60).unwrap() },
            "no ceiling, the choice"
        );
    }

    #[test]
    fn the_judges_walk_a_share_on_from_where_its_ceiling_began_it_and_back_to_it() {
        let four_k = sized(3840, 2160);
        let mut steps = starting_steps(&at_most(choices::Ceiling::Mbit4), four_k);
        let encoder = Judged::Over(Limit::Encoder);
        assert_eq!(move_on(&mut steps, encoder, (3840, 2160)), Some(Cause::Overused));
        assert_eq!(steps.now(), fifteen(Size::ShortSide(1440)), "the size comes down once the frame rate cannot");
        let rate = Judged::Over(Limit::Rate);
        assert_eq!(move_on(&mut steps, rate, (3840, 2160)), Some(Cause::Busy));
        let room = Judged::Under;
        assert_eq!(move_on(&mut steps, room, (3840, 2160)), Some(Cause::Roomy));
        assert_eq!(move_on(&mut steps, room, (3840, 2160)), Some(Cause::Roomy));
        assert_eq!(steps.now(), fifteen(Size::Source), "back to where it began");
        assert!(steps.at_start());
    }

    #[test]
    fn a_share_is_told_reduced_by_what_holds_it_below_the_choice_and_not_at_all_when_it_sends_it() {
        let four_k = (3840, 2160);
        let ceiling = Some(4_000_000);
        let mut steps = starting_steps(&at_most(choices::Ceiling::Mbit4), sized(four_k.0, four_k.1));
        assert_eq!(
            reduced_from(&steps, four_k, ceiling, None),
            Some(screens::Reduced {
                from_size: None,
                from_frame_rate: 60,
                cause: screens::Reduction::Ceiling { bits_per_second: 4_000_000 }
            }),
            "started low to fit the ceiling"
        );
        steps.down(four_k.0, four_k.1);
        let told = reduced_from(&steps, four_k, ceiling, Some(Limit::Rate)).expect("reduced");
        assert_eq!(told.from_size, Some(four_k), "the size came down too");
        assert_eq!(told.cause, screens::Reduction::Busy { ceiling });
        let told = reduced_from(&steps, four_k, None, Some(Limit::Encoder)).expect("reduced");
        assert_eq!(told.cause, screens::Reduction::Encoder);
        let whole = starting_steps(&ShareQuality::default(), sized(four_k.0, four_k.1));
        assert_eq!(reduced_from(&whole, four_k, None, None), None);
    }

    #[test]
    fn the_engine_moves_a_shares_bits_and_its_step_keeps_the_frame_rate() {
        let sixty = NonZeroU32::new(60).unwrap();
        let envelope = net::Envelope::for_screen(1920, 1080, 60, None);
        assert_eq!(
            encoder_rate(envelope.start, envelope, sixty),
            Ok(Rate { bits_per_second: u32::try_from(envelope.start).unwrap(), frames_per_second: 60 })
        );
        let kept = encoder_rate(envelope.max * 2, envelope, sixty).unwrap();
        assert_eq!(u64::from(kept.bits_per_second), envelope.max, "inside the envelope");
    }

    fn encoder(backend: Backend, device: Option<&str>) -> media::Encoder {
        let name = if backend == Backend::Vulkan { media::Name::H264Vulkan } else { media::Name::H264Vaapi };
        let runs = media::Runs::Gpu(media::Device {
            backend,
            node: device.map(str::to_string),
            vendor: None,
            validation: media::Validation::Off,
        });
        media::Encoder { name, runs, multiple: media::Multiple::EVEN }
    }

    fn software() -> media::Encoder {
        let runs = media::Runs::Cpu {
            convert: Some(media::Device {
                backend: Backend::Vulkan,
                node: None,
                vendor: None,
                validation: media::Validation::Off,
            }),
        };
        media::Encoder { name: media::Name::X264, runs, multiple: media::Multiple::EVEN }
    }

    #[test]
    fn the_first_encoder_in_order_that_opens_takes_the_screen() {
        let (vulkan, vaapi) = (encoder(Backend::Vulkan, None), encoder(Backend::VaApi, Some("renderD128")));
        let order = || VecDeque::from([vulkan.clone(), vaapi.clone()]);
        let (_, opened) = first_open(&mut order(), EncodeError::Device, |_| Ok(())).unwrap();
        assert_eq!(opened.path(), Path::Hardware(Backend::Vulkan), "the first in order");

        let refuses_vulkan = |tried: &media::Encoder| {
            if tried.path() == Path::Hardware(Backend::Vulkan) { Err(EncodeError::Device) } else { Ok(()) }
        };
        let (_, opened) = first_open(&mut order(), EncodeError::Device, refuses_vulkan).unwrap();
        assert_eq!(opened.path(), Path::Hardware(Backend::VaApi), "the next of this device's took it");
        let none = first_open(&mut order(), EncodeError::Device, |_| Err::<(), _>(EncodeError::Open));
        assert_eq!(none.err(), Some(EncodeError::Open), "none opens: the last refusal");
    }

    #[test]
    fn a_failed_encoder_hands_over_to_the_next_that_opens_and_the_end_is_the_last_refusal() {
        let (vulkan, vaapi) = (encoder(Backend::Vulkan, None), encoder(Backend::VaApi, Some("renderD128")));
        let mut order = VecDeque::from([vulkan, vaapi, software()]);
        let (_, running) = first_open(&mut order, EncodeError::Device, |_| Ok(())).unwrap();
        assert_eq!(running.path(), Path::Hardware(Backend::Vulkan));
        // The running encoder fails; VA-API refuses to take over, so the processor does.
        let refuses_vaapi = |tried: &media::Encoder| {
            if tried.path() == Path::Hardware(Backend::VaApi) { Err(EncodeError::Open) } else { Ok(()) }
        };
        let (_, running) =
            first_open(&mut order, EncodeError::Encoder(media::AvError::Other(-5)), refuses_vaapi).unwrap();
        assert_eq!(running.path(), Path::Software, "software last");
        let ended = first_open(&mut order, EncodeError::Encoder(media::AvError::Other(-5)), |_| Ok(()));
        assert_eq!(
            ended.err(),
            Some(EncodeError::Encoder(media::AvError::Other(-5))),
            "with none left, the failure ends the share"
        );
    }
}
