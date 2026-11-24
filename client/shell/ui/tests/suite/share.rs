//! Sharing a screen and watching one inside a call, each part a reference in both schemes; what a press sends and
//! what a screen reader hears are asked of the simulator.

use crate::common;

use choices::{
    Ceiling, Codec, EncoderOption, EncodesOn, FrameRate, PresetOrCustom, QualityPick, Resolution, ShareQuality,
};
use common::{Plain, boxes, label, press, semantics, settle, sim, sim_sized, tab};
use design::{Accent, CallControl, Scheme, SemanticRole, Surface};
use iced_core::keyboard::key::Named;
use iced_core::{Element, Length, Point, Size, Widget};
use iced_widget::{Row, container};
use proto::video::Video;
use screens::{
    About, Action, Health, Outgoing, OwnVideo, OwnWatch, RateMessage, Route, ShareCard, ShareState, SoundSaid, Tile,
};
use ui::testing::{self, Renderer, Scene};
use ui::{
    Bitrate, CallButton, CallSize, FocusScope, Icon, ScreenCard, Surfaced, Theme, ToastSends, Tool, card_bar,
    info_panel, quality_rows, reveal, screen_card, share_panel, stats_popover, voice_tile,
};

/// The English the views are drawn in, for as long as they borrow it.
static WORDS: std::sync::LazyLock<words::Words> = std::sync::LazyLock::new(words::Words::english);

type El<'a, M> = Element<'a, M, Theme, Renderer>;

const CARD: (f32, f32) = (320.0, 200.0);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Said {
    Share,
    Choose,
    /// A press on a person's tile.
    Person,
    StopSharing,
    About,
    /// A tab of the connection popover's own video.
    Show(Video),
    Pin,
    /// A right-click on a playing screen: the popover of its sound.
    Sound,
    /// A double-click on a playing screen: it fills the whole display.
    Full,
    Again,
    Away,
    Quality(QualityPick),
    /// The share panel's custom bitrate.
    Rate(RateMessage),
}

fn share_button<'a>(control: CallControl, glyph: Icon, enabled: bool) -> El<'a, Said> {
    let mut button = CallButton::new(label("Share"), glyph, control).size(CallSize::Bar);
    if enabled {
        button = button.on_press(Said::Share);
    }
    button.view::<Renderer>()
}

fn card(decoder: Route, state: ShareState) -> ShareCard {
    ShareCard {
        who: screens::harness::member("Ada"),
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

fn outgoing(limited: bool) -> Outgoing {
    Outgoing {
        codec: "H.264".into(),
        encoder: Route::Hardware { via: "VA-API".into() },
        width: 1920,
        height: 1080,
        scaled_from: None,
        frame_rate: Some(60),
        bits_per_second: 8_000_000,
        frames: 4_210,
        key_frames_made: 2,
        key_frames_asked: 1,
        resent_bytes: 12_400,
        restarts: 1,
        limited,
        reduced: None,
    }
}

fn picture<'a>() -> El<'a, Said> {
    container(iced_widget::Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .class(Surfaced::On(Surface::Inset))
        ._boxed()
}

#[test]
fn the_share_button_in_each_state_is_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let cell = |control: CallControl, glyph: Icon, enabled: bool| {
            let on = container(share_button(control, glyph, enabled))
                .center(Length::Fill)
                .class(Surfaced::On(Surface::Inset));
            testing::render(&theme, on, Size::new(80.0, 64.0), 1.0)
        };
        ui::refs!().assert(
            &format!("share-button-{scheme:?}").to_lowercase(),
            &testing::side_by_side(&[
                cell(CallControl::Rest, Icon::ScreenShare, true),
                cell(CallControl::Rest, Icon::ScreenShare, false),
                cell(CallControl::Live, Icon::ScreenShareOff, true),
            ]),
        );
    }
}

/// The Bitrate row on a preset, which has no line under it.
fn preset(rate: &PresetOrCustom<Ceiling>) -> Bitrate<'_> {
    Bitrate { rate, note: None }
}

static EIGHT: PresetOrCustom<Ceiling> = PresetOrCustom::Preset(Ceiling::Mbit8);
static AUTOMATIC: PresetOrCustom<Ceiling> = PresetOrCustom::Preset(Ceiling::Automatic);

