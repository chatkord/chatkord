//! The strip of the other cards under a pinned card, and pausing a picture.

use super::*;

/// Whether the strip of the other cards shows under a pinned card.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Strip {
    /// Under the pinned card, which takes the rest of the stage.
    #[default]
    Shown,
    /// Put away: the pinned card takes the whole stage.
    Hidden,
}

impl Call {
    /// Whether the strip shows under a pinned card on a stage of `stage`: shown on a tall one and put away on a short one
    /// until a person says otherwise there.
    pub fn strip(&self, stage: Stage) -> Strip {
        match stage {
            Stage::Tall => self.strip.0,
            Stage::Short => self.strip.1,
        }
    }

    /// Whether the person paused `user_id`'s picture of `video` here, before it was watched or since.
    pub fn is_paused(&self, user_id: &MemberId, video: Video) -> bool {
        self.paused.contains(&(user_id.clone(), video))
    }
}

impl Place {
    pub(super) fn toggle_strip(&mut self, stage: Stage) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        if let Some(call) = signed.call.as_mut().filter(|call| call.live() && call.pinned.is_some()) {
            let strip = match stage {
                Stage::Tall => &mut call.strip.0,
                Stage::Short => &mut call.strip.1,
            };
            *strip = match strip {
                Strip::Shown => Strip::Hidden,
                Strip::Hidden => Strip::Shown,
            };
        }
        Vec::new()
    }

    pub(super) fn toggle_pause(&mut self, pin: Pinned, at: Moment) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        let Some(call) = signed.call.as_mut().filter(|call| call.live() && call.joined()) else {
            return Vec::new();
        };
        let (user_id, video) = match pin {
            Pinned::Screen(user_id)
                if match call.screens.get(&user_id) {
                    Some(Received::Playing(_)) => true,
                    Some(Received::Waiting) | None => call.is_paused(&user_id, Video::Screen),
                    Some(Received::Unshown(_)) => false,
                } =>
            {
                (user_id, Video::Screen)
            }
            Pinned::Person(user_id) if call.camera_pictured(&user_id) => (user_id, Video::Camera),
            Pinned::Screen(_) | Pinned::Person(_) | Pinned::OwnScreen => return Vec::new(),
        };
        let was = (user_id.clone(), video);
        let paused = !call.paused.remove(&was);
        // The card says it at once; the call's next look says it as it is.
        if let Some(Received::Playing(card)) = call.screens.get_mut(&user_id).filter(|_| video == Video::Screen) {
            match (paused, card.state) {
                (true, _) => card.state = ShareState::Paused,
                (false, ShareState::Paused) => card.state = ShareState::Live,
                (false, _) => {}
            }
        }
        let control = if paused {
            call.paused.insert(was);
            Control::Pause { user_id, video }
        } else {
            Control::Resume { user_id, video }
        };
        call.refresh_tiles(at.now);
        vec![Ask::Call(control)]
    }
}
