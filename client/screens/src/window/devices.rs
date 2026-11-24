//! This device's microphone, speaker and camera: picked, tested and measured by the probe behind the voice settings.

use super::*;

/// What this device's settings page and probe hold, besides what is remembered for calls.
#[derive(Debug, Default)]
pub(super) struct DeviceSettings {
    /// The page of this device's settings shown, over whichever server is open.
    pub(super) device: Option<DevicePage>,
    /// The probe's side of the voice settings: there exactly while the page is shown.
    pub(super) probe: Option<VoiceProbe>,
    /// This device's encoders for a screen; none until the host has listed them, and a share is offered till then.
    pub(super) encoders: Option<Vec<EncoderOption>>,
    /// Which picker a share is chosen in on this system.
    pub(super) chooser: Chooser,
}

impl Window {
    /// A microphone picked: kept, and asked of the call that is running.
    pub(super) fn on_pick_microphone(&mut self, asked: Option<Picked>) -> Vec<Ask> {
        self.voice.microphone.clone_from(&asked);
        self.picked(Keep::Microphone(asked.clone()), place::Control::Microphone(asked))
    }

    /// A speaker picked: kept, and asked of the call that is running.
    pub(super) fn on_pick_speaker(&mut self, asked: Option<Picked>) -> Vec<Ask> {
        self.voice.speaker.clone_from(&asked);
        self.picked(Keep::Speaker(asked.clone()), place::Control::Speaker(asked))
    }

    /// The test sound pressed: it plays once the probe can.
    pub(super) fn on_play_test_sound(&self) -> Vec<Ask> {
        if self.hardware.probe.as_ref().is_some_and(VoiceProbe::can_play) {
            vec![Ask::Probe(ProbeAsk::PlayTone)]
        } else {
            Vec::new()
        }
    }

    /// The processing chosen: kept and handed on.
    pub(super) fn on_set_processing(&mut self, asked: VoiceProcessing) -> Vec<Ask> {
        self.voice.processing = asked;
        self.processed(Some(Keep::Processing(asked)))
    }

    /// The processing dragged: handed on, and kept once let go.
    pub(super) fn on_tune_processing(&mut self, asked: VoiceProcessing) -> Vec<Ask> {
        self.voice.processing = asked;
        self.processed(None)
    }

    /// A camera picked.
    pub(super) fn on_pick_camera(&mut self, picked: Option<Picked>) -> Vec<Ask> {
        self.voice.camera.device = picked;
        self.camera_chosen()
    }

    /// A camera resolution picked.
    pub(super) fn on_pick_camera_resolution(&mut self, resolution: choices::CameraResolution) -> Vec<Ask> {
        self.voice.camera.resolution = resolution;
        self.camera_chosen()
    }

    /// A camera rate picked.
    pub(super) fn on_pick_camera_rate(&mut self, rate: choices::CameraRate) -> Vec<Ask> {
        self.voice.camera.rate = rate;
        self.camera_chosen()
    }

    /// The camera's mirror switched.
    pub(super) fn on_toggle_mirror(&mut self) -> Vec<Ask> {
        self.voice.camera.mirror = self.voice.camera.mirror.toggled();
        self.camera_chosen()
    }

    /// The probe said something: the page shows it, and a camera listed again after the test lost it is tested again.
    /// None while there is no probe, which leaves nothing to settle.
    pub(super) fn on_probed(&mut self, heard: ProbeHeard) -> Option<Vec<Ask>> {
        let probe = self.hardware.probe.as_mut()?;
        let was_listed = probe.camera_listed(&self.voice.camera);
        probe.heard(heard);
        let back = probe.camera_returned(&self.voice.camera, was_listed) && !self.camera_live();
        Some(back.then(|| Ask::Probe(ProbeAsk::TestCamera(self.voice.camera.ask()))).into_iter().collect())
    }

    /// One camera is opened once: a camera turned on in a call ends the test, and the preview shows the call's.
    pub(super) fn camera_test_yielded(&mut self) -> Option<Ask> {
        let live = self.camera_live();
        let probe = self.hardware.probe.as_mut().filter(|probe| live && probe.camera_test.running())?;
        probe.camera_test = crate::CameraTest::Idle;
        Some(Ask::Probe(ProbeAsk::EndCameraTest))
    }

