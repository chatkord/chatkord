use super::handle::{Ends, MicrophoneSender, SpeakerSender, hear_stream};
use super::mixer::{Frames, Slot, Source, ticks};
use super::thread::{Local, turn};
use super::*;
use crate::devices::DevicesRefused;
use crate::engine::Engine;
use crate::keeper::Keeping;
use crate::loopback::Toned;
use crate::processing::Processing;
use libwebrtc::audio_frame::AudioFrame;
use libwebrtc::native::audio_mixer::AudioMixerSource;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::task::{Context, Poll, Waker};
use std::thread::{self, Thread};
use std::time::{Duration, Instant};
use tokio::time::{sleep, timeout};

const WAIT: Duration = Duration::from_secs(15);
/// A bound on a keeper's call far past any hold of a test, so one that holds a keeper never has it given up on.
const PATIENT: Duration = Duration::from_secs(3600);

/// A microphone that hears a 440 Hz tone at a quarter of full scale, a frame each 10 ms since it opened.
struct Tone {
    opened: Instant,
    given: u32,
}

impl MicrophoneEnd for Tone {
    fn frame(&mut self) -> Option<Mono> {
        let due = u32::try_from(self.opened.elapsed().as_millis() / 10).unwrap_or(u32::MAX);
        (self.given < due).then(|| {
            let frame = crate::tone::frame(440.0, 8_000.0, 1.0, self.given);
            self.given += 1;
            let mut mono = [0; FRAME];
            mono.copy_from_slice(&frame.data);
            mono
        })
    }
    fn delay(&self) -> Duration {
        Duration::ZERO
    }
}

/// A device whose stream is open and stays so: the keeper half of every fake whose end does the work.
struct Always;

impl Microphone for Always {
    fn keep(&mut self, _now: Instant) {}
    fn choose(&mut self, _device: Option<&str>) -> Result<(), DevicesRefused> {
        Ok(())
    }
    fn is_open(&self) -> bool {
        true
    }
}

impl Speaker for Always {
    fn keep(&mut self, _now: Instant) {}
    fn choose(&mut self, _device: Option<&str>) -> Result<(), DevicesRefused> {
        Ok(())
    }
    fn is_open(&self) -> bool {
        true
    }
}

/// A speaker end that keeps the loudest sample of what it was given since last asked.
struct Kept(Arc<AtomicI32>, Duration);

impl SpeakerEnd for Kept {
    fn play(&mut self, frame: &Stereo) {
        let loudest = frame.iter().map(|sample| i32::from(sample.unsigned_abs())).max().unwrap_or(0);
        self.0.fetch_max(loudest, Ordering::Relaxed);
    }
    fn delay(&self) -> Duration {
        self.1
    }
}

fn toned() -> MicrophoneOpened {
    Ok((Box::new(Always), Box::new(Tone { opened: Instant::now(), given: 0 })))
}

fn kept(played: &Arc<AtomicI32>) -> SpeakerOpened {
    Ok((Box::new(Always), Box::new(Kept(Arc::clone(played), Duration::ZERO))))
}

struct Fake(Arc<AtomicI32>);

impl Sound for Fake {
    fn microphone(&self, _device: Option<&str>, _wake: Thread) -> MicrophoneOpened {
        toned()
    }
    fn speaker(&self, _device: Option<&str>, _now: Instant) -> SpeakerOpened {
        kept(&self.0)
    }
    fn listing(&self) -> Result<Listing, DevicesRefused> {
        let listed = |id: &str| Listed { id: id.into(), label: id.to_uppercase() };
        Ok(Listing {
            microphones: vec![listed("mic")],
            speakers: vec![listed("left"), listed("right")],
            default_microphone: None,
            default_speaker: Some("right".into()),
        })
    }
}

fn engine() -> (Engine, Arc<AtomicI32>) {
    let played = Arc::new(AtomicI32::new(0));
    (Engine::start(Arc::new(Fake(Arc::clone(&played))), Processing::PROCESSOR), played)
}

/// Waits until `what` holds, as a test without a runtime waits for the chain's thread to answer an ask.
pub(crate) fn eventually(what: impl Fn() -> bool, why: &str) {
    let began = Instant::now();
    while !what() {
        assert!(began.elapsed() < WAIT, "{why}");
        thread::sleep(Duration::from_millis(5));
    }
}

/// Devices for a test that needs some: a microphone that hears a tone and a speaker that plays to nobody.
pub(crate) fn silent() -> Arc<dyn Sound> {
    Arc::new(Fake(Arc::new(AtomicI32::new(0))))
}

#[test]
fn our_devices_are_listed_the_default_first_by_what_it_follows() {
    let (engine, _) = engine();
    let devices = engine.devices().expect("ours");
    let outputs: Vec<_> = devices.outputs().iter().map(|d| (d.label().to_owned(), d.id.clone())).collect();
    assert_eq!(
        outputs,
        [("RIGHT".into(), None), ("LEFT".into(), Some("left".into())), ("RIGHT".into(), Some("right".into()))]
    );
    assert_eq!(devices.inputs()[0].id, None, "a default with no name said is still first");
}

