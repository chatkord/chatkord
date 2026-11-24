//! A device kept: a stream that fails is opened again after a growing wait, a device chosen anew takes over only once
//! it runs, and the speaker's rate follows how full it runs. The system's stream only moves sound.
//!
//! Each device is two halves. The keeper ([`Microphone`], [`Speaker`]) opens, chooses, keeps and drops streams, and
//! may be held in the system's calls for as long as they last. Its end ([`Frames`], [`Voiced`]) holds only a ring's
//! end and atomics, so reading or feeding a device never waits on one.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::Thread;
use std::time::{Duration, Instant};

use super::system::{Input, Output};
use super::{
    Ask, Capture, CatchUp, FRAME, Gauge, Health, MICROPHONE_KEPT, Mono, PACE_EVERY, Pace, Playback, Playout, Retry,
    SPEAKER_KEPT, Stereo, VoiceError,
};
use crate::share::Chunker;
use report::{Fact, word};

/// How long `frames` frames of 10 ms last.
fn frames(frames: usize) -> Duration {
    Duration::from_millis(10).saturating_mul(u32::try_from(frames).unwrap_or(u32::MAX))
}

/// How a stream's device was asked for, as a log line says it: never which device.
fn asked(device: Option<&str>) -> Fact {
    Fact::Word(if device.is_some() { word!("chosen") } else { word!("default") })
}

/// A stream's delay as its keeper hands it to its end.
fn nanos(delay: Duration) -> u64 {
    u64::try_from(delay.as_nanos()).unwrap_or(u64::MAX)
}

/// Whether choosing `device` changes nothing: it is what `asked` already names and that stream has not stopped, so
/// opening it again would only run a second stream on one device.
fn stands(asked: Option<&str>, health: Health, device: Option<&str>) -> bool {
    asked == device && health != Health::Stopped
}

/// One microphone stream and the delay its keeper last read from it.
struct Heard {
    stream: Input,
    delay: Arc<AtomicU64>,
}

/// A microphone stream's ring and the delay its keeper keeps for it: what [`Frames`] takes over.
type Ring = (rtrb::Consumer<Mono>, Arc<AtomicU64>);

impl Heard {
    fn open(
        device: Option<&str>,
        wake: Option<Thread>,
        dropped: Arc<AtomicU64>,
    ) -> Result<(Self, rtrb::Consumer<Mono>), VoiceError> {
        let (mut producer, ring) = rtrb::RingBuffer::new(MICROPHONE_KEPT);
        let mut chunker = Chunker::<FRAME, _>::new(move |frame: &Mono| {
            if producer.push(*frame).is_err() {
                dropped.fetch_add(1, Ordering::Relaxed);
            } else if let Some(wake) = &wake {
                wake.unpark();
            }
        });
        let stream = Input::open(device, move |samples| chunker.push_samples(samples))?;
        Ok((Self { stream, delay: Arc::default() }, ring))
    }
}

/// A microphone chosen later, and the ring that is its frames until it takes over.
struct Next {
    device: Option<String>,
    heard: Heard,
    ring: rtrb::Consumer<Mono>,
}

/// The microphone chosen, and the one chosen after it until it runs. Dropping it lets go of the device.
pub struct Microphone {
    device: Option<String>,
    wake: Option<Thread>,
    heard: Option<Heard>,
    next: Option<Next>,
    retry: Retry,
    dropped: Arc<AtomicU64>,
    rings: mpsc::Sender<Ring>,
}

/// The microphone's frames: the reading half, which never waits on the device.
pub struct Frames {
    ring: Option<rtrb::Consumer<Mono>>,
    rings: mpsc::Receiver<Ring>,
    delay: Arc<AtomicU64>,
}

impl Microphone {
    /// Opens the microphone `device` names (PipeWire's `node.name`, WASAPI's endpoint id), or the default one, and
    /// the frames it fills. Each frame that arrives unparks `wake`, the thread that reads them, from the sound
    /// server's own thread.
    pub fn open(device: Option<&str>, wake: Option<Thread>) -> Result<(Self, Frames), VoiceError> {
        let dropped = Arc::new(AtomicU64::new(0));
        let (heard, ring) = Heard::open(device, wake.clone(), Arc::clone(&dropped))?;
        let (rings, later) = mpsc::channel();
        let frames = Frames { ring: Some(ring), rings: later, delay: Arc::clone(&heard.delay) };
        let microphone = Self {
            device: device.map(str::to_owned),
            wake,
            heard: Some(heard),
            next: None,
            retry: Retry::default(),
            dropped,
            rings,
        };
        Ok((microphone, frames))
    }

