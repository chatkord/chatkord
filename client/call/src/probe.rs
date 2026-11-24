//! A microphone level, a test sound and a microphone test with no call: the devices are listed and chosen as a call's
//! are, and the microphone processed as a call's is; its loudness is reported while it runs, the test sound plays on
//! the chosen speaker on request, and a test plays one's voice back. What it hears reaches no call; it ends when told
//! to close.

use crate::sound::{self, Direction, Looking, Settling, Sounds, Standings, Told};
use screens::{Sound, Subject};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{Instant, MissedTickBehavior, interval};
use webrtc::{Engine, Probe, VoiceRefused};
use words::{Failure, Sentence};

/// What the probe starts with.
#[derive(Clone, Debug)]
pub struct Open {
    /// The microphone to use, none for the system's default; one not here is said and the default used.
    pub microphone: Option<choices::Picked>,
    /// The speaker to use, as `microphone` is.
    pub speaker: Option<choices::Picked>,
    /// How the microphone is processed.
    pub processing: choices::VoiceProcessing,
    /// How often the microphone's loudness is reported.
    pub level_every: Duration,
    /// The watch on the engine's voice chain the host keeps ([`crate::Engines::chain_watch`]), which the probe looks
    /// at with whoever else does.
    pub chain: crate::ChainWatch,
}

/// Why a probe ended by itself.
#[derive(Debug)]
pub enum Ended {
    /// It could not start.
    Refused(VoiceRefused),
    /// The voice chain that carries its sound stopped.
    SoundStopped,
}

impl Ended {
    /// What a person is told of it.
    pub fn failure(self) -> Failure {
        match self {
            Self::Refused(refused) => crate::failure::voice(refused),
            Self::SoundStopped => Failure::SoundStopped,
        }
    }
}

impl From<VoiceRefused> for Ended {
    fn from(refused: VoiceRefused) -> Self {
        Self::Refused(refused)
    }
}

/// What the probe reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Report {
    /// The microphones and speakers there are and which are in use, at the start and whenever a device comes, goes or
    /// is chosen.
    Sound(Sound),
    /// The loudest 10 ms of the microphone since the last report, before the gate, in whole dB below full scale.
    Loudness(i16),
    /// What a person is told because the probe could not do what they asked, about its subject: one short sentence.
    Notice(Subject, Sentence),
}

/// What the host asks of a probe that is running.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Use this microphone, or the system's default for none.
    Microphone(Option<choices::Picked>),
    /// Use this speaker, or the system's default for none.
    Speaker(Option<choices::Picked>),
    /// The system's sound devices came or went: look again.
    DevicesChanged,
    /// Play the test sound on the speaker in use.
    PlayTone,
    /// Process the microphone so.
    Processing(choices::VoiceProcessing),
    /// Play the microphone back a little late on the speaker in use, until [`Command::EndTest`].
    Test,
    /// Stop playing the microphone back, and let go of the speaker.
    EndTest,
    /// Stop and give every device back. A closed command channel means the same, but a sender kept elsewhere holds it
    /// open.
    Close,
}

/// What the probe says of a step still out at its bound: the log's line, and the person's sentence.
fn stalled(within: Duration, subject: Subject, report: &mut impl FnMut(Report)) {
    report::note!(warn, "a step did not come back within its bound and is let go", within_ms = report::millis(within));
    report(Report::Notice(subject, words::failure::of(&Failure::EngineStalled)));
}

/// The probe's devices: which it uses, the look at them under way, and what the chain said became of them.
struct Using {
    engine: Engine,
    sounds: Sounds,
    settling: Settling,
    standings: Standings,
}

impl Using {
    /// Looks at the devices again.
    fn settle(&mut self, looking: Looking, now: Instant, report: &mut impl FnMut(Report)) {
        // A hold of its own, for the look's thread.
        match self.engine.devices() {
            Ok(devices) => self.settling.ask(&self.sounds, devices, looking, now),
            Err(_) => self.sounds.unavailable(&mut |subject, said| report(Report::Notice(subject, said))),
        }
    }

    /// A look answered: said, and the look asked for meanwhile made.
    fn settled(&mut self, looked: Option<(sound::Plan, sound::Chosen)>, now: Instant, report: &mut impl FnMut(Report)) {
        if let Some((plan, chosen)) = looked {
            let sound = self.sounds.settled(plan, chosen, &mut |subject, said| report(Report::Notice(subject, said)));
            report(Report::Sound(sound));
        }
        if let Some(looking) = self.settling.again() {
            self.settle(looking, now, report);
        }
    }

    /// The look is still out at its bound at `now`: said, and let go.
    fn stalled(&mut self, now: Instant, report: &mut impl FnMut(Report)) {
        if self.settling.stalled(now) {
            self.settling.again();
            stalled(sound::LISTING_WITHIN, Subject::Devices, report);
        }
    }