/// The share panel with its custom rate typed as `typed`, the line under it `note`.
fn custom_panel<'a>(typed: &'a PresetOrCustom<Ceiling>, note: words::Sentence) -> El<'a, Said> {
    let encoders = [EncoderOption { codec: Codec::H264, on: EncodesOn::GraphicsCard }];
    let chosen = ShareQuality::default();
    let bitrate = Bitrate { rate: typed, note: Some(note) };
    let rows = quality_rows::<Said, Renderer>(&chosen, &encoders, bitrate, Said::Quality, Said::Rate, &WORDS);
    share_panel::<Said, Renderer>(label("Share"), rows, label("Choose a screen or window…"), Said::Choose)
}

/// Custom's slider and field under the Bitrate row: the ceiling a rate makes said quietly under them, a rate out of
/// range refused there, the field and the slider each sending what is done to them.
#[test]
fn the_custom_bitrate_has_a_slider_and_a_field_and_says_what_it_makes_or_why_not() {
    let typed = PresetOrCustom::Custom("2.5".to_string());
    let at_most = words::Sentence::ShareRateAtMost(2_500_000);
    let out = PresetOrCustom::Custom("45".to_string());
    let range = words::Sentence::ShareRateOutOfRange { least: 500_000, most: 40_000_000, step: 500_000 };
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        for (name, rate, note) in [("custom", &typed, at_most.clone()), ("custom-refused", &out, range.clone())] {
            let on = container(custom_panel(rate, note)).center(Length::Fill).class(Surfaced::On(Surface::Base));
            ui::refs!().assert(
                &format!("share-panel-{name}-{scheme:?}").to_lowercase(),
                &testing::render(&theme, on, Size::new(420.0, 760.0), 1.0),
            );
        }
    }
    let mut typing = sim(FocusScope::new(iced_widget::Column::with_children([custom_panel(&typed, at_most)])));
    let found = semantics(&mut typing);
    let name = "Most the share sends, in Mbit/s";
    assert!(
        found
            .iter()
            .any(|each| each.role == SemanticRole::Slider { value: 5, least: 1, most: 80 }
                && each.label.as_str() == name),
        "the slider stands at 2.5 of 0.5 to 40: {found:?}"
    );
    assert!(found.iter().any(|each| each.label.as_str() == "Bitrate (Mbit/s)"), "{found:?}");
    let note = "At most 2.5 Mbit/s; less when the screen or your connection needs less.";
    assert!(typing.find(note).is_ok(), "the note says what the rate makes");
    let mut refused = sim(FocusScope::new(iced_widget::Column::with_children([custom_panel(&out, range)])));
    assert!(refused.find("Pick a rate from 0.5 to 40 Mbit/s, in steps of 0.5.").is_ok(), "the refusal is said");
}

/// Custom follows the presets, the least first, and its press asks for the custom rate.
#[test]
fn the_bitrate_row_ascends_to_automatic_then_custom_whose_press_opens_its_field() {
    let encoders = [EncoderOption { codec: Codec::H264, on: EncodesOn::GraphicsCard }];
    let chosen = ShareQuality::default();
    let rows =
        quality_rows::<Said, Renderer>(&chosen, &encoders, preset(&AUTOMATIC), Said::Quality, Said::Rate, &WORDS);
    let panel = share_panel::<Said, Renderer>(label("Share"), rows, label("Choose a screen or window…"), Said::Choose);
    let mut sim = sim_sized(panel, Size::new(420.0, 680.0));
    settle(&mut sim);
    let radios: Vec<_> = semantics(&mut sim)
        .into_iter()
        .filter(|each| matches!(each.role, SemanticRole::Radio { .. }))
        .map(|each| each.label.as_str().to_string())
        .collect();
    let bitrate = ["4", "8", "16", "Automatic", "Custom"].map(str::to_string);
    assert!(radios.windows(bitrate.len()).any(|row| row == bitrate), "{radios:?}");
    let resolutions = ["720p", "1080p", "1440p", "Source"].map(str::to_string);
    assert!(radios.windows(resolutions.len()).any(|row| row == resolutions), "{radios:?}");
    let at = boxes(&mut sim)
        .into_iter()
        .find(|(each, _)| matches!(each.role, SemanticRole::Radio { .. }) && each.label.as_str() == "Custom")
        .map(|(_, bounds)| bounds.center())
        .expect("Custom");
    sim.point_at(at);
    sim.simulate(testing::click());
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Rate(RateMessage::Custom)]);
}

