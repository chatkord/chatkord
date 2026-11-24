//! The card that says what a shared screen is: its size and rate, how this viewer decodes it, marked "software"
//! where a hardware path did not run, and what this viewer did not paint.

use crate::call::health::Outgoing;
use design::Presence;
use proto::video::Video;
use terms::MemberId;
use words::Sentence;
use words::{Phrase, Words};

/// What this account's own screen share reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shared {
    /// The screen goes out at this size, encoded this way.
    Started {
        /// The sent picture's width, in pixels.
        width: u32,
        /// The sent picture's height, in pixels.
        height: u32,
        /// How this device encodes it.
        encoder: Route,
    },
    /// How it is going, once a second.
    Looked(Outgoing),
    /// It ended without being stopped, or could not begin, and why.
    Failed(words::Failure),
    /// It was stopped.
    Stopped,
    /// The server stopped it: this account's roles no longer allow sharing.
    Denied,
    /// This device cannot show the screen it sends; the share goes on.
    Unshown,
    /// The screen's encoder failed and another, coding this way, took over: the share goes on.
    HandedOver(Route),
    /// What the share's sound is, once the call has looked.
    Sound(SoundSaid),
}

/// A share's sound, as the call found it for what was picked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SoundSaid {
    /// A window's app, by the name a person reads.
    App(String),
    /// A whole screen: every app's but Chatkord's.
    Everything,
    /// Its switch was off.
    Off,
    /// A window whose app this device cannot find, so it goes without sound.
    NotFound,
    /// Chatkord's own window, shared without sound: the call would hear itself.
    Own,
}

impl SoundSaid {
    /// Whether the sound is carried, which the popover's glyph for its line says.
    pub fn kind(&self) -> crate::about::LineKind {
        match self {
            SoundSaid::App(_) | SoundSaid::Everything => crate::about::LineKind::Sound,
            SoundSaid::Off | SoundSaid::NotFound | SoundSaid::Own => crate::about::LineKind::NoSound,
        }
    }

    /// The popover's line: "Sound: Brave Web Browser".
    pub fn line(&self, words: &Words) -> String {
        match self {
            SoundSaid::App(name) => words.say(&Sentence::SoundOf(name.clone())),
            SoundSaid::Everything => words.phrase(Phrase::SoundEverything).into_owned(),
            SoundSaid::Off => words.phrase(Phrase::SoundOff).into_owned(),
            SoundSaid::NotFound => words.phrase(Phrase::SoundNotFound).into_owned(),
            SoundSaid::Own => words.phrase(Phrase::SoundOwn).into_owned(),
        }
    }
}

/// An application playing sound, which a window share may send: the sound server's name for it, and its program when
/// told; together they are how the server finds it again.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SoundApp {
    /// What it calls itself: what a person reads.
    pub name: String,
    /// The program it runs, when the sound server was told.
    pub binary: Option<String>,
}

/// What a picture sent in the call reports, each by its sender's id and kind; a card and a sound are a screen's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Watched {
    /// How the screen's picture is, once a second, once there is one.
    Card(ShareCard),
    /// This device cannot show it, and why.
    Failed {
        /// The sender's user id.
        user_id: MemberId,
        /// Which picture.
        video: Video,
        /// Why this device cannot show it.
        failed: words::Failure,
    },
    /// Its sound is heard, or the server could not carry it and why; its picture is as it was.
    Sound {
        /// The sharer's user id.
        user_id: MemberId,
        /// Why it is not heard; none once it is.
        refused: Option<words::Failure>,
    },
    /// Its decoder broke after showing pictures, and one decoding this way took over.
    HandedOver {
        /// The sender's user id.
        user_id: MemberId,
        /// Which picture.
        video: Video,
        /// How it is decoded now.
        route: Route,
    },
    /// The picture began held, because the person paused it before it was watched: its card says so and offers Resume.
    Paused {
        /// The sender's user id.
        user_id: MemberId,
        /// Which picture.
        video: Video,
    },
    /// This member's picture is no longer shown: they ended it or left, or the call is over.
    Ended {
        /// The sender's user id.
        user_id: MemberId,
        /// Which picture.
        video: Video,
    },
}

/// Whether this account can send its screen from this device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Can {
    /// It can: the share button is enabled.
    Yes,
    /// The graphics card has no video encoder Chatkord can use.
    NoEncoder,
    /// The account's roles do not let it share on this server.
    NotAllowed,
}