    /// The chain told what became of the devices: one newly refused or unanswered is said once, and again once it
    /// opens by a name, the speaker's as the test's, which is all the probe plays on it. A chain that stopped ends
    /// the probe.
    fn sounded(&mut self, now: webrtc::Sounding, alive: bool, report: &mut impl FnMut(Report)) -> Result<(), Ended> {
        for told in self.standings.hear(now, alive) {
            let (direction, said) = match told {
                Told::Refused(direction, failed) | Told::Unanswered(direction, failed) => {
                    (direction, Some(words::failure::of(&failed)))
                }
                // Said when it first did not answer, and the probe has no place to mute.
                Told::Hung(direction, _) => (direction, None),
                Told::Answered(direction) => (direction, self.sounds.answered(direction)),
                Told::Stopped => return Err(Ended::SoundStopped),
            };
            let subject = match direction {
                Direction::Microphone => Subject::Microphone,
                Direction::Speaker => Subject::Tone,
            };
            if let Some(said) = said {
                report(Report::Notice(subject, said));
            }
        }
        Ok(())
    }
}

/// Measures the microphone until it is told to close or `commands` closes, which gives every device back, or until
/// the voice chain stops, which is why it ended.
pub async fn run(
    engine: &Engine,
    open: Open,
    mut commands: mpsc::Receiver<Command>,
    mut report: impl FnMut(Report),
) -> Result<(), Ended> {
    let devices = engine.devices().map_err(VoiceRefused::Sound)?;
    devices.process(crate::processing::of(&open.processing));
    let mut told = Some(devices.sounding());
    let mut using = Using {
        engine: engine.clone(),
        sounds: Sounds::asking(open.microphone, open.speaker),
        settling: Settling::default(),
        standings: Standings::default(),
    };
    // Before the microphone is opened, so it starts on the device that is meant and is not moved after.
    using.settle(Looking::Forced, Instant::now(), &mut report);
    if let Some(due) = using.settling.due() {
        match tokio::time::timeout_at(due, using.settling.answer()).await {
            Ok(looked) => using.settled(looked.ok(), Instant::now(), &mut report),
            Err(_) => using.stalled(Instant::now(), &mut report),
        }
    }
    let mut probe = Probe::open(engine).await?;
    if let Some(told) = &mut told {
        let now = *told.borrow_and_update();
        using.sounded(now, devices.alive(), &mut report)?;
    }
    let mut _playing = None;
    let mut levels = interval(open.level_every);
    levels.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut health = interval(screens::LOOK_EVERY);
    health.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        let due = using.settling.due();
        tokio::select! {
            asked = commands.recv() => {
                let Some(first) = asked else { return Ok(()) };
                let mut next = Some(first);
                let mut look = false;
                let mut closing = false;
                while let Some(command) = next.take().or_else(|| commands.try_recv().ok()) {
                    match command {
                        Command::Microphone(name) => {
                            using.sounds.ask_microphone(name);
                            look = true;
                        }
                        Command::Speaker(name) => {
                            using.sounds.ask_speaker(name);
                            look = true;
                        }
                        Command::DevicesChanged => look = true,
                        Command::Close => closing = true,
                        Command::Processing(asked) => devices.process(crate::processing::of(&asked)),
                        // Asked only: a speaker that would not open is told with what became of it.
                        Command::Test => match devices.play() {
                            Ok(playing) => {
                                probe.test();
                                _playing = Some(playing);
                            }
                            Err(refused) => {
                                report(Report::Notice(Subject::Tone, words::failure::of(&crate::failure::devices(&refused))));
                            }
                        },
                        Command::EndTest => {
                            probe.end_test();
                            _playing = None;
                        }
                        Command::PlayTone => {
                            let speaker = using.sounds.speaker().map(str::to_owned);
                            if let Err(refused) = ::sound::Sound::shared().chime(::sound::chime::TEST, speaker).await {
                                report(Report::Notice(Subject::Tone, words::failure::of(&crate::failure::played(refused))));
                            }
                        }
                    }
                }
                if closing {
                    return Ok(());
                }
                if look {
                    using.settle(Looking::Moved, Instant::now(), &mut report);
                }
            }
            looked = using.settling.answer() => using.settled(looked.ok(), Instant::now(), &mut report),
            () = crate::bounded::at(due) => using.stalled(Instant::now(), &mut report),
            now = sound::sounding(&mut told) => using.sounded(now, devices.alive(), &mut report)?,
            _ = health.tick() => {
                if !open.chain.look(&devices, Instant::now()).alive {
                    return Err(Ended::SoundStopped);
                }
            }
            _ = levels.tick() => {
                if let Some(db) = probe.loudness() {
                    #[expect(clippy::cast_possible_truncation, reason = "a loudness from -100 to 0 dB")]
                    report(Report::Loudness(db.round() as i16));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use testserver::sound::Lab;
    use tokio::time::timeout;

    use tier::skipped;

    const WAIT: Duration = Duration::from_secs(10);

    /// A probe running in a task, its reports arriving in `reports` and its commands sent through `commands`.
    struct Running {
        commands: mpsc::Sender<Command>,
        reports: mpsc::UnboundedReceiver<Report>,
        task: tokio::task::JoinHandle<Result<(), Ended>>,
    }

    fn start(engine: &Engine, microphone: Option<String>, speaker: Option<String>) -> Running {
        let (commands, queued) = mpsc::channel(8);
        let (told, reports) = mpsc::unbounded_channel();
        let engine = engine.clone();
        let picked = |id: String| choices::Picked { label: id.clone(), id };
        let open = Open {
            microphone: microphone.map(picked),
            speaker: speaker.map(picked),
            // The lab's microphone hears a steady tone, which the strong suppressor takes for noise.
            processing: choices::VoiceProcessing { suppression: choices::Suppression::Off, ..Default::default() },
            level_every: Duration::from_millis(50),
            chain: crate::ChainWatch::default(),
        };
        let task = tokio::spawn(async move { run(&engine, open, queued, |report| drop(told.send(report))).await });
        Running { commands, reports, task }
    }

    impl Running {
        /// The next report that `wanted` takes, within [`WAIT`].
        async fn until<T>(&mut self, mut wanted: impl FnMut(Report) -> Option<T>) -> Option<T> {
            timeout(WAIT, async {
                while let Some(report) = self.reports.recv().await {
                    if let Some(found) = wanted(report) {
                        return Some(found);
                    }
                }
                None
            })
            .await
            .ok()
            .flatten()
        }
    }

    mod lab {
        use super::*;

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn the_probe_lists_the_devices_reports_the_level_plays_the_chime_and_ends_when_told_to_close() {
            let Some(lab) = Lab::take() else { return skipped("the sound lab cannot be locked") };
            let Some(mut sound) = lab.make("callprobe") else {
                return skipped("no sound server to make a microphone on");
            };
            let engine = crate::devices::engine();
            let Ok((inputs, outputs)) = crate::devices::listed().await else { return skipped("no sound") };
            let (Some(microphone), Some(speaker)) = (
                webrtc::virtual_sound::listed(&inputs, &sound.microphone),
                webrtc::virtual_sound::listed(&outputs, &sound.speaker),
            ) else {
                return skipped("the sound server lists no virtual devices");
            };
            let mut probe = start(&engine, Some(microphone.clone()), Some(speaker.clone()));

            let using = probe
                .until(|report| match report {
                    Report::Sound(sound) => Some((sound.microphone, sound.speaker)),
                    _ => None,
                })
                .await;
            assert_eq!(using, Some((Some(microphone), Some(speaker))), "the devices asked for are the ones in use");

            sound.speak();
            let level = probe
                .until(|report| match report {
                    Report::Loudness(db) if db >= -40 => Some(db),
                    _ => None,
                })
                .await;
            assert!(level.is_some(), "the loudness rose with a tone in the microphone");

            let Some(recording) = sound.record(WAIT).await else { return skipped("the speaker cannot be recorded") };
            probe.commands.send(Command::PlayTone).await.expect("the probe is listening");
            let heard = recording.heard(1_000, WAIT).await;
            recording.stop();
            assert!(
                heard.is_ok(),
                "the chime came out of the chosen speaker within {WAIT:?}, loudest sample {heard:?}"
            );

            let Some(recording) = sound.record(WAIT).await else { return skipped("the speaker cannot be recorded") };
            probe.commands.send(Command::Test).await.expect("the probe is listening");
            let heard = recording.heard(1_000, WAIT).await;
            probe.commands.send(Command::EndTest).await.expect("the probe is listening");
            recording.stop();
            assert!(heard.is_ok(), "the test played the microphone's tone back on the speaker, loudest {heard:?}");

            probe.commands.send(Command::Close).await.expect("the probe is listening");
            let ended = timeout(WAIT, probe.task).await.expect("the probe ended when told to close");
            assert!(matches!(ended, Ok(Ok(()))), "and ended well");
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn a_device_asked_for_that_is_not_here_is_said_and_the_default_is_used() {
            let engine = crate::devices::engine();
            let Ok((inputs, _)) = crate::devices::listed().await else {
                return skipped("this machine has no sound to listen to");
            };
            if inputs.len() < 2 {
                return skipped("this machine has no microphone");
            }
            let mut probe = start(&engine, Some("A microphone nobody has".into()), None);
            let told = probe
                .until(|report| match report {
                    Report::Notice(_, said) => Some(said),
                    Report::Sound(_) | Report::Loudness(_) => None,
                })
                .await;
            assert!(
                told.is_some_and(|said| words::Words::english().said(&said).contains("A microphone nobody has")),
                "the sentence names the device"
            );
            let using = probe
                .until(|report| match report {
                    Report::Sound(sound) => Some(sound.microphone),
                    _ => None,
                })
                .await;
            assert_eq!(using, Some(None), "the default stands in");
            drop(probe.commands);
            let ended = timeout(WAIT, probe.task).await.expect("the probe ended with its commands");
            if let Ok(Err(Ended::Refused(VoiceRefused::Sound(why)))) = &ended {
                return skipped(format!("{why:?}"));
            }
            assert!(matches!(ended, Ok(Ok(()))), "a closed channel ends it as well: {ended:?}");
        }
    }
}