#[test]
fn the_share_panel_offers_the_picker_and_sends_it_by_a_press_or_the_keyboard() {
    let encoders = || {
        vec![
            EncoderOption { codec: Codec::H264, on: EncodesOn::GraphicsCard },
            EncoderOption { codec: Codec::Av1, on: EncodesOn::GraphicsCard },
        ]
    };
    let panel = || {
        let mut chosen = ShareQuality { ceiling: Ceiling::Mbit8, ..ShareQuality::default() };
        chosen.sound.window = false;
        let rows =
            quality_rows::<Said, Renderer>(&chosen, &encoders(), preset(&EIGHT), Said::Quality, Said::Rate, &WORDS);
        share_panel::<Said, Renderer>(label("Share"), rows, label("Choose a screen or window…"), Said::Choose)
    };
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let on = container(panel()).center(Length::Fill).class(Surfaced::On(Surface::Base));
        ui::refs!().assert(
            &format!("share-panel-{scheme:?}").to_lowercase(),
            &testing::render(&theme, on, Size::new(420.0, 680.0), 1.0),
        );
    }
    let mut sim = sim(FocusScope::new(iced_widget::Column::with_children([panel()])));
    let found = semantics(&mut sim);
    assert!(found.iter().any(|each| each.role == SemanticRole::Dialog), "{found:?}");
    assert_eq!(found.iter().filter(|each| each.role == SemanticRole::RadioGroup).count(), 4, "{found:?}");
    let switches: Vec<_> = found.iter().filter(|each| matches!(each.role, SemanticRole::Switch { .. })).collect();
    assert_eq!(
        switches.iter().map(|each| (each.role, each.label.as_str())).collect::<Vec<_>>(),
        [
            (SemanticRole::Switch { on: true }, "Share system sound with a whole screen"),
            (SemanticRole::Switch { on: false }, "Share the app’s sound with a window"),
        ],
    );
    tab(&mut sim, false);
    tab(&mut sim, false);
    press(&mut sim, Named::Enter);
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Quality(QualityPick::Resolution(Resolution::P1080))]);
    let resolutions_after = Resolution::ALL.len() - 2;
    let custom = 1;
    let first_switch = resolutions_after + FrameRate::ALL.len() + Ceiling::ALL.len() + custom + Codec::ALL.len() + 1;
    for _ in 0..first_switch {
        tab(&mut sim, false);
    }
    press(&mut sim, Named::Space);
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Quality(QualityPick::ScreenSound(false))]);
    let past_the_second_switch_to_the_picker = 2;
    for _ in 0..past_the_second_switch_to_the_picker {
        tab(&mut sim, false);
    }
    press(&mut sim, Named::Enter);
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Choose]);
}

fn tools() -> Vec<Tool<Said>> {
    let tool = |icon: Icon, said: &str, message: Said| Tool { icon, label: label(said), message, held: false };
    vec![tool(Icon::Info, "About this picture", Said::About), tool(Icon::Pin, "Pin", Said::Pin)]
}

