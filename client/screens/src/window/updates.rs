//! Chatkord's own update in the window: what a call and a share mean for it, the switch kept, and the restart.

use super::*;
use crate::update::{InVoice, SendsVideo, UpdateCard, UpdateMessage, UpdateNotice, Updates};

impl Window {
    /// Whether this build and run can update themselves, and where the update stands.
    pub fn updates(&self) -> &Updates {
        &self.desktop.updates
    }

    /// The side pane's notice of the update, when it shows one.
    pub fn update_notice(&self) -> Option<UpdateNotice> {
        self.desktop.updates.notice(self.in_voice(), self.voice.auto_update)
    }

    /// What About Chatkord's Updates card shows.
    pub fn update_card(&self) -> UpdateCard {
        self.desktop.updates.card(self.in_voice(), self.voice.auto_update)
    }

    /// Whether a call runs in this window, which a restart would end.
    pub(super) fn in_voice(&self) -> InVoice {
        if self.places.iter().any(Place::in_call) { InVoice::Yes } else { InVoice::No }
    }

    /// Whether this window sends a screen or a camera.
    fn sends_video(&self) -> SendsVideo {
        let sharing = self.places.iter().filter_map(Place::call).any(|call| *call.sending() != place::Sending::No);
        if sharing || self.camera_live() { SendsVideo::Yes } else { SendsVideo::No }
    }

    /// The check by itself as now scheduled, asked of the host only when it changed: whatever changed it, a share, a
    /// call, the switch or the update itself.
    pub(super) fn reschedule_update(&mut self) -> Option<Ask> {
        let scheduled = self.desktop.updates.schedule(self.voice.auto_update, self.sends_video());
        (scheduled != self.desktop.update_scheduled).then(|| {
            self.desktop.update_scheduled = scheduled;
            Ask::Update(UpdateAsk::Schedule(scheduled))
        })
    }