    /// Takes `device` in place of the one heard, once it runs; until then the one heard goes on. Choosing what is
    /// already heard, or already chosen and being linked, opens nothing; the one heard chosen again takes back a
    /// choice still being linked.
    pub fn choose(&mut self, device: Option<&str>) -> Result<(), VoiceError> {
        if self.next.as_ref().is_some_and(|next| stands(next.device.as_deref(), next.heard.stream.health(), device)) {
            return Ok(());
        }
        if stands(self.device.as_deref(), self.health(), device) {
            self.next = None;
            return Ok(());
        }
        let (heard, ring) = Heard::open(device, self.wake.clone(), Arc::clone(&self.dropped))?;
        self.next = Some(Next { device: device.map(str::to_owned), heard, ring });
        Ok(())
    }

    /// Hands over to a device chosen once it runs, opens a failed stream again after a wait, and keeps the stream's
    /// delay for the frames to read. Called as often as that delay is worth reading.
    pub fn keep(&mut self, now: Instant) {
        if let Some(next) = self.next.take_if(|next| next.heard.stream.health() == Health::Running) {
            report::note!(
                info,
                "a kept stream moved to the device chosen",
                end = Fact::Word(word!("microphone")),
                from = asked(self.device.as_deref()),
                to = asked(next.device.as_deref()),
            );
            self.device = next.device;
            self.take_over(next.heard, next.ring);
        }
        match self.heard.as_ref().map(|heard| heard.stream.health()) {
            Some(Health::Running) => self.retry.ran(),
            Some(Health::Starting) => {}
            Some(Health::Stopped) | None => {
                if self.retry.due(now) {
                    let opened = Heard::open(self.device.as_deref(), self.wake.clone(), Arc::clone(&self.dropped));
                    report::note!(
                        info,
                        "a kept stream was opened again",
                        end = Fact::Word(word!("microphone")),
                        device = asked(self.device.as_deref()),
                        opened = opened.is_ok(),
                    );
                    match opened {
                        Ok((heard, ring)) => self.take_over(heard, ring),
                        Err(_) => self.heard = None,
                    }
                }
            }
        }
        if let Some(heard) = &self.heard {
            heard.delay.store(nanos(heard.stream.delay()), Ordering::Relaxed);
        }
    }

    /// Makes `heard` the stream in use: its ring goes to the frames before the stream it replaces is dropped, so
    /// the reader never holds a ring whose stream is gone while another runs.
    fn take_over(&mut self, heard: Heard, ring: rtrb::Consumer<Mono>) {
        // A reader that is gone has nothing left to hand frames to.
        let _ = self.rings.send((ring, Arc::clone(&heard.delay)));
        self.heard = Some(heard);
    }

    /// Whether its sound flows.
    pub fn health(&self) -> Health {
        self.heard.as_ref().map_or(Health::Stopped, |heard| heard.stream.health())
    }

    /// Frames dropped because the reader fell 200 ms behind.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl Frames {
    /// The oldest frame waiting, if any; a stream that took over since the last call is read from first.
    pub fn frame(&mut self) -> Option<Mono> {
        while let Ok((ring, delay)) = self.rings.try_recv() {
            (self.ring, self.delay) = (Some(ring), delay);
        }
        self.ring.as_mut()?.pop().ok()
    }

    /// How long a sound took from the device to be read now: the stream's delay and what waits in the ring.
    pub fn delay(&self) -> Duration {
        let waiting = self.ring.as_ref().map_or(0, rtrb::Consumer::slots);
        Duration::from_nanos(self.delay.load(Ordering::Relaxed)) + frames(waiting)
    }
}

/// One speaker stream, the gauge it reports through, and the delay its keeper last read from it.
struct Played {
    stream: Output,
    gauge: Arc<Gauge>,
    delay: Arc<AtomicU64>,
}

/// A speaker stream's ring and the delay its keeper keeps for it: what [`Voiced`] takes over.
type Feed = (rtrb::Producer<Stereo>, Arc<AtomicU64>);

impl Played {
    fn open(device: Option<&str>) -> Result<(Self, rtrb::Producer<Stereo>), VoiceError> {
        let (ring, consumer) = rtrb::RingBuffer::new(SPEAKER_KEPT);
        let gauge = Arc::new(Gauge::default());
        let mut playout = Playout::new(consumer, Arc::clone(&gauge));
        let stream = Output::open(device, move |out| playout.fill(out))?;
        Ok((Self { stream, gauge, delay: Arc::default() }, ring))
    }
}