    /// The probe is started when the voice settings come up and stopped when they go: for a pick, a server's change or a
    /// connection lost alike, so it never outlives the page.
    pub(super) fn probing(&mut self) -> Vec<Ask> {
        match (self.hardware.device.is_some(), self.hardware.probe.is_some()) {
            (true, false) => {
                self.hardware.probe = Some(VoiceProbe::default());
                report::note!(info, "voice probe opened");
                vec![Ask::Probe(ProbeAsk::Open {
                    microphone: self.voice.microphone.clone(),
                    speaker: self.voice.speaker.clone(),
                    processing: self.voice.processing,
                })]
            }
            (false, true) => {
                let testing = self.hardware.probe.take().and_then(|probe| probe.testing);
                report::note!(info, "voice probe closed", testing = testing.is_some());
                let mut todo = vec![Ask::Probe(ProbeAsk::Close)];
                todo.extend(unmute_after_test(testing));
                todo
            }
            (true, true) | (false, false) => Vec::new(),
        }
    }

    /// Whether a call here sends this account's camera, or is opening it.
    pub fn camera_live(&self) -> bool {
        self.places.iter().any(|place| *place.own_camera() != crate::OwnCamera::Off)
    }

    /// The camera test started or stopped; none while a call sends the camera, whose picture the preview shows.
    pub(super) fn test_camera(&mut self) -> Vec<Ask> {
        let live = self.camera_live();
        let ask = self.voice.camera.ask();
        let Some(probe) = self.hardware.probe.as_mut().filter(|_| !live) else { return Vec::new() };
        if probe.camera_test.held() {
            probe.camera_test = crate::CameraTest::Idle;
            return vec![Ask::Probe(ProbeAsk::EndCameraTest)];
        }
        probe.camera_test = crate::CameraTest::Starting;
        vec![Ask::Probe(ProbeAsk::TestCamera(ask))]
    }

    /// A camera choice made: kept, opened again by a call that sends the camera, and by a test that shows it.
    pub(super) fn camera_chosen(&mut self) -> Vec<Ask> {
        let ask = self.voice.camera.ask();
        let mut todo = vec![Ask::Keep(Keep::Camera(self.voice.camera.clone()))];
        for place in &mut self.places {
            if let Some(asked) = place.set_camera(&self.voice.camera) {
                todo.push(Ask::At(place.id(), asked));
            }
        }
        if let Some(probe) = self.hardware.probe.as_mut().filter(|probe| probe.camera_test.running()) {
            probe.camera_test = crate::CameraTest::Starting;
            todo.push(Ask::Probe(ProbeAsk::TestCamera(ask)));
        }
        todo
    }

    /// The microphone test started or stopped. A call running unmuted is muted for its length and unmuted after, as
    /// Sharkord does, so what one says to test is not sent; the chain plays the test in place of the call's sound.
    pub(super) fn test_microphone(&mut self) -> Vec<Ask> {
        let testing = |window: &Self| window.hardware.probe.as_ref().is_some_and(|probe| probe.testing.is_some());
        let before = testing(self);
        let todo = self.toggle_microphone_test();
        report::note!(info, "microphone test pressed", ran_before = before, runs_now = testing(self));
        todo
    }

    /// The microphone test turned on if it was off and off if it was on, where the probe can play.
    fn toggle_microphone_test(&mut self) -> Vec<Ask> {
        let Some(probe) = self.hardware.probe.as_mut().filter(|probe| probe.can_play()) else { return Vec::new() };
        if let Some(testing) = probe.testing.take() {
            return std::iter::once(Ask::Probe(ProbeAsk::EndTest)).chain(unmute_after_test(Some(testing))).collect();
        }
        let unmuted = self
            .places
            .iter()
            .find(|place| place.in_call())
            .filter(|place| place.call().is_some_and(|call| !call.state().mic_muted))
            .map(Place::id);
        probe.testing = Some(Testing { muted: unmuted });
        let mute = unmuted.map(|id| Ask::At(id, place::Ask::Call(place::Control::Mute(true))));
        std::iter::once(Ask::Probe(ProbeAsk::Test)).chain(mute).collect()
    }

    /// How the microphone is processed, handed to the call running and the probe, and kept when `keep` says.
    pub(super) fn processed(&self, keep: Option<Keep>) -> Vec<Ask> {
        let asked = self.voice.processing;
        let call = self
            .places
            .iter()
            .find(|place| place.in_call())
            .map(|place| Ask::At(place.id(), place::Ask::Call(place::Control::Processing(asked))));
        let probe = self.hardware.probe.as_ref().map(|_| Ask::Probe(ProbeAsk::Processing(asked)));
        keep.map(Ask::Keep).into_iter().chain(call).chain(probe).collect()
    }