#[test]
fn a_chain_with_no_device_asked_takes_no_turn_and_one_asked_turns_until_let_go() {
    /// Twenty turns of a thread kept turning; a parked one may wake once or twice, as `park` allows.
    const SPAN: Duration = Duration::from_millis(200);
    const SPURIOUS: u64 = 2;
    let turns = |devices: &crate::Devices| {
        let health = devices.health();
        [health.turns, health.microphone.turns, health.speaker.turns]
    };
    let still = |before: [u64; 3], after: [u64; 3], how: &str| {
        for (before, after) in before.into_iter().zip(after) {
            assert!(after.saturating_sub(before) <= SPURIOUS, "{how}: {before} turns, then {after}");
        }
    };
    let (engine, _) = engine();
    let devices = engine.devices().expect("ours");
    still([0; 3], turns(&devices), "made");
    let made = turns(&devices);
    thread::sleep(SPAN);
    still(made, turns(&devices), "nothing asked: neither the mix thread nor a keeper woke");

    let capture = devices.capture().expect("the microphone is asked for");
    eventually(|| capture.is_active(), "the microphone opens");
    eventually(|| turns(&devices)[0] > 10, "the mix thread turns while the microphone is open");
    drop(capture);
    eventually(|| !devices.is_capturing(), "the last holder closes it");
    eventually(|| devices.health().microphone.doing == Keeping::Idle, "its keeper has let it go");
    thread::sleep(SPAN / 4);
    let settled = turns(&devices);
    thread::sleep(SPAN);
    still(settled, turns(&devices), "let go: both threads sleep until asked again");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn our_microphone_is_processed_into_its_level_and_closed_with_its_last_holder() {
    let (engine, _) = engine();
    let devices = engine.devices().expect("ours");
    let chain = devices.chain();
    assert_eq!(chain.level(), None, "no level while the microphone is closed");
    let capture = devices.capture().expect("the microphone is asked for");
    let level = timeout(WAIT, async {
        loop {
            if let Some(level) = chain.level().filter(|level| *level > 5) {
                break level;
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the tone is heard through the processor");
    assert!(devices.is_capturing() && capture.is_active());
    drop(capture);
    assert!(!devices.is_capturing(), "the last holder closes it");
    assert_eq!(chain.level(), None);
    println!("level {level}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn processing_changed_mid_call_applies_from_the_next_frames_with_the_microphone_kept_open() {
    use crate::processing::{Gain, Noise};
    let plain = Processing { echo_cancellation: false, noise: Noise::Off, gain: Gain::Fixed(0.0), gate: None };
    let engine = Engine::start(silent(), plain);
    let devices = engine.devices().expect("ours");
    let chain = devices.chain();
    let capture = devices.capture().expect("the microphone is asked for");
    eventually(|| capture.is_active(), "the microphone opens");
    sleep(Duration::from_millis(300)).await;
    chain.level();
    sleep(Duration::from_millis(300)).await;
    let before = chain.level().expect("the microphone is open");
    devices.process(Processing { gain: Gain::Fixed(-20.0), ..plain });
    sleep(Duration::from_millis(300)).await;
    chain.level();
    sleep(Duration::from_millis(300)).await;
    let after = chain.level().expect("the microphone is open");
    println!("the tone at 0 dB: {before} of 100; at -20 dB: {after}");
    assert!(before > 15 && after * 5 < before, "taken at once: {before} then {after}");
    assert!(capture.is_active(), "with the microphone kept open, never closed and opened again");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_received_track_is_played_at_its_gain_and_metered_before_it() {
    let (engine, played) = engine();
    let devices = engine.devices().expect("ours");
    let chain = devices.chain();
    let _playing = devices.play().expect("the speaker is asked for");
    let toned = Toned::start(&engine.factory).await;
    let (gain, peak) = (Arc::new(AtomicU32::new(1.0_f32.to_bits())), Arc::new(AtomicU32::new(0)));
    let _heard = chain.hear(toned.track.clone(), 1, Arc::clone(&gain), Arc::clone(&peak));
    let loud = |played: &AtomicI32| played.swap(0, Ordering::Relaxed);
    timeout(WAIT, async {
        while loud(&played) < 4_000 {
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the tone is played on our speaker");
    gain.store(0.0_f32.to_bits(), Ordering::Relaxed);
    sleep(Duration::from_millis(200)).await;
    loud(&played);
    peak.store(0, Ordering::Relaxed);
    sleep(Duration::from_millis(300)).await;
    assert!(loud(&played) < 50, "at no gain it plays silence");
    assert!(peak.load(Ordering::Relaxed) > 4_000, "and its meter still reads it");
}

/// A room whose microphone hears the speaker 40 ms after it plays, and nothing else: a pure echo.
#[derive(Clone, Default)]
struct Room(Arc<Mutex<VecDeque<Mono>>>);

/// Frames of the speaker in the air before the microphone hears them.
const IN_THE_AIR: usize = 4;

impl MicrophoneEnd for Room {
    fn frame(&mut self) -> Option<Mono> {
        let mut air = lock(&self.0);
        (air.len() > IN_THE_AIR).then(|| air.pop_front()).flatten()
    }
    fn delay(&self) -> Duration {
        Duration::ZERO
    }
}

impl SpeakerEnd for Room {
    fn play(&mut self, frame: &Stereo) {
        let mut heard = [0; FRAME];
        for (sample, pair) in heard.iter_mut().zip(frame.as_chunks::<2>().0) {
            *sample = pair[0];
        }
        lock(&self.0).push_back(heard);
    }
    fn delay(&self) -> Duration {
        Duration::from_millis(10 * IN_THE_AIR as u64)
    }
}

impl Sound for Room {
    fn microphone(&self, _device: Option<&str>, _wake: Thread) -> MicrophoneOpened {
        Ok((Box::new(Always), Box::new(self.clone())))
    }
    fn speaker(&self, _device: Option<&str>, _now: Instant) -> SpeakerOpened {
        Ok((Box::new(Always), Box::new(self.clone())))
    }
    fn listing(&self) -> Result<Listing, DevicesRefused> {
        Ok(Listing::default())
    }
}

/// Frames of the echo the processor hears before what it sends is measured: AEC3 leaves its initial state after
/// 2.5 s of render heard (`initial_state_seconds`), and is given half a second more.
const LEARNT: u64 = 300;

/// The frames of a received track a test sends one at a time, and the stream the chain polls them from.
fn fed() -> (mpsc::Sender<AudioFrame<'static>>, Frames) {
    let (sender, receiver) = mpsc::channel();
    let frames = futures_util::stream::poll_fn(move |_| match receiver.try_recv() {
        Ok(frame) => Poll::Ready(Some(frame)),
        Err(mpsc::TryRecvError::Empty) => Poll::Pending,
        Err(mpsc::TryRecvError::Disconnected) => Poll::Ready(None),
    });
    (sender, Box::pin(frames))
}

/// The chain's turns taken by a test one at a time, each a 10 ms step of the frames it is given, so a loaded
/// machine cannot bunch them: the state and ends `Chain::start` makes, with no thread.
struct Stepped {
    shared: Arc<Shared>,
    own: Local,
    ends: Ends,
    microphone: MicrophoneSender,
    speaker: SpeakerSender,
}

impl Stepped {
    fn new(processing: Processing) -> Self {
        let (ends, microphone, speaker) = Ends::channels();
        Self {
            shared: Arc::new(Shared::new(silent(), PATIENT)),
            own: Local::new(processing),
            ends,
            microphone,
            speaker,
        }
    }

    /// Takes `microphone` and `speaker` as the ends in use from the next step on.
    fn open(&self, microphone: impl MicrophoneEnd + 'static, speaker: impl SpeakerEnd + 'static) {
        self.microphone.send(Some(Box::new(microphone))).expect("the receiver is held");
        self.speaker.send(Some(Box::new(speaker))).expect("the receiver is held");
    }

    /// Mixes `frames` as a received track of one channel.
    fn hear(&self, frames: Frames, gain: Arc<AtomicU32>, peak: Arc<AtomicU32>) -> super::Heard {
        hear_stream(&self.shared, frames, 1, gain, peak)
    }

    /// One turn of the chain.
    fn step(&mut self) {
        let mut context = Context::from_waker(Waker::noop());
        turn(&self.shared, &|_| {}, &mut self.own, &mut self.ends, &mut context);
    }
}

/// How loud what is sent of a pure echo is, from 0 to 100, once the processor has heard [`LEARNT`] frames of it,
/// with `processing`: the median of ten tenths of a second's loudest; and how loud the echo itself is.
fn sent_of_an_echo(processing: Processing) -> (u8, u8) {
    let mut call = Stepped::new(processing);
    let room = Room::default();
    call.open(room.clone(), room);
    let (tone, frames) = fed();
    let (gain, peak) = (Arc::new(AtomicU32::new(1.0_f32.to_bits())), Arc::new(AtomicU32::new(0)));
    let _heard = call.hear(frames, gain, Arc::clone(&peak));
    // The room's microphone hears nothing but the speaker, so every frame the chain captures is one of the echo.
    let mut sent = 0;
    let mut step = |call: &mut Stepped| {
        tone.send(crate::tone::frame(440.0, 8_000.0, 1.0, sent)).expect("the chain holds the track");
        sent += 1;
        call.step();
    };
    for _ in 0..LEARNT {
        step(&mut call);
    }
    call.shared.take_level();
    peak.store(0, Ordering::Relaxed);
    let mut left = Vec::new();
    for _ in 0..10 {
        for _ in 0..10 {
            step(&mut call);
        }
        left.push(call.shared.take_level().expect("the microphone is heard"));
    }
    left.sort_unstable();
    let echo = peak.load(Ordering::Relaxed);
    (left[left.len() / 2], u8::try_from(echo * 100 / u32::from(i16::MAX.unsigned_abs())).unwrap_or(100))
}

#[test]
fn what_the_speaker_plays_is_cancelled_from_the_microphone_that_hears_it() {
    // Gain control lifts whatever is left, so the canceller is measured without it.
    let cancelling = Processing { gain: crate::processing::Gain::Fixed(0.0), ..Processing::PROCESSOR };
    let uncancelled = Processing { echo_cancellation: false, ..cancelling };
    let (cancelled, echo) = sent_of_an_echo(cancelling);
    let (kept, _) = sent_of_an_echo(uncancelled);
    let strong = Processing { noise: crate::processing::Noise::Rnnoise, ..Processing::PROCESSOR };
    let (standard_agc, _) = sent_of_an_echo(Processing::PROCESSOR);
    let (strong_agc, _) = sent_of_an_echo(strong);
    println!("the echo peaks at {echo} of 100; sent of it: {cancelled} cancelled, {kept} not");
    println!("with automatic gain: {standard_agc} with the standard suppressor, {strong_agc} with the strong one");
    assert!(echo > 10, "the speaker played the tone the microphone hears");
    assert!(u32::from(cancelled) * 2 < u32::from(kept), "cancelling takes at least 6 dB more of the echo");
}

/// A microphone whose open waits for the test to say how it ends, and a speaker that opens at once.
struct Gated {
    ends: Mutex<mpsc::Receiver<Result<(), DevicesRefused>>>,
    opens: Arc<AtomicU32>,
}

impl Sound for Gated {
    fn microphone(&self, _device: Option<&str>, _wake: Thread) -> MicrophoneOpened {
        self.opens.fetch_add(1, Ordering::Relaxed);
        lock(&self.ends).recv().unwrap_or(Err(DevicesRefused::Unavailable))?;
        toned()
    }
    fn speaker(&self, _device: Option<&str>, _now: Instant) -> SpeakerOpened {
        kept(&Arc::default())
    }
    fn listing(&self) -> Result<Listing, DevicesRefused> {
        Ok(Listing::default())
    }
}

/// An engine on [`Gated`] devices, how a microphone's open is ended, and how many were begun.
fn gated() -> (Engine, mpsc::Sender<Result<(), DevicesRefused>>, Arc<AtomicU32>) {
    let (end, ends) = mpsc::channel();
    let opens = Arc::new(AtomicU32::new(0));
    let sound = Gated { ends: Mutex::new(ends), opens: Arc::clone(&opens) };
    (Engine::bounded(Arc::new(sound), Processing::PROCESSOR, PATIENT), end, opens)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_microphone_whose_open_does_not_return_holds_up_no_handle() {
    let (engine, end, _) = gated();
    let devices = engine.devices().expect("ours");
    let chain = devices.chain();
    let mut sounding = devices.sounding();
    let toned = Toned::start(&engine.factory).await;
    let capture = devices.capture().expect("asked for, not opened");
    assert_eq!(sounding.borrow().microphone, Standing::Opening, "said at once");
    eventually(|| devices.health().microphone.doing == Keeping::Opening, "its keeper is in the open");
    assert_eq!(chain.level(), None, "the level answers");
    devices.process(Processing::PROCESSOR);
    let playing = devices.play().expect("the speaker is asked for behind it");
    let speaker_open = Sounding { microphone: Standing::Opening, speaker: Standing::Open };
    timeout(WAIT, sounding.wait_for(|sounding| *sounding == speaker_open))
        .await
        .expect("in time")
        .expect("the speaker opens while the microphone is held in its open");
    eventually(|| playing.is_active(), "and plays");
    assert_eq!(devices.choose_input(None), Ok(()));
    let _heard = chain.hear(toned.track.clone(), 1, Arc::default(), Arc::default());
    assert!(!capture.is_active() && !devices.is_capturing());
    let health = devices.health();
    assert_eq!((health.microphone.doing, health.speaker.doing), (Keeping::Opening, Keeping::Idle));
    assert_eq!(*sounding.borrow(), speaker_open, "all while it opens");
    end.send(Ok(())).expect("the open is let go");
    let open = Sounding { microphone: Standing::Open, speaker: Standing::Open };
    timeout(WAIT, sounding.wait_for(|sounding| *sounding == open)).await.expect("in time").expect("both open");
    assert!(capture.is_active());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_microphone_is_said_and_tried_again_only_when_what_is_asked_changes() {
    let (engine, end, opens) = gated();
    let devices = engine.devices().expect("ours");
    let mut sounding = devices.sounding();
    let refused = Standing::Refused(DevicesRefused::Refused);
    end.send(Err(DevicesRefused::Refused)).expect("the first open is refused");
    let capture = devices.capture().expect("asked for");
    timeout(WAIT, sounding.wait_for(|sounding| sounding.microphone == refused)).await.expect("in time").expect("said");
    let second = devices.capture().expect("a second hold asks nothing new");
    sleep(Duration::from_millis(200)).await;
    assert_eq!((opens.load(Ordering::Relaxed), sounding.borrow().microphone), (1, refused), "not tried again");
    assert!(!capture.is_active());
    assert_eq!(devices.choose_input(None), Ok(()));
    assert_eq!(sounding.borrow().microphone, Standing::Opening, "a choice after a refusal is a new try");
    end.send(Ok(())).expect("the next open is taken");
    timeout(WAIT, sounding.wait_for(|sounding| sounding.microphone == Standing::Open))
        .await
        .expect("in time")
        .expect("open");
    assert_eq!(opens.load(Ordering::Relaxed), 2);
    drop((capture, second));
    timeout(WAIT, sounding.wait_for(|sounding| sounding.microphone == Standing::Closed))
        .await
        .expect("in time")
        .expect("closed");
}

#[test]
fn dropping_the_chain_waits_for_a_keeper_held_in_a_device_no_longer_than_its_bound() {
    let (engine, end, _) = gated();
    let devices = engine.devices().expect("ours");
    let capture = devices.capture().expect("asked for");
    eventually(|| devices.health().microphone.doing == Keeping::Opening, "its keeper is in the open");
    let began = Instant::now();
    drop((capture, devices, engine));
    let waited = began.elapsed();
    assert!(waited >= STOP_BOUND && waited < STOP_BOUND * 3, "left behind after its bound: {waited:?}");
    drop(end);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_microphone_test_goes_on_until_its_last_hold_is_let_go() {
    let clicked = Clicked::default();
    let engine = Engine::start(Arc::new(Tested(Arc::clone(&clicked))), PLAIN);
    let devices = engine.devices().expect("ours");
    let (first, second) = (devices.test(), devices.test());
    drop(first);
    assert_eq!(devices.health().tests, 1, "one hold let go leaves the other");
    let (_playing, _capturing) = (devices.play().expect("the speaker"), devices.capture().expect("the microphone"));
    timeout(WAIT, async {
        while lock(&clicked).played.is_none() {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the click is still played back for the hold that is left");
    drop(second);
    assert_eq!(devices.health().tests, 0);
}

/// A microphone that hears a tone, and panics as it is read while `left` is above zero, counting it down.
struct Tripping {
    tone: Tone,
    left: Arc<AtomicU32>,
}

impl MicrophoneEnd for Tripping {
    fn frame(&mut self) -> Option<Mono> {
        let tripped = self.left.try_update(Ordering::Relaxed, Ordering::Relaxed, |left| left.checked_sub(1));
        assert!(tripped.is_err(), "a microphone that panics as it is read");
        self.tone.frame()
    }
    fn delay(&self) -> Duration {
        Duration::ZERO
    }
}

struct Faulty(Arc<AtomicU32>);

impl Sound for Faulty {
    fn microphone(&self, _device: Option<&str>, _wake: Thread) -> MicrophoneOpened {
        let tone = Tone { opened: Instant::now(), given: 0 };
        Ok((Box::new(Always), Box::new(Tripping { tone, left: Arc::clone(&self.0) })))
    }
    fn speaker(&self, _device: Option<&str>, _now: Instant) -> SpeakerOpened {
        kept(&Arc::default())
    }
    fn listing(&self) -> Result<Listing, DevicesRefused> {
        Ok(Listing::default())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_that_panics_once_is_counted_and_sound_flows_again() {
    let engine = Engine::start(Arc::new(Faulty(Arc::new(AtomicU32::new(1)))), Processing::PROCESSOR);
    let devices = engine.devices().expect("ours");
    let chain = devices.chain();
    let (_playing, capture) = (devices.play().expect("the speaker"), devices.capture().expect("the microphone"));
    timeout(WAIT, async {
        while chain.level().is_none_or(|level| level <= 5) {
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the tone is heard from the microphone opened again");
    let health = devices.health();
    assert_eq!((health.restarts, health.alive), (1, true));
    assert!(health.turns > 0 && health.captured > 0, "{health:?}");
    let open = Sounding { microphone: Standing::Open, speaker: Standing::Open };
    assert_eq!(*devices.sounding().borrow(), open);
    assert!(capture.is_active());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_that_panics_again_at_once_stops_the_chain_and_every_hold_after_is_refused() {
    let engine = Engine::start(Arc::new(Faulty(Arc::new(AtomicU32::new(u32::MAX)))), Processing::PROCESSOR);
    let devices = engine.devices().expect("ours");
    let mut sounding = devices.sounding();
    let capture = devices.capture().expect("the microphone is asked for");
    let gone = Standing::Refused(DevicesRefused::Unavailable);
    let stopped = Sounding { microphone: gone, speaker: gone };
    timeout(WAIT, sounding.wait_for(|sounding| *sounding == stopped)).await.expect("in time").expect("said");
    let health = devices.health();
    assert_eq!((health.restarts, health.alive), (1, false), "restarted once, then stopped: {health:?}");
    assert!(!capture.is_active());
    assert!(matches!(devices.capture(), Err(DevicesRefused::Unavailable)));
    assert!(matches!(devices.play(), Err(DevicesRefused::Unavailable)));
}

/// Devices whose speaker takes `0` to be heard.
struct Late(Duration);

impl Sound for Late {
    fn microphone(&self, _device: Option<&str>, _wake: Thread) -> MicrophoneOpened {
        toned()
    }
    fn speaker(&self, _device: Option<&str>, _now: Instant) -> SpeakerOpened {
        Ok((Box::new(Always), Box::new(Kept(Arc::default(), self.0))))
    }
    fn listing(&self) -> Result<Listing, DevicesRefused> {
        Fake(Arc::default()).listing()
    }
}

#[test]
fn the_health_report_carries_the_speakers_delay_and_the_most_of_it_since_it_was_last_read() {
    let engine = Engine::start(Arc::new(Late(Duration::from_millis(37))), Processing::PROCESSOR);
    let devices = engine.devices().expect("ours");
    let _playing = devices.play().expect("the speaker is asked for");
    eventually(|| devices.health().speaker_delay_ms == 37, "the mix thread reads the speaker's delay");
    eventually(|| devices.health().speaker_delay_most_ms == 37, "and the most it read since the health was last read");
}

#[test]
fn a_turn_is_read_back_as_it_was_set() {
    for turn in Turn::ALL {
        assert_eq!(Turn::of(turn.code()), turn);
    }
}

#[test]
fn a_source_hands_the_mixer_its_frame_once_at_its_gain() {
    let slot = Arc::new(Mutex::new(Slot { frame: Some((vec![1_000; FRAME], 1)) }));
    let gain = Arc::new(AtomicU32::new(0.5_f32.to_bits()));
    let source = Source { id: 1, slot: Arc::clone(&slot), gain, mixed: Arc::default() };
    let frame = source.get_audio_frame_with_info(RATE_HZ).expect("its frame");
    assert!(frame.data.iter().all(|sample| *sample == 500));
    drop(frame);
    assert!(source.get_audio_frame_with_info(RATE_HZ).is_none(), "taken once: the next tick is silent without one");
}

#[test]
fn a_tick_waits_for_every_track_or_for_one_two_frames_ahead() {
    assert!(!ticks(&[]), "nothing to mix");
    assert!(ticks(&[1, 1, 1]), "every track has its frame");
    assert!(!ticks(&[1, 0, 1]), "one track's frame of this tick has not come");
    assert!(ticks(&[2, 0, 1]), "but one is a frame ahead: the late one is left out of this tick");
    assert!(!ticks(&[0]));
}

/// The microphone's frames so far, which of them was the click, and how many had come when it was played: shared by
/// a test's microphone and speaker, so the delay is told on the microphone's own clock, whatever the machine's load.
#[derive(Default)]
struct Counted {
    given: u32,
    heard: Option<u32>,
    played: Option<u32>,
}

type Clicked = Arc<Mutex<Counted>>;

/// A microphone that hears a click, one loud frame, half a second after it opened, then silence.
struct Click {
    opened: Instant,
    clicked: Clicked,
}

impl MicrophoneEnd for Click {
    fn frame(&mut self) -> Option<Mono> {
        let due = u32::try_from(self.opened.elapsed().as_millis() / 10).unwrap_or(u32::MAX);
        let mut counted = lock(&self.clicked);
        (counted.given < due).then(|| {
            counted.given += 1;
            if counted.given != 50 {
                return [0; FRAME];
            }
            counted.heard = Some(counted.given);
            [16_000; FRAME]
        })
    }
    fn delay(&self) -> Duration {
        Duration::ZERO
    }
}

/// A speaker that keeps when it was first given a loud frame.
struct Heard(Clicked);

impl SpeakerEnd for Heard {
    fn play(&mut self, frame: &Stereo) {
        let mut counted = lock(&self.0);
        if counted.played.is_none() && frame.iter().any(|sample| sample.unsigned_abs() > 1_000) {
            counted.played = Some(counted.given);
        }
    }
    fn delay(&self) -> Duration {
        Duration::ZERO
    }
}

struct Tested(Clicked);

/// Processing that leaves a click as it was heard.
const PLAIN: Processing = Processing {
    echo_cancellation: false,
    noise: crate::processing::Noise::Off,
    gain: crate::processing::Gain::Fixed(0.0),
    gate: None,
};

impl Sound for Tested {
    fn microphone(&self, _device: Option<&str>, _wake: Thread) -> MicrophoneOpened {
        Ok((Box::new(Always), Box::new(Click { opened: Instant::now(), clicked: Arc::clone(&self.0) })))
    }
    fn speaker(&self, _device: Option<&str>, _now: Instant) -> SpeakerOpened {
        Ok((Box::new(Always), Box::new(Heard(Arc::clone(&self.0)))))
    }
    fn listing(&self) -> Result<Listing, DevicesRefused> {
        Ok(Listing::default())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_microphone_test_plays_ones_voice_back_120_ms_late() {
    let clicked = Clicked::default();
    let engine = Engine::start(Arc::new(Tested(Arc::clone(&clicked))), PLAIN);
    let devices = engine.devices().expect("ours");
    let _testing = devices.test();
    let (_playing, _capturing) = (devices.play().expect("the speaker"), devices.capture().expect("the microphone"));
    timeout(WAIT, async {
        while lock(&clicked).played.is_none() {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the click is played back");
    let counted = lock(&clicked);
    let (Some(heard), Some(played)) = (counted.heard, counted.played) else { panic!("both kept") };
    let late = Duration::from_millis(10) * (played - heard);
    assert_eq!(late, Duration::from_millis(120), "played back that long after it was heard, in 10 ms frames");
}

/// A gate a fake's call waits at until the test lets it through.
type Gate = Arc<(Mutex<bool>, std::sync::Condvar)>;

fn gate() -> Gate {
    Arc::new((Mutex::new(false), std::sync::Condvar::new()))
}

fn pass(gate: &Gate) {
    *lock(&gate.0) = true;
    gate.1.notify_all();
}

fn wait_at(gate: &Gate) {
    let mut open = lock(&gate.0);
    while !*open {
        open = gate.1.wait(open).unwrap_or_else(PoisonError::into_inner);
    }
}

/// A microphone whose stream goes away when `vanished` is set: its end hears nothing, and its keeper is held in
/// `keep` until the test opens the gate, then hands the end a fresh tone.
struct Vanishing {
    vanished: Arc<AtomicBool>,
    gate: Gate,
    fresh: mpsc::Sender<Tone>,
}

impl Microphone for Vanishing {
    fn keep(&mut self, _now: Instant) {
        if self.vanished.load(Ordering::Relaxed) {
            wait_at(&self.gate);
            let _ = self.fresh.send(Tone { opened: Instant::now(), given: 0 });
            self.vanished.store(false, Ordering::Relaxed);
        }
    }
    fn choose(&mut self, _device: Option<&str>) -> Result<(), DevicesRefused> {
        Ok(())
    }
    fn is_open(&self) -> bool {
        true
    }
}

struct VanishingEnd {
    tone: Tone,
    fresh: mpsc::Receiver<Tone>,
    vanished: Arc<AtomicBool>,
}

impl MicrophoneEnd for VanishingEnd {
    fn frame(&mut self) -> Option<Mono> {
        while let Ok(tone) = self.fresh.try_recv() {
            self.tone = tone;
        }
        if self.vanished.load(Ordering::Relaxed) { None } else { self.tone.frame() }
    }
    fn delay(&self) -> Duration {
        Duration::ZERO
    }
}

struct Vanish {
    played: Arc<AtomicI32>,
    vanished: Arc<AtomicBool>,
    gate: Gate,
}

impl Sound for Vanish {
    fn microphone(&self, _device: Option<&str>, _wake: Thread) -> MicrophoneOpened {
        let (fresh, later) = mpsc::channel();
        let keeper = Vanishing { vanished: Arc::clone(&self.vanished), gate: Arc::clone(&self.gate), fresh };
        let tone = Tone { opened: Instant::now(), given: 0 };
        let end = VanishingEnd { tone, fresh: later, vanished: Arc::clone(&self.vanished) };
        Ok((Box::new(keeper), Box::new(end)))
    }
    fn speaker(&self, _device: Option<&str>, _now: Instant) -> SpeakerOpened {
        kept(&self.played)
    }
    fn listing(&self) -> Result<Listing, DevicesRefused> {
        Ok(Listing::default())
    }
}

/// The loudest sample a speaker was given since last asked, read and reset.
fn loudest(played: &AtomicI32) -> i32 {
    played.swap(0, Ordering::Relaxed)
}

/// Asserts the mix thread goes on twice, 300 ms apart: the speaker played the tone again and the turns grew.
async fn the_mix_goes_on(devices: &crate::Devices, played: &AtomicI32) {
    let mut turns = devices.health().turns;
    for _ in 0..2 {
        loudest(played);
        sleep(Duration::from_millis(300)).await;
        assert!(loudest(played) > 4_000, "the speaker goes on playing the tone");
        let now = devices.health().turns;
        assert!(now > turns, "the mix thread goes on turning: {turns} then {now}");
        turns = now;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_microphone_whose_reopen_hangs_holds_up_neither_the_mix_nor_a_rejoin() {
    let (played, vanished, gate) = (Arc::new(AtomicI32::new(0)), Arc::new(AtomicBool::new(false)), gate());
    let sound = Vanish { played: Arc::clone(&played), vanished: Arc::clone(&vanished), gate: Arc::clone(&gate) };
    // The speaker plays the microphone's own tone: a canceller would take it out by how the wall clock fell.
    let engine = Engine::bounded(Arc::new(sound), PLAIN, PATIENT);
    let devices = engine.devices().expect("ours");
    let chain = devices.chain();
    let mut sounding = devices.sounding();
    let playing = devices.play().expect("the speaker is asked for");
    let toned = Toned::start(&engine.factory).await;
    let (gain, peak) = (Arc::new(AtomicU32::new(1.0_f32.to_bits())), Arc::new(AtomicU32::new(0)));
    let heard = chain.hear(toned.track.clone(), 1, Arc::clone(&gain), Arc::clone(&peak));
    let capture = devices.capture().expect("the microphone is asked for");
    eventually(|| loudest(&played) > 4_000, "the tone is played");
    eventually(|| capture.is_active(), "the microphone opens");

    vanished.store(true, Ordering::Relaxed);
    eventually(|| devices.health().microphone.doing == Keeping::Keeping, "its keeper is held in keeping it");
    the_mix_goes_on(&devices, &played).await;

    drop((playing, heard));
    let playing = devices.play().expect("the speaker is asked for again");
    let heard = chain.hear(toned.track.clone(), 1, gain, peak);
    let open = Standing::Open;
    timeout(WAIT, sounding.wait_for(|sounding| sounding.speaker == open))
        .await
        .expect("in time")
        .expect("a rejoin's hold is answered");
    eventually(|| playing.is_active(), "and plays");
    loudest(&played);
    eventually(|| loudest(&played) > 4_000, "the tone is still heard");
    assert_eq!(sounding.borrow().microphone, Standing::Open, "the microphone is said open while it is kept");

    pass(&gate);
    chain.level();
    timeout(WAIT, async {
        while chain.level().is_none_or(|level| level <= 5) {
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the fresh stream's tone is heard");
    assert!(capture.is_active() && sounding.borrow().microphone == Standing::Open);
    drop(heard);
}

/// A speaker whose move to another device is held until the test opens the gate; its end then plays on `new`.
struct Swapping {
    gate: Gate,
    fresh: mpsc::Sender<Arc<AtomicI32>>,
    new: Arc<AtomicI32>,
}

impl Speaker for Swapping {
    fn keep(&mut self, _now: Instant) {}
    fn choose(&mut self, device: Option<&str>) -> Result<(), DevicesRefused> {
        if device.is_some() {
            wait_at(&self.gate);
            let _ = self.fresh.send(Arc::clone(&self.new));
        }
        Ok(())
    }
    fn is_open(&self) -> bool {
        true
    }
}

struct SwappingEnd {
    played: Arc<AtomicI32>,
    fresh: mpsc::Receiver<Arc<AtomicI32>>,
}

impl SpeakerEnd for SwappingEnd {
    fn play(&mut self, frame: &Stereo) {
        while let Ok(played) = self.fresh.try_recv() {
            self.played = played;
        }
        Kept(Arc::clone(&self.played), Duration::ZERO).play(frame);
    }
    fn delay(&self) -> Duration {
        Duration::ZERO
    }
}

struct Swap {
    old: Arc<AtomicI32>,
    new: Arc<AtomicI32>,
    gate: Gate,
}

impl Sound for Swap {
    fn microphone(&self, _device: Option<&str>, _wake: Thread) -> MicrophoneOpened {
        toned()
    }
    fn speaker(&self, _device: Option<&str>, _now: Instant) -> SpeakerOpened {
        let (fresh, later) = mpsc::channel();
        let keeper = Swapping { gate: Arc::clone(&self.gate), fresh, new: Arc::clone(&self.new) };
        Ok((Box::new(keeper), Box::new(SwappingEnd { played: Arc::clone(&self.old), fresh: later })))
    }
    fn listing(&self) -> Result<Listing, DevicesRefused> {
        Ok(Listing { speakers: vec![Listed { id: "right".into(), label: "RIGHT".into() }], ..Listing::default() })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_speaker_swap_that_hangs_keeps_the_mix_on_the_old_speaker() {
    let (old, new, gate) = (Arc::new(AtomicI32::new(0)), Arc::new(AtomicI32::new(0)), gate());
    let sound = Swap { old: Arc::clone(&old), new: Arc::clone(&new), gate: Arc::clone(&gate) };
    let engine = Engine::bounded(Arc::new(sound), Processing::PROCESSOR, PATIENT);
    let devices = engine.devices().expect("ours");
    let mut sounding = devices.sounding();
    let playing = devices.play().expect("the speaker is asked for");
    let toned = Toned::start(&engine.factory).await;
    let unit = Arc::new(AtomicU32::new(1.0_f32.to_bits()));
    let _heard = devices.chain().hear(toned.track.clone(), 1, unit, Arc::default());
    eventually(|| loudest(&old) > 4_000, "the tone is played on the first speaker");
    assert_eq!(devices.choose_output(Some("right")), Ok(()));
    eventually(|| devices.health().speaker.doing == Keeping::Choosing, "its keeper is held in the move");
    the_mix_goes_on(&devices, &old).await;
    assert_eq!(loudest(&new), 0, "nothing is played on the speaker still being linked");
    assert_eq!((sounding.borrow().speaker, playing.is_active()), (Standing::Opening, true));

    pass(&gate);
    eventually(|| loudest(&new) > 4_000, "the tone moves to the speaker chosen once it takes over");
    timeout(WAIT, sounding.wait_for(|sounding| sounding.speaker == Standing::Open))
        .await
        .expect("in time")
        .expect("said open");
    loudest(&old);
    sleep(Duration::from_millis(200)).await;
    assert_eq!(loudest(&old), 0, "and the first gets nothing after");
}

/// A microphone whose first opens are held at a gate each, and a speaker that keeps what it plays in `played`.
struct Stuck {
    opens: Arc<AtomicU32>,
    gates: Vec<Gate>,
    played: Arc<AtomicI32>,
}

impl Sound for Stuck {
    fn microphone(&self, _device: Option<&str>, _wake: Thread) -> MicrophoneOpened {
        let n = usize::try_from(self.opens.fetch_add(1, Ordering::Relaxed)).unwrap_or(usize::MAX);
        if let Some(gate) = self.gates.get(n) {
            wait_at(gate);
        }
        toned()
    }
    fn speaker(&self, _device: Option<&str>, _now: Instant) -> SpeakerOpened {
        kept(&self.played)
    }
    fn listing(&self) -> Result<Listing, DevicesRefused> {
        Ok(Listing { microphones: vec![Listed { id: "mic".into(), label: "MIC".into() }], ..Listing::default() })
    }
}

/// Waits until the microphone's keeper finishes a look, so the one in hand now, if any, is over.
fn looked_again(devices: &crate::Devices) {
    let turns = devices.health().microphone.turns;
    eventually(|| devices.health().microphone.turns > turns, "its keeper finishes a look");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_choice_while_a_keeper_is_held_within_the_bound_waits_for_the_keeper_it_has() {
    let (opens, gates) = (Arc::new(AtomicU32::new(0)), vec![gate()]);
    let sound = Stuck { opens: Arc::clone(&opens), gates: gates.clone(), played: Arc::default() };
    let engine = Engine::bounded(Arc::new(sound), Processing::PROCESSOR, PATIENT);
    let devices = engine.devices().expect("ours");
    let mut sounding = devices.sounding();
    let capture = devices.capture().expect("asked for");
    eventually(|| opens.load(Ordering::Relaxed) == 1, "the first open begins and is held");
    assert_eq!(devices.choose_input(Some("mic")), Ok(()));
    sleep(Duration::from_millis(200)).await;
    assert_eq!(opens.load(Ordering::Relaxed), 1, "a choice before the bound waits for the keeper it has");
    assert_eq!(sounding.borrow().microphone, Standing::Opening);

    pass(&gates[0]);
    timeout(WAIT, sounding.wait_for(|sounding| sounding.microphone == Standing::Open))
        .await
        .expect("in time")
        .expect("the keeper it has answers the choice");
    assert!(capture.is_active());
    assert_eq!(opens.load(Ordering::Relaxed), 1, "the choice moved the device opened, not a new one");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_keeper_stuck_past_the_bound_is_given_up_on_once_at_a_time() {
    let (opens, gates) = (Arc::new(AtomicU32::new(0)), vec![gate(), gate()]);
    let sound = Stuck { opens: Arc::clone(&opens), gates: gates.clone(), played: Arc::default() };
    let engine = Engine::start(Arc::new(sound), Processing::PROCESSOR);
    let devices = engine.devices().expect("ours");
    let mut sounding = devices.sounding();
    let capture = devices.capture().expect("asked for");
    eventually(|| opens.load(Ordering::Relaxed) == 2, "past the bound a fresh keeper opens what is asked");
    timeout(WAIT, sounding.wait_for(|sounding| sounding.microphone == Standing::Hung))
        .await
        .expect("in time")
        .expect("the fresh keeper held past the bound too is said hung");
    assert_eq!(opens.load(Ordering::Relaxed), 2);
    assert_eq!(devices.choose_input(Some("mic")), Ok(()));
    timeout(WAIT, sounding.wait_for(|sounding| sounding.microphone == Standing::Hung))
        .await
        .expect("in time")
        .expect("which nothing can try while the one given up on holds its device");
    sleep(Duration::from_millis(300)).await;
    assert_eq!(opens.load(Ordering::Relaxed), 2, "the one given up on has not ended: no third keeper");
    assert!(!capture.is_active());

    pass(&gates[1]);
    timeout(WAIT, sounding.wait_for(|sounding| sounding.microphone == Standing::Open))
        .await
        .expect("in time")
        .expect("the fresh keeper's open is said");
    looked_again(&devices);
    pass(&gates[0]);
    sleep(Duration::from_millis(200)).await;
    assert!(capture.is_active() && sounding.borrow().microphone == Standing::Open);
    assert_eq!(opens.load(Ordering::Relaxed), 2, "what the abandoned open returned was dropped, not kept");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_microphone_whose_open_hangs_is_said_unanswered_and_heard_past_the_bound_with_no_new_choice() {
    let (opens, gates, played) = (Arc::new(AtomicU32::new(0)), vec![gate(), gate()], Arc::new(AtomicI32::new(0)));
    let sound = Stuck { opens: Arc::clone(&opens), gates: gates.clone(), played: Arc::clone(&played) };
    let engine = Engine::start(Arc::new(sound), Processing::PROCESSOR);
    let devices = engine.devices().expect("ours");
    let chain = devices.chain();
    let mut sounding = devices.sounding();
    let _playing = devices.play().expect("the speaker is asked for");
    let toned = Toned::start(&engine.factory).await;
    let unit = Arc::new(AtomicU32::new(1.0_f32.to_bits()));
    let _heard = chain.hear(toned.track.clone(), 1, unit, Arc::default());
    eventually(|| loudest(&played) > 4_000, "the tone is played");
    let capture = devices.capture().expect("the microphone is asked for");
    assert_eq!(sounding.borrow().microphone, Standing::Opening, "said at once");
    eventually(|| opens.load(Ordering::Relaxed) >= 1, "the first open begins and is held");

    timeout(WAIT, sounding.wait_for(|sounding| sounding.microphone == Standing::Unanswered))
        .await
        .expect("in time")
        .expect("past the bound it is said unanswered");
    eventually(|| opens.load(Ordering::Relaxed) == 2, "and a fresh keeper opens it again, nothing chosen");
    the_mix_goes_on(&devices, &played).await;
    assert!(!capture.is_active());
    let held = sounding.borrow().microphone;
    assert!(matches!(held, Standing::Unanswered | Standing::Hung), "the fresh try is no answer yet: {held:?}");

    pass(&gates[1]);
    timeout(WAIT, sounding.wait_for(|sounding| sounding.microphone == Standing::Open))
        .await
        .expect("in time")
        .expect("the second open's answer is said");
    chain.level();
    timeout(WAIT, async {
        while chain.level().is_none_or(|level| level <= 5) {
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the tone of the second open is heard");
    assert!(capture.is_active());
    looked_again(&devices);
    pass(&gates[0]);
    sleep(Duration::from_millis(200)).await;
    assert_eq!(opens.load(Ordering::Relaxed), 2, "what the first open returned at last was dropped");
    assert_eq!(sounding.borrow().microphone, Standing::Open);
}
