//! The call's messages, each handed to the file of what it is about.

use super::*;

impl Place {
    /// The call: only a signed-in server has one, and each message says what it does without one.
    pub(crate) fn on_call(&mut self, message: CallMessage, told: &mut Option<Told>, at: Moment) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut() {
            // A control pressed is a new try: what failed of it is forgotten.
            match &message {
                CallMessage::ToggleMute => call.faults.clear(Device::Microphone),
                CallMessage::ToggleCamera => call.faults.clear(Device::Camera),
                CallMessage::ToggleShare | CallMessage::ChooseScreen => call.faults.clear(Device::Share),
                _ => {}
            }
        }
        match message {
            CallMessage::Room(event) => self.room_said(event, told, at),
            CallMessage::Leave => self.leave_asked(told),
            CallMessage::Notice(subject, said) => self.notice_told(subject, said, told),
            CallMessage::Answered(subject) => self.device_answered(subject),
            CallMessage::Sound(sound) => self.devices_found(sound),
            CallMessage::Health(health) => self.health_looked(health),
            CallMessage::Devices => toggled(signed, Opened::Devices),
            CallMessage::Stats => toggled(signed, Opened::Connection),
            CallMessage::ShareSound(user_id) => self.toggle_share_sound(user_id),
            CallMessage::AudioControls => toggled(signed, Opened::Audio),
            CallMessage::ViewMenu => toggled(signed, Opened::View),
            CallMessage::Volume(user_id, ..)
            | CallMessage::KeepVolume(user_id, ..)
            | CallMessage::MuteForMe(user_id, ..)
                if self.standing.as_ref().is_some_and(|standing| standing.user_id == user_id) =>
            {
                Vec::new()
            }
            CallMessage::MuteForMe(user_id, audio) => self.mute_for_me(user_id, audio),
            CallMessage::Volume(user_id, audio, percent) => self.set_volume(user_id, audio, percent),
            CallMessage::KeepVolume(user_id, audio, percent) => self.keep_volume(user_id, audio, percent),
            CallMessage::CloseOver => self.close_over(),
            CallMessage::ToggleShare => self.toggle_share(),
            CallMessage::ToggleCamera => self.toggle_camera(),
            CallMessage::ChooseScreen => self.choose_screen(),
            CallMessage::Sources(listed) => self.sources_listed(listed),
            CallMessage::ShowOwn(video) => self.show_own(video),
            CallMessage::SourcesTab(tab) => self.show_sources_tab(tab),
            CallMessage::SelectSource(key) => self.select_source(key),
            CallMessage::ShareSource => self.share_source(),
            CallMessage::Quality(pick) => self.pick_quality(pick),
            CallMessage::Rate(message) => self.pick_rate(message),
            CallMessage::Starting => self.share_starting(),
            CallMessage::Shared(reported) => self.share_reported(reported, told),
            CallMessage::Camera(reported) => self.camera_reported(reported, told),
            CallMessage::Pin(pinned) => self.pin(pinned),
            CallMessage::Unpin => self.unpin(),
            CallMessage::PopOut(user_id, video) => self.pop_out(user_id, video),
            CallMessage::PopIn => self.pop_in(),
            CallMessage::FullScreen(user_id) => self.full_screen_on(user_id, at),
            CallMessage::LeaveFullScreen => self.full_screen_off(),
            CallMessage::ToggleZoom(user_id) => self.toggle_zoom(&user_id),
            CallMessage::ZoomWheel { notches, at, area } => self.zoom_wheel(notches, at, area),
            CallMessage::Pan { by, area } => self.pan(by, area),
            CallMessage::Stirred => self.stirred(at),
            CallMessage::ToggleStrip(stage) => self.toggle_strip(stage),
            CallMessage::ToggleInfo(user_id) => self.toggle_info(user_id),
            CallMessage::TogglePause(pin) => self.toggle_pause(pin, at),
            CallMessage::OwnWatched { video, watched } => self.own_watched(video, watched),
            CallMessage::SoundApps(apps) => self.sound_apps_listed(apps),
            CallMessage::PickSoundApp(app) => self.pick_sound_app(app),
            CallMessage::ToggleOwnInfo => self.toggle_own_info(),
            CallMessage::ToggleOwnCameraInfo => self.toggle_own_camera_info(),
            CallMessage::Watched(reported) => self.watched_reported(reported, told, at),
            CallMessage::ToggleMute => self.toggle_mute(told),
            CallMessage::ToggleDeafen => self.toggle_deafen(told),
            CallMessage::CallOver(end) => self.call_over(end, at),
        }
    }
}
