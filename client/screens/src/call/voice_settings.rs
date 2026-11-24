//! Settings › Voice and video: the microphones, speakers and cameras set against what this device remembers, how loud
//! the microphone is, whether the probe that measures it could start, the microphone test and the camera's. All of it
//! exists only while the page is shown.

use std::collections::VecDeque;

use crate::call::camera::ListedCamera;
use crate::call::sound::{DeviceOption, Sound};
use crate::notice::{self, Notice, Subject};
use crate::place::PlaceId;
use choices::{CameraAsk, CameraRate, CameraSettings};
use choices::{Picked, Remembered};
use choices::{Threshold, VoiceProcessing, on_the_meter};
use words::Failure;
use words::Sentence;
use words::Words;

/// What the probe that measures the microphone tells the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeHeard {
    /// The devices there are and which are in use.
    Sound(Sound),
    /// The loudest 10 ms of the microphone since the last, before the gate, in whole dB below full scale.
    Loudness(i16),
    /// Said on the page about its subject: a device that went, a choice that could not be made.
    Notice(Subject, Sentence),
    /// The probe could not start or has ended: one sentence on the page, and no level.
    Failed(Failure),
    /// The cameras there are, at the start and whenever one comes or goes, or why none can be listed.
    Cameras(Result<Vec<ListedCamera>, Failure>),
    /// The camera test's picture is coming, or why it is not.
    CameraTest(Result<(), Failure>),
}

/// What the window asks of the probe, for the host to carry out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeAsk {
    /// Start measuring, on the devices this device remembers.
    Open {
        /// The microphone remembered; none for the system's default.
        microphone: Option<Picked>,
        /// The speaker remembered; none for the system's default.
        speaker: Option<Picked>,
        /// How the microphone is processed.
        processing: VoiceProcessing,
    },
    /// Stop, and give every device back.
    Close,
    /// Measure this microphone instead; none for the system's default.
    Microphone(Option<Picked>),
    /// Play on this speaker instead; none for the system's default.
    Speaker(Option<Picked>),
    /// Play the test sound on the speaker in use.
    PlayTone,
    /// Process the microphone so.
    Processing(VoiceProcessing),
    /// Play the microphone back a little late on the speaker in use, until [`ProbeAsk::EndTest`].
    Test,
    /// Stop playing the microphone back.
    EndTest,
    /// Show this camera in the preview, in place of any shown there.
    TestCamera(CameraAsk),
    /// Let go of the camera the preview shows.
    EndCameraTest,
}

/// The camera test, as the page shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum CameraTest {
    /// Not running: the preview waits for "Test camera".
    #[default]
    Idle,
    /// Asked for, before its first picture.
    Starting,
    /// The camera's pictures fill the preview.
    On,
    /// It stopped or would not start, and why.
    Failed(Failure),
}

impl CameraTest {
    /// Whether it holds the camera, or is about to.
    pub fn running(&self) -> bool {
        matches!(self, CameraTest::Starting | CameraTest::On)
    }

    /// Whether it goes on, its camera's return waited for included: what "Stop test" ends.
    pub fn held(&self) -> bool {
        self.running() || *self == CameraTest::Failed(Failure::CameraGone)
    }
}

/// How many readings the room's quietest moment is found among: the last three seconds, a reading each 100 ms.
const ROOM_READINGS: usize = 30;

/// A microphone test running, and the call it muted for its length, to unmute after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Testing {
    pub(crate) muted: Option<PlaceId>,
}

/// What the page knows while the probe runs.
#[derive(Debug, Default)]
pub struct VoiceProbe {
    sound: Option<Sound>,
    /// The last readings, newest last.
    readings: VecDeque<i16>,
    pub(crate) testing: Option<Testing>,
    failed: Option<Failure>,
    /// What the engine said about the devices (this page's notices).
    notices: Vec<Notice>,
    /// The cameras listed, or why none can be; none before the host has looked.
    cameras: Option<Result<Vec<ListedCamera>, Failure>>,
    pub(crate) camera_test: CameraTest,
}

impl VoiceProbe {
    /// What the probe said.
    pub(crate) fn heard(&mut self, heard: ProbeHeard) {
        match heard {
            ProbeHeard::Sound(sound) if self.failed.is_none() => self.sound = Some(sound),
            ProbeHeard::Loudness(db) if self.failed.is_none() => {
                if self.readings.len() == ROOM_READINGS {
                    self.readings.pop_front();
                }
                self.readings.push_back(db);
            }
            ProbeHeard::Notice(subject, said) => {
                notice::raise(&mut self.notices, Notice::new(subject, design::Status::Warning, said));
            }
            ProbeHeard::Failed(said) => {
                self.failed = Some(said);
                self.readings.clear();
                self.testing = None;
            }
            ProbeHeard::Cameras(listed) => self.cameras = Some(listed),
            ProbeHeard::CameraTest(Ok(())) if self.camera_test == CameraTest::Starting => {
                self.camera_test = CameraTest::On;
            }
            ProbeHeard::CameraTest(Err(failed)) if self.camera_test.running() => {
                self.camera_test = CameraTest::Failed(failed);
            }
            ProbeHeard::Sound(_) | ProbeHeard::Loudness(_) | ProbeHeard::CameraTest(_) => {}
        }
    }