#[test]
fn a_shared_screen_is_a_card_waiting_playing_or_unshown_and_one_s_own_stops() {
    let unshown = || ScreenCard::Unshown { said: "This device cannot decode the stream.".into() };
    let own = |said: &str| ScreenCard::Own { said: said.into(), stop: Said::StopSharing };
    let playing = || ScreenCard::Playing {
        picture: picture(),
        reconnecting: None,
        menu: None,
        double_click: None,
        zoom: None,
        unheard: None,
        behind: None,
        paused: None,
    };
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let render = |title: &str, held: ScreenCard<'static, Said, Renderer>, size: (f32, f32)| {
            let card = screen_card::<Said, Renderer>(title.into(), size, held, &WORDS);
            let on = container(card).center(Length::Fill).class(Surfaced::On(Surface::Base));
            testing::render(&theme, on, Size::new(CARD.0 + 40.0, CARD.1 + 40.0), 1.0)
        };
        ui::refs!().assert(
            &format!("screen-card-{scheme:?}").to_lowercase(),
            &testing::side_by_side(&[
                render("Ada’s stream", ScreenCard::Starting { said: "Waiting for the stream.".into() }, CARD),
                render("Ada’s stream", playing(), CARD),
                render("Ada’s stream", unshown(), CARD),
                render("Your stream", ScreenCard::Starting { said: "Starting your share…".into() }, CARD),
                render("Your stream", own("Your stream can’t be shown on this device."), CARD),
                render("Ada’s stream", unshown(), design::STRIP_TILE),
            ]),
        );
    }
    let press_through = |held: ScreenCard<'static, Said, Renderer>, stops: usize| {
        let card = screen_card::<Said, Renderer>("Ada’s stream".into(), CARD, held, &WORDS);
        let mut sim = sim_sized(FocusScope::new(Row::with_children([card])), Size::new(360.0, 240.0));
        for _ in 0..stops {
            tab(&mut sim, false);
            press(&mut sim, Named::Enter);
        }
        sim.drain().collect::<Vec<_>>()
    };
    assert_eq!(press_through(unshown(), 1), [], "someone's screen asks nothing: it plays, or says why not");
    assert_eq!(
        press_through(own("Your stream can’t be shown on this device."), 1),
        [Said::StopSharing],
        "and this account's own card, showing nothing, stops the share"
    );
}

#[test]
fn a_watched_screen_that_reconnects_is_dimmed_with_a_circle_and_the_words() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let reconnecting = Some("Reconnecting to Ada’s stream…".to_string());
        let card = screen_card::<Said, Renderer>(
            "Ada’s stream".into(),
            CARD,
            ScreenCard::Playing {
                picture: picture(),
                reconnecting,
                menu: None,
                double_click: None,
                zoom: None,
                unheard: None,
                behind: None,
                paused: None,
            },
            &WORDS,
        );
        let on = container(card).center(Length::Fill).class(Surfaced::On(Surface::Base));
        ui::refs!().assert(
            &format!("screen-card-reconnecting-{scheme:?}").to_lowercase(),
            &testing::render(&theme, on, Size::new(CARD.0 + 40.0, CARD.1 + 40.0), 1.0),
        );
    }
}

/// This account's own screen card, its bar shown.
fn own_barred<'a>() -> El<'a, Said> {
    let tool = |icon: Icon, said: &str, message: Said| Tool { icon, label: label(said), message, held: false };
    let tools = vec![tool(Icon::ScreenShareOff, "Stop sharing", Said::StopSharing), tool(Icon::Pin, "Pin", Said::Pin)];
    let card = screen_card::<Said, Renderer>(
        "Your stream".into(),
        CARD,
        ScreenCard::Playing {
            picture: picture(),
            reconnecting: None,
            menu: None,
            double_click: None,
            zoom: None,
            unheard: None,
            behind: None,
            paused: None,
        },
        &WORDS,
    );
    reveal(card, card_bar(tools, None, CARD.1), true)
}

/// A playing screen with a sound opens its popover on a right-click; a left click, or one with none, asks nothing,
/// and the card is no stop for Tab: its bar's Volume is.
#[test]
fn a_right_click_on_a_playing_screen_with_a_sound_opens_its_popover() {
    let clicked = |menu: Option<Said>| {
        let held = ScreenCard::Playing {
            picture: picture(),
            reconnecting: None,
            menu,
            double_click: None,
            zoom: None,
            unheard: None,
            behind: None,
            paused: None,
        };
        let card = screen_card::<Said, Renderer>("Ada’s stream".into(), CARD, held, &WORDS);
        let mut sim = sim_sized(FocusScope::new(Row::with_children([card])), Size::new(360.0, 240.0));
        assert_eq!(common::focusable_total(&mut sim), 0);
        sim.point_at(Point::new(CARD.0 / 2.0, CARD.1 / 2.0));
        sim.simulate(testing::click());
        assert!(sim.drain().next().is_none(), "a left click opens nothing");
        sim.simulate(testing::right_click());
        sim.drain().collect::<Vec<_>>()
    };
    assert_eq!(clicked(Some(Said::Sound)), [Said::Sound]);
    assert_eq!(clicked(None), [], "a screen with no sound has no popover");
}

