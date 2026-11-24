//! The window's own sounds: which moments sound, and whether any does. How each sounds is the host's.

use choices::{EventSounds, MessageSounds, State};
use std::time::Duration;

/// How long after a message's sound is asked for the next one is not: a burst of messages is one sound, not a rattle.
pub(crate) const MESSAGE_CHIME_GAP: Duration = Duration::from_millis(1500);

/// A moment the window marks with a sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq, report::Public)]
pub enum Chime {
    /// This account's call began.
    Joined,
    /// This account left its call.
    Left,
    /// Someone came into this account's call.
    MemberJoined,
    /// Someone left this account's call.
    MemberLeft,
    /// This account's microphone was muted.
    Muted,
    /// This account's microphone was opened again.
    Unmuted,
    /// This account stopped hearing the call.
    Deafened,
    /// This account hears the call again.
    Undeafened,
    /// This account began sharing a screen in its call.
    ShareStarted,
    /// This account's screen share stopped.
    ShareStopped,
    /// Someone else in the call began sharing a screen.
    MemberShareStarted,
    /// Someone else's screen share in the call stopped.
    MemberShareStopped,
    /// A signed-in connection to a server was lost, and is being tried again.
    Disconnected,
    /// A message that would alert this account was told, in a conversation not being read.
    Message,
}

impl Chime {
    /// The sound of a press that moved this account from `before` to `after`: deafening says more than muting.
    pub(crate) fn pressed(before: State, after: State) -> Option<Chime> {
        if before.deafened != after.deafened {
            Some(if after.deafened { Chime::Deafened } else { Chime::Undeafened })
        } else if before.mic_muted != after.mic_muted {
            Some(if after.mic_muted { Chime::Muted } else { Chime::Unmuted })
        } else {
            None
        }
    }