    /// The camera test, as the page shows it.
    pub fn camera_test(&self) -> &CameraTest {
        &self.camera_test
    }

    /// The cameras as the page lists them: each by name with no default, the one opened ticked (the one chosen, else
    /// the first), and a chosen camera that is not here last as not connected, with none ticked: a camera is never
    /// swapped for another. Empty until the host has looked.
    pub fn cameras(&self, chosen: &CameraSettings, words: &Words) -> Vec<DeviceOption> {
        let Some(Ok(listed)) = &self.cameras else { return Vec::new() };
        let listed: Vec<_> = listed.iter().map(|each| each.camera.clone()).collect();
        let using = match &chosen.device {
            Some(picked) => Some(picked.id.as_str()),
            None => listed.first().and_then(|first| first.asked.as_deref()),
        };
        crate::call::sound::options(&listed, using, chosen.device.as_ref(), words)
    }

    /// The camera opened for `chosen`: the one chosen, if it is here, else the first.
    fn using(&self, chosen: &CameraSettings) -> Option<&ListedCamera> {
        let Some(Ok(listed)) = &self.cameras else { return None };
        match &chosen.device {
            Some(picked) => listed.iter().find(|each| each.camera.asked.as_ref() == Some(&picked.id)),
            None => listed.first(),
        }
    }

    /// Whether the camera `chosen` opens is listed now.
    pub(crate) fn camera_listed(&self, chosen: &CameraSettings) -> bool {
        self.using(chosen).is_some()
    }

    /// The camera in use came back to the list, `was_listed` saying whether it was before: a test it ended by going
    /// away starts again, as the person left it running. True when it does.
    pub(crate) fn camera_returned(&mut self, chosen: &CameraSettings, was_listed: bool) -> bool {
        let disconnected = self.camera_test == CameraTest::Failed(Failure::CameraGone);
        let back = disconnected && !was_listed && self.camera_listed(chosen);
        if back {
            self.camera_test = CameraTest::Starting;
        }
        back
    }

    /// The rates the row offers: 60 only where the camera opened captures it at the resolution chosen.
    pub fn camera_rates(&self, chosen: &CameraSettings) -> Vec<CameraRate> {
        let sixty = self.using(chosen).is_some_and(|camera| camera.sixty_at.contains(&chosen.resolution));
        CameraRate::ALL.into_iter().filter(|rate| sixty || *rate != CameraRate::Sixty).collect()
    }

    /// The rate the row shows as chosen: a 60 kept for a camera that does not capture it is shown as the 30 it opens
    /// at.
    pub fn camera_rate(&self, chosen: &CameraSettings) -> CameraRate {
        if self.camera_rates(chosen).contains(&chosen.rate) { chosen.rate } else { CameraRate::Thirty }
    }

    /// Why no camera can be listed, when none can.
    pub fn cameras_failed(&self) -> Option<&Failure> {
        self.cameras.as_ref().and_then(|listed| listed.as_ref().err())
    }

    /// What the page says about the devices, as it shows it.
    pub fn notices(&self) -> Vec<Notice> {
        notice::shown(&self.notices)
    }

    pub(crate) fn put_away(&mut self, subject: Subject) {
        notice::put_away(&mut self.notices, subject);
    }

    /// How loud the microphone is on the meter, from 0 to 100.
    pub fn level(&self) -> u8 {
        self.loudness().map_or(0, on_the_meter)
    }

    /// How loud the microphone is, in dB below full scale; none before a reading.
    pub fn loudness(&self) -> Option<i16> {
        self.readings.back().copied()
    }

    /// The gate's threshold over the room's quietest moment of the last three seconds; none before a reading.
    pub fn room(&self) -> Option<Threshold> {
        self.readings.iter().min().map(|floor| Threshold::over_the_room(*floor))
    }

    /// Whether the microphone test is playing one's voice back.
    pub fn is_testing(&self) -> bool {
        self.testing.is_some()
    }

    /// Why there is nothing to measure with, when the probe could not run.
    pub fn failed(&self) -> Option<&Failure> {
        self.failed.as_ref()
    }