/// A double-click on a playing screen's picture sends it to the whole display; one on its bar's button presses that
/// button twice and asks nothing more.
#[test]
fn a_watched_screens_sound_not_carried_is_said_over_its_picture_beside_its_name() {
    let said = "The server can’t send you this stream’s sound now.";
    let held = ScreenCard::Playing {
        picture: picture(),
        reconnecting: None,
        menu: None,
        double_click: None,
        zoom: None,
        unheard: Some(said.into()),
        behind: None,
        paused: None,
    };
    let card = screen_card::<Said, Renderer>("Ada’s stream".into(), CARD, held, &WORDS);
    let mut sim = sim_sized(card, Size::new(CARD.0, CARD.1));
    assert!(sim.find(said).is_ok(), "the line is on the card");
    assert!(sim.find(Plain("Ada’s stream")).is_ok(), "beside whose screen it is");
}

/// A stream this computer cannot keep up with says so over its picture, above its name and above a note on its sound.
#[test]
fn a_watched_stream_this_computer_cannot_keep_up_with_says_so_above_its_name() {
    let behind = "This computer can’t keep up with this stream.";
    let unheard = "The server can’t send you this stream’s sound now.";
    let playing = |unheard: Option<&str>| ScreenCard::Playing {
        picture: picture(),
        reconnecting: None,
        menu: None,
        double_click: None,
        zoom: None,
        unheard: unheard.map(Into::into),
        behind: Some(behind.into()),
        paused: None,
    };
    let card = screen_card::<Said, Renderer>("Ada’s stream".into(), CARD, playing(Some(unheard)), &WORDS);
    let mut sim = sim_sized(card, Size::new(CARD.0, CARD.1));
    let mut top = |said: &str| sim.find(said).map(|found| found.bounds().y).expect("on the card");
    let (behind_at, unheard_at, name_at) = (top(behind), top(unheard), top("Ada’s stream"));
    assert!(behind_at < unheard_at && unheard_at < name_at, "behind, then the sound, then the name");
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let render = |held: ScreenCard<'static, Said, Renderer>| {
            let card = screen_card::<Said, Renderer>("Ada’s stream".into(), CARD, held, &WORDS);
            let on = container(card).center(Length::Fill).class(Surfaced::On(Surface::Base));
            testing::render(&theme, on, Size::new(CARD.0 + 40.0, CARD.1 + 40.0), 1.0)
        };
        ui::refs!().assert(
            &format!("screen-card-behind-{scheme:?}").to_lowercase(),
            &testing::side_by_side(&[render(playing(None)), render(playing(Some(unheard)))]),
        );
    }
}

#[test]
fn a_double_click_on_a_watched_screen_goes_full_screen_and_a_bar_press_does_not() {
    let double = |at: Point| {
        let held = ScreenCard::Playing {
            picture: picture(),
            reconnecting: None,
            menu: None,
            double_click: Some(Said::Full),
            zoom: None,
            unheard: None,
            behind: None,
            paused: None,
        };
        let card = screen_card::<Said, Renderer>("Ada’s stream".into(), CARD, held, &WORDS);
        let mut sim = sim_sized(reveal(card, card_bar(tools(), None, CARD.1), true), Size::new(CARD.0, CARD.1));
        sim.point_at(at);
        sim.simulate(testing::click());
        sim.simulate(testing::click());
        sim.drain().collect::<Vec<_>>()
    };
    assert_eq!(double(Point::new(CARD.0 / 2.0, CARD.1 / 2.0)), [Said::Full]);
    let pin = {
        let held = ScreenCard::Playing {
            picture: picture(),
            reconnecting: None,
            menu: None,
            double_click: None,
            zoom: None,
            unheard: None,
            behind: None,
            paused: None,
        };
        let card = screen_card::<Said, Renderer>("Ada’s stream".into(), CARD, held, &WORDS);
        let mut sim = sim_sized(reveal(card, card_bar(tools(), None, CARD.1), true), Size::new(CARD.0, CARD.1));
        boxes(&mut sim).into_iter().find(|(said, _)| said.label.as_str() == "Pin").expect("the bar shows").1
    };
    assert_eq!(double(pin.center()), [Said::Pin, Said::Pin], "the bar's own button, twice");
}

/// `person`'s tile with a bar of controls over it, held shown while `held`.
fn barred<'a>(person: &'a Tile, held: bool) -> El<'a, Said> {
    reveal(
        voice_tile::<Said, Renderer>(person, CARD, None, None, false, None, &WORDS),
        card_bar(tools(), None, CARD.1),
        held,
    )
}