    /// The voice settings' side: what the probe has said, while the page is shown.
    pub fn voice_probe(&self) -> Option<&VoiceProbe> {
        self.hardware.probe.as_ref()
    }

    /// A device picked: kept for the next run, and handed to the call that is running, if one is.
    pub(super) fn picked(&self, keep: Keep, control: place::Control) -> Vec<Ask> {
        let call = self
            .places
            .iter()
            .find(|place| place.in_call())
            .map(|place| Ask::At(place.id(), place::Ask::Call(control)));
        let probe = self.hardware.probe.as_ref().and_then(|_| match &keep {
            Keep::Microphone(asked) => Some(Ask::Probe(ProbeAsk::Microphone(asked.clone()))),
            Keep::Speaker(asked) => Some(Ask::Probe(ProbeAsk::Speaker(asked.clone()))),
            Keep::Share(_)
            | Keep::Look(_)
            | Keep::EventSounds(_)
            | Keep::MessageSounds(_)
            | Keep::Popups(_)
            | Keep::Processing(_)
            | Keep::Startup(_)
            | Keep::Camera(_)
            | Keep::Language(_)
            | Keep::AutoUpdate(_)
            | Keep::Emoji(_) => None,
        });
        std::iter::once(Ask::Keep(keep)).chain(call).chain(probe).collect()
    }

    /// The gate on, its threshold over the room's quietest moment, once the probe has heard the room.
    pub(super) fn gate_from_room(&mut self, at: Moment) -> Vec<Ask> {
        match self.hardware.probe.as_ref().and_then(VoiceProbe::room) {
            Some(threshold) => {
                let asked = VoiceProcessing { gate: NoiseGate::On, threshold, ..self.voice.processing };
                self.update(Message::SetProcessing(asked), at)
            }
            None => Vec::new(),
        }
    }

    /// What the voice settings page says of `subject`, put away.
    pub(super) fn dismiss_probe_notice(&mut self, subject: Subject) -> Vec<Ask> {
        if let Some(probe) = self.hardware.probe.as_mut() {
            probe.put_away(subject);
        }
        Vec::new()
    }
}