    /// What the host or a person told of the update; the switch is kept, where updates are offered.
    pub(super) fn updated(&mut self, message: UpdateMessage) -> Vec<Ask> {
        if message == UpdateMessage::ToggleAutomatic {
            if self.desktop.updates == Updates::Unoffered {
                return Vec::new();
            }
            self.voice.auto_update = self.voice.auto_update.toggled();
            return vec![Ask::Keep(Keep::AutoUpdate(self.voice.auto_update))];
        }
        let (in_voice, sends) = (self.in_voice(), self.sends_video());
        self.desktop
            .updates
            .update(message, self.voice.auto_update, in_voice, sends)
            .into_iter()
            .map(Ask::Update)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{Clock, ada_in_call, sharer_in_call, two_signed_in};
    use crate::update::{CHECK_EVERY, FIRST_CHECK, RestartOffer, UpdateAsk, UpdateFound, UpdateStage};
    use crate::{CallMessage, CameraSent};
    use choices::AutoUpdate;
    use std::time::SystemTime;

    const VERSION: &str = "0.9.1";

    fn wall(after: Duration) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000) + after
    }

    fn told(window: &mut Window, clock: &Clock, message: UpdateMessage) -> Vec<Ask> {
        window.update(Message::Update(message), clock.at())
    }

    fn update_asks(todo: &[Ask]) -> Vec<UpdateAsk> {
        todo.iter().filter_map(|ask| if let Ask::Update(asked) = ask { Some(*asked) } else { None }).collect()
    }

    /// The first check, as the host is asked to schedule it.
    fn first() -> UpdateAsk {
        UpdateAsk::Schedule(Some(wall(FIRST_CHECK)))
    }

    /// `window` told its updates are offered, with a newer version found and put in place.
    fn ready(window: &mut Window, clock: &Clock) {
        told(window, clock, UpdateMessage::Offered(wall(Duration::ZERO)));
        told(window, clock, UpdateMessage::Check);
        told(
            window,
            clock,
            UpdateMessage::Checked { at: wall(FIRST_CHECK), found: UpdateFound::Newer(VERSION.into()) },
        );
        told(window, clock, UpdateMessage::Placed);
    }

    #[test]
    fn a_window_whose_build_has_no_channel_offers_no_update_and_its_switch_does_nothing() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        assert_eq!(window.updates(), &Updates::Unoffered);
        assert!(told(&mut window, &clock, UpdateMessage::ToggleAutomatic).is_empty());
        assert_eq!(window.voice().auto_update, AutoUpdate::On);
        assert!(told(&mut window, &clock, UpdateMessage::Clock(wall(CHECK_EVERY))).is_empty());
        assert_eq!(window.update_card().automatic, None);
    }

    #[test]
    fn the_switch_is_kept_and_off_holds_the_checks_made_by_themselves() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let todo = told(&mut window, &clock, UpdateMessage::Offered(wall(Duration::ZERO)));
        assert_eq!(update_asks(&todo), [first()]);
        let todo = told(&mut window, &clock, UpdateMessage::ToggleAutomatic);
        assert!(
            matches!(&todo[..], [Ask::Keep(Keep::AutoUpdate(AutoUpdate::Off)), Ask::Update(UpdateAsk::Schedule(None))]),
            "{todo:?}"
        );
        assert_eq!(window.update_card().automatic, Some(AutoUpdate::Off));
        assert!(update_asks(&told(&mut window, &clock, UpdateMessage::Clock(wall(FIRST_CHECK)))).is_empty());
        let todo = told(&mut window, &clock, UpdateMessage::ToggleAutomatic);
        assert_eq!(update_asks(&todo), [first()], "switched on, the check due is scheduled again");
        let todo = told(&mut window, &clock, UpdateMessage::Clock(wall(CHECK_EVERY)));
        assert_eq!(update_asks(&todo), [UpdateAsk::Check, UpdateAsk::Schedule(None)]);
    }

    #[test]
    fn a_check_made_by_itself_waits_while_this_window_shares_its_screen() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        told(&mut window, &clock, UpdateMessage::Offered(wall(Duration::ZERO)));
        let todo = crate::harness::call_says(&mut window, &clock, place, CallMessage::Starting);
        assert_eq!(update_asks(&todo), [UpdateAsk::Schedule(None)], "the share's first step holds the check off");
        let started = crate::Shared::Started { width: 1920, height: 1080, encoder: crate::Route::Software };
        let todo = crate::harness::call_says(&mut window, &clock, place, CallMessage::Shared(started));
        assert!(update_asks(&todo).is_empty(), "already held off from when the share was picked: {todo:?}");
        let todo = told(&mut window, &clock, UpdateMessage::Clock(wall(FIRST_CHECK)));
        assert!(update_asks(&todo).is_empty(), "postponed: {todo:?}");
        let todo = told(&mut window, &clock, UpdateMessage::Check);
        assert_eq!(update_asks(&todo), [UpdateAsk::Check], "one asked for is made");
    }

    #[test]
    fn a_check_made_by_itself_waits_while_this_window_sends_its_camera() {
        let clock = Clock(Instant::now());
        let (mut window, place) = ada_in_call(&clock);
        told(&mut window, &clock, UpdateMessage::Offered(wall(Duration::ZERO)));
        let started = CameraSent::Started { width: 1280, height: 720, encoder: crate::Route::Software };
        let todo = crate::harness::call_says(&mut window, &clock, place, CallMessage::Camera(started));
        assert!(window.camera_live(), "the camera goes out");
        assert_eq!(update_asks(&todo), [UpdateAsk::Schedule(None)]);
        assert!(update_asks(&told(&mut window, &clock, UpdateMessage::Clock(wall(FIRST_CHECK)))).is_empty());
        let todo = crate::harness::call_says(&mut window, &clock, place, CallMessage::Camera(CameraSent::Stopped));
        assert_eq!(update_asks(&todo), [first()], "scheduled again once the camera stops");
    }

    /// The host is asked to schedule only when the schedule changes, so what else the window is told never makes it
    /// read the clock again, and a reading before the time due never reaches the window at all.
    #[test]
    fn the_check_is_scheduled_again_only_when_its_time_changes() {
        let clock = Clock(Instant::now());
        let (mut window, place) = ada_in_call(&clock);
        assert_eq!(update_asks(&told(&mut window, &clock, UpdateMessage::Offered(wall(Duration::ZERO)))), [first()]);
        for message in [Message::NextMute, Message::Tick, Message::OpenDevice(DevicePage::AboutChatkord)] {
            let todo = window.update(message, clock.at());
            assert!(update_asks(&todo).is_empty(), "{todo:?}");
        }
        crate::harness::call_says(&mut window, &clock, place, CallMessage::ToggleMute);
        let todo = told(&mut window, &clock, UpdateMessage::Clock(wall(FIRST_CHECK)));
        assert_eq!(update_asks(&todo), [UpdateAsk::Check, UpdateAsk::Schedule(None)]);
        let found = UpdateMessage::Checked { at: wall(FIRST_CHECK), found: UpdateFound::Nothing };
        let todo = told(&mut window, &clock, found);
        assert_eq!(update_asks(&todo), [UpdateAsk::Schedule(Some(wall(FIRST_CHECK + CHECK_EVERY)))]);
    }

    #[test]
    fn a_restart_waits_until_the_call_is_left_and_the_tray_offers_it_only_then() {
        let clock = Clock(Instant::now());
        let (mut window, place) = ada_in_call(&clock);
        ready(&mut window, &clock);
        assert_eq!(window.updates().restart_offer(window.in_voice()), RestartOffer::DeferredUntilVoiceEnds);
        let restart = |window: &Window| window.tray_menu().items.iter().any(|item| item.pick == TrayPick::Restart);
        assert!(!restart(&window), "never offered in a call");
        assert!(update_asks(&told(&mut window, &clock, UpdateMessage::Restart)).is_empty());
        assert!(update_asks(&window.update(Message::FromTray(TrayPick::Restart), clock.at())).is_empty());
        crate::harness::call_says(&mut window, &clock, place, CallMessage::Leave);
        crate::harness::call_says(&mut window, &clock, place, CallMessage::CallOver(None));
        assert_eq!(window.in_voice(), InVoice::No, "the call is left");
        assert!(restart(&window));
        let todo = window.update(Message::FromTray(TrayPick::Restart), clock.at());
        assert_eq!(update_asks(&todo), [UpdateAsk::Restart]);
        assert!(matches!(window.updates(), Updates::Offered { stage: UpdateStage::Ready { .. }, .. }));
    }
}
