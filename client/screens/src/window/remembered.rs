//! What this device remembers for calls and for the window, as the host read it, and each choice kept again.

use super::*;

impl Window {
    /// The account bar's mute pressed outside a call: the next call starts as it says.
    pub(super) fn on_next_mute(&mut self) -> Vec<Ask> {
        self.press_next_call(self.voice.next_call.mute_pressed())
    }

    /// The account bar's deafen pressed outside a call: the next call starts as it says.
    pub(super) fn on_next_deafen(&mut self) -> Vec<Ask> {
        self.press_next_call(State { deafened: !self.voice.next_call.deafened, ..self.voice.next_call })
    }

    /// The window knowing what this device remembers for calls, as the host read it at the start, and how loud each
    /// server's members play, by the server's address: each goes to its place, their one owner from then on.
    pub fn remembering(mut self, voice: Remembered, mut volumes: BTreeMap<String, Volumes>) -> Window {
        for place in &mut self.places {
            if let Some(kept) = volumes.remove(place.server().url.as_str()) {
                place.set_volumes(kept);
            }
        }
        self.voice = voice;
        self.tell_alerting();
        self.tell_zone();
        self.tell_places();
        self.speak(self.chosen_language());
        self
    }

    /// The window knowing which categories this device keeps folded on each server, by the server's address, as the
    /// host read them at the start: each goes to its place, their one owner from then on.
    pub fn folding(mut self, mut folds: BTreeMap<String, std::collections::BTreeSet<terms::CategoryId>>) -> Window {
        for place in &mut self.places {
            if let Some(kept) = folds.remove(place.server().url.as_str()) {
                place.set_folded(kept);
            }
        }
        self
    }

    /// This device's encoders for a screen, in the order they are tried; none until the host has listed them.
    pub fn encoders(&self) -> &[EncoderOption] {
        self.hardware.encoders.as_deref().unwrap_or_default()
    }

    /// How a share starts here: the last share's choice, with a codec this device does not make taken as H.264.
    pub fn share_starts(&self) -> ShareQuality {
        match &self.hardware.encoders {
            Some(encoders) => self.voice.share.clone().on(encoders),
            None => self.voice.share.clone(),
        }
    }

    /// Every place told whether this device shares and how a share starts.
    pub(super) fn tell_places(&mut self) {
        let (encodes, starts) =
            (self.hardware.encoders.as_ref().is_none_or(|encoders| !encoders.is_empty()), self.share_starts());
        for place in &mut self.places {
            place.set_chooser(self.hardware.chooser);
            place.set_encodes(encodes);
            place.set_share_starts(starts.clone());
            place.set_call_view(self.voice.look.call);
            place.set_previews(self.voice.look.previews);
            // A place told before any call of its own has no camera to open again.
            let _ = place.set_camera(&self.voice.camera);
        }
    }

    /// How the next call starts, which the account bar shows outside a call.
    pub fn next_call(&self) -> State {
        self.voice.next_call
    }

    /// What this device remembers for calls.
    pub fn voice(&self) -> &Remembered {
        &self.voice
    }

    /// Whether the window's own sounds play on this device.
    pub fn event_sounds(&self) -> EventSounds {
        self.voice.event_sounds
    }

    /// The account bar's press outside a call, taking the next call to `next`, with its sound.
    pub(super) fn press_next_call(&mut self, next: State) -> Vec<Ask> {
        let chime = Chime::pressed(self.voice.next_call, next).and_then(|chime| self.chime(chime));
        self.voice.next_call = next;
        std::iter::once(Ask::NextCall(next)).chain(chime).collect()
    }

    /// `chime`'s sound, unless the window's own sounds are off or the account is not to be disturbed on any server.
    pub(super) fn chime(&self, chime: Chime) -> Option<Ask> {
        if self.quiet_anywhere() {
            return None;
        }
        chime.under(self.voice.event_sounds, self.voice.message_sounds).map(Ask::Chime)
    }