/// A speaker chosen later, and the ring that is its feed until it takes over.
struct NextPlayed {
    device: Option<String>,
    played: Played,
    ring: rtrb::Producer<Stereo>,
}

/// The speaker chosen, and the one chosen after it until it runs. Dropping it lets go of the device.
pub struct Speaker {
    device: Option<String>,
    played: Option<Played>,
    next: Option<NextPlayed>,
    retry: Retry,
    pace: Pace,
    /// Whether the delay it has run up is being caught up by skipping.
    catch: CatchUp,
    /// When the rate was last set.
    paced: Instant,
    /// The frames [`Voiced`] had been given when the rate was last set.
    seen: u64,
    pushed: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
    rings: mpsc::Sender<Feed>,
}

/// The speaker's feeding half, which never waits on the device.
pub struct Voiced {
    ring: Option<rtrb::Producer<Stereo>>,
    rings: mpsc::Receiver<Feed>,
    delay: Arc<AtomicU64>,
    pushed: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
}

impl Speaker {
    /// Opens the speaker `device` names (PipeWire's `node.name`, WASAPI's endpoint id), or the default one, at
    /// `now`, and the feed that plays on it.
    pub fn open(device: Option<&str>, now: Instant) -> Result<(Self, Voiced), VoiceError> {
        let (played, ring) = Played::open(device)?;
        let (rings, later) = mpsc::channel();
        let (pushed, dropped) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
        let voiced = Voiced {
            ring: Some(ring),
            rings: later,
            delay: Arc::clone(&played.delay),
            pushed: Arc::clone(&pushed),
            dropped: Arc::clone(&dropped),
        };
        let speaker = Self {
            device: device.map(str::to_owned),
            played: Some(played),
            next: None,
            retry: Retry::default(),
            pace: Pace::default(),
            catch: CatchUp::default(),
            paced: now,
            seen: 0,
            pushed,
            dropped,
            rings,
        };
        Ok((speaker, voiced))
    }

    /// Takes `device` in place of the one played to, once it runs; until then the one played to goes on. Choosing
    /// what is already played to, or already chosen and being linked, opens nothing; the one played to chosen
    /// again takes back a choice still being linked.
    pub fn choose(&mut self, device: Option<&str>) -> Result<(), VoiceError> {
        if self.next.as_ref().is_some_and(|next| stands(next.device.as_deref(), next.played.stream.health(), device)) {
            return Ok(());
        }
        if stands(self.device.as_deref(), self.health(), device) {
            self.next = None;
            return Ok(());
        }
        let (played, ring) = Played::open(device)?;
        self.next = Some(NextPlayed { device: device.map(str::to_owned), played, ring });
        Ok(())
    }