#[test]
fn a_cards_controls_show_while_the_pointer_is_over_it_and_not_otherwise() {
    let person = Tile::new("Ada");
    let size = Size::new(360.0, 240.0);
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let render = |scene: &Scene| {
            let on = container(barred(&person, false)).center(Length::Fill).class(Surfaced::On(Surface::Base));
            testing::render_scene(&theme, on, size, 1.0, scene)
        };
        let away = render(&Scene::default());
        let over = render(&Scene::hovering(Point::new(60.0, 120.0)));
        assert!(away.rgba != over.rgba, "the pointer over the card shows its bar");
        let own = testing::render(
            &theme,
            container(own_barred()).center(Length::Fill).class(Surfaced::On(Surface::Base)),
            size,
            1.0,
        );
        ui::refs!().assert(&format!("card-bar-{scheme:?}").to_lowercase(), &testing::side_by_side(&[away, over, own]));
    }
}

#[test]
fn a_hidden_bar_is_reached_by_the_keyboard_and_named_only_once_shown() {
    let person = Tile::new("Ada");
    let mut sim = sim_sized(FocusScope::new(Row::with_children([barred(&person, false)])), Size::new(360.0, 240.0));
    let named = |found: &[design::Semantics], said: &str| found.iter().any(|each| each.label.as_str() == said);
    assert!(!named(&semantics(&mut sim), "Pin"), "a control nobody sees is not read out");
    tab(&mut sim, false);
    // The scope moves the focus after the card heard the key; the redraw that follows shows the bar, as in the window.
    settle(&mut sim);
    assert!(named(&semantics(&mut sim), "About this picture"), "the keyboard in the bar shows it");
    tab(&mut sim, false);
    press(&mut sim, Named::Enter);
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Pin], "Tab reaches the bar's controls in order");

    let mut held = sim_sized(FocusScope::new(Row::with_children([barred(&person, true)])), Size::new(360.0, 240.0));
    assert!(named(&semantics(&mut held), "Pin"), "a bar held open by its popover stays");
}

#[test]
fn a_bar_a_click_left_focused_hides_when_the_pointer_leaves() {
    use iced_core::mouse;
    let person = Tile::new("Ada");
    let mut sim = sim_sized(FocusScope::new(Row::with_children([barred(&person, false)])), Size::new(360.0, 240.0));
    let move_to = |sim: &mut common::Sim<'_, Said>, at: Point| {
        sim.point_at(at);
        sim.simulate([iced_core::Event::Mouse(mouse::Event::CursorMoved { position: at })]);
        settle(sim);
    };
    move_to(&mut sim, Point::new(60.0, 120.0));
    let pin = boxes(&mut sim).into_iter().find(|(said, _)| said.label.as_str() == "Pin").expect("the bar shows").1;
    move_to(&mut sim, pin.center());
    sim.simulate([
        iced_core::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        iced_core::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
    ]);
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Pin]);
    move_to(&mut sim, Point::new(350.0, 230.0));
    let named = |found: &[(design::Semantics, iced_core::Rectangle)]| {
        found.iter().any(|(said, _)| said.label.as_str() == "Pin")
    };
    assert!(!named(&boxes(&mut sim)), "the focus the click left does not keep the bar up");
}

#[test]
fn what_the_picture_is_is_said_in_plain_words_and_a_fallback_is_marked() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let one = |decoder: Route| {
            let panel = info_panel::<Said, Renderer>(
                &About::watched(&card(decoder, ShareState::Live), "Ada", &WORDS),
                None,
                &WORDS,
            );
            let on = container(panel).center(Length::Fill).class(Surfaced::On(Surface::Base));
            testing::render(&theme, on, Size::new(320.0, 150.0), 1.0)
        };
        ui::refs!().assert(
            &format!("picture-info-{scheme:?}").to_lowercase(),
            &testing::stacked(&[one(Route::Hardware { via: "VA-API".into() }), one(Route::Software)]),
        );
    }
}