impl Can {
    /// The tooltip that says why not, none when it can.
    pub fn why_not(self) -> Option<Phrase> {
        match self {
            Can::Yes => None,
            Can::NoEncoder => Some(Phrase::CantShareNoEncoder),
            Can::NotAllowed => Some(Phrase::CantShareNotAllowed),
        }
    }
}
/// How a picture is being made or taken apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    /// On the graphics hardware, through the named interface.
    Hardware {
        /// The interface as a person reads it ("VA-API").
        via: String,
    },
    /// On a graphics card's encoder whose family nobody has tested with Chatkord, through the named interface: an
    /// encoder's route only.
    UntestedHardware {
        /// The interface as a person reads it ("D3D11").
        via: String,
    },
    /// On the processor: decoding's fallback, or the software encoder of a codec no graphics card here makes or keeps
    /// making, which the card marks.
    Software,
}

impl Route {
    /// What a share is told when its encoder failed and one coding this way took over.
    pub fn handed_over(&self) -> Sentence {
        Sentence::Phrase(match self {
            Route::Software => Phrase::ShareNowOnProcessor,
            Route::Hardware { .. } | Route::UntestedHardware { .. } => Phrase::ShareNowOnAnotherEncoder,
        })
    }

    /// What a viewer is told when the decoder of `name`'s screen failed and one decoding this way took over.
    pub fn watched_handed_over(&self, name: String) -> Sentence {
        match self {
            Route::Software => Sentence::WatchedNowOnProcessor(name),
            Route::Hardware { .. } | Route::UntestedHardware { .. } => Sentence::WatchedNowOnAnotherDecoder(name),
        }
    }
}

/// Whether a watched share's picture is arriving, shown as the card's badge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareState {
    /// The connection that carries it is up.
    Live,
    /// The connection that carries it is down and being made again.
    Reconnecting,
    /// It arrives, but this computer cannot decode it as fast as it comes.
    Behind,
    /// The viewer paused it: nothing of it arrives until they resume it.
    Paused,
}

/// What a watched screen's card says of it, as this viewer measured it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShareCard {
    /// Whose screen it is, by their user id.
    pub who: MemberId,
    /// The decoded picture's width, in pixels.
    pub width: u32,
    /// The decoded picture's height, in pixels.
    pub height: u32,
    /// The pictures a second the viewer measured; none until it has.
    pub frame_rate: Option<u32>,
    /// Pictures a newer one replaced before this viewer took them, counted since this share began. A slow paint
    /// costs pictures, so the card says how many were lost; it is this viewer's own, not the sender's.
    pub dropped: u64,
    /// How the picture is coded, as the engine names it ("H.264").
    pub codec: String,
    /// How this viewer decodes it.
    pub decoder: Route,
    /// Frames this viewer's decoder refused, since the share began.
    pub refused: u64,
    /// How often this viewer's picture froze, and the longest.
    pub freezes: (u64, std::time::Duration),
    /// Whether it is arriving, for the badge.
    pub state: ShareState,
}

/// "1920 × 1080 · 60 fps", or the size alone until the rate is known.
pub(crate) fn size_and_rate(width: u32, height: u32, frame_rate: Option<u32>, words: &Words) -> String {
    let size = words.dimensions(width, height);
    match frame_rate {
        Some(rate) => format!("{size} \u{b7} {}", words.fps(rate)),
        None => size,
    }
}

impl ShareCard {
    /// "1920 × 1080 · 60 fps", or the size alone until the rate is known.
    pub fn size_and_rate(&self, words: &Words) -> String {
        size_and_rate(self.width, self.height, self.frame_rate, words)
    }

    /// The badge for the share's state.
    pub fn badge(&self) -> Presence {
        match self.state {
            ShareState::Live | ShareState::Behind | ShareState::Paused => Presence::Live,
            ShareState::Reconnecting => Presence::Reconnecting,
        }
    }