    /// It, if the switch that owns it is on: a message's own, the window's event sounds for every other.
    pub(crate) fn under(self, events: EventSounds, messages: MessageSounds) -> Option<Chime> {
        let on = match self {
            Chime::Message => messages == MessageSounds::On,
            Chime::Joined
            | Chime::Left
            | Chime::MemberJoined
            | Chime::MemberLeft
            | Chime::Muted
            | Chime::Unmuted
            | Chime::Deafened
            | Chime::Undeafened
            | Chime::ShareStarted
            | Chime::ShareStopped
            | Chime::MemberShareStarted
            | Chime::MemberShareStopped
            | Chime::Disconnected => events == EventSounds::On,
        };
        on.then_some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call::room::{Event as RoomEvent, Person, Track};
    use crate::call::sound::Keep;
    use crate::door::Opening;
    use crate::harness::{ADA, Clock, accepted_as, ada_in_call, channels, flow, id, member, owls, tell, token};
    use crate::window::Window;
    use crate::{Ask, WindowMessage, place};
    use choices::Remembered;
    use std::collections::BTreeMap;
    use std::time::Instant;
    use terms::Ended;

    const HEARD: State = State { mic_muted: false, deafened: false };
    const MUTED: State = State { mic_muted: true, deafened: false };
    const DEAFENED: State = State { mic_muted: true, deafened: true };

    #[test]
    fn a_press_sounds_what_changed_and_deafening_says_more_than_muting() {
        assert_eq!(Chime::pressed(HEARD, MUTED), Some(Chime::Muted));
        assert_eq!(Chime::pressed(MUTED, HEARD), Some(Chime::Unmuted));
        assert_eq!(Chime::pressed(HEARD, DEAFENED), Some(Chime::Deafened));
        assert_eq!(Chime::pressed(DEAFENED, HEARD), Some(Chime::Undeafened));
        assert_eq!(Chime::pressed(MUTED, MUTED), None);
    }

    #[test]
    fn off_sounds_nothing_and_the_switch_flips_back() {
        let (events, messages) = (EventSounds::default(), MessageSounds::default());
        assert_eq!(Chime::Left.under(events, messages), Some(Chime::Left));
        assert_eq!(Chime::Left.under(EventSounds::Off, messages), None);
        assert_eq!(Chime::Left.under(events, MessageSounds::Off), Some(Chime::Left), "the message switch is its own");
        assert_eq!(Chime::Message.under(events, messages), Some(Chime::Message));
        assert_eq!(Chime::Message.under(events, MessageSounds::Off), None);
        assert_eq!(Chime::Message.under(EventSounds::Off, messages), Some(Chime::Message), "and the event one is not");
        assert_eq!(EventSounds::On.toggled().toggled(), EventSounds::On);
    }

    fn chimes(todo: &[Ask]) -> Vec<Chime> {
        todo.iter().filter_map(|todo| if let Ask::Chime(chime) = todo { Some(*chime) } else { None }).collect()
    }

    #[test]
    fn mute_and_deafen_sound_on_press_in_and_out_of_a_call() {
        let clock = Clock(Instant::now());
        let (window, _) = Window::launch(Vec::new(), None, clock.0);
        let mut window = window.remembering(Remembered::default(), BTreeMap::new());
        assert_eq!(chimes(&window.update(WindowMessage::NextMute, clock.at())), [Chime::Muted], "outside a call too");
        assert_eq!(chimes(&window.update(WindowMessage::NextDeafen, clock.at())), [Chime::Deafened]);
        assert_eq!(
            chimes(&window.update(WindowMessage::NextMute, clock.at())),
            [Chime::Undeafened],
            "mute undoes deafen"
        );

        let (mut window, place) = ada_in_call(&clock);
        let call = |message| place::Message::Call(message);
        assert_eq!(chimes(&tell(&mut window, &clock, place, call(place::CallMessage::ToggleMute))), [Chime::Muted]);
        tell(
            &mut window,
            &clock,
            place,
            call(place::CallMessage::Room(RoomEvent::State { user_id: member("ada"), state: State::default() })),
        );
        assert_eq!(
            chimes(&tell(&mut window, &clock, place, call(place::CallMessage::ToggleDeafen))),
            [Chime::Deafened],
            "on the press, before the server agrees"
        );
    }

    #[test]
    fn a_call_sounds_its_start_its_arrivals_its_shares_and_its_leave() {
        let clock = Clock(Instant::now());
        let (mut window, _) = Window::launch(vec![Opening::Account(owls(), ADA.into())], None, clock.0);
        let place = id(&window, 0);
        tell(&mut window, &clock, place, flow(token()));
        tell(&mut window, &clock, place, flow(accepted_as("ada")));
        tell(&mut window, &clock, place, place::Message::Lobby(place::LobbyMessage::Channels(Ok(channels()))));
        let call = |message| place::Message::Call(message);
        tell(
            &mut window,
            &clock,
            place,
            place::Message::Lobby(place::LobbyMessage::Join(crate::harness::channel_id("lounge"))),
        );
        let snapshot = || RoomEvent::Joined {
            members: vec![Person::new(member("ada"), ADA), Person::new(member("bo"), "Bo")],
            streams: vec![(member("bo"), Track::ScreenVideo)],
        };
        let room =
            |window: &mut Window, event| chimes(&tell(window, &clock, place, call(place::CallMessage::Room(event))));
        assert_eq!(room(&mut window, snapshot()), [Chime::Joined], "one's own start, not each person or share there");
        assert_eq!(room(&mut window, snapshot()), [], "a later snapshot sounds nothing");
        let cy = || RoomEvent::MemberJoined { user_id: member("cy"), name: "Cy".into(), colour: None };
        assert_eq!(room(&mut window, cy()), [Chime::MemberJoined]);
        assert_eq!(room(&mut window, cy()), [], "said twice, sounded once");
        let shares = |user_id: &str| RoomEvent::Started { user_id: member(user_id), track: Track::ScreenVideo };
        assert_eq!(room(&mut window, shares("cy")), [Chime::MemberShareStarted]);
        assert_eq!(room(&mut window, shares("ada")), [Chime::ShareStarted], "one's own share sounds its own");
        let sound = RoomEvent::Started { user_id: member("cy"), track: Track::ScreenAudio };
        assert_eq!(room(&mut window, sound), [], "a share's sound is part of the share");
        let stops = RoomEvent::Stopped { user_id: member("cy"), track: Track::ScreenVideo };
        assert_eq!(room(&mut window, stops), [Chime::MemberShareStopped]);
        let mine = RoomEvent::Stopped { user_id: member("ada"), track: Track::ScreenVideo };
        assert_eq!(room(&mut window, mine), [Chime::ShareStopped]);
        let leaves = || RoomEvent::MemberLeft { user_id: member("cy") };
        assert_eq!(room(&mut window, leaves()), [Chime::MemberLeft]);
        assert_eq!(room(&mut window, leaves()), [], "someone not here leaves nothing to hear");
        assert_eq!(chimes(&tell(&mut window, &clock, place, call(place::CallMessage::Leave))), [Chime::Left]);
        assert_eq!(chimes(&tell(&mut window, &clock, place, call(place::CallMessage::Leave))), [], "left once");
    }

    #[test]
    fn a_signed_in_connection_that_drops_sounds_once_and_not_while_it_tries_again() {
        let clock = Clock(Instant::now());
        let (mut window, place) = ada_in_call(&clock);
        let dropped = || place::Message::Session(place::SessionMessage::Disconnected(Ended::default()));
        assert_eq!(chimes(&tell(&mut window, &clock, place, dropped())), [Chime::Disconnected]);
        assert_eq!(chimes(&tell(&mut window, &clock, place, dropped())), [], "already trying again");
        window.update(WindowMessage::ToggleEventSounds, clock.at());
        let (mut window, place) = ada_in_call(&clock);
        window.update(WindowMessage::ToggleEventSounds, clock.at());
        assert_eq!(chimes(&tell(&mut window, &clock, place, dropped())), [], "with the sounds off");
    }

    #[test]
    fn with_event_sounds_off_nothing_sounds_and_the_switch_is_kept() {
        let clock = Clock(Instant::now());
        let (mut window, place) = ada_in_call(&clock);
        assert_eq!(window.event_sounds(), EventSounds::On, "on until switched off");
        let todo = window.update(WindowMessage::ToggleEventSounds, clock.at());
        assert!(matches!(&todo[..], [Ask::Keep(Keep::EventSounds(EventSounds::Off))]), "kept: {todo:?}");
        let call = |message| place::Message::Call(message);
        let bo = RoomEvent::MemberJoined { user_id: member("bo"), name: "Bo".into(), colour: None };
        assert_eq!(chimes(&tell(&mut window, &clock, place, call(place::CallMessage::Room(bo)))), []);
        assert_eq!(chimes(&tell(&mut window, &clock, place, call(place::CallMessage::ToggleMute))), []);
        assert_eq!(chimes(&tell(&mut window, &clock, place, call(place::CallMessage::Leave))), []);
        let (window, _) = Window::launch(Vec::new(), None, clock.0);
        let remembered = Remembered { event_sounds: EventSounds::Off, ..Remembered::default() };
        let mut window = window.remembering(remembered, BTreeMap::new());
        assert_eq!(chimes(&window.update(WindowMessage::NextMute, clock.at())), [], "as the last run left it");
    }
}