    /// `chime`'s sound as told at `now`: a message's is not asked again within [`MESSAGE_CHIME_GAP`] of the last
    /// one asked, so a burst sounds once.
    pub(super) fn chime_at(&mut self, chime: Chime, now: Instant) -> Option<Ask> {
        if chime != Chime::Message {
            return self.chime(chime);
        }
        let lately =
            self.alerts.message_chimed.is_some_and(|last| now.saturating_duration_since(last) < MESSAGE_CHIME_GAP);
        let ask = self.chime(chime).filter(|_| !lately)?;
        self.alerts.message_chimed = Some(now);
        Some(ask)
    }

    /// Whether a message that would alert plays a sound on this device.
    pub fn message_sounds(&self) -> MessageSounds {
        self.voice.message_sounds
    }

    /// How the window looks on this device.
    pub fn look(&self) -> Look {
        self.voice.look
    }

    /// This device's encoders, listed by the host, told to every place.
    pub(super) fn listed_encoders(&mut self, encoders: Vec<EncoderOption>) -> Vec<Ask> {
        self.hardware.encoders = Some(encoders);
        self.tell_places();
        Vec::new()
    }

    /// The picker a share is chosen in, told to every place.
    pub(super) fn chooser(&mut self, chooser: Chooser) -> Vec<Ask> {
        self.hardware.chooser = chooser;
        self.tell_places();
        Vec::new()
    }

    /// A choice on the Appearance page: told to every place and kept.
    pub(super) fn pick_look(&mut self, pick: LookPick) -> Vec<Ask> {
        let messages = self.voice.look.messages;
        self.voice.look.pick(pick);
        if self.voice.look.messages != messages {
            self.places.iter_mut().for_each(Place::messages_rescaled);
        }
        self.tell_places();
        vec![Ask::Keep(Keep::Look(self.voice.look))]
    }

    /// What the emoji picker remembers on this device: the emoji picked lately and the skin tone.
    pub fn emoji_picks(&self) -> &choices::EmojiPicks {
        &self.voice.emoji
    }

    /// The picker's skin tone changed, and kept.
    pub(super) fn emoji_tone(&mut self, tone: chat::emoji::SkinTone) -> Vec<Ask> {
        if self.voice.emoji.tone == tone {
            return Vec::new();
        }
        self.voice.emoji.tone = tone;
        vec![Ask::Keep(Keep::Emoji(self.voice.emoji.clone()))]
    }

    /// The window's own sounds switched, and kept.
    pub(super) fn toggle_event_sounds(&mut self) -> Vec<Ask> {
        self.voice.event_sounds = self.voice.event_sounds.toggled();
        vec![Ask::Keep(Keep::EventSounds(self.voice.event_sounds))]
    }

    /// The sound of a message that would alert switched, and kept.
    pub(super) fn toggle_message_sounds(&mut self) -> Vec<Ask> {
        self.voice.message_sounds = self.voice.message_sounds.toggled();
        vec![Ask::Keep(Keep::MessageSounds(self.voice.message_sounds))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call::room::{Event as RoomEvent, Track};
    use crate::harness::{
        ADA, Clock, accepted_as, ada_in_call, books, cards, channels, flow, id, in_call, member, owls, sharer_in_call,
        tell, the_call, token, two_signed_in,
    };
    use choices::{Audio, Volume};

    /// Outside a call the account bar sets how the next call starts: as the last run left it, a
    /// press on mute or deafen, and the state the last call ended in; each change is told to the host to keep.
    #[test]
    fn the_next_call_starts_as_the_account_bar_and_the_last_call_left_it() {
        let muted = State { mic_muted: true, deafened: false };
        let (window, _) = Window::launch(Vec::new(), None, Instant::now());
        let mut window = window.remembering(Remembered { next_call: muted, ..Remembered::default() }, BTreeMap::new());
        assert_eq!(window.next_call(), muted, "as the last run left it");
        let clock = Clock(Instant::now());
        let todo = window.update(Message::NextMute, clock.at());
        assert!(
            matches!(&todo[..], [Ask::NextCall(state), _] if *state == State::default()),
            "the host is told: {todo:?}"
        );
        window.update(Message::NextDeafen, clock.at());
        assert_eq!(window.next_call(), State { mic_muted: false, deafened: true });
        window.update(Message::NextMute, clock.at());
        assert_eq!(window.next_call(), State::default(), "mute while deafened undoes both, as in a call");
        let todo = window.update(Message::NextMute, clock.at());
        assert_eq!(window.next_call(), muted);
        assert!(
            matches!(&todo[..], [Ask::NextCall(state), _] if *state == muted),
            "the host is told to keep it: {todo:?}"
        );

        let (mut window, place) = ada_in_call(&clock);
        let deafened = State { mic_muted: false, deafened: true };
        tell(
            &mut window,
            &clock,
            place,
            place::Message::Call(place::CallMessage::Room(RoomEvent::State {
                user_id: member("ada"),
                state: deafened,
            })),
        );
        let todo = tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::CallOver(None)));
        assert_eq!(window.next_call(), deafened, "the call ended deafened, so the next one starts so");
        assert!(todo.iter().any(|done| matches!(done, Ask::NextCall(state) if *state == deafened)), "{todo:?}");
    }

