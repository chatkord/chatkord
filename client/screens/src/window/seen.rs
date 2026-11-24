//! Where the call's pictures show: over the whole display, in a window of their own, in the floating card, or nowhere.

use super::*;

impl Window {
    /// The floating card goes back to where it starts.
    pub(super) fn on_float_reset(&mut self) -> Vec<Ask> {
        self.layout.float_at = FloatAt::HOME;
        Vec::new()
    }

    /// The floating card is closed until the call's view is shown again.
    pub(super) fn on_float_close(&mut self) -> Vec<Ask> {
        self.layout.away = Away::Closed;
        Vec::new()
    }

    /// The host says whether the window shows on the display.
    pub(super) fn on_shown(&mut self, shown: bool) -> Vec<Ask> {
        self.layout.shown = shown;
        Vec::new()
    }

    /// The watched screen that fills the whole display, in the open place, and whether its controls show now.
    pub fn full_screen(&self) -> Option<FullView<'_>> {
        let place = self.open()?;
        let call = place.call()?;
        let screen = call.full_screen()?;
        let zoom = call.zoom(screen.user_id);
        Some(FullView { place: place.id(), screen, controls: screen.calm_at > self.times.now, zoom })
    }

    /// The screen or camera in a window of its own, in the call where it is: what that window shows.
    pub fn pop_out(&self) -> Option<PoppedOut<'_>> {
        let place = self.places.iter().find(|place| place.calling())?;
        let screen = place.call()?.out()?;
        Some(PoppedOut { place, screen })
    }

    /// The floating card in a window of `room`: the pinned share of a call whose view is not shown, unless it was closed
    /// since that view last was. Its room is the whole window, so it floats over the members column and is dragged
    /// anywhere in it.
    pub fn floating(&self, room: Room) -> Option<Floating<'_>> {
        let place = self.places.iter().find(|place| place.calling())?;
        if self.layout.away == Away::Closed || self.call_shown(place) {
            return None;
        }
        let screen = place.call()?.pinned_share()?;
        Some(Floating { place: place.id(), screen, at: self.layout.float_at.place(room) })
    }

    /// The call's place and the pictures sent in it that are shown nowhere now. A screen is shown over the whole
    /// display, in a window of its own, and while this window shows, on the call's view (pinned or as a card) or in the
    /// floating card. A screen in a strip put away counts as shown, so a shown one is never taken for unseen. A camera is
    /// shown only on its person's card, on the call's view while this window shows, or in a window of its own.
    pub fn unseen(&self) -> Option<(PlaceId, Vec<(MemberId, Video)>)> {
        let place = self.places.iter().find(|place| place.calling())?;
        let call = place.call()?;
        let cards = call.cards();
        let shared = cards.iter().filter_map(|card| match card {
            crate::Card::Screen { user_id, .. } => Some(*user_id),
            crate::Card::Person(_) | crate::Card::OwnScreen(_) => None,
        });
        // One's own pinned share is nobody's picture to watch, so it is never asked for.
        let pinned = match call.pinned_share() {
            Some(place::PinnedShare::Someone(pinned)) => Some(pinned.user_id),
            Some(place::PinnedShare::Own) | None => None,
        };
        let out = call.out();
        let out_screen = out.filter(|out| out.video == Video::Screen).map(|out| out.user_id);
        let out_camera = out.filter(|out| out.video == Video::Camera).map(|out| out.user_id);
        let full = call.full_screen().map(|full| full.user_id);
        let mut seen: Vec<&MemberId> = out_screen.into_iter().collect();
        if self.layout.shown {
            seen.extend(full);
            if self.call_shown(place) {
                seen.extend(pinned);
                seen.extend(shared.clone());
            } else if self.layout.away == Away::Shown {
                seen.extend(pinned);
            }
        }
        // A camera is on its person's card, which only the call's view shows.
        let cameras = cards.iter().filter_map(|card| match card {
            crate::Card::Person(tile) if tile.camera.pictured() && !tile.own => tile.user_id.as_ref(),
            crate::Card::Person(_) | crate::Card::OwnScreen(_) | crate::Card::Screen { .. } => None,
        });
        let cameras_seen = self.layout.shown && self.call_shown(place);
        let mut unseen: Vec<(MemberId, Video)> = shared
            .chain(pinned)
            .chain(full)
            .filter(|user_id| !seen.contains(user_id))
            .map(|user_id| (user_id.clone(), Video::Screen))
            .chain(
                cameras
                    .filter(|user_id| !cameras_seen && out_camera != Some(*user_id))
                    .map(|user_id| (user_id.clone(), Video::Camera)),
            )
            .collect();
        unseen.sort();
        unseen.dedup();
        Some((place.id(), unseen))
    }

    /// The call is told which shared screens are shown nowhere, once that changed.
    pub(super) fn tell_unseen(&mut self) -> Option<Ask> {
        let (place, unseen) = self.unseen()?;
        if self.layout.told_unseen == unseen {
            return None;
        }
        self.layout.told_unseen.clone_from(&unseen);
        Some(Ask::At(place, place::Ask::Call(place::Control::Unseen(unseen))))
    }

    /// Tells the live call whether the window shows, whenever that changed for it.
    pub(super) fn tell_levels(&mut self) -> Option<Ask> {
        let shown = self.layout.shown;
        let place = self.places.iter_mut().find(|place| place.calling())?;
        place.levels_news(shown).then(|| Ask::Levels { place: place.id(), shown })
    }

    /// Whether the content shows the call of `place` now.
    fn call_shown(&self, place: &Place) -> bool {
        self.desk.open == Some(place.id()) && place.panel() == Panel::Call && self.hardware.device.is_none()
    }

    /// The floating card shows again once the call's view has been shown, or there is no call.
    pub(super) fn float_back(&mut self) {
        if self.places.iter().find(|place| place.calling()).is_none_or(|place| self.call_shown(place)) {
            self.layout.away = Away::Shown;
        }
    }

    /// The floating card let go at (`x`, `y`), held whole inside the window.
    pub(super) fn float_to(&mut self, x: f32, y: f32) -> Vec<Ask> {
        if let Some(room) = self.layout.room {
            self.layout.float_at = FloatAt::dropped((x, y), room);
        }
        Vec::new()
    }

    /// The floating card's Go to call: the call's server opens with nothing over it, and the call in place of a
    /// text channel open there.
    pub(super) fn go_to_call(&mut self, at: Moment) -> Vec<Ask> {
        let Some(place) = self.places.iter_mut().find(|place| place.calling()) else { return Vec::new() };
        let id = place.id();
        let channel = place.call().map(|call| call.channel.id.clone());
        let mut todo: Vec<Ask> = place.close_pages().into_iter().map(|asked| Ask::At(id, asked)).collect();
        self.hardware.device = None;
        todo.extend(self.apply(Message::Open(id), at));
        if let Some(channel) = channel {
            let shown = place::Message::Lobby(place::LobbyMessage::Join(channel));
            todo.extend(self.apply(Message::At(id, shown), at));
        }
        todo
    }
}