/// Where no graphics card here makes the codec chosen, the codec row says it is software encoded.
#[test]
fn a_codec_the_processor_encodes_says_so_under_the_codec_row() {
    let encoders = [
        EncoderOption { codec: Codec::H264, on: EncodesOn::GraphicsCard },
        EncoderOption { codec: Codec::Av1, on: EncodesOn::Processor },
    ];
    let chosen = ShareQuality { codec: Some(Codec::Av1), ..ShareQuality::default() };
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let rows =
            quality_rows::<Said, Renderer>(&chosen, &encoders, preset(&AUTOMATIC), Said::Quality, Said::Rate, &WORDS);
        let panel =
            share_panel::<Said, Renderer>(label("Share"), rows, label("Choose a screen or window…"), Said::Choose);
        let on = container(panel).center(Length::Fill).class(Surfaced::On(Surface::Base));
        ui::refs!().assert(
            &format!("share-panel-processor-{scheme:?}").to_lowercase(),
            &testing::render(&theme, on, Size::new(420.0, 680.0), 1.0),
        );
    }
}

#[test]
fn ones_own_screen_names_its_encoder_and_a_note_follows_when_there_is_one() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let one = |about: About| {
            let on = container(info_panel::<Said, Renderer>(&about, None, &WORDS))
                .center(Length::Fill)
                .class(Surfaced::On(Surface::Base));
            testing::render(&theme, on, Size::new(320.0, 190.0), 1.0)
        };
        let dropped = ShareCard { dropped: 37, ..card(Route::Hardware { via: "VA-API".into() }, ShareState::Live) };
        ui::refs!().assert(
            &format!("picture-info-notes-{scheme:?}").to_lowercase(),
            &testing::stacked(&[
                one(About::sent(&outgoing(false), Some(&SoundSaid::App("Brave".into())), &WORDS)),
                one(About::sent(&outgoing(true), Some(&SoundSaid::Everything), &WORDS)),
                one(About::sent_camera(
                    OwnWatch::Watched,
                    &Outgoing { limited: true, ..camera(Route::Hardware { via: "VA-API".into() }) },
                    &WORDS,
                )),
                one(About::watched(&dropped, "Ada", &WORDS)),
            ]),
        );
    }
}

/// The connection popover with `own` video, its tabs sending the video they show.
fn popover_of<'a>(own: Option<OwnVideo<'a>>) -> El<'a, Said> {
    let (health, trips) = (Health::default(), screens::RoundTrips::default());
    stats_popover::<Said, Renderer>(label("Connection"), Some(&health), &trips, false, own, Said::Show, &WORDS)
}

#[test]
fn the_stats_popover_has_a_section_for_this_accounts_own_share_only_while_it_shares() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let one = |share: Outgoing| {
            let on = container(popover_of(Some(OwnVideo::Alone(Video::Screen, &share, OwnWatch::Watched))))
                .center(Length::Fill)
                .class(Surfaced::On(Surface::Base));
            testing::render(&theme, on, Size::new(340.0, 540.0), 1.0)
        };
        ui::refs!().assert(
            &format!("connection-popover-sharing-{scheme:?}").to_lowercase(),
            &testing::side_by_side(&[one(outgoing(false)), one(outgoing(true))]),
        );
    }
}

/// One's camera at 720p30, as the call looks at it; `encoder` is how it is encoded.
fn camera(encoder: Route) -> Outgoing {
    Outgoing {
        codec: "H.264".into(),
        encoder,
        width: 1280,
        height: 720,
        scaled_from: None,
        frame_rate: Some(30),
        bits_per_second: 1_500_000,
        frames: 54_210,
        key_frames_made: 1,
        key_frames_asked: 0,
        resent_bytes: 0,
        restarts: 0,
        limited: false,
        reduced: None,
    }
}

#[test]
fn the_stats_popover_has_a_section_for_this_accounts_own_camera_which_says_a_software_encoder() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let one = |own: Outgoing| {
            let on = container(popover_of(Some(OwnVideo::Alone(Video::Camera, &own, OwnWatch::Watched))))
                .center(Length::Fill)
                .class(Surfaced::On(Surface::Base));
            testing::render(&theme, on, Size::new(340.0, 540.0), 1.0)
        };
        ui::refs!().assert(
            &format!("connection-popover-camera-{scheme:?}").to_lowercase(),
            &testing::side_by_side(&[
                one(camera(Route::Hardware { via: "VA-API".into() })),
                one(camera(Route::Software)),
            ]),
        );
    }
}