    /// Whether the test sound can be played: the probe is running and has listed the speakers.
    pub fn can_play(&self) -> bool {
        self.sound.is_some() && self.failed.is_none()
    }

    /// The microphones as the page lists them, the remembered one last as not connected when it is not here; empty
    /// until the probe has listed them.
    pub fn microphones(&self, remembered: &Remembered, words: &Words) -> Vec<DeviceOption> {
        self.sound
            .as_ref()
            .map_or_else(Vec::new, |sound| sound.microphone_options(remembered.microphone.as_ref(), words))
    }

    /// The speakers as the page lists them, as for the microphones.
    pub fn speakers(&self, remembered: &Remembered, words: &Words) -> Vec<DeviceOption> {
        self.sound.as_ref().map_or_else(Vec::new, |sound| sound.speaker_options(remembered.speaker.as_ref(), words))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call::sound::Listed;

    const HEADSET: &str = "Headset";
    const DESK: &str = "Desk";

    fn device(label: &str, asked: Option<&str>) -> Listed {
        Listed { asked: asked.map(str::to_string), label: label.to_string() }
    }

    fn here() -> Vec<Listed> {
        vec![device(DESK, None), device(HEADSET, Some(HEADSET)), device(DESK, Some(DESK))]
    }

    fn sound(using: Option<&str>) -> Sound {
        Sound {
            microphones: here(),
            speakers: here(),
            microphone: using.map(str::to_string),
            speaker: using.map(str::to_string),
        }
    }

    fn remembering(name: Option<&str>) -> Remembered {
        Remembered {
            microphone: name.map(crate::call::sound::picked),
            speaker: name.map(crate::call::sound::picked),
            ..Remembered::default()
        }
    }

    fn probe(using: Option<&str>) -> VoiceProbe {
        let mut probe = VoiceProbe::default();
        probe.heard(ProbeHeard::Sound(sound(using)));
        probe
    }

    #[test]
    fn nothing_is_listed_before_the_probe_has_said_what_there_is() {
        let probe = VoiceProbe::default();
        assert!(probe.microphones(&remembering(Some(HEADSET)), &Words::english()).is_empty());
        assert!(probe.speakers(&remembering(None), &Words::english()).is_empty());
        assert!(!probe.can_play());
    }

    #[test]
    fn the_meter_reads_the_loudness_on_the_gates_range_and_a_failure_ends_it_and_the_test_sound() {
        let mut probe = probe(None);
        assert!(probe.can_play());
        probe.heard(ProbeHeard::Loudness(-35));
        assert_eq!((probe.loudness(), probe.level()), (Some(-35), 50));
        probe.heard(ProbeHeard::Loudness(12));
        assert_eq!(probe.level(), 100, "never past the end of the meter");
        probe.heard(ProbeHeard::Failed(Failure::SoundUnavailable));
        assert_eq!((probe.level(), probe.loudness()), (0, None));
        assert_eq!(probe.failed(), Some(&Failure::SoundUnavailable));
        assert!(!probe.can_play());
        probe.heard(ProbeHeard::Loudness(-40));
        assert_eq!(probe.loudness(), None, "a reading that comes after the end is not one");
    }

    #[test]
    fn the_room_is_the_quietest_of_the_last_three_seconds() {
        let mut probe = probe(None);
        assert_eq!(probe.room(), None, "nothing heard yet");
        probe.heard(ProbeHeard::Loudness(-62));
        for _ in 0..ROOM_READINGS - 1 {
            probe.heard(ProbeHeard::Loudness(-30));
        }
        assert_eq!(probe.room().map(Threshold::db), Some(-54), "eight over the quietest");
        probe.heard(ProbeHeard::Loudness(-30));
        assert_eq!(probe.room().map(Threshold::db), Some(-22), "older than three seconds, forgotten");
    }

    #[test]
    fn what_the_engine_says_of_the_devices_is_a_notice_of_the_page_said_once_and_put_away_by_a_press() {
        let mut probe = probe(None);
        probe.heard(ProbeHeard::Loudness(-49));
        for _ in 0..2 {
            probe.heard(ProbeHeard::Notice(Subject::Microphone, Sentence::SoundGone { was: "Desk".into(), now: None }));
        }
        let shown: Vec<_> = probe.notices().into_iter().map(|one| one.said.in_english()).collect();
        assert_eq!(shown, ["“Desk” isn’t connected, so Chatkord uses the default."], "said twice, shown once");
        assert_eq!((probe.loudness(), probe.failed()), (Some(-49), None), "the page itself is as it was");
        probe.put_away(Subject::Microphone);
        assert!(probe.notices().is_empty());
    }
}