/// A watched screen filling the whole display.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FullView<'a> {
    /// The place whose call it is in.
    pub place: PlaceId,
    /// The screen, and until when its controls show.
    pub screen: FullScreen<'a>,
    /// Whether its controls and the pointer show now.
    pub controls: bool,
    /// Its zoom, while it is on.
    pub zoom: Option<crate::Lens>,
}

/// A watched screen or camera in a window of its own.
#[derive(Clone, Copy, Debug)]
pub struct PoppedOut<'a> {
    /// The place whose call it is in, for its volume and what its controls send.
    pub place: &'a Place,
    /// The picture, and a screen's sound.
    pub screen: place::Out<'a>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call::room::{Event as RoomEvent, Track};
    use crate::harness::{
        Clock, call_says, id, in_call, in_call_offering_a_camera, member, picture_of, sharer_in_call, tell, the_call,
        two_signed_in, watching_bo,
    };
    use proto::rules::Grants;

    #[test]
    fn a_call_reads_its_levels_only_while_the_window_shows_and_is_told_each_change_once() {
        let clock = Clock(Instant::now());
        let (mut window, place) = crate::harness::ada_in_call(&clock);
        let told = |todo: Vec<Ask>| {
            todo.iter().find_map(|done| match done {
                Ask::Levels { place: at, shown } if *at == place => Some(*shown),
                _ => None,
            })
        };
        assert_eq!(told(window.update(Message::Tick, clock.at())), None, "a call starts reading them, shown");
        assert_eq!(told(window.update(Message::Shown(false), clock.at())), Some(false), "hidden: read none");
        assert_eq!(told(window.update(Message::Tick, clock.at())), None, "told once, not again");
        assert_eq!(told(window.update(Message::Shown(true), clock.at())), Some(true), "shown again");
        assert_eq!(told(window.update(Message::Shown(true), clock.at())), None);
    }

    #[test]
    fn another_s_camera_is_unseen_while_the_call_is_not_shown_and_one_s_own_never_is() {
        let clock = Clock(Instant::now());
        let (mut window, place) = in_call_offering_a_camera(&clock, Grants::from_words(&["SHARE_CAMERA"]));
        for user_id in ["bo", "ada"] {
            let started = RoomEvent::Started { user_id: member(user_id), track: Track::Camera };
            tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Room(started)));
        }
        let unseen = |window: &Window| window.unseen().map(|(_, unseen)| unseen);
        assert_eq!(unseen(&window), Some(Vec::new()), "on the call's view");
        window.update(Message::Shown(false), clock.at());
        assert_eq!(unseen(&window), Some(vec![(member("bo"), Video::Camera)]), "a hidden window shows none");
    }

    fn full_of(window: &Window) -> Option<(String, bool)> {
        window.full_screen().map(|full| (full.screen.user_id.as_str().to_owned(), full.controls))
    }

    #[test]
    fn a_watched_screen_goes_full_screen_from_its_card_and_leaves_by_any_way() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        let call = |window: &mut Window, message| tell(window, &clock, place, place::Message::Call(message));
        call(&mut window, place::CallMessage::FullScreen(member("bo")));
        assert_eq!(full_of(&window), None, "a screen before its first picture has nothing to fill the display with");
        call(&mut window, place::CallMessage::FullScreen(member("cy")));
        assert_eq!(full_of(&window), None, "nor one not shared");

        let (mut window, place) = watching_bo(&clock);
        let call = |window: &mut Window, message| tell(window, &clock, place, place::Message::Call(message));
        call(&mut window, place::CallMessage::ToggleInfo(member("bo")));
        let todo = call(&mut window, place::CallMessage::FullScreen(member("bo")));
        assert!(matches!(&todo[..], [Ask::WakeAt(_)]), "the window's own, but for its calm: {todo:?}");
        assert_eq!(full_of(&window), Some(("bo".into(), true)));
        let full = window.full_screen().expect("full screen");
        assert_eq!((full.place, full.screen.name), (place, "Bo"));
        assert_eq!(the_call(&window).open(), None, "what was open over the call closes");
        call(&mut window, place::CallMessage::LeaveFullScreen);
        assert_eq!(full_of(&window), None);
    }

    #[test]
    fn full_screen_ends_with_its_share_or_its_call() {
        let clock = Clock(Instant::now());
        let (mut window, place) = watching_bo(&clock);
        let call = |window: &mut Window, message| tell(window, &clock, place, place::Message::Call(message));
        call(&mut window, place::CallMessage::FullScreen(member("bo")));
        let stopped = RoomEvent::Stopped { user_id: member("bo"), track: Track::ScreenVideo };
        call(&mut window, place::CallMessage::Room(stopped));
        assert_eq!(full_of(&window), None, "the share ended");

        let (mut window, place) = watching_bo(&clock);
        let call = |window: &mut Window, message| tell(window, &clock, place, place::Message::Call(message));
        call(&mut window, place::CallMessage::FullScreen(member("bo")));
        call(
            &mut window,
            place::CallMessage::Watched(crate::Watched::Failed {
                user_id: member("bo"),
                video: Video::Screen,
                failed: words::Failure::CannotDecode,
            }),
        );
        assert_eq!(full_of(&window), None, "this device can no longer show it");

        let (mut window, place) = watching_bo(&clock);
        let call = |window: &mut Window, message| tell(window, &clock, place, place::Message::Call(message));
        call(&mut window, place::CallMessage::FullScreen(member("bo")));
        call(&mut window, place::CallMessage::CallOver(None));
        assert_eq!(full_of(&window), None, "the call ended");
    }

    #[test]
    fn a_still_pointer_hides_the_full_screen_controls_after_three_seconds() {
        let mut clock = Clock(Instant::now());
        let (mut window, place) = watching_bo(&clock);
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::FullScreen(member("bo"))));
        clock.pass(design::FULL_SCREEN_CALM - Duration::from_millis(1));
        window.update(Message::Tick, clock.at());
        assert_eq!(full_of(&window), Some(("bo".into(), true)), "until three seconds have gone");
        clock.pass(Duration::from_millis(1));
        window.update(Message::Tick, clock.at());
        assert_eq!(full_of(&window), Some(("bo".into(), false)), "the controls and the pointer go");
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Stirred));
        assert_eq!(full_of(&window), Some(("bo".into(), true)), "a move brings them back");
    }

    #[test]
    fn each_stir_moves_the_one_wake_to_when_the_controls_could_go() {
        let mut clock = Clock(Instant::now());
        let (mut window, place) = watching_bo(&clock);
        let call =
            |window: &mut Window, clock: &Clock, message| tell(window, clock, place, place::Message::Call(message));
        let first = call(&mut window, &clock, place::CallMessage::FullScreen(member("bo")));
        let calm = clock.0 + design::FULL_SCREEN_CALM;
        assert!(matches!(&first[..], [Ask::WakeAt(when)] if *when == calm), "{first:?}");
        for _ in 0..3 {
            clock.pass(Duration::from_millis(100));
            let todo = call(&mut window, &clock, place::CallMessage::Stirred);
            let later = clock.0 + design::FULL_SCREEN_CALM;
            assert!(matches!(&todo[..], [Ask::WakeAt(when)] if *when == later), "the wake, replaced: {todo:?}");
        }
        let todo = window.update(Message::Tick, clock.at());
        assert!(todo.is_empty(), "the wake asked stands: {todo:?}");
    }

    #[test]
    fn a_full_screen_does_not_outlive_a_switch_of_server() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let (owls, books) = (id(&window, 0), id(&window, 1));
        window.update(Message::Open(owls), clock.at());
        in_call(&mut window, &clock, owls);
        let call = |window: &mut Window, message| tell(window, &clock, owls, place::Message::Call(message));
        let joined = RoomEvent::MemberJoined { user_id: member("bo"), name: "Bo".into(), colour: None };
        call(&mut window, place::CallMessage::Room(joined));
        call(
            &mut window,
            place::CallMessage::Room(RoomEvent::Started { user_id: member("bo"), track: Track::ScreenVideo }),
        );
        call(&mut window, place::CallMessage::Watched(crate::Watched::Card(picture_of("bo"))));
        call(&mut window, place::CallMessage::FullScreen(member("bo")));
        assert_eq!(full_of(&window).map(|full| full.0), Some("bo".into()));
        window.update(Message::Open(books), clock.at());
        assert_eq!(full_of(&window), None, "another server shows its own place");
        window.update(Message::Open(owls), clock.at());
        assert_eq!(full_of(&window), None, "and coming back, the window is as it was");
    }

    fn out_of(window: &Window) -> Option<String> {
        window.pop_out().map(|out| out.screen.user_id.as_str().to_owned())
    }

    /// Cy starts sharing beside Bo, and their screen plays.
    fn cy_shares_too(window: &mut Window, clock: &Clock, place: PlaceId) {
        let started = RoomEvent::Started { user_id: member("cy"), track: Track::ScreenVideo };
        call_says(window, clock, place, place::CallMessage::Room(started));
        call_says(window, clock, place, place::CallMessage::Watched(crate::Watched::Card(picture_of("cy"))));
    }

    #[test]
    fn picture_in_picture_takes_a_playing_screen_to_its_own_window_one_at_a_time() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        call_says(&mut window, &clock, place, place::CallMessage::PopOut(member("bo"), Video::Screen));
        assert_eq!(out_of(&window), None, "a screen before its first picture has nothing to show there");
        call_says(&mut window, &clock, place, place::CallMessage::PopOut(member("cy"), Video::Screen));
        assert_eq!(out_of(&window), None, "nor one not shared");

        let (mut window, place) = watching_bo(&clock);
        let todo = call_says(&mut window, &clock, place, place::CallMessage::PopOut(member("bo"), Video::Screen));
        assert!(todo.is_empty(), "the window's own: {todo:?}");
        let out = window.pop_out().expect("Bo's screen in its own window");
        assert_eq!((out.place.id(), out.screen.name, out.screen.sound), (place, "Bo", None));
        assert!(the_call(&window).is_out(&member("bo"), Video::Screen), "its card says so");
        cy_shares_too(&mut window, &clock, place);
        call_says(&mut window, &clock, place, place::CallMessage::PopOut(member("cy"), Video::Screen));
        assert_eq!(out_of(&window), Some("cy".into()), "one at a time: the window moves to Cy's");
        assert!(!the_call(&window).is_out(&member("bo"), Video::Screen), "and Bo's plays in its card again");
        call_says(&mut window, &clock, place, place::CallMessage::PopIn);
        assert_eq!(out_of(&window), None);
    }

    #[test]
    fn a_screen_in_its_own_window_comes_back_with_its_share_or_its_call() {
        let clock = Clock(Instant::now());
        // Each ends it for good: a share that plays again later stays in its card.
        let out = |message: place::CallMessage| {
            let (mut window, place) = watching_bo(&clock);
            call_says(&mut window, &clock, place, place::CallMessage::PopOut(member("bo"), Video::Screen));
            assert_eq!(out_of(&window), Some("bo".into()));
            call_says(&mut window, &clock, place, message);
            let now = out_of(&window);
            let joined = RoomEvent::MemberJoined { user_id: member("bo"), name: "Bo".into(), colour: None };
            let started = RoomEvent::Started { user_id: member("bo"), track: Track::ScreenVideo };
            for again in [place::CallMessage::Room(joined), place::CallMessage::Room(started)] {
                call_says(&mut window, &clock, place, again);
            }
            call_says(&mut window, &clock, place, place::CallMessage::Watched(crate::Watched::Card(picture_of("bo"))));
            (now, out_of(&window))
        };
        let stopped = RoomEvent::Stopped { user_id: member("bo"), track: Track::ScreenVideo };
        assert_eq!(out(place::CallMessage::Room(stopped)), (None, None), "the share ended");
        let failed = crate::Watched::Failed {
            user_id: member("bo"),
            video: Video::Screen,
            failed: words::Failure::CannotDecode,
        };
        assert_eq!(out(place::CallMessage::Watched(failed)), (None, None), "this device could no longer show it");
        let gone = RoomEvent::MemberLeft { user_id: member("bo") };
        assert_eq!(out(place::CallMessage::Room(gone)), (None, None), "its sharer left");
        assert_eq!(out(place::CallMessage::CallOver(None)), (None, None), "the call ended");
    }

    #[test]
    fn a_screen_in_its_own_window_is_never_also_large_full_screen_or_floating() {
        use crate::Pinned;
        let clock = Clock(Instant::now());
        let (mut window, owls, books) = pinned_bo(&clock);
        let call = |window: &mut Window, message| tell(window, &clock, owls, place::Message::Call(message));
        call(&mut window, place::CallMessage::FullScreen(member("bo")));
        call(&mut window, place::CallMessage::PopOut(member("bo"), Video::Screen));
        assert_eq!(full_of(&window), None, "it leaves full screen for its own window");
        assert!(!the_call(&window).zoomable(&member("bo")), "nor is it shown large on the stage");
        call(&mut window, place::CallMessage::ToggleZoom(member("bo")));
        assert_eq!(the_call(&window).zoom(&member("bo")), None);
        call(&mut window, place::CallMessage::FullScreen(member("bo")));
        assert_eq!(full_of(&window), None, "its card offers no full screen, and a double-click there does nothing");
        window.update(Message::Open(books), clock.at());
        assert_eq!(floating_at(&window), None, "its own window shows it, not the floating card");
        assert_eq!(out_of(&window), Some("bo".into()), "which another server open keeps");
        tell(&mut window, &clock, owls, place::Message::Call(place::CallMessage::PopIn));
        assert_eq!(floating_at(&window).map(|at| at.0), Some("bo".into()), "back, it floats as a pinned screen does");
        let owls_call = window.places().iter().find(|place| place.id() == owls).and_then(Place::call).unwrap();
        assert_eq!(owls_call.pinned(), Some(&Pinned::Screen(member("bo"))), "its pin kept throughout");
    }

    #[test]
    fn a_screen_in_its_own_window_opens_its_volume_there_not_over_the_call() {
        let clock = Clock(Instant::now());
        let (mut window, place) = watching_bo(&clock);
        let started = RoomEvent::Started { user_id: member("bo"), track: Track::ScreenAudio };
        call_says(&mut window, &clock, place, place::CallMessage::Room(started));
        call_says(&mut window, &clock, place, place::CallMessage::PopOut(member("bo"), Video::Screen));
        let sound = |window: &Window| window.pop_out().map(|out| out.screen.sound.map(|sound| sound.open));
        assert_eq!(sound(&window), Some(Some(false)), "its window offers Volume");
        call_says(&mut window, &clock, place, place::CallMessage::ShareSound(member("bo")));
        assert_eq!(sound(&window), Some(Some(true)), "and opens it there");
        assert_eq!(the_call(&window).share_sound(), None, "not over the call");
        call_says(&mut window, &clock, place, place::CallMessage::ShareSound(member("bo")));
        assert_eq!(sound(&window), Some(Some(false)), "pressed again, it closes");
        call_says(&mut window, &clock, place, place::CallMessage::ShareSound(member("bo")));
        call_says(&mut window, &clock, place, place::CallMessage::PopIn);
        assert_eq!(
            the_call(&window).share_sound(),
            Some(&member("bo")),
            "back in its card, an open one is over the call"
        );
    }

    /// Night Owls's call with Bo's screen playing and pinned, in a window of 1280 × 800, both servers signed in.
    fn pinned_bo(clock: &Clock) -> (Window, PlaceId, PlaceId) {
        let mut window = two_signed_in(clock);
        let (owls, books) = (id(&window, 0), id(&window, 1));
        window.set_room(Room { width: 1280.0, height: 800.0 });
        window.update(Message::Open(owls), clock.at());
        in_call(&mut window, clock, owls);
        let call = |window: &mut Window, message| tell(window, clock, owls, place::Message::Call(message));
        let joined = RoomEvent::MemberJoined { user_id: member("bo"), name: "Bo".into(), colour: None };
        call(&mut window, place::CallMessage::Room(joined));
        call(
            &mut window,
            place::CallMessage::Room(RoomEvent::Started { user_id: member("bo"), track: Track::ScreenVideo }),
        );
        call(&mut window, place::CallMessage::Watched(crate::Watched::Card(picture_of("bo"))));
        call(&mut window, place::CallMessage::Pin(crate::Pinned::Screen(member("bo"))));
        (window, owls, books)
    }

    const LARGE: Room = Room { width: 1280.0, height: 800.0 };

    fn floating_at(window: &Window) -> Option<(String, (f32, f32))> {
        window.floating(LARGE).map(|card| {
            let whose = match card.screen {
                place::PinnedShare::Someone(screen) => screen.user_id.as_str().to_owned(),
                place::PinnedShare::Own => "own".to_owned(),
            };
            (whose, card.at)
        })
    }

    fn appearance(window: &mut Window, clock: &Clock) {
        window.update(Message::OpenDevice(DevicePage::Appearance), clock.at());
    }

    /// The card's home in a window of [`LARGE`]: by the bottom right corner.
    const HOME: (f32, f32) = (1280.0 - 320.0 - 20.0, 800.0 - 180.0 - 20.0);

    /// The card's home with a server open: still by the window's bottom right corner, over its members column.
    const HOME_BESIDE: (f32, f32) = HOME;

    #[test]
    fn a_pinned_screen_floats_while_its_call_is_out_of_view_and_never_beside_it() {
        let clock = Clock(Instant::now());
        let (mut window, owls, books) = pinned_bo(&clock);
        assert_eq!(floating_at(&window), None, "the call's view shows the screen itself");
        appearance(&mut window, &clock);
        assert_eq!(floating_at(&window), Some(("bo".into(), HOME)), "a page over the call");
        let card = window.floating(LARGE).expect("the card");
        let place::PinnedShare::Someone(screen) = card.screen else { panic!("someone's screen: {card:?}") };
        assert_eq!((card.place, screen.name), (owls, "Bo"));
        window.update(Message::CloseDevice, clock.at());
        assert_eq!(floating_at(&window), None, "the call's view again");
        window.update(Message::Open(books), clock.at());
        assert_eq!(floating_at(&window), Some(("bo".into(), HOME_BESIDE)), "another server open, over its members");
        window.update(Message::Open(owls), clock.at());
        assert_eq!(floating_at(&window), None);
    }

    /// The screens the call was told the window shows nowhere, the last time it was told.
    fn told_unseen(todo: &[Ask]) -> Option<Vec<(MemberId, Video)>> {
        todo.iter().rev().find_map(|done| match done {
            Ask::At(_, place::Ask::Call(place::Control::Unseen(unseen))) => Some(unseen.clone()),
            _ => None,
        })
    }

    #[test]
    fn a_screen_is_seen_on_the_calls_view_or_floating_and_never_while_the_window_is_hidden() {
        let clock = Clock(Instant::now());
        let (mut window, owls, books) = pinned_bo(&clock);
        let unseen = |window: &Window| window.unseen().map(|(_, unseen)| unseen);
        let bo = || vec![(member("bo"), Video::Screen)];
        assert_eq!(unseen(&window), Some(Vec::new()), "pinned on the call's view");
        let todo = window.update(Message::Shown(false), clock.at());
        assert_eq!(told_unseen(&todo), Some(bo()), "a window minimised or in the tray shows none");
        assert_eq!(told_unseen(&window.update(Message::Tick, clock.at())), None, "told once, not again");
        let todo = window.update(Message::Shown(true), clock.at());
        assert_eq!(told_unseen(&todo), Some(Vec::new()), "shown again");
        window.update(Message::Open(books), clock.at());
        assert_eq!(unseen(&window), Some(Vec::new()), "the floating card shows it");
        let call = |window: &mut Window, message| tell(window, &clock, owls, place::Message::Call(message));
        call(&mut window, place::CallMessage::Unpin);
        assert_eq!(unseen(&window), Some(bo()), "nothing floats and the call's view is out of sight");
        window.update(Message::Open(owls), clock.at());
        assert_eq!(unseen(&window), Some(Vec::new()), "its card on the call's view");
    }

    #[test]
    fn a_pinned_share_floats_mine_too_but_never_a_person() {
        let clock = Clock(Instant::now());
        let (mut window, owls, books) = pinned_bo(&clock);
        let call = |window: &mut Window, message| tell(window, &clock, owls, place::Message::Call(message));
        call(&mut window, place::CallMessage::Unpin);
        window.update(Message::Open(books), clock.at());
        assert_eq!(floating_at(&window), None, "nothing pinned");
        window.update(Message::Open(owls), clock.at());
        call(&mut window, place::CallMessage::Pin(crate::Pinned::Person(member("bo"))));
        window.update(Message::Open(books), clock.at());
        assert_eq!(floating_at(&window), None, "a person pinned");

        window.update(Message::Open(owls), clock.at());
        call(&mut window, place::CallMessage::Pin(crate::Pinned::OwnScreen));
        window.update(Message::Open(books), clock.at());
        assert_eq!(floating_at(&window), None, "one's own screen pinned, its preview not yet shown");
        window.update(Message::Open(owls), clock.at());
        call(&mut window, place::CallMessage::Starting);
        let started = crate::Shared::Started { width: 1920, height: 1080, encoder: crate::Route::Software };
        call(&mut window, place::CallMessage::Shared(started));
        call(&mut window, place::CallMessage::Pin(crate::Pinned::OwnScreen));
        window.update(Message::Open(books), clock.at());
        assert_eq!(floating_at(&window), Some(("own".into(), HOME)), "one's own screen, its preview shown");
        let unseen = window.unseen().map(|(_, unseen)| unseen);
        assert_eq!(unseen, Some(vec![(member("bo"), Video::Screen)]), "Bo's screen, shown nowhere; never one's own");
    }

    #[test]
    fn the_card_starts_at_the_window_s_bottom_right_over_the_members() {
        let clock = Clock(Instant::now());
        let (mut window, _, books) = pinned_bo(&clock);
        window.update(Message::Open(books), clock.at());
        let members = design::MEMBERS_WIDTH + design::BORDER_WIDTH;
        let (_, at) = floating_at(&window).expect("the card");
        assert_eq!(at, (LARGE.width - 320.0 - 20.0, LARGE.height - 180.0 - 20.0));
        assert!(at.0 + 320.0 > LARGE.width - members, "it lies over the docked members column");
        window.update(Message::Float(FloatMessage::To(0.0, 0.0)), clock.at());
        assert_eq!(floating_at(&window).map(|card| card.1), Some((0.0, 0.0)), "and is dragged anywhere in the window");
    }

    #[test]
    fn the_floating_card_goes_with_its_pin_its_share_or_its_call() {
        let clock = Clock(Instant::now());
        let away = |window: &mut Window, books| {
            window.update(Message::Open(books), clock.at());
            assert!(floating_at(window).is_some());
        };
        let call = |window: &mut Window, owls, message| tell(window, &clock, owls, place::Message::Call(message));

        let (mut window, owls, books) = pinned_bo(&clock);
        away(&mut window, books);
        call(&mut window, owls, place::CallMessage::Unpin);
        assert_eq!(floating_at(&window), None, "unpinned");

        let (mut window, owls, books) = pinned_bo(&clock);
        away(&mut window, books);
        let stopped = RoomEvent::Stopped { user_id: member("bo"), track: Track::ScreenVideo };
        call(&mut window, owls, place::CallMessage::Room(stopped));
        assert_eq!(floating_at(&window), None, "the share ended");

        let (mut window, owls, books) = pinned_bo(&clock);
        away(&mut window, books);
        call(&mut window, owls, place::CallMessage::CallOver(None));
        assert_eq!(floating_at(&window), None, "the call ended");
    }

    #[test]
    fn close_hides_the_card_until_the_calls_view_is_shown_again() {
        let clock = Clock(Instant::now());
        let (mut window, owls, books) = pinned_bo(&clock);
        appearance(&mut window, &clock);
        let todo = window.update(Message::Float(FloatMessage::Close), clock.at());
        assert_eq!(
            told_unseen(&todo),
            Some(vec![(member("bo"), Video::Screen)]),
            "the call is told the screen shows nowhere"
        );
        assert_eq!(todo.len(), 1, "and nothing else: the window's own: {todo:?}");
        assert_eq!(floating_at(&window), None);
        window.update(Message::Open(books), clock.at());
        assert_eq!(floating_at(&window), None, "still closed elsewhere");
        window.update(Message::Open(owls), clock.at());
        assert_eq!(floating_at(&window), None, "a page is still over the call");
        window.update(Message::CloseDevice, clock.at());
        window.update(Message::Open(books), clock.at());
        assert_eq!(floating_at(&window), Some(("bo".into(), HOME_BESIDE)), "the call was shown, so it floats again");
        assert_eq!(
            the_place_call(&window, owls).pinned(),
            Some(&crate::Pinned::Screen(member("bo"))),
            "never unpinned"
        );
    }

    fn the_place_call(window: &Window, place: PlaceId) -> &place::Call {
        window.place(place).and_then(Place::call).unwrap()
    }

    #[test]
    fn go_to_the_call_opens_its_server_and_closes_what_is_over_it() {
        let clock = Clock(Instant::now());
        let (mut window, owls, books) = pinned_bo(&clock);
        appearance(&mut window, &clock);
        window.update(Message::Open(books), clock.at());
        let todo = window.update(Message::Float(FloatMessage::GoToCall), clock.at());
        assert!(
            matches!(&todo[..], [Ask::RememberOpen(_), Ask::Probe(ProbeAsk::Close)]),
            "this device's page too: {todo:?}"
        );
        assert_eq!(window.open().map(Place::id), Some(owls));
        assert_eq!(window.open().map(Place::panel), Some(Panel::Call));
        assert_eq!(floating_at(&window), None);
    }

    #[test]
    fn a_text_channel_opened_beside_the_call_floats_the_pinned_screen_and_the_call_s_row_takes_it_back() {
        let clock = Clock(Instant::now());
        let (mut window, owls, _) = pinned_bo(&clock);
        let general = place::DiscussionMessage::Open(crate::harness::channel_id(crate::harness::GENERAL));
        tell(&mut window, &clock, owls, place::Message::Discussion(general));
        assert_eq!(window.open().map(Place::panel), Some(Panel::Discussion));
        assert_eq!(floating_at(&window).map(|(user, _)| user), Some("bo".into()));
        let lounge = place::LobbyMessage::Join(crate::harness::channel_id("lounge"));
        assert!(tell(&mut window, &clock, owls, place::Message::Lobby(lounge)).is_empty(), "the call goes on");
        assert_eq!(window.open().map(Place::panel), Some(Panel::Call));
        assert_eq!(floating_at(&window), None);
    }

    /// The card floats over a text channel of the call's own server too: its way back shows the call there.
    #[test]
    fn go_to_the_call_from_a_text_channel_of_its_own_server_shows_the_call() {
        let clock = Clock(Instant::now());
        let (mut window, owls, _) = pinned_bo(&clock);
        let general = place::DiscussionMessage::Open(crate::harness::channel_id(crate::harness::GENERAL));
        tell(&mut window, &clock, owls, place::Message::Discussion(general));
        assert_eq!(floating_at(&window).map(|(user, _)| user), Some("bo".into()));
        window.update(Message::Float(FloatMessage::GoToCall), clock.at());
        assert_eq!(window.open().map(Place::panel), Some(Panel::Call));
        assert_eq!(floating_at(&window), None);
    }

    #[test]
    fn a_dropped_card_stays_where_it_was_let_go_whole_inside_the_window() {
        let clock = Clock(Instant::now());
        let (mut window, owls, books) = pinned_bo(&clock);
        window.update(Message::Open(books), clock.at());
        window.update(Message::Float(FloatMessage::To(390.0, 40.0)), clock.at());
        assert_eq!(floating_at(&window), Some(("bo".into(), (390.0, 40.0))));
        window.update(Message::Float(FloatMessage::To(-500.0, -500.0)), clock.at());
        assert_eq!(floating_at(&window), Some(("bo".into(), (0.0, 0.0))), "held inside the window");
        window.update(Message::Float(FloatMessage::To(600.0, 200.0)), clock.at());

        window.update(Message::Open(owls), clock.at());
        appearance(&mut window, &clock);
        assert_eq!(
            floating_at(&window),
            Some(("bo".into(), (600.0, 200.0))),
            "the window is its room on a page and beside a server alike"
        );

        let small = Room { width: 500.0, height: 400.0 };
        window.set_room(small);
        let card = window.floating(small).expect("in a small window");
        assert!(card.at.0 + 320.0 <= small.width && card.at.1 + 180.0 <= small.height, "{:?}", card.at);
        assert!(card.at.0 >= 0.0 && card.at.1 >= 0.0, "{:?}", card.at);

        window.set_room(LARGE);
        assert_eq!(floating_at(&window).map(|card| card.1), Some((600.0, 200.0)), "back with the room");
        window.update(Message::Float(FloatMessage::Reset), clock.at());
        assert_eq!(floating_at(&window), Some(("bo".into(), HOME)));
    }

    #[test]
    fn a_camera_the_person_paused_is_still_on_its_card_and_unseen_with_the_call_out_of_view() {
        let clock = Clock(Instant::now());
        let (mut window, place) = in_call_offering_a_camera(&clock, Grants::from_words(&["SHARE_CAMERA"]));
        let started = RoomEvent::Started { user_id: member("bo"), track: Track::Camera };
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Room(started)));
        let pause = place::CallMessage::TogglePause(crate::Pinned::Person(member("bo")));
        tell(&mut window, &clock, place, place::Message::Call(pause));
        let tile = |window: &Window| {
            the_call(window)
                .tiles()
                .iter()
                .find(|tile| tile.user_id.as_ref() == Some(&member("bo")))
                .map(|tile| tile.camera)
        };
        assert_eq!(tile(&window), Some(crate::CameraSeen::Paused));
        assert!(crate::CameraSeen::Paused.pictured() && !crate::CameraSeen::Unshown.pictured());
        let unseen = |window: &Window| window.unseen().map(|(_, unseen)| unseen);
        assert_eq!(unseen(&window), Some(Vec::new()), "on the call's view");
        window.update(Message::Shown(false), clock.at());
        assert_eq!(unseen(&window), Some(vec![(member("bo"), Video::Camera)]), "paused, it is a card with a picture");
    }

    fn bo_camera_in_call(clock: &Clock) -> (Window, PlaceId) {
        let (mut window, place) = in_call_offering_a_camera(clock, Grants::from_words(&["SHARE_CAMERA"]));
        let started = RoomEvent::Started { user_id: member("bo"), track: Track::Camera };
        call_says(&mut window, clock, place, place::CallMessage::Room(started));
        (window, place)
    }

    fn out_video(window: &Window) -> Option<(String, Video)> {
        window.pop_out().map(|out| (out.screen.user_id.as_str().to_owned(), out.screen.video))
    }

    #[test]
    fn a_camera_pops_out_and_back_with_its_pause() {
        let clock = Clock(Instant::now());
        let (mut window, place) = bo_camera_in_call(&clock);
        let unseen = |window: &Window| window.unseen().map(|(_, unseen)| unseen);
        call_says(&mut window, &clock, place, place::CallMessage::PopOut(member("ada"), Video::Camera));
        assert_eq!(out_video(&window), None, "one's own camera has no window");
        let todo = call_says(&mut window, &clock, place, place::CallMessage::PopOut(member("bo"), Video::Camera));
        assert!(todo.is_empty(), "the window's own: {todo:?}");
        assert_eq!(out_video(&window), Some(("bo".into(), Video::Camera)));
        let out = window.pop_out().expect("Bo's camera");
        assert_eq!((out.screen.name, out.screen.card.is_none(), out.screen.sound), ("Bo", true, None));
        assert!(
            the_call(&window).is_out(&member("bo"), Video::Camera)
                && !the_call(&window).is_out(&member("bo"), Video::Screen)
        );

        let pause = place::CallMessage::TogglePause(crate::Pinned::Person(member("bo")));
        let todo = call_says(&mut window, &clock, place, pause);
        assert!(
            matches!(&todo[..], [Ask::At(_, place::Ask::Call(place::Control::Pause { video: Video::Camera, .. }))]),
            "{todo:?}"
        );
        assert_eq!(out_video(&window), Some(("bo".into(), Video::Camera)), "paused, it stays in its window");
        window.update(Message::Shown(false), clock.at());
        assert_eq!(unseen(&window), Some(Vec::new()), "its window shows it, hidden or not");
        window.update(Message::Shown(true), clock.at());

        call_says(&mut window, &clock, place, place::CallMessage::PopIn);
        assert_eq!(out_video(&window), None);
        assert!(the_call(&window).is_paused(&member("bo"), Video::Camera), "back in its card, still paused");
    }

    #[test]
    fn a_stopped_camera_closes_its_window() {
        let clock = Clock(Instant::now());
        let (mut window, place) = bo_camera_in_call(&clock);
        call_says(&mut window, &clock, place, place::CallMessage::PopOut(member("bo"), Video::Camera));
        assert_eq!(out_video(&window), Some(("bo".into(), Video::Camera)));
        let stopped = RoomEvent::Stopped { user_id: member("bo"), track: Track::Camera };
        call_says(&mut window, &clock, place, place::CallMessage::Room(stopped));
        assert_eq!(out_video(&window), None, "the camera went off");
        let started = RoomEvent::Started { user_id: member("bo"), track: Track::Camera };
        call_says(&mut window, &clock, place, place::CallMessage::Room(started));
        assert_eq!(out_video(&window), None, "and its next one stays in its card");
    }
}