#[test]
fn sharing_with_the_camera_on_shows_a_tab_each_and_the_picked_one_s_rows_at_the_height_of_one() {
    let (share, own) = (outgoing(false), camera(Route::Software));
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let one = |shown: Video| {
            let outgoing = if shown == Video::Camera { &own } else { &share };
            let on = container(popover_of(Some(OwnVideo::Tabbed { shown, outgoing, watch: OwnWatch::Watched })))
                .center(Length::Fill)
                .class(Surfaced::On(Surface::Base));
            testing::render(&theme, on, Size::new(340.0, 540.0), 1.0)
        };
        ui::refs!().assert(
            &format!("connection-popover-both-{scheme:?}").to_lowercase(),
            &testing::side_by_side(&[one(Video::Screen), one(Video::Camera)]),
        );
    }
    let tall = |own: Option<OwnVideo<'_>>| {
        sim_sized(popover_of(own), Size::new(340.0, 900.0)).find(Plain("THIS CALL")).map(|text| text.bounds().y)
    };
    let alone = tall(Some(OwnVideo::Alone(Video::Screen, &share, OwnWatch::Watched))).expect("its totals");
    let tabbed = tall(Some(OwnVideo::Tabbed { shown: Video::Camera, outgoing: &own, watch: OwnWatch::Watched }))
        .expect("its totals");
    assert!(tabbed - alone < 24.0, "the tabs cost no more than a heading's height again: {alone} and {tabbed}");
}

#[test]
fn a_tab_of_one_s_own_video_sends_the_video_it_shows_and_names_the_group_for_a_screen_reader() {
    let share = outgoing(false);
    let mut sim = sim_sized(
        popover_of(Some(OwnVideo::Tabbed { shown: Video::Screen, outgoing: &share, watch: OwnWatch::Watched })),
        Size::new(340.0, 900.0),
    );
    let found = semantics(&mut sim);
    assert!(
        found.iter().any(|each| each.role == SemanticRole::RadioGroup && each.label.as_str() == "Your video"),
        "{found:?}"
    );
    sim.click(Plain("Your camera")).expect("the camera's tab");
    assert_eq!(sim.into_messages().collect::<Vec<_>>(), [Said::Show(Video::Camera)]);
}

#[test]
fn a_toast_that_offers_to_share_again_has_its_button_beside_the_sentence() {
    let telling = screens::Telling::of(design::Status::Danger, words::Sentence::Phrase(words::Phrase::ScreenSilent))
        .offering(Action::ShareAgain);
    let toast = screens::harness::toast(telling, std::time::Instant::now());
    let drawn = || {
        let sends = ToastSends { act: Said::Again, away: Said::Away, held: |_| Said::Away };
        ui::toast::<Said, Renderer>(&toast, &sends, &WORDS)
    };
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let on = container(drawn()).padding(12.0).class(Surfaced::On(Surface::Base));
        ui::refs!().assert(
            &format!("toast-share-again-{scheme:?}").to_lowercase(),
            &testing::render(&theme, on, Size::new(560.0, 70.0), 1.0),
        );
    }
    let mut sim = sim_sized(FocusScope::new(drawn()), Size::new(560.0, 70.0));
    tab(&mut sim, false);
    press(&mut sim, Named::Enter);
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Again], "the action comes first, then the ✕");
}

/// A card the keyboard reached shows its bar as it does under the pointer, and draws nothing round itself.
#[test]
fn a_card_tab_reached_shows_its_bar_as_under_the_pointer() {
    let person = Tile::new("Ada");
    let size = Size::new(360.0, 240.0);
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let render = |scene: &Scene| {
            let tile = voice_tile::<Said, Renderer>(&person, CARD, None, None, false, Some(Said::Person), &WORDS);
            let card = FocusScope::new(Row::with_children([reveal(tile, card_bar(tools(), None, CARD.1), false)]));
            let on = container(card).center(Length::Fill).class(Surfaced::On(Surface::Base));
            testing::render_scene(&theme, on, size, 1.0, scene)
        };
        let hovered = render(&Scene::hovering(Point::new(size.width / 2.0, size.height / 2.0)));
        let tabbed = render(&Scene::tabbing(false));
        let differs = testing::compare(&tabbed, &hovered).expect("the same size");
        assert!(differs.within_tolerance(), "{scheme:?}: {differs:?}");
    }
}