    #[test]
    fn each_server_is_handed_its_own_kept_folds_at_the_start() {
        let clock = Clock(Instant::now());
        let window = two_signed_in(&clock);
        let projects: std::collections::BTreeSet<terms::CategoryId> =
            terms::CategoryId::parse("projects").into_iter().collect();
        let folds = BTreeMap::from([(owls().url.as_str().to_owned(), projects.clone())]);
        let window = window.folding(folds);
        assert_eq!(window.places()[0].folded(), &projects);
        assert!(window.places()[1].folded().is_empty(), "and no other: a category's id is the server's own");
    }

    #[test]
    fn each_server_is_handed_its_own_kept_volumes_at_the_start() {
        let clock = Clock(Instant::now());
        let (window, _) = Window::launch(
            vec![Opening::Account(owls(), ADA.into()), Opening::Account(books(), ADA.into())],
            None,
            clock.0,
        );
        let quiet = Volume::new(10).unwrap();
        let kept: Volumes =
            [(member("bo"), choices::Loudness { voice: quiet, share: Volume::FULL })].into_iter().collect();
        let window =
            window.remembering(Remembered::default(), BTreeMap::from([(owls().url.as_str().to_string(), kept)]));
        assert_eq!(window.places()[0].volumes().of(&member("bo"), Audio::Voice), quiet, "the server it was kept at");
        assert_eq!(window.places()[1].volumes(), &Volumes::default(), "and no other: an id is the server's own");
    }

    #[test]
    fn a_choice_on_the_appearance_page_is_the_windows_look_and_is_kept() {
        use choices::ZoomPick;
        use design::{Contrast, Preference};
        let clock = Clock(Instant::now());
        let (window, _) = Window::launch(Vec::new(), None, clock.0);
        let mut window = window.remembering(Remembered::default(), BTreeMap::new());
        let todo = window.update(Message::Look(LookPick::Theme(Preference::Dark)), clock.at());
        let dark = Look { theme: Preference::Dark, ..Look::default() };
        assert!(matches!(&todo[..], [Ask::Keep(Keep::Look(kept))] if *kept == dark), "{todo:?}");
        window.update(Message::Look(LookPick::MoreContrast), clock.at());
        let todo = window.update(Message::Look(LookPick::Zoom(ZoomPick::In)), clock.at());
        let zoom = design::Zoom::of_percent(110).unwrap();
        assert!(matches!(&todo[..], [Ask::Keep(Keep::Look(kept))] if kept.zoom == zoom), "{todo:?}");
        assert_eq!(window.look(), Look { theme: Preference::Dark, contrast: Contrast::More, zoom, ..Look::default() });
        let size = design::MessageSize::of_px(24).unwrap();
        let todo = window.update(Message::Look(LookPick::MessageSize(size)), clock.at());
        assert!(matches!(&todo[..], [Ask::Keep(Keep::Look(kept))] if kept.messages.size == size), "{todo:?}");
    }

