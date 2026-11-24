//! What this account does about hearing and being heard: mute and deafen, and an administrator's hold.

use super::*;

impl Call {
    /// What this account has done about hearing and being heard: what the person pressed, which the call carries out.
    pub fn state(&self) -> State {
        self.me.as_ref().map(|me| self.room.state_of(me)).unwrap_or_default()
    }

    /// Whether an administrator has this account muted for everyone: its microphone stays off whatever its own mute,
    /// which stays its own.
    pub fn held(&self) -> bool {
        self.me.as_ref().is_some_and(|me| self.room.held(me))
    }

    /// Whether an administrator has `user_id` muted for everyone.
    pub fn held_by_admin(&self, user_id: &MemberId) -> bool {
        self.room.held(user_id)
    }
}

impl Place {
    pub(super) fn toggle_mute(&mut self, told: &mut Option<Told>) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        match signed.call.as_ref() {
            // The own mute stays the person's while an administrator holds them, apart from the hold.
            Some(call) if call.live() && call.joined() => {
                let (now, next) = (call.state(), call.state().mute_pressed());
                *told = Chime::pressed(now, next).map(Told::Chime);
                let undeafen = (now.deafened != next.deafened).then_some(Ask::Call(Control::Deafen(next.deafened)));
                undeafen.into_iter().chain([Ask::Call(Control::Mute(next.mic_muted))]).collect()
            }
            Some(_) | None => Vec::new(),
        }
    }

    pub(super) fn toggle_deafen(&mut self, told: &mut Option<Told>) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        match signed.call.as_ref() {
            Some(call) if call.live() && call.joined() => {
                let (now, next) = (call.state(), State { deafened: !call.state().deafened, ..call.state() });
                *told = Chime::pressed(now, next).map(Told::Chime);
                vec![Ask::Call(Control::Deafen(next.deafened))]
            }
            Some(_) | None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::call::room::Event as RoomEvent;
    use crate::harness::{Clock, ada_in_call, member, tell};
    use crate::place;
    use crate::window::Window;
    use std::time::Instant;

    #[test]
    fn a_mute_for_everyone_says_so_and_leaves_the_own_mute_and_deafening_to_the_account() {
        let clock = Clock(Instant::now());
        let (mut window, place) = ada_in_call(&clock);
        let controls = |todo: Vec<crate::Ask>| -> Vec<place::Control> {
            todo.into_iter()
                .filter_map(
                    |todo| if let crate::Ask::At(_, place::Ask::Call(control)) = todo { Some(control) } else { None },
                )
                .collect()
        };
        tell(
            &mut window,
            &clock,
            place,
            place::Message::Call(place::CallMessage::Room(RoomEvent::State {
                user_id: member("ada"),
                state: choices::State { mic_muted: true, deafened: false },
            })),
        );
        tell(
            &mut window,
            &clock,
            place,
            place::Message::Call(place::CallMessage::Room(RoomEvent::Held { user_id: member("ada"), held: true })),
        );
        let said = |window: &Window| -> Option<(design::Status, String)> {
            crate::StateLine::of(&window.places()[0]).map(|line| (line.kind, line.said.in_english()))
        };
        assert_eq!(
            said(&window),
            Some((
                design::Status::Danger,
                "An admin muted you for everyone. When they undo it, your microphone is muted, as you set it."
                    .to_string()
            ))
        );
        let badges = |window: &Window| {
            window.places()[0].call().map(|call| {
                call.tiles().iter().filter(|tile| tile.own()).flat_map(|tile| tile.badges()).collect::<Vec<_>>()
            })
        };
        assert_eq!(badges(&window), Some(vec![design::Presence::MutedByAdmin]), "held, with the own mute on");
        assert_eq!(
            controls(tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleMute))),
            [place::Control::Mute(false)],
            "the own mute is still theirs, asked as ever"
        );
        tell(
            &mut window,
            &clock,
            place,
            place::Message::Call(place::CallMessage::Room(RoomEvent::State {
                user_id: member("ada"),
                state: choices::State::default(),
            })),
        );
        assert_eq!(
            said(&window).map(|(_, said)| said).as_deref(),
            Some("An admin muted you for everyone. When they undo it, your microphone is on.")
        );
        assert_eq!(badges(&window), Some(vec![design::Presence::MutedByAdmin]), "held, with the own mute off");
        tell(
            &mut window,
            &clock,
            place,
            place::Message::Call(place::CallMessage::Room(RoomEvent::State {
                user_id: member("ada"),
                state: choices::State { mic_muted: true, deafened: false },
            })),
        );
        assert_eq!(
            controls(tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleDeafen))),
            [place::Control::Deafen(true)],
            "hearing is still theirs"
        );
        tell(
            &mut window,
            &clock,
            place,
            place::Message::Call(place::CallMessage::Room(RoomEvent::Held { user_id: member("ada"), held: false })),
        );
        assert!(said(&window).is_none(), "lifted: the words go");
        assert_eq!(
            controls(tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleMute))),
            [place::Control::Mute(false)],
            "and the mute is theirs again, still on as they left it"
        );
    }

    #[test]
    fn the_mute_button_asks_the_opposite_and_deafened_it_undoes_both() {
        let clock = Clock(Instant::now());
        let (mut window, place) = ada_in_call(&clock);
        let control = |todo: Vec<crate::Ask>| -> Vec<place::Control> {
            todo.into_iter()
                .filter_map(
                    |todo| if let crate::Ask::At(_, place::Ask::Call(control)) = todo { Some(control) } else { None },
                )
                .collect()
        };
        assert_eq!(
            control(tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleMute))),
            [place::Control::Mute(true)]
        );
        let state = |mic_muted, deafened| {
            place::Message::Call(place::CallMessage::Room(RoomEvent::State {
                user_id: member("ada"),
                state: choices::State { mic_muted, deafened },
            }))
        };
        tell(&mut window, &clock, place, state(true, false));
        assert_eq!(
            control(tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleMute))),
            [place::Control::Mute(false)]
        );
        assert_eq!(
            control(tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleDeafen))),
            [place::Control::Deafen(true)]
        );
        tell(&mut window, &clock, place, state(true, true));
        assert_eq!(
            control(tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleMute))),
            [place::Control::Deafen(false), place::Control::Mute(false)],
            "a microphone cannot be on while nothing is heard"
        );
        tell(&mut window, &clock, place, state(false, true));
        assert_eq!(
            control(tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleDeafen))),
            [place::Control::Deafen(false)]
        );
    }
}