    /// Hands over to a device chosen once it runs, opens a failed stream again after a wait, and sets the rate from
    /// how full it ran. Called as often as the rate is worth setting.
    pub fn keep(&mut self, now: Instant) {
        if let Some(next) = self.next.take_if(|next| next.played.stream.health() == Health::Running) {
            report::note!(
                info,
                "a kept stream moved to the device chosen",
                end = Fact::Word(word!("speaker")),
                from = asked(self.device.as_deref()),
                to = asked(next.device.as_deref()),
            );
            self.device = next.device;
            self.take_over(next.played, next.ring);
        }
        match self.played.as_ref().map(|played| played.stream.health()) {
            Some(Health::Running) => self.retry.ran(),
            Some(Health::Starting) => {}
            Some(Health::Stopped) | None => {
                if self.retry.due(now) {
                    let opened = Played::open(self.device.as_deref());
                    report::note!(
                        info,
                        "a kept stream was opened again",
                        end = Fact::Word(word!("speaker")),
                        device = asked(self.device.as_deref()),
                        opened = opened.is_ok(),
                    );
                    match opened {
                        Ok((played, ring)) => self.take_over(played, ring),
                        Err(_) => {
                            self.played = None;
                            self.pace = Pace::default();
                            self.catch = CatchUp::default();
                        }
                    }
                }
            }
        }
        if let Some(played) = &self.played {
            played.delay.store(nanos(played.stream.delay()), Ordering::Relaxed);
        }
        let elapsed = now.saturating_duration_since(self.paced);
        if elapsed < PACE_EVERY {
            return;
        }
        self.paced = now;
        let Some(played) = self.played.as_ref().filter(|played| played.stream.health() == Health::Running) else {
            return;
        };
        // With nothing sent the ring drains by design, which is no clock to follow.
        let pushed = self.pushed.load(Ordering::Relaxed);
        let fed = std::mem::replace(&mut self.seen, pushed) != pushed;
        if let (Some((level, aim)), true) = (played.gauge.take(), fed) {
            #[expect(clippy::cast_possible_truncation, reason = "a rate within half a percent of one")]
            played.stream.set_rate(self.pace.rate(level, aim, elapsed) as f32);
            // The rate follows a clock; a delay run up at once (a stall, a burst) is skipped, not waited out. What is
            // left of a skip never takes the ring under its aim, however much the rate has worked off meanwhile.
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "clamped at zero, a few thousand"
            )]
            let over = (level - aim).max(0.0) as u32;
            let left = played.gauge.skip.fetch_min(over, Ordering::Relaxed).min(over) as usize;
            match self.catch.step(now, level, aim, left) {
                Some(Ask::Skip(samples)) => {
                    played.gauge.skip.store(u32::try_from(samples).unwrap_or(u32::MAX), Ordering::Relaxed);
                }
                Some(Ask::Cut(samples)) => {
                    played.gauge.skip.store(0, Ordering::Relaxed);
                    played.gauge.cut.store(u32::try_from(samples).unwrap_or(u32::MAX), Ordering::Relaxed);
                }
                None => {}
            }
        }
    }

    /// Makes `played` the stream in use: its ring goes to the feed before the stream it replaces is dropped, so the
    /// feeder never writes to a ring whose stream is gone while another runs.
    fn take_over(&mut self, played: Played, ring: rtrb::Producer<Stereo>) {
        // A feeder that is gone has nothing left to play.
        let _ = self.rings.send((ring, Arc::clone(&played.delay)));
        self.played = Some(played);
        self.pace = Pace::default();
        self.catch = CatchUp::default();
    }

    /// Whether its sound flows.
    pub fn health(&self) -> Health {
        self.played.as_ref().map_or(Health::Stopped, |played| played.stream.health())
    }

    /// Frames dropped because 200 ms already waited, and times it ran out and played silence.
    pub fn losses(&self) -> (u64, u64) {
        let underruns = self.played.as_ref().map_or(0, |played| played.gauge.underruns.load(Ordering::Relaxed));
        (self.dropped.load(Ordering::Relaxed), underruns)
    }
}

impl Voiced {
    /// Queues a frame to play, on the newest stream's ring; false, and counted, when 200 ms already wait or the
    /// stream it was written for is gone.
    pub fn play(&mut self, frame: &Stereo) -> bool {
        while let Ok((ring, delay)) = self.rings.try_recv() {
            (self.ring, self.delay) = (Some(ring), delay);
        }
        self.pushed.fetch_add(1, Ordering::Relaxed);
        let queued = self.ring.as_mut().is_some_and(|ring| !ring.is_abandoned() && ring.push(*frame).is_ok());
        self.dropped.fetch_add(u64::from(!queued), Ordering::Relaxed);
        queued
    }

    /// How long a frame played now takes to reach the device: what waits in the ring and the stream's delay.
    pub fn delay(&self) -> Duration {
        let waiting = self.ring.as_ref().map_or(0, |ring| SPEAKER_KEPT - ring.slots());
        Duration::from_nanos(self.delay.load(Ordering::Relaxed)) + frames(waiting)
    }
}

#[cfg(test)]
mod tests {
    use super::super::RETRY;
    use super::super::fake::{device, held};
    use super::*;

    fn set(name: &str, health: Health) {
        held(&device(name)).health = Some(health);
    }

    fn opened(name: &str) -> usize {
        held(&device(name)).opened
    }

    #[test]
    fn a_device_chosen_takes_over_only_once_it_runs() {
        set("default", Health::Running);
        set("headset", Health::Starting);
        let start = Instant::now();
        let (mut microphone, _frames) = Microphone::open(None, None).unwrap();
        microphone.choose(Some("headset")).unwrap();
        microphone.keep(start);
        assert_eq!(microphone.device, None, "the default goes on while the headset links");
        set("headset", Health::Running);
        microphone.keep(start);
        assert_eq!(microphone.device.as_deref(), Some("headset"));
    }