    /// A message size or spacing picked while a channel is open: the rows built say their new heights, the rows not
    /// built are guessed from those, never their old height, and the row in view stays the one in view.
    #[test]
    fn a_message_scale_picked_re_measures_the_rows_and_keeps_the_one_in_view() {
        use crate::harness::{chatting, discussion, first_page, said};
        use crate::place::DiscussionMessage;
        let clock = Clock(Instant::now());
        let open = |window: &Window| window.places()[0].discussion().cloned().unwrap();
        for pick in [
            LookPick::MessageSize(design::MessageSize::of_px(24).unwrap()),
            LookPick::RunGap(design::RunGap::of_px(0).unwrap()),
        ] {
            let (mut window, owls) = chatting(&clock);
            let page = chat::Page {
                messages: (10..60).map(|n| said(n, n, ADA, "line")).collect(),
                older: false,
                newer: false,
                revision: 60,
            };
            tell(&mut window, &clock, owls, first_page(page));
            let rows: Vec<chat::RowKey> = open(&window).rows().iter().map(chat::Row::key).collect();
            for row in &rows {
                tell(
                    &mut window,
                    &clock,
                    owls,
                    discussion(DiscussionMessage::Measured { row: row.clone(), height: 40.0 }),
                );
            }
            tell(
                &mut window,
                &clock,
                owls,
                discussion(DiscussionMessage::Scrolled { from_end: 1_000.0, viewport: 300.0 }),
            );
            let before = open(&window);
            let chat::Scroll::At { row: anchor, offset } = before.scroll().clone() else { panic!("scrolled up") };
            let built = before.window();
            assert!(built.first > 0 && built.end < rows.len(), "rows not built either side: {built:?}");

            window.update(Message::Look(pick), clock.at());
            assert_eq!(open(&window).scroll(), before.scroll(), "{pick:?}: nothing measured yet, nothing moves");
            for row in &rows[built.first..built.end] {
                tell(
                    &mut window,
                    &clock,
                    owls,
                    discussion(DiscussionMessage::Measured { row: row.clone(), height: 60.0 }),
                );
            }
            let after = open(&window);
            assert_eq!(after.scroll(), before.scroll(), "{pick:?}: the row in view stays the one in view");
            let below = rows.len() - 1 - rows.iter().position(|row| *row == anchor).unwrap();
            let grown = 60.0 - offset + 60.0 * below as f32;
            assert_eq!(after.scrolled_up(), grown, "{pick:?}: every row below it is now 60 tall, measured or guessed");
        }
    }