/// Unmutes the call a microphone test muted.
fn unmute_after_test(testing: Option<Testing>) -> Option<Ask> {
    testing.and_then(|testing| testing.muted).map(|id| Ask::At(id, place::Ask::Call(place::Control::Mute(false))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call::room::Event as RoomEvent;
    use crate::harness::{Clock, ada_in_call, id, in_call, in_call_offering_a_camera, member, tell, two_signed_in};
    use proto::rules::Grants;

    #[test]
    fn a_device_picked_is_kept_and_asked_of_the_call_that_is_running() {
        let clock = Clock(Instant::now());
        let (mut window, place) = ada_in_call(&clock);
        let asked = window.update(Message::PickMicrophone(Some(crate::call::sound::picked("Headset"))), clock.at());
        assert!(matches!(&asked[0], Ask::Keep(Keep::Microphone(Some(name))) if name.id == "Headset"), "{asked:?}");
        assert!(
            matches!(&asked[1], Ask::At(at, place::Ask::Call(place::Control::Microphone(Some(name)))) if *at == place && name.id == "Headset"),
            "{asked:?}"
        );
        assert_eq!(window.voice().microphone.as_ref().map(|picked| picked.id.as_str()), Some("Headset"));
        let back = window.update(Message::PickSpeaker(None), clock.at());
        assert!(matches!(&back[0], Ask::Keep(Keep::Speaker(None))), "the default is kept as none: {back:?}");
        assert!(matches!(&back[1], Ask::At(_, place::Ask::Call(place::Control::Speaker(None)))), "{back:?}");
    }

    #[test]
    fn a_device_picked_with_no_call_is_only_kept() {
        let clock = Clock(Instant::now());
        let (mut window, _) = Window::launch(vec![Opening::Fresh], None, clock.0);
        let asked = window.update(Message::PickSpeaker(Some(crate::call::sound::picked("Headset"))), clock.at());
        assert_eq!(asked.len(), 1, "{asked:?}");
        assert!(matches!(&asked[0], Ask::Keep(Keep::Speaker(Some(_)))));
    }

    /// The window with the voice settings open on the first server, remembering a headset for the microphone.
    fn on_the_voice_page(clock: &Clock) -> (Window, PlaceId, Vec<Ask>) {
        let remembered = Remembered {
            microphone: Some(crate::call::sound::picked("Headset")),
            speaker: None,
            ..Remembered::default()
        };
        let mut window = two_signed_in(clock).remembering(remembered, BTreeMap::new());
        let owls_id = id(&window, 0);
        let todo = window.update(Message::OpenDevice(DevicePage::VoiceAndVideo), clock.at());
        (window, owls_id, todo)
    }

    fn probed(window: &mut Window, clock: &Clock, heard: ProbeHeard) -> Vec<Ask> {
        window.update(Message::Probed(heard), clock.at())
    }

    /// A USB camera that captures 60 a second at 720p, and a built-in one that never does.
    fn cameras() -> Vec<crate::ListedCamera> {
        let listed = |name: &str, sixty_at: Vec<choices::CameraResolution>| crate::ListedCamera {
            camera: crate::Listed { asked: Some(name.to_lowercase()), label: name.into() },
            sixty_at,
        };
        vec![listed("USB camera", vec![choices::CameraResolution::P720]), listed("Built-in camera", Vec::new())]
    }

    #[test]
    fn sixty_a_second_is_offered_only_where_the_camera_opened_captures_it() {
        use choices::CameraRate::{Fifteen, Sixty, Thirty};
        let clock = Clock(Instant::now());
        let (mut window, _, _) = on_the_voice_page(&clock);
        probed(&mut window, &clock, ProbeHeard::Cameras(Ok(cameras())));
        let row = |window: &Window| {
            let (probe, kept) = (window.voice_probe().unwrap(), &window.voice().camera);
            (probe.camera_rates(kept), probe.camera_rate(kept))
        };
        assert_eq!(row(&window), (vec![Sixty, Thirty, Fifteen], Thirty), "the USB camera at 720p");
        window.update(Message::PickCameraRate(Sixty), clock.at());
        assert_eq!(row(&window).1, Sixty);
        window.update(Message::PickCameraResolution(choices::CameraResolution::P1080), clock.at());
        assert_eq!(row(&window), (vec![Thirty, Fifteen], Thirty), "60 kept is shown as the 30 it opens at");
        let built_in = Picked { id: "built-in camera".into(), label: "Built-in camera".into() };
        window.update(Message::PickCameraResolution(choices::CameraResolution::P720), clock.at());
        window.update(Message::PickCamera(Some(built_in)), clock.at());
        assert_eq!(row(&window).0, [Thirty, Fifteen], "a camera that never captures 60 is not offered it");
    }

    #[test]
    fn cameras_have_no_default_and_one_not_plugged_in_comes_last() {
        let clock = Clock(Instant::now());
        let (mut window, _, _) = on_the_voice_page(&clock);
        probed(&mut window, &clock, ProbeHeard::Cameras(Ok(cameras())));
        let rows = |window: &Window| {
            let kept = &window.voice().camera;
            let rows = window.voice_probe().unwrap().cameras(kept, window.words());
            rows.into_iter().map(|row| (row.name.clone(), row.ticked(), row.away())).collect::<Vec<_>>()
        };
        let row = |name: &str, ticked: bool, away: bool| (name.to_string(), ticked, away);
        assert_eq!(
            rows(&window),
            [row("USB camera", true, false), row("Built-in camera", false, false)],
            "none chosen: the first is in use"
        );
        let desk = Picked { id: "desk".into(), label: "Desk camera".into() };
        let todo = window.update(Message::PickCamera(Some(desk)), clock.at());
        assert!(matches!(&todo[..], [Ask::Keep(Keep::Camera(kept))] if kept.device.is_some()), "{todo:?}");
        assert_eq!(
            rows(&window),
            [row("USB camera", false, false), row("Built-in camera", false, false), row("Desk camera", false, true)],
            "a chosen camera not here is listed last, and none stands in for it"
        );
        let usb = Picked { id: "usb camera".into(), label: "USB camera".into() };
        window.update(Message::PickCamera(Some(usb)), clock.at());
        assert_eq!(rows(&window), [row("USB camera", true, false), row("Built-in camera", false, false)]);
        probed(&mut window, &clock, ProbeHeard::Cameras(Err(words::Failure::CameraUnavailable)));
        assert_eq!(window.voice_probe().unwrap().cameras_failed(), Some(&words::Failure::CameraUnavailable));
    }

    #[test]
    fn a_test_shows_starting_then_the_picture_or_its_sentence() {
        let clock = Clock(Instant::now());
        let (mut window, _, _) = on_the_voice_page(&clock);
        let test = |window: &Window| window.voice_probe().unwrap().camera_test().clone();
        let todo = window.update(Message::TestCamera, clock.at());
        assert!(
            matches!(&todo[..], [Ask::Probe(ProbeAsk::TestCamera(asked))] if *asked == choices::CameraAsk::default()),
            "{todo:?}"
        );
        assert_eq!(test(&window), crate::CameraTest::Starting);
        probed(&mut window, &clock, ProbeHeard::CameraTest(Ok(())));
        assert_eq!(test(&window), crate::CameraTest::On);
        let todo = window.update(Message::TestCamera, clock.at());
        assert!(matches!(&todo[..], [Ask::Probe(ProbeAsk::EndCameraTest)]), "{todo:?}");
        assert_eq!(test(&window), crate::CameraTest::Idle);
        probed(&mut window, &clock, ProbeHeard::CameraTest(Err(words::Failure::CameraBusy)));
        assert_eq!(test(&window), crate::CameraTest::Idle, "a word after the end is nothing");
        window.update(Message::TestCamera, clock.at());
        probed(&mut window, &clock, ProbeHeard::CameraTest(Err(words::Failure::CameraBusy)));
        assert_eq!(test(&window), crate::CameraTest::Failed(words::Failure::CameraBusy));
        let todo = window.update(Message::PickCameraRate(choices::CameraRate::Fifteen), clock.at());
        assert!(matches!(&todo[..], [Ask::Keep(_)]), "a test that failed is not opened again: {todo:?}");
    }

    #[test]
    fn a_camera_disconnected_mid_test_and_listed_again_is_tested_again() {
        let clock = Clock(Instant::now());
        let (mut window, _, _) = on_the_voice_page(&clock);
        let test = |window: &Window| window.voice_probe().unwrap().camera_test().clone();
        probed(&mut window, &clock, ProbeHeard::Cameras(Ok(cameras())));
        window.update(Message::TestCamera, clock.at());
        probed(&mut window, &clock, ProbeHeard::CameraTest(Err(words::Failure::CameraGone)));
        probed(&mut window, &clock, ProbeHeard::Cameras(Ok(Vec::new())));
        assert_eq!(test(&window), crate::CameraTest::Failed(words::Failure::CameraGone), "while it is away");
        assert!(test(&window).held(), "its test goes on, and Stop test ends the wait");
        let todo = probed(&mut window, &clock, ProbeHeard::Cameras(Ok(cameras())));
        assert!(matches!(&todo[..], [Ask::Probe(ProbeAsk::TestCamera(_))]), "back, it is opened again: {todo:?}");
        assert_eq!(test(&window), crate::CameraTest::Starting);
        probed(&mut window, &clock, ProbeHeard::CameraTest(Ok(())));
        window.update(Message::TestCamera, clock.at());
        window.update(Message::TestCamera, clock.at());
        probed(&mut window, &clock, ProbeHeard::CameraTest(Err(words::Failure::CameraBusy)));
        probed(&mut window, &clock, ProbeHeard::Cameras(Ok(cameras())));
        assert_eq!(test(&window), crate::CameraTest::Failed(words::Failure::CameraBusy), "another refusal stays said");
        assert!(!test(&window).held(), "and is tested again by Test camera");
        window.update(Message::TestCamera, clock.at());
        probed(&mut window, &clock, ProbeHeard::CameraTest(Err(words::Failure::CameraGone)));
        probed(&mut window, &clock, ProbeHeard::Cameras(Ok(Vec::new())));
        let todo = window.update(Message::TestCamera, clock.at());
        assert!(matches!(&todo[..], [Ask::Probe(ProbeAsk::EndCameraTest)]), "Stop test ends the wait: {todo:?}");
        let todo = probed(&mut window, &clock, ProbeHeard::Cameras(Ok(cameras())));
        assert!(todo.is_empty() && test(&window) == crate::CameraTest::Idle, "and it is not opened on its return");
    }

    #[test]
    fn turning_the_camera_on_ends_a_test_and_the_preview_shows_the_call_s() {
        let clock = Clock(Instant::now());
        let (mut window, place) = in_call_offering_a_camera(&clock, Grants::from_words(&["SHARE_CAMERA"]));
        window.update(Message::OpenDevice(DevicePage::VoiceAndVideo), clock.at());
        window.update(Message::TestCamera, clock.at());
        probed(&mut window, &clock, ProbeHeard::CameraTest(Ok(())));
        let todo = tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleCamera));
        assert!(
            matches!(
                &todo[..],
                [Ask::Probe(ProbeAsk::EndCameraTest), Ask::At(_, place::Ask::Call(place::Control::Camera(Some(_))))]
            ),
            "the test lets the camera go before the call opens it: {todo:?}"
        );
        assert!(window.camera_live());
        assert_eq!(*window.voice_probe().unwrap().camera_test(), crate::CameraTest::Idle);
        assert!(window.update(Message::TestCamera, clock.at()).is_empty(), "no test while the call sends it");
    }

    fn listed_sound() -> crate::Sound {
        let device =
            |label: &str, asked: Option<&str>| crate::Listed { asked: asked.map(str::to_string), label: label.into() };
        crate::Sound {
            microphones: vec![device("Desk", None), device("Desk", Some("Desk"))],
            speakers: vec![device("Desk", None)],
            microphone: None,
            speaker: None,
        }
    }

    #[test]
    fn the_voice_settings_start_the_probe_on_what_this_device_remembers_and_stop_it_when_they_go() {
        let clock = Clock(Instant::now());
        let (mut window, _, opened) = on_the_voice_page(&clock);
        assert_eq!(window.device(), Some(DevicePage::VoiceAndVideo));
        assert!(
            matches!(&opened[..], [Ask::Probe(ProbeAsk::Open { microphone: Some(mic), speaker: None, .. })] if mic.id == "Headset"),
            "{opened:?}"
        );
        assert!(window.voice_probe().is_some());
        let again = window.update(Message::Tick, clock.at());
        assert!(again.is_empty(), "one probe however long the page is shown: {again:?}");
        let closed = window.update(Message::CloseDevice, clock.at());
        assert!(matches!(&closed[..], [Ask::Probe(ProbeAsk::Close)]), "{closed:?}");
        assert!(window.voice_probe().is_none());
        assert_eq!(window.open().unwrap().panel(), Panel::Discussion);
    }

    #[test]
    fn processing_set_is_kept_and_handed_to_the_probe_and_a_drag_is_kept_only_once_let_go() {
        let clock = Clock(Instant::now());
        let (mut window, _, _) = on_the_voice_page(&clock);
        let asked = VoiceProcessing { automatic_gain: choices::AutomaticGain::Off, ..VoiceProcessing::default() };
        let todo = window.update(Message::SetProcessing(asked), clock.at());
        assert!(
            matches!(&todo[..], [Ask::Keep(Keep::Processing(kept)), Ask::Probe(ProbeAsk::Processing(handed))] if *kept == asked && *handed == asked),
            "{todo:?}"
        );
        let dragged = VoiceProcessing { volume: choices::InputGain::new(6).unwrap(), ..asked };
        let todo = window.update(Message::TuneProcessing(dragged), clock.at());
        assert!(
            matches!(&todo[..], [Ask::Probe(ProbeAsk::Processing(handed))] if *handed == dragged),
            "applied, not kept yet: {todo:?}"
        );
        assert_eq!(window.voice().processing, dragged);
    }

    #[test]
    fn the_room_turns_the_gate_on_eight_db_over_its_quietest_moment() {
        let clock = Clock(Instant::now());
        let (mut window, _, _) = on_the_voice_page(&clock);
        assert!(window.update(Message::GateFromRoom, clock.at()).is_empty(), "nothing heard, nothing set");
        probed(&mut window, &clock, ProbeHeard::Sound(listed_sound()));
        probed(&mut window, &clock, ProbeHeard::Loudness(-62));
        probed(&mut window, &clock, ProbeHeard::Loudness(-40));
        let todo = window.update(Message::GateFromRoom, clock.at());
        let gate = window.voice().processing;
        assert_eq!((gate.gate, gate.threshold.db()), (NoiseGate::On, -54));
        assert!(matches!(&todo[0], Ask::Keep(Keep::Processing(kept)) if *kept == gate), "{todo:?}");
    }

    /// Whether `todo` mutes, or unmutes, the call of `place`.
    fn mutes(todo: &[Ask], place: PlaceId, muted: bool) -> bool {
        todo.iter().any(
            |one| matches!(one, Ask::At(at, place::Ask::Call(place::Control::Mute(m))) if *at == place && *m == muted),
        )
    }

    #[test]
    fn a_microphone_test_mutes_the_call_for_its_length_and_unmutes_it_after() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id, _) = on_the_voice_page(&clock);
        in_call(&mut window, &clock, owls_id);
        probed(&mut window, &clock, ProbeHeard::Sound(listed_sound()));
        let started = window.update(Message::TestMicrophone, clock.at());
        assert!(matches!(started[0], Ask::Probe(ProbeAsk::Test)), "{started:?}");
        assert!(mutes(&started, owls_id, true), "{started:?}");
        assert!(window.voice_probe().unwrap().is_testing());
        let stopped = window.update(Message::TestMicrophone, clock.at());
        assert!(matches!(stopped[0], Ask::Probe(ProbeAsk::EndTest)), "{stopped:?}");
        assert!(mutes(&stopped, owls_id, false), "{stopped:?}");
        assert!(!window.voice_probe().unwrap().is_testing());
    }

    #[test]
    fn each_press_of_the_microphone_test_and_the_probe_opening_and_closing_are_noted() {
        use tracing_subscriber::layer::SubscriberExt as _;
        let ring = report::Ring::new(std::time::SystemTime::now);
        tracing::subscriber::with_default(tracing_subscriber::registry().with(ring.layer()), || {
            let clock = Clock(Instant::now());
            let (mut window, _, _) = on_the_voice_page(&clock);
            probed(&mut window, &clock, ProbeHeard::Sound(listed_sound()));
            window.update(Message::TestMicrophone, clock.at());
            window.update(Message::TestMicrophone, clock.at());
            window.update(Message::TestMicrophone, clock.at());
            window.update(Message::CloseDevice, clock.at());
        });
        let said: Vec<String> = ring.lines().iter().map(|line| line.as_str().to_string()).collect();
        let count = |what: &str| said.iter().filter(|line| line.contains(what)).count();
        assert_eq!(count("voice probe opened"), 1, "{said:?}");
        assert_eq!(count("microphone test pressed  ran_before=false  runs_now=true"), 2, "{said:?}");
        assert_eq!(count("microphone test pressed  ran_before=true  runs_now=false"), 1, "{said:?}");
        assert_eq!(count("voice probe closed  testing=true"), 1, "a test that was still running when the page closed");
    }

    #[test]
    fn a_test_left_by_closing_the_page_unmutes_the_call_and_a_muted_call_is_left_muted() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id, _) = on_the_voice_page(&clock);
        in_call(&mut window, &clock, owls_id);
        probed(&mut window, &clock, ProbeHeard::Sound(listed_sound()));
        window.update(Message::TestMicrophone, clock.at());
        let closed = window.update(Message::CloseDevice, clock.at());
        assert!(mutes(&closed, owls_id, false), "{closed:?}");
        let (mut window, place) = ada_in_call(&clock);
        let state = choices::State { mic_muted: true, deafened: false };
        let muted = place::Message::Call(place::CallMessage::Room(RoomEvent::State { user_id: member("ada"), state }));
        tell(&mut window, &clock, place, muted);
        window.update(Message::OpenDevice(DevicePage::VoiceAndVideo), clock.at());
        probed(&mut window, &clock, ProbeHeard::Sound(listed_sound()));
        let started = window.update(Message::TestMicrophone, clock.at());
        let ended = window.update(Message::TestMicrophone, clock.at());
        assert!(matches!(started[..], [Ask::Probe(ProbeAsk::Test)]), "a call muted before is left so: {started:?}");
        assert!(matches!(ended[..], [Ask::Probe(ProbeAsk::EndTest)]), "{ended:?}");
    }

    #[test]
    fn device_settings_stay_open_while_a_server_reconnects() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id, _) = on_the_voice_page(&clock);
        probed(&mut window, &clock, ProbeHeard::Sound(listed_sound()));
        window.update(Message::TestMicrophone, clock.at());
        let lost = place::Message::Session(place::SessionMessage::Disconnected(terms::Ended::default()));
        let todo = tell(&mut window, &clock, owls_id, lost);
        assert!(!todo.iter().any(|done| matches!(done, Ask::Probe(_))), "the probe is left as it is: {todo:?}");
        assert_eq!(window.device(), Some(DevicePage::VoiceAndVideo));
        assert!(window.voice_probe().is_some_and(VoiceProbe::is_testing), "the microphone test goes on");
    }

    #[test]
    fn device_settings_stay_open_at_a_servers_door() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id, _) = on_the_voice_page(&clock);
        let reset = terms::Ended { reason: Some(proto::chatkord_v1::CloseReason::PasswordReset), ..Default::default() };
        tell(&mut window, &clock, owls_id, place::Message::Session(place::SessionMessage::Disconnected(reset)));
        assert_eq!(window.open().unwrap().panel(), Panel::SignedOut);
        assert_eq!(window.device(), Some(DevicePage::VoiceAndVideo));
        assert!(window.voice_probe().is_some(), "the probe runs on");
    }

    #[test]
    fn the_probe_runs_while_the_device_page_is_shown_whatever_the_server() {
        let clock = Clock(Instant::now());
        let (mut window, _, _) = on_the_voice_page(&clock);
        let books = id(&window, 1);
        let todo = window.update(Message::Open(books), clock.at());
        assert!(!todo.iter().any(|done| matches!(done, Ask::Probe(_))), "another server opened: {todo:?}");
        assert!(window.voice_probe().is_some());
        let todo = tell(&mut window, &clock, books, place::Message::Page(place::PageMessage::OpenAccount));
        assert!(matches!(&todo[..], [Ask::Probe(ProbeAsk::Close)]), "a server's own page replaces it: {todo:?}");
        assert!(window.voice_probe().is_none());
    }

    #[test]
    fn a_pick_is_kept_and_steers_the_probe_only_while_the_page_is_shown() {
        let clock = Clock(Instant::now());
        let (mut window, _, _) = on_the_voice_page(&clock);
        let todo = window.update(Message::PickMicrophone(Some(crate::call::sound::picked("Desk"))), clock.at());
        assert!(
            matches!(&todo[..], [Ask::Keep(Keep::Microphone(Some(kept))), Ask::Probe(ProbeAsk::Microphone(Some(asked)))]
                if kept.id == "Desk" && asked.id == "Desk"),
            "{todo:?}"
        );
        let todo = window.update(Message::PickSpeaker(None), clock.at());
        assert!(matches!(&todo[..], [Ask::Keep(Keep::Speaker(None)), Ask::Probe(ProbeAsk::Speaker(None))]), "{todo:?}");
        window.update(Message::CloseDevice, clock.at());
        let todo = window.update(Message::PickMicrophone(None), clock.at());
        assert!(matches!(&todo[..], [Ask::Keep(Keep::Microphone(None))]), "no probe, nothing to steer: {todo:?}");
    }

    #[test]
    fn the_test_sound_waits_for_the_probe_then_plays() {
        let clock = Clock(Instant::now());
        let (mut window, _, _) = on_the_voice_page(&clock);
        assert!(window.update(Message::PlayTestSound, clock.at()).is_empty(), "no probe has listed the speakers yet");
        probed(&mut window, &clock, ProbeHeard::Sound(listed_sound()));
        let todo = window.update(Message::PlayTestSound, clock.at());
        assert!(matches!(&todo[..], [Ask::Probe(ProbeAsk::PlayTone)]), "{todo:?}");
        assert_eq!(window.toast().map(|toast| toast.said().in_english()).as_deref(), None, "the sound is the answer");
    }

    #[test]
    fn what_the_probe_says_reaches_the_page_and_a_notice_is_the_page_s_and_a_late_word_is_dropped() {
        let clock = Clock(Instant::now());
        let (mut window, _, _) = on_the_voice_page(&clock);
        probed(&mut window, &clock, ProbeHeard::Sound(listed_sound()));
        probed(&mut window, &clock, ProbeHeard::Loudness(-27));
        let page = window.voice_probe().unwrap();
        assert_eq!(page.level(), 61);
        let rows = page.microphones(window.voice(), window.words());
        assert_eq!(
            rows.iter().map(|row| (row.name.as_str(), row.away())).collect::<Vec<_>>(),
            [("Default", false), ("Desk", false), ("Headset", true)],
            "the saved headset is listed, away"
        );
        probed(
            &mut window,
            &clock,
            ProbeHeard::Notice(
                Subject::Microphone,
                Sentence::SoundGone { was: "Headset".into(), now: Some("Desk".into()) },
            ),
        );
        assert_eq!(
            window.toast().map(|toast| toast.said().in_english()).as_deref(),
            None,
            "a device that fell back is no confirmation: it is not a toast"
        );
        let said: Vec<_> =
            window.voice_probe().unwrap().notices().into_iter().map(|one| one.said.in_english()).collect();
        assert_eq!(said, ["“Headset” isn’t connected, so Chatkord uses “Desk”."]);
        window.update(Message::DismissProbeNotice(Subject::Microphone), clock.at());
        assert!(window.voice_probe().unwrap().notices().is_empty(), "put away by a press");
        window.update(Message::CloseDevice, clock.at());
        assert!(
            probed(&mut window, &clock, ProbeHeard::Notice(Subject::Microphone, Sentence::Said("late".into())))
                .is_empty()
        );
        assert_eq!(
            window.toast().map(|toast| toast.said().in_english()).as_deref(),
            None,
            "the probe is closed: what it said after is nothing"
        );
    }
}