    #[test]
    fn a_choice_that_changes_nothing_opens_no_second_stream() {
        set("default", Health::Running);
        set("headset", Health::Starting);
        let start = Instant::now();
        let (mut speaker, _voiced) = Speaker::open(None, start).unwrap();
        let (mut microphone, _frames) = Microphone::open(None, None).unwrap();
        speaker.choose(None).unwrap();
        microphone.choose(None).unwrap();
        assert_eq!(opened("default"), 2, "each opened once: the default chosen again while it runs changes nothing");
        speaker.choose(Some("headset")).unwrap();
        speaker.choose(Some("headset")).unwrap();
        microphone.choose(Some("headset")).unwrap();
        microphone.choose(Some("headset")).unwrap();
        assert_eq!(opened("headset"), 2, "nor does the one being linked, chosen again");
        speaker.choose(None).unwrap();
        microphone.choose(None).unwrap();
        set("headset", Health::Running);
        speaker.keep(start);
        microphone.keep(start);
        assert_eq!((opened("default"), &speaker.device, &microphone.device), (2, &None, &None), "back before it ran");
        set("default", Health::Stopped);
        speaker.choose(None).unwrap();
        microphone.choose(None).unwrap();
        assert_eq!(opened("default"), 4, "a stream that stopped is opened again by being chosen");
    }

    #[test]
    fn a_failed_stream_is_opened_again_after_its_wait_and_not_before() {
        set("default", Health::Running);
        let start = Instant::now();
        let (mut speaker, _voiced) = Speaker::open(None, start).unwrap();
        set("default", Health::Stopped);
        speaker.keep(start);
        speaker.keep(start + RETRY.wait(1) / 2);
        assert_eq!(opened("default"), 1, "put off at first");
        speaker.keep(start + RETRY.wait(1));
        assert_eq!(opened("default"), 2, "opened again once its wait is over");
        set("default", Health::Running);
        speaker.keep(start + RETRY.wait(1));
        assert_eq!((opened("default"), speaker.health()), (2, Health::Running), "and kept once it runs");
    }

    #[test]
    fn the_rate_is_set_only_while_frames_are_fed_and_the_stream_runs() {
        set("default", Health::Running);
        let start = Instant::now();
        let (mut speaker, mut voiced) = Speaker::open(None, start).unwrap();
        let fill = || held(&device("default")).fill.as_mut().unwrap()(&mut [0; 2 * FRAME]);
        fill();
        speaker.keep(start + PACE_EVERY);
        assert!(held(&device("default")).rates.is_empty(), "nothing fed: no clock to follow");
        voiced.play(&[0; 2 * FRAME]);
        fill();
        speaker.keep(start + PACE_EVERY * 2);
        assert_eq!(held(&device("default")).rates.len(), 1, "fed and run: its rate is set");
        assert_eq!(voiced.delay(), Duration::from_millis(15), "the stream's delay and the frame still waiting");
    }

    #[test]
    fn a_stream_taken_over_hands_its_ring_to_the_frames_once_and_before_the_old_drops() {
        set("default", Health::Running);
        set("headset", Health::Starting);
        let start = Instant::now();
        let (mut microphone, frames) = Microphone::open(None, None).unwrap();
        microphone.choose(Some("headset")).unwrap();
        microphone.keep(start);
        assert!(frames.rings.try_recv().is_err(), "nothing is handed over while the headset links");
        let first = Arc::downgrade(&microphone.heard.as_ref().unwrap().delay);
        set("headset", Health::Running);
        microphone.keep(start);
        assert_eq!(microphone.device.as_deref(), Some("headset"));
        let (_, delay) = frames.rings.try_recv().expect("the ring of the stream now in use");
        assert!(Arc::ptr_eq(&delay, &microphone.heard.as_ref().unwrap().delay), "the new stream's, not the old one's");
        assert!(frames.rings.try_recv().is_err(), "handed over once");
        assert_eq!(first.strong_count(), 1, "the old stream is dropped: only the frames still name its delay");
        microphone.keep(start);
        assert!(frames.rings.try_recv().is_err(), "a stream that runs hands over nothing more");
        set("headset", Health::Stopped);
        microphone.keep(start);
        microphone.keep(start + RETRY.wait(1));
        assert!(frames.rings.try_recv().is_ok(), "a stream opened again hands over its ring too");
        assert_eq!(frames.delay(), Duration::from_millis(5), "the stream's delay once kept");
    }
}