    #[test]
    fn a_share_keeps_the_panels_choice_as_the_next_ones_start() {
        use choices::{Ceiling, FrameRate, QualityPick, ShareQuality};
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        let pick = |window: &mut Window, pick| {
            tell(window, &clock, place, place::Message::Call(place::CallMessage::Quality(pick)));
        };
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleShare));
        assert_eq!(the_call(&window).quality(), &ShareQuality::default(), "the first share opens on the defaults");
        pick(&mut window, QualityPick::FrameRate(FrameRate::Thirty));
        pick(&mut window, QualityPick::Ceiling(Ceiling::Mbit8));
        let chosen = ShareQuality { frame_rate: FrameRate::Thirty, ceiling: Ceiling::Mbit8, ..Default::default() };
        assert_eq!(window.voice().share, ShareQuality::default(), "a pick alone keeps nothing");
        let todo = tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ChooseScreen));
        assert!(
            matches!(&todo[..], [Ask::At(_, place::Ask::ChooseScreen(asked)), Ask::Keep(Keep::Share(kept))]
                if *asked == chosen && *kept == chosen),
            "{todo:?}"
        );
        assert_eq!(window.voice().share, chosen, "the share's choice is kept");

        pick(&mut window, QualityPick::FrameRate(FrameRate::Fifteen));
        assert_eq!(the_call(&window).quality(), &chosen, "a pick with the panel shut changes nothing");
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleShare));
        assert_eq!(the_call(&window).quality(), &chosen, "the next share opens on the last one's choice");
    }

    #[test]
    fn a_custom_rate_out_of_bounds_is_refused_in_one_sentence_and_nothing_is_asked() {
        use choices::{Ceiling, Kbits, PresetOrCustom, QualityPick};
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        let rate = |window: &mut Window, message| {
            tell(window, &clock, place, place::Message::Call(place::CallMessage::Rate(message)))
        };
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleShare));
        rate(&mut window, place::RateMessage::Custom);
        assert_eq!(the_call(&window).rate(), &PresetOrCustom::Custom("12".into()), "Automatic starts it at 12");
        rate(&mut window, place::RateMessage::Typed("45".into()));
        let said = the_call(&window).rate_note().map(|note| words::Words::english().said(&note));
        assert_eq!(said.as_deref(), Some("Pick a rate from 0.5 to 40 Mbit/s, in steps of 0.5."));
        let todo = tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ChooseScreen));
        assert!(todo.is_empty(), "nothing asked with a rate the share does not take: {todo:?}");
        rate(&mut window, place::RateMessage::Typed("2.5".into()));
        let said = the_call(&window).rate_note().map(|note| words::Words::english().said(&note));
        assert_eq!(said.as_deref(), Some("At most 2.5 Mbit/s; less when the screen or your connection needs less."));
        let custom = Ceiling::Custom(Kbits::new(2_500).expect("a step"));
        assert_eq!(the_call(&window).quality().ceiling, custom);
        rate(&mut window, place::RateMessage::Slid(80));
        assert_eq!(the_call(&window).quality().ceiling, Ceiling::Custom(Kbits::MOST));
        tell(
            &mut window,
            &clock,
            place,
            place::Message::Call(place::CallMessage::Quality(QualityPick::Ceiling(Ceiling::Mbit4))),
        );
        assert_eq!((the_call(&window).rate_note(), the_call(&window).quality().ceiling), (None, Ceiling::Mbit4));
        let todo = tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ChooseScreen));
        assert!(!todo.is_empty(), "a preset starts the share");
    }

    #[test]
    fn a_choice_put_away_with_the_panel_is_not_kept() {
        use choices::{FrameRate, QualityPick, ShareQuality};
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleShare));
        let todo = tell(
            &mut window,
            &clock,
            place,
            place::Message::Call(place::CallMessage::Quality(QualityPick::FrameRate(FrameRate::Fifteen))),
        );
        assert!(todo.is_empty(), "{todo:?}");
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleShare));
        assert_eq!(window.voice().share, ShareQuality::default());
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleShare));
        assert_eq!(the_call(&window).quality(), &ShareQuality::default(), "the panel opens again on what was kept");
    }

    #[test]
    fn a_new_call_starts_with_the_kept_view() {
        let clock = Clock(Instant::now());
        let (window, _) = Window::launch(vec![Opening::Account(owls(), ADA.into())], None, clock.0);
        let look = choices::Look {
            call: choices::CallView {
                people: choices::People::Pictured,
                own_share: choices::OwnShare::Hidden,
                ..choices::CallView::default()
            },
            ..choices::Look::default()
        };
        let mut window = window.remembering(Remembered { look, ..Remembered::default() }, BTreeMap::new());
        let place = id(&window, 0);
        tell(&mut window, &clock, place, flow(token()));
        tell(&mut window, &clock, place, flow(accepted_as("ada")));
        tell(&mut window, &clock, place, place::Message::Lobby(place::LobbyMessage::Channels(Ok(channels()))));
        in_call(&mut window, &clock, place);
        let shares = RoomEvent::Started { user_id: member("ada"), track: Track::ScreenVideo };
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Room(shares)));
        let bo = RoomEvent::MemberJoined { user_id: member("bo"), name: "Bo".into(), colour: None };
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Room(bo)));
        let shares = RoomEvent::Started { user_id: member("bo"), track: Track::ScreenVideo };
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Room(shares)));
        assert_eq!(cards(&window), ["Bo's screen"]);
    }
}