    /// Whether a path ran in software: the card flags it, because a fallback is not what was meant.
    pub fn software(route: &Route) -> bool {
        matches!(route, Route::Software)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call::room::{Event as RoomEvent, Track};
    use crate::call::sources::Chooser;
    use crate::harness::{Clock, member, moderator_in_call, picture_of, sharer_in_call, tell, the_call};
    use crate::place::{self, Over};
    use crate::window::Window;
    use crate::{Ask, WindowMessage};
    use proto::rules::Grants;
    use proto::video::Video;
    use std::time::Instant;
    use words::{Sentence, Words};

    #[test]
    fn ones_own_shares_sound_is_said_in_one_line() {
        assert_eq!(words::without_isolation(&SoundSaid::App("Brave".into()).line(&Words::english())), "Sound: Brave");
        assert_eq!(SoundSaid::Everything.line(&Words::english()), "Sound: every app except Chatkord");
        assert_eq!(SoundSaid::Off.line(&Words::english()), "Sound: off");
        assert_eq!(SoundSaid::NotFound.line(&Words::english()), "No sound found for this window");
        assert_eq!(SoundSaid::Own.line(&Words::english()), "No sound: Chatkord’s own is never shared");
    }

    fn card(decoder: Route, state: ShareState) -> ShareCard {
        ShareCard {
            who: member("Ada"),
            width: 1920,
            height: 1080,
            frame_rate: Some(60),
            dropped: 0,
            codec: "H.264".into(),
            decoder,
            refused: 0,
            freezes: (0, std::time::Duration::ZERO),
            state,
        }
    }

    #[test]
    fn size_and_rate_read_as_one_line() {
        let card = card(Route::Software, ShareState::Live);
        assert_eq!(card.size_and_rate(&Words::english()), "1920 \u{d7} 1080 \u{b7} 60 fps");
    }

    #[test]
    fn the_rate_is_left_out_until_it_is_measured() {
        let mut card = card(Route::Software, ShareState::Live);
        card.frame_rate = None;
        assert_eq!(card.size_and_rate(&Words::english()), "1920 \u{d7} 1080");
    }

    #[test]
    fn a_software_route_is_flagged_and_a_hardware_one_is_not() {
        assert!(ShareCard::software(&Route::Software));
        assert!(!ShareCard::software(&Route::Hardware { via: "VA-API".into() }));
    }

    #[test]
    fn the_badge_follows_the_state() {
        for (state, badge) in [
            (ShareState::Live, Presence::Live),
            (ShareState::Reconnecting, Presence::Reconnecting),
            (ShareState::Paused, Presence::Live),
        ] {
            assert_eq!(card(Route::Software, state).badge(), badge);
        }
    }

    #[test]
    fn our_own_picker_opens_where_the_system_has_one_selects_one_card_and_shares_it_with_the_panels_quality() {
        use crate::{Source, SourceKey, SourceKind, SourcesTab};
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        window.update(WindowMessage::Chooser(Chooser::Own), clock.at());
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleShare));
        let quality = the_call(&window).quality().clone();
        let todo = tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ChooseScreen));
        assert!(matches!(&todo[..], [Ask::At(_, place::Ask::WatchSources)]), "{todo:?}");
        assert_eq!(the_call(&window).open(), Some(Over::Sources));
        let source = |key: &str, kind| Source {
            key: SourceKey(key.into()),
            kind,
            title: (kind == SourceKind::Application).then(|| "Notes".to_string()),
            size: (kind == SourceKind::Screen).then_some((3840, 2160)),
            icon: None,
            still: None,
        };
        tell(
            &mut window,
            &clock,
            place,
            place::Message::Call(place::CallMessage::Sources(vec![
                source("m1", SourceKind::Screen),
                source("w1", SourceKind::Application),
            ])),
        );
        assert!(
            tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ShareSource)).is_empty(),
            "nothing selected: Share does nothing"
        );
        tell(
            &mut window,
            &clock,
            place,
            place::Message::Call(place::CallMessage::SourcesTab(SourcesTab::Applications)),
        );
        tell(
            &mut window,
            &clock,
            place,
            place::Message::Call(place::CallMessage::SelectSource(SourceKey("w1".into()))),
        );
        assert_eq!(the_call(&window).sources().map(|sources| sources.cards(&Words::english())[0].selected), Some(true));
        let todo = tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ShareSource));
        assert!(
            matches!(&todo[..], [Ask::At(_, place::Ask::StopSources), Ask::At(_, place::Ask::ShareSource { key, quality: asked })]
                if *key == SourceKey("w1".into()) && *asked == quality),
            "{todo:?}"
        );
        assert_eq!(the_call(&window).open(), None, "the picker closed as it shared");
    }

    #[test]
    fn closing_our_own_picker_stops_its_listing_and_the_desktops_picker_is_asked_where_there_is_one() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        window.update(WindowMessage::Chooser(Chooser::Own), clock.at());
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleShare));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ChooseScreen));
        let todo = tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::CloseOver));
        assert!(matches!(&todo[..], [Ask::At(_, place::Ask::StopSources)]), "{todo:?}");
        window.update(WindowMessage::Chooser(Chooser::Desktop), clock.at());
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleShare));
        let todo = tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ChooseScreen));
        assert!(matches!(&todo[..], [Ask::At(_, place::Ask::ChooseScreen(_))]), "{todo:?}");
    }

    #[test]
    fn the_panes_share_shows_the_calls_page_before_its_panel_and_leaving_the_page_closes_the_panel() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        window.update(WindowMessage::OpenSettings, clock.at());
        assert_ne!(window.open().map(place::Place::panel), Some(place::Panel::Call), "the settings are shown");
        window.update(WindowMessage::PaneShare(place), clock.at());
        assert_eq!(window.open().map(place::Place::panel), Some(place::Panel::Call), "the call's page is shown");
        assert_eq!(the_call(&window).open(), Some(Over::Share), "and the panel opened over it");
        window.update(WindowMessage::PaneShare(place), clock.at());
        assert_eq!(the_call(&window).open(), None, "a second press, the page shown, closes the panel");
        window.update(WindowMessage::PaneShare(place), clock.at());
        assert_eq!(the_call(&window).open(), Some(Over::Share));
        window.update(WindowMessage::OpenSettings, clock.at());
        assert_eq!(the_call(&window).open(), None, "a panel that is not drawn is not left open");
    }

    #[test]
    fn the_share_button_opens_its_panel_only_for_who_may_and_can() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        assert_eq!(window.open().unwrap().can_share(), crate::Can::Yes);
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleShare));
        assert_eq!(the_call(&window).open(), Some(place::Over::Share));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleShare));
        assert_eq!(the_call(&window).open(), None, "a second press closes it");

        window.update(WindowMessage::Encoders(Vec::new()), clock.at());
        assert_eq!(window.open().unwrap().can_share(), crate::Can::NoEncoder);
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleShare));
        assert_eq!(the_call(&window).open(), None, "a device with no encoder opens nothing");
        assert!(tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ChooseScreen)).is_empty());

        let (mut window, place) = moderator_in_call(&clock, proto::rules::Place::BOTTOM, Grants::default());
        assert_eq!(window.open().unwrap().can_share(), crate::Can::NotAllowed);
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleShare));
        assert_eq!(the_call(&window).open(), None, "an account whose roles do not allow it opens nothing");
    }

    #[test]
    fn a_shares_sound_is_said_while_sharing_and_a_new_share_starts_with_nothing_said() {
        use crate::{Shared, SoundSaid};
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        let started = || Shared::Started { width: 1920, height: 1080, encoder: crate::Route::Software };
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Starting));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Shared(started())));
        tell(
            &mut window,
            &clock,
            place,
            place::Message::Call(place::CallMessage::Shared(Shared::Sound(SoundSaid::Everything))),
        );
        assert_eq!(the_call(&window).own_sound(), Some(&SoundSaid::Everything));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Shared(Shared::Stopped)));
        assert_eq!(the_call(&window).own_sound(), None, "said only while sharing");
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Starting));
        assert_eq!(the_call(&window).own_sound(), None, "a new share starts with nothing said");
    }

    #[test]
    fn a_window_whose_app_was_not_found_lists_the_apps_and_sends_the_one_picked() {
        use crate::{Shared, SoundApp, SoundSaid};
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        let call = |message| place::Message::Call(message);
        tell(&mut window, &clock, place, call(place::CallMessage::Starting));
        let started = Shared::Started { width: 1920, height: 1080, encoder: crate::Route::Software };
        tell(&mut window, &clock, place, call(place::CallMessage::Shared(started)));
        let asked =
            tell(&mut window, &clock, place, call(place::CallMessage::Shared(Shared::Sound(SoundSaid::NotFound))));
        assert!(matches!(&asked[..], [Ask::At(_, place::Ask::ListSoundApps)]), "the host lists them: {asked:?}");
        let app = |name: &str| SoundApp { name: name.into(), binary: Some(name.to_lowercase()) };
        let stranger = tell(&mut window, &clock, place, call(place::CallMessage::PickSoundApp(app("Spotify"))));
        assert!(stranger.is_empty(), "nothing is picked before it is listed");
        tell(&mut window, &clock, place, call(place::CallMessage::SoundApps(vec![app("Firefox"), app("Spotify")])));
        assert_eq!(the_call(&window).own_sound_apps().map(|(apps, picked)| (apps.len(), picked)), Some((2, None)));
        let picked = tell(&mut window, &clock, place, call(place::CallMessage::PickSoundApp(app("Spotify"))));
        assert!(
            matches!(&picked[..], [Ask::At(_, place::Ask::Call(place::Control::ShareSoundOf(sent)))] if *sent == app("Spotify")),
            "{picked:?}"
        );
        let heard = Shared::Sound(SoundSaid::App("Spotify".into()));
        tell(&mut window, &clock, place, call(place::CallMessage::Shared(heard)));
        assert_eq!(the_call(&window).own_sound_apps().and_then(|(_, picked)| picked), Some("Spotify"), "still listed");
    }

    #[test]
    fn a_screen_is_picked_sent_and_stopped_with_no_toast() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleShare));
        let todo = tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ChooseScreen));
        assert!(
            matches!(&todo[..], [Ask::At(at, place::Ask::ChooseScreen(quality))] if *at == place && *quality == choices::ShareQuality::default()),
            "{todo:?}"
        );
        assert_eq!(the_call(&window).open(), None, "the picker is the desktop's: the panel goes");
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Starting));
        assert_eq!(the_call(&window).sending(), &place::Sending::Starting);
        assert!(
            tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ChooseScreen)).is_empty(),
            "one at a time"
        );

        tell(
            &mut window,
            &clock,
            place,
            place::Message::Call(place::CallMessage::Shared(crate::Shared::Started {
                width: 1920,
                height: 1080,
                encoder: crate::Route::Hardware { via: "VA-API".into() },
            })),
        );
        assert_eq!(the_call(&window).sending(), &place::Sending::Live(None));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleOwnInfo));
        assert_eq!(the_call(&window).open(), None, "nothing to say about the screen before its first look");
        let outgoing = crate::Outgoing {
            codec: "H.264".into(),
            encoder: crate::Route::Hardware { via: "VA-API".into() },
            width: 1920,
            height: 1080,
            scaled_from: None,
            frame_rate: Some(60),
            bits_per_second: 8_000_000,
            frames: 120,
            key_frames_made: 0,
            key_frames_asked: 0,
            resent_bytes: 0,
            restarts: 0,
            limited: false,
            reduced: None,
        };
        tell(
            &mut window,
            &clock,
            place,
            place::Message::Call(place::CallMessage::Shared(crate::Shared::Looked(outgoing.clone()))),
        );
        assert_eq!(the_call(&window).outgoing(), Some(&outgoing));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleOwnInfo));
        assert_eq!(the_call(&window).open(), Some(place::Over::OwnPicture));

        let todo = tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleShare));
        assert!(matches!(&todo[..], [Ask::At(_, place::Ask::Call(place::Control::StopShare))]), "{todo:?}");
        assert_eq!(the_call(&window).sending(), &place::Sending::No);
        assert_eq!(
            window.toast().map(|toast| toast.said().in_english()).as_deref(),
            None,
            "the card goes: that is the answer"
        );
        assert_eq!(the_call(&window).open(), None, "and what was open about it closes with it");
        tell(
            &mut window,
            &clock,
            place,
            place::Message::Call(place::CallMessage::Shared(crate::Shared::Looked(outgoing))),
        );
        assert_eq!(the_call(&window).sending(), &place::Sending::No, "a late look does not bring the share back");
    }

    #[test]
    fn one_s_own_screen_card_waits_then_shows_what_is_sent_or_says_this_device_cannot() {
        use crate::{Card, Preview};
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        let own = |window: &Window| {
            the_call(window).cards().iter().find_map(|card| match card {
                Card::OwnScreen(preview) => Some(*preview),
                Card::Person(_) | Card::Screen { .. } => None,
            })
        };
        let started = || {
            place::Message::Call(place::CallMessage::Shared(crate::Shared::Started {
                width: 1920,
                height: 1080,
                encoder: crate::Route::Hardware { via: "VA-API".into() },
            }))
        };
        assert_eq!(own(&window), None, "no card before a share");
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Starting));
        assert_eq!(own(&window), Some(Preview::Waiting));
        tell(&mut window, &clock, place, started());
        assert_eq!(own(&window), Some(Preview::Shown));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Shared(crate::Shared::Unshown)));
        assert_eq!(own(&window), Some(Preview::Unshown), "the share goes on, and its card says why it shows nothing");
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Shared(crate::Shared::Stopped)));
        assert_eq!(own(&window), None);
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Shared(crate::Shared::Unshown)));
        assert_eq!(own(&window), None, "a late word brings no card back");
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Starting));
        assert_eq!(own(&window), Some(Preview::Waiting), "the next share waits for its own picture");
    }

    #[test]
    fn a_share_that_failed_is_a_danger_toast_that_offers_to_share_again_and_marks_its_control() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Starting));
        tell(
            &mut window,
            &clock,
            place,
            place::Message::Call(place::CallMessage::Shared(crate::Shared::Failed(words::Failure::ScreenSilent))),
        );
        assert_eq!(the_call(&window).sending(), &place::Sending::No);
        let said = "Your share stopped. The screen sent no picture in that time.";
        assert_eq!(the_call(&window).fault(crate::Device::Share).map(Sentence::in_english).as_deref(), Some(said));
        let toast = window.toast().expect("a failure is told in a toast");
        assert_eq!((toast.kind(), toast.said().in_english()), (design::Status::Danger, said.into()));
        assert_eq!(toast.action(), Some(&crate::Action::ShareAgain));
        let todo = window.update(crate::WindowMessage::ToastAct, clock.at());
        assert!(
            matches!(&todo[..], [Ask::At(_, place::Ask::ChooseScreen(_))]),
            "Share again opens the picker: {todo:?}"
        );
        assert_eq!(the_call(&window).fault(crate::Device::Share), None, "pressed: its control is as at rest");
    }

    #[test]
    fn a_share_whose_encoder_was_handed_over_goes_on_with_a_warning_of_what_encodes_it_now() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Starting));
        let started = crate::Shared::Started {
            width: 1920,
            height: 1080,
            encoder: crate::Route::Hardware { via: "Vulkan".into() },
        };
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Shared(started)));
        for (route, said) in [
            (
                crate::Route::Hardware { via: "VA-API".into() },
                "Your stream’s encoder stopped; another one encodes it now.",
            ),
            (crate::Route::Software, "Your stream’s encoder stopped; the processor encodes it now."),
        ] {
            let handed = crate::Shared::HandedOver(route);
            tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Shared(handed)));
            assert!(matches!(the_call(&window).sending(), &place::Sending::Live(_)), "the share goes on");
            let toast = window.toast().expect("told");
            assert_eq!(
                (toast.kind(), toast.said().in_english(), toast.action()),
                (design::Status::Warning, said.into(), None)
            );
            assert_eq!(the_call(&window).fault(crate::Device::Share), None, "it goes on: nothing failed");
        }
    }

    #[test]
    fn a_share_the_server_denied_is_a_danger_toast_with_no_action_and_the_call_goes_on() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Starting));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Shared(crate::Shared::Denied)));
        assert_eq!(the_call(&window).sending(), &place::Sending::No);
        let toast = window.toast().expect("told");
        assert_eq!(
            (toast.kind(), toast.said().in_english(), toast.action()),
            (design::Status::Danger, words::Phrase::ShareDenied.in_english(), None)
        );
        assert!(the_call(&window).fault(crate::Device::Share).is_some(), "its control says why");
        assert!(the_call(&window).live(), "only the share ended");
    }

    #[test]
    fn the_share_denied_mark_goes_when_sharing_is_allowed_again() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        // The server tells the account its standing, and the list it then reads carries its words in each channel.
        let standing = |window: &mut Window, grants: Grants| {
            let rolled = place::SessionMessage::Standing { place: proto::rules::Place::BOTTOM, grants };
            tell(window, &clock, place, place::Message::Session(rolled));
            let listed = place::LobbyMessage::Channels(Ok(crate::harness::channels_holding(grants)));
            tell(window, &clock, place, place::Message::Lobby(listed));
        };
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Starting));
        standing(&mut window, Grants::default());
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Shared(crate::Shared::Denied)));
        standing(&mut window, Grants::default());
        assert!(the_call(&window).fault(crate::Device::Share).is_some(), "a standing that still denies keeps it");
        let account_wide = Grants::from_words(&["SHARE_SCREEN"]);
        tell(
            &mut window,
            &clock,
            place,
            place::Message::Session(place::SessionMessage::Standing {
                place: proto::rules::Place::BOTTOM,
                grants: account_wide,
            }),
        );
        assert!(
            the_call(&window).fault(crate::Device::Share).is_some(),
            "the channel's words decide, as the list tells them"
        );
        standing(&mut window, account_wide);
        assert_eq!(the_call(&window).fault(crate::Device::Share), None, "allowed again, nothing is denied any more");
    }

    #[test]
    fn a_new_share_puts_the_share_denied_mark_away() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Starting));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Shared(crate::Shared::Denied)));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Starting));
        assert_eq!(the_call(&window).fault(crate::Device::Share), None);
    }

    /// How `user_id`'s screen card shows, none without one.
    fn received(window: &Window, user_id: &str) -> Option<crate::Received> {
        the_call(window).cards().iter().find_map(|card| match card {
            crate::Card::Screen { user_id: id, received, .. } if id.as_str() == user_id => Some((*received).clone()),
            crate::Card::Person(_) | crate::Card::OwnScreen(_) | crate::Card::Screen { .. } => None,
        })
    }

    #[test]
    fn a_screens_sound_not_carried_is_said_on_its_card_until_it_is_heard_or_the_screen_ends() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        let sound = |window: &mut Window, refused| {
            let said = crate::Watched::Sound { user_id: member("bo"), refused };
            tell(window, &clock, place, place::Message::Call(place::CallMessage::Watched(said)));
            the_call(window).unheard(&member("bo")).cloned()
        };
        let busy = Some(words::Failure::ShareSoundNotCarried);
        assert_eq!(sound(&mut window, busy.clone()), busy);
        assert_eq!(
            words::Words::english().said(&words::failure::of(&words::Failure::ShareSoundNotCarried)),
            "The server can’t send you this stream’s sound now."
        );
        assert_eq!(sound(&mut window, None), None, "heard again, the line goes");
        sound(&mut window, busy);
        let ended = crate::Watched::Ended { user_id: member("bo"), video: Video::Screen };
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Watched(ended)));
        assert_eq!(the_call(&window).unheard(&member("bo")), None, "the screen's end takes it");
    }

    #[test]
    fn a_watched_screen_whose_decoder_broke_is_toasted_by_its_sharers_name() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        let handed = |window: &mut Window, route| {
            let said = crate::Watched::HandedOver { user_id: member("bo"), video: Video::Screen, route };
            tell(window, &clock, place, place::Message::Call(place::CallMessage::Watched(said)));
            window.toast().map(|toast| toast.said().in_english())
        };
        assert_eq!(
            handed(&mut window, crate::Route::Hardware { via: "Vulkan".into() }),
            Some("The decoder of Bo’s stream stopped; another one decodes it now.".into())
        );
        assert_eq!(
            handed(&mut window, crate::Route::Software),
            Some("The decoder of Bo’s stream stopped; the processor decodes it now.".into()),
            "a later one takes the place of the first"
        );
        let stranger =
            crate::Watched::HandedOver { user_id: member("zed"), video: Video::Screen, route: crate::Route::Software };
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Watched(stranger)));
        assert_eq!(
            window.toast().map(|toast| toast.said().in_english()).as_deref(),
            Some("The decoder of Bo’s stream stopped; the processor decodes it now."),
            "someone not in the call is not named"
        );
    }

    #[test]
    fn every_share_plays_in_its_card_from_its_start_and_ends_with_it() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        assert_eq!(received(&window, "bo"), Some(crate::Received::Waiting), "playing before its first picture");
        assert_eq!(received(&window, "cy"), None, "someone who shares nothing has no screen card");

        let watched = |window: &mut Window, said| {
            tell(window, &clock, place, place::Message::Call(place::CallMessage::Watched(said)))
        };
        assert!(
            watched(&mut window, crate::Watched::Card(picture_of("bo"))).is_empty(),
            "nothing is asked of the call"
        );
        assert_eq!(received(&window, "bo"), Some(crate::Received::Playing(picture_of("bo"))));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleInfo(member("bo"))));
        assert_eq!(
            (the_call(&window).open(), the_call(&window).about()),
            (Some(place::Over::Picture), Some(&member("bo")))
        );

        watched(&mut window, crate::Watched::Card(picture_of("cy")));
        assert_eq!(received(&window, "cy"), None, "a picture of a screen not shared here is late, and kept out");

        let room = |window: &mut Window, event| {
            tell(window, &clock, place, place::Message::Call(place::CallMessage::Room(event)))
        };
        room(&mut window, RoomEvent::Stopped { user_id: member("bo"), track: Track::ScreenVideo });
        assert_eq!(received(&window, "bo"), None, "a screen that is no longer shared shows nothing");
        assert_eq!(the_call(&window).open(), None, "what was open about its picture closes with it");
        room(&mut window, RoomEvent::Started { user_id: member("bo"), track: Track::ScreenVideo });
        assert_eq!(received(&window, "bo"), Some(crate::Received::Waiting), "shared again, it starts from nothing");
    }

    #[test]
    fn two_shares_play_at_once_each_with_its_own_about() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        let started = RoomEvent::Started { user_id: member("cy"), track: Track::ScreenVideo };
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Room(started)));
        for who in ["bo", "cy"] {
            tell(
                &mut window,
                &clock,
                place,
                place::Message::Call(place::CallMessage::Watched(crate::Watched::Card(picture_of(who)))),
            );
        }
        assert_eq!(received(&window, "bo"), Some(crate::Received::Playing(picture_of("bo"))));
        assert_eq!(received(&window, "cy"), Some(crate::Received::Playing(picture_of("cy"))));

        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleInfo(member("bo"))));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleInfo(member("cy"))));
        assert_eq!(the_call(&window).about(), Some(&member("cy")), "one popover at a time, the last pressed");
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleInfo(member("cy"))));
        assert_eq!(the_call(&window).about(), None, "a second press closes it");

        tell(
            &mut window,
            &clock,
            place,
            place::Message::Call(place::CallMessage::Watched(crate::Watched::Ended {
                user_id: member("bo"),
                video: Video::Screen,
            })),
        );
        assert_eq!(received(&window, "bo"), Some(crate::Received::Waiting), "its card waits while it is still shared");
        assert_eq!(received(&window, "cy"), Some(crate::Received::Playing(picture_of("cy"))), "the other plays on");
    }

    #[test]
    fn this_account_s_tile_and_its_row_are_live_while_its_screen_goes_out() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        let live = |window: &Window| {
            let own = the_call(window).tiles().iter().find(|tile| tile.own()).unwrap().sharing;
            let call = the_call(window);
            let row = window.open().unwrap().seats(&call.channel).into_iter().find(|seat| seat.own()).unwrap().sharing;
            (own, row)
        };
        assert_eq!(live(&window), (false, false));
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Starting));
        assert_eq!(live(&window), (true, true), "the server never says so: the call does");
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleShare));
        assert_eq!(live(&window), (false, false));
    }

    #[test]
    fn a_screen_this_device_cannot_show_says_so_in_its_card_and_only_there() {
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        let failed = crate::Watched::Failed {
            user_id: member("bo"),
            video: Video::Screen,
            failed: words::Failure::CannotDecode,
        };
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::Watched(failed)));
        assert_eq!(received(&window, "bo"), Some(crate::Received::Unshown(words::Failure::CannotDecode)));
        assert!(window.toast().is_none(), "the card says it: no toast repeats it");
        tell(&mut window, &clock, place, place::Message::Call(place::CallMessage::ToggleInfo(member("bo"))));
        assert_eq!(the_call(&window).open(), None, "a screen not shown has no picture to be about");
    }

    #[test]
    fn pausing_a_watched_screen_asks_the_call_to_hold_it_and_resuming_to_start_it_and_the_card_says_so_at_once() {
        let clock = Clock(Instant::now());
        let (mut window, place) = crate::harness::watching_bo(&clock);
        let state = |window: &Window| match the_call(window).cards().iter().find_map(|card| match card {
            crate::Card::Screen { received, .. } => Some(*received),
            crate::Card::Person(_) | crate::Card::OwnScreen(_) => None,
        }) {
            Some(place::Received::Playing(card)) => Some(card.state),
            Some(_) | None => None,
        };
        let toggle = |window: &mut Window| {
            let pin = crate::Pinned::Screen(member("bo"));
            tell(window, &clock, place, place::Message::Call(place::CallMessage::TogglePause(pin)))
        };
        let todo = toggle(&mut window);
        assert!(
            matches!(&todo[..], [Ask::At(_, place::Ask::Call(place::Control::Pause { user_id, video: Video::Screen }))] if *user_id == member("bo")),
            "{todo:?}"
        );
        assert!(the_call(&window).is_paused(&member("bo"), Video::Screen));
        assert_eq!(state(&window), Some(ShareState::Paused));
        let todo = toggle(&mut window);
        assert!(matches!(&todo[..], [Ask::At(_, place::Ask::Call(place::Control::Resume { .. }))]), "{todo:?}");
        assert_eq!(state(&window), Some(ShareState::Live));
        let own = tell(
            &mut window,
            &clock,
            place,
            place::Message::Call(place::CallMessage::TogglePause(crate::Pinned::OwnScreen)),
        );
        assert!(own.is_empty(), "one's own screen has nothing to hold");
    }

    #[test]
    fn a_picture_that_began_paused_is_told_paused_and_forgotten_when_it_ends() {
        let clock = Clock(Instant::now());
        let (mut window, place) = crate::harness::watching_bo(&clock);
        let call = |window: &mut Window, message| tell(window, &clock, place, place::Message::Call(message));
        let began = Watched::Paused { user_id: member("bo"), video: Video::Screen };
        call(&mut window, place::CallMessage::Watched(began));
        assert!(the_call(&window).is_paused(&member("bo"), Video::Screen));
        let ended = Watched::Ended { user_id: member("bo"), video: Video::Screen };
        call(&mut window, place::CallMessage::Watched(ended));
        assert!(!the_call(&window).is_paused(&member("bo"), Video::Screen), "the next of theirs is a new one");
    }

    #[test]
    fn whether_anyone_takes_ones_own_screen_is_kept_until_told_otherwise() {
        use crate::OwnWatch;
        let clock = Clock(Instant::now());
        let (mut window, place) = sharer_in_call(&clock);
        assert_eq!(the_call(&window).own_watch(Video::Screen), OwnWatch::Watched);
        let told = |window: &mut Window, video, watched| {
            let message = place::CallMessage::OwnWatched { video, watched };
            tell(window, &clock, place, place::Message::Call(message));
        };
        told(&mut window, Video::Screen, OwnWatch::Unwatched);
        assert_eq!(the_call(&window).own_watch(Video::Screen), OwnWatch::Unwatched);
        assert_eq!(the_call(&window).own_watch(Video::Camera), OwnWatch::Watched, "each of the two on its own");
        told(&mut window, Video::Screen, OwnWatch::Watched);
        assert_eq!(the_call(&window).own_watch(Video::Screen), OwnWatch::Watched);
    }
}
