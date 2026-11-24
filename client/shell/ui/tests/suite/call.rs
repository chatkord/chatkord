//! What a call is made of beside its tiles, each a reference in both schemes: the colours that carry meaning are
//! measured from the pixels, and what a keyboard and a screen reader reach is asked of the simulator.

use crate::common;

use common::{focusable_total, label, press, semantics, sim, tab};
use design::{
    Accent, AccentRole, CallControl, Content, DestructiveRole, Popup, Role, Scheme, SemanticRole, Status, StatusRole,
    Surface,
};
use iced_core::keyboard::key::Named;
use iced_core::{Element, Length, Point, Size};
use iced_widget::{Column, Row, container};
use screens::{EndCard, Flow, Health, Quality, RoundTrips, Tile, WayBack};
use std::time::Duration;
use ui::testing::{self, Image, Renderer, Scene};
use ui::{
    CallButton, CallSize, DeviceSection, FocusScope, GraphWords, Icon, MenuChoice, Surfaced, Theme, bar_divider,
    device_menu, end_card, line_graph, seat_row, stage_bar, stats_popover,
};
use words::Sentence;

/// The English the views are drawn in, for as long as they borrow it.
static WORDS: std::sync::LazyLock<words::Words> = std::sync::LazyLock::new(words::Words::english);

type El<'a, M> = Element<'a, M, Theme, Renderer>;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Said {
    Pressed,
    Switch,
    Back,
    Arrow,
    Chose(&'static str),
}

fn count_near(image: &Image, colour: design::Rgb, tolerance: u8) -> usize {
    image
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| {
            p[0].abs_diff(colour.r) <= tolerance
                && p[1].abs_diff(colour.g) <= tolerance
                && p[2].abs_diff(colour.b) <= tolerance
        })
        .count()
}

fn control<'a>(control: CallControl, size: CallSize, enabled: bool) -> El<'a, Said> {
    let wide = control == CallControl::Leave;
    let glyph = if matches!(control, CallControl::Signal { .. }) { Icon::Signal } else { Icon::MicOff };
    let mut button = CallButton::new(label("Mute"), glyph, control).size(size);
    if wide {
        button = button.wide();
    }
    if control == CallControl::Failed {
        button = button.failed(label("The microphone could not be opened."));
    }
    if enabled {
        button = button.on_press(Said::Pressed);
    }
    button.view::<Renderer>()
}

const CELL: Size = Size::new(80.0, 64.0);

#[test]
fn every_call_control_in_each_state_is_the_reference_in_both_schemes() {
    let centre = Point::new(CELL.width / 2.0, CELL.height / 2.0);
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        for (name, size) in [("bar", CallSize::Bar), ("pane", CallSize::Pane)] {
            let cell = |kind: CallControl, scene: &Scene, enabled: bool| {
                let on = container(FocusScope::new(control(kind, size, enabled)))
                    .center(Length::Fill)
                    .class(Surfaced::On(Surface::Inset));
                testing::render_scene(&theme, on, CELL, 1.0, scene)
            };
            let mut row = Vec::new();
            for kind in CallControl::ALL {
                row.push(cell(kind, &Scene::default(), true));
                row.push(cell(kind, &Scene::hovering(centre), true));
            }
            row.push(cell(CallControl::Rest, &Scene::default(), false));
            row.push(cell(CallControl::Rest, &Scene::tabbing(false), true));
            ui::refs!().assert(&format!("call-buttons-{name}-{scheme:?}").to_lowercase(), &testing::side_by_side(&row));
        }
        let palette = theme.palette();
        let idle = |kind| {
            let on =
                container(control(kind, CallSize::Bar, true)).center(Length::Fill).class(Surfaced::On(Surface::Inset));
            testing::render(&theme, on, CELL, 1.0)
        };
        assert!(
            count_near(&idle(CallControl::Off), palette.color(Role::Status(Status::Danger, StatusRole::Tint)), 2) > 800,
            "off is tinted red"
        );
        assert!(
            count_near(&idle(CallControl::Live), palette.color(Role::Accent(AccentRole::Tint)), 2) > 800,
            "live is tinted in the accent"
        );
        assert!(
            count_near(&idle(CallControl::Leave), palette.color(Role::Destructive(DestructiveRole::Fill)), 2) > 1000,
            "leaving is solid red"
        );
        assert!(
            count_near(&idle(CallControl::Rest), palette.color(Role::Surface(Surface::Hover)), 2) > 800,
            "at rest is the hover surface"
        );
    }
}

#[test]
fn a_call_control_is_pressed_by_the_keyboard_and_a_dimmed_one_takes_no_press() {
    let mut sim = sim(FocusScope::new(Column::with_children([control(CallControl::Rest, CallSize::Bar, true)])));
    assert_eq!(focusable_total(&mut sim), 1);
    tab(&mut sim, false);
    press(&mut sim, Named::Space);
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Pressed]);
    let found = semantics(&mut sim);
    assert!(found.iter().any(|each| each.role == SemanticRole::Button && each.label.as_str() == "Mute"), "{found:?}");

    let mut dimmed =
        common::sim(FocusScope::new(Column::with_children([control(CallControl::Rest, CallSize::Bar, false)])));
    tab(&mut dimmed, false);
    press(&mut dimmed, Named::Space);
    assert_eq!(dimmed.drain().count(), 0);
}

#[test]
fn the_call_bar_is_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let bar = stage_bar::<Said, Renderer>(vec![
            control(CallControl::Off, CallSize::Bar, true),
            control(CallControl::Rest, CallSize::Bar, true),
            bar_divider(),
            control(CallControl::Leave, CallSize::Bar, true),
        ]);
        let on = container(bar).center(Length::Fill).class(Surfaced::On(Surface::Base));
        let image = testing::render(&theme, on, Size::new(300.0, 90.0), 1.0);
        ui::refs!().assert(&format!("call-bar-{scheme:?}").to_lowercase(), &image);
    }
}

fn seat(edit: impl FnOnce(&mut Tile)) -> Tile {
    let mut tile = Tile::new("Grace Hopper");
    edit(&mut tile);
    tile
}

#[test]
fn a_person_listed_under_a_channel_is_the_reference_and_says_where_they_are() {
    let seats = [
        seat(|t| t.own = true),
        seat(|t| t.speaking = true),
        seat(|t| t.muted = true),
        seat(|t| (t.muted, t.deafened) = (true, true)),
        seat(|t| t.sharing = true),
        seat(|t| {
            t.own = true;
            t.elsewhere = true;
        }),
    ];
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let rows: Vec<El<'_, Said>> =
            seats.iter().map(|each| seat_row::<Said, Renderer>(each, None, None, None, &WORDS)).collect();
        let list = container(Column::with_children(rows)).width(Length::Fill).class(Surfaced::On(Surface::Raised));
        let image = testing::render(&theme, list, Size::new(240.0, 6.0 * 32.0), 1.0);
        ui::refs!().assert(&format!("seat-rows-{scheme:?}").to_lowercase(), &image);
    }
    let mut here = sim(seat_row::<Said, Renderer>(&seats[5], None, Some(Said::Pressed), None, &WORDS));
    let found = semantics(&mut here);
    assert!(
        found.iter().any(|each| each.role == SemanticRole::ListItem && each.label.as_str().ends_with(", other device")),
        "{found:?}"
    );
}

#[test]
fn only_a_seat_with_something_to_do_is_a_control() {
    let tile = seat(|_| {});
    let mut plain =
        sim(FocusScope::new(Row::with_children([seat_row::<Said, Renderer>(&tile, None, None, None, &WORDS)])));
    assert_eq!(focusable_total(&mut plain), 0, "shown, and Tab passes it");
    let mut taking = common::sim(FocusScope::new(Row::with_children([seat_row::<Said, Renderer>(
        &tile,
        None,
        Some(Said::Pressed),
        None,
        &WORDS,
    )])));
    assert_eq!(focusable_total(&mut taking), 1);
}

/// Someone else's row opens their popover on a right-click, or Enter once Tab reaches it; a left click does nothing.
#[test]
fn a_seat_opens_its_persons_popover_on_a_right_click_and_by_the_keyboard() {
    let tile = seat(|_| {});
    let row = || {
        FocusScope::new(Row::with_children([seat_row::<Said, Renderer>(&tile, None, None, Some(Said::Arrow), &WORDS)]))
    };
    let mut sim_row = common::sim(row());
    sim_row.point_at(iced_core::Point::new(60.0, 16.0));
    sim_row.simulate(testing::click());
    assert!(sim_row.drain().next().is_none(), "a left click opens nothing");
    sim_row.simulate(testing::right_click());
    assert_eq!(sim_row.drain().collect::<Vec<_>>(), [Said::Arrow]);
    let mut keyed = common::sim(row());
    assert_eq!(focusable_total(&mut keyed), 1, "Tab reaches it");
    common::tab(&mut keyed, false);
    common::press(&mut keyed, iced_core::keyboard::key::Named::Enter);
    assert_eq!(keyed.drain().collect::<Vec<_>>(), [Said::Arrow]);
}

fn card(way_back: Option<WayBack>) -> EndCard {
    let (title, body) = match way_back {
        Some(WayBack::SwitchHere) => ("You left Lounge", "This account joined voice from another device."),
        Some(WayBack::JoinAgain) => ("You’re out of Lounge", "An admin took you out of Lounge."),
        None => ("Lounge was deleted", "You left the call when the channel was deleted."),
    };
    EndCard { title: Sentence::Said(title.into()), body: Sentence::Said(body.into()), way_back }
}

#[test]
fn the_card_of_a_call_that_is_over_is_the_reference_and_a_move_or_a_removal_offers_the_way_back() {
    let cards = [Some(WayBack::SwitchHere), Some(WayBack::JoinAgain), None];
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let row: Vec<_> = cards
            .into_iter()
            .map(|way| {
                let on = container(end_card::<Said, Renderer>(&card(way), Said::Switch, Said::Back, false, &WORDS))
                    .class(Surfaced::On(Surface::Base));
                testing::render(&theme, on, Size::new(520.0, 360.0), 1.0)
            })
            .collect();
        ui::refs!().assert(&format!("end-card-{scheme:?}").to_lowercase(), &testing::side_by_side(&row));
    }
    let reach = |way| {
        let mut sim = common::sim_sized(
            FocusScope::new(end_card::<Said, Renderer>(&card(way), Said::Switch, Said::Back, false, &WORDS)),
            Size::new(520.0, 360.0),
        );
        let total = focusable_total(&mut sim);
        tab(&mut sim, false);
        press(&mut sim, Named::Space);
        (total, sim.drain().collect::<Vec<_>>())
    };
    assert_eq!(reach(Some(WayBack::SwitchHere)), (2, vec![Said::Switch]), "the seat first, then the way back");
    assert_eq!(reach(Some(WayBack::JoinAgain)), (2, vec![Said::Switch]), "joining again first, then the way back");
    assert_eq!(reach(None), (1, vec![Said::Back]));
    let _ = (Content::Primary, ());
}

fn split<'a>(kind: CallControl, enabled: bool) -> El<'a, Said> {
    let mut button = CallButton::new(label("Mute"), Icon::Mic, kind).size(CallSize::Bar).menu(
        label("Microphone and speaker"),
        Said::Arrow,
        false,
    );
    if enabled {
        button = button.on_press(Said::Pressed);
    }
    button.view::<Renderer>()
}

#[test]
fn a_control_with_an_arrow_is_the_reference_in_both_schemes() {
    let cell = Size::new(120.0, 64.0);
    let centre = Point::new(cell.width / 2.0, cell.height / 2.0);
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let one = |kind, enabled, scene: &Scene| {
            let on = container(FocusScope::new(split(kind, enabled)))
                .center(Length::Fill)
                .class(Surfaced::On(Surface::Inset));
            testing::render_scene(&theme, on, cell, 1.0, scene)
        };
        let row = [
            one(CallControl::Rest, true, &Scene::default()),
            one(CallControl::Rest, true, &Scene::hovering(centre)),
            one(CallControl::Off, true, &Scene::default()),
            one(CallControl::Rest, false, &Scene::default()),
            one(CallControl::Rest, true, &Scene::tabbing(false)),
        ];
        ui::refs!().assert(&format!("call-split-{scheme:?}").to_lowercase(), &testing::side_by_side(&row));
    }
}

#[test]
fn the_arrow_beside_a_control_is_its_own_target_named_for_what_it_opens_and_dimmed_with_the_control() {
    let mut sim = sim(FocusScope::new(Column::with_children([split(CallControl::Rest, true)])));
    assert_eq!(focusable_total(&mut sim), 2, "the control and its arrow");
    tab(&mut sim, false);
    tab(&mut sim, false);
    press(&mut sim, Named::Space);
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Arrow]);
    let found = semantics(&mut sim);
    let opens = SemanticRole::Opens { layer: Popup::Menu, open: false };
    for (role, name) in [(SemanticRole::Button, "Mute"), (opens, "Microphone and speaker")] {
        assert!(found.iter().any(|each| each.role == role && each.label.as_str() == name), "{found:?}");
    }
    let mut dimmed = common::sim(FocusScope::new(Column::with_children([split(CallControl::Rest, false)])));
    assert_eq!(focusable_total(&mut dimmed), 0, "neither part takes a press until the call has joined");
}

fn devices<'a>(follows: &str) -> El<'a, Said> {
    let rows = |chosen: usize, pick: [&'static str; 3]| {
        ["Default", "Headset microphone", "Laptop microphone"]
            .into_iter()
            .enumerate()
            .map(|(at, name)| MenuChoice {
                leading: None,
                label: label(name),
                note: (at == 0).then(|| label(follows)),
                chosen: at == chosen,
                message: Said::Chose(pick[at]),
            })
            .collect()
    };
    device_menu::<Said, Renderer>(
        label("Microphone and speaker"),
        vec![
            DeviceSection { heading: label("Microphone"), rows: rows(1, ["mic default", "mic headset", "mic laptop"]) },
            DeviceSection { heading: label("Speaker"), rows: rows(0, ["spk default", "spk headset", "spk laptop"]) },
        ],
    )
}

#[test]
fn the_device_menu_is_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let one = |follows: &str| {
            let on = container(devices(follows)).center(Length::Fill).class(Surfaced::On(Surface::Base));
            testing::render(&theme, on, Size::new(340.0, 440.0), 1.0)
        };
        // A real device's name is long: it wraps, and the word "Default" beside it is whole.
        let long = "Built-in Audio Analogue Stereo with a name as long as a driver makes one";
        ui::refs!().assert(
            &format!("device-menu-{scheme:?}").to_lowercase(),
            &testing::side_by_side(&[one("Laptop microphone"), one(long)]),
        );
    }
}

#[test]
fn the_device_menu_is_a_menu_whose_rows_are_chosen_by_the_keyboard() {
    let mut sim = sim(FocusScope::new(Column::with_children([devices("Laptop microphone")])));
    let found = semantics(&mut sim);
    assert!(
        found.iter().any(|each| each.role == SemanticRole::Menu && each.label.as_str() == "Microphone and speaker")
    );
    assert_eq!(
        found.iter().filter(|each| matches!(each.role, SemanticRole::MenuItemRadio { .. })).count(),
        6,
        "three microphones and three speakers, nothing else: {found:?}"
    );
    const SPEAKERS_DEFAULT_ROW: usize = 4;
    for _ in 0..SPEAKERS_DEFAULT_ROW {
        tab(&mut sim, false);
    }
    press(&mut sim, Named::Space);
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Chose("spk default")]);
}

fn bars<'a>(status: Option<Status>, open: bool) -> El<'a, Said> {
    CallButton::new(label("Connection"), Icon::Signal, CallControl::Signal { status, open })
        .size(CallSize::Pane)
        .on_press(Said::Pressed)
        .view::<Renderer>()
}

#[test]
fn the_connection_bars_are_drawn_in_the_colour_of_the_verdict_and_quiet_before_one() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let palette = theme.palette();
        let drawn = |status| {
            let on = container(bars(status, false)).center(Length::Fill).class(Surfaced::On(Surface::Base));
            testing::render(&theme, on, CELL, 1.0)
        };
        let [good, fair, poor] = [Status::Success, Status::Warning, Status::Danger].map(|status| {
            let colour = palette.color(Role::Status(status, StatusRole::Text));
            (drawn(Some(status)), colour)
        });
        for (image, colour) in [&good, &fair, &poor] {
            assert!(count_near(image, *colour, 24) > 8, "{scheme:?}: the bars are in the status colour");
        }
        for (at, (image, _)) in [&good, &fair, &poor].into_iter().enumerate() {
            for (other, (_, colour)) in [&good, &fair, &poor].into_iter().enumerate().filter(|(other, _)| *other != at)
            {
                assert_eq!(
                    count_near(image, *colour, 8),
                    0,
                    "{scheme:?}: bars {at} show none of grade {other}'s colour"
                );
            }
        }
        let quiet = palette.color(Role::Content(Content::Secondary));
        assert!(count_near(&drawn(None), quiet, 24) > 8, "{scheme:?}: quiet before there is a verdict");
    }
}

fn flow(bits_per_second: u64, packets: u64) -> Flow {
    Flow { bits_per_second, packets }
}

/// A look at how the call is: what the canvas draws for each state of its connection popover.
fn look(quality: Option<Quality>, round_trip: Option<u64>, lost: u64) -> Health {
    Health {
        round_trip: round_trip
            .map(|millis| screens::RoundTrip { took: Duration::from_millis(millis), on: screens::MeasuredOn::Route }),
        quality,
        sending: flow(48_000, 12_408),
        receiving: flow(96_000, 24_117),
        lost,
        up_bytes: 12_400_000,
        down_bytes: 48_100_000,
    }
}

#[test]
fn the_connection_popover_is_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let one = |health: Option<Health>, alone: bool| {
            let mut trips = RoundTrips::default();
            let looks = health.and_then(|health| health.quality).map(testing::canvas_round_trips).unwrap_or_default();
            looks.into_iter().for_each(|trip| trips.push(Some(trip)));
            let popover = stats_popover::<Said, Renderer>(
                label("Connection"),
                health.as_ref(),
                &trips,
                alone,
                None,
                |_| Said::Pressed,
                &WORDS,
            );
            let on = container(popover).center(Length::Fill).class(Surfaced::On(Surface::Base));
            testing::render(&theme, on, Size::new(340.0, 370.0), 1.0)
        };
        let nothing = Health::default();
        ui::refs!().assert(
            &format!("connection-popover-{scheme:?}").to_lowercase(),
            &testing::side_by_side(&[
                one(Some(look(Some(Quality::Good), Some(38), 0)), false),
                one(Some(look(Some(Quality::Fair), Some(172), 9)), false),
                one(Some(look(Some(Quality::Poor), Some(268), 126)), false),
                one(
                    Some(Health {
                        sending: flow(0, 0),
                        receiving: flow(0, 0),
                        ..look(Some(Quality::Good), Some(38), 0)
                    }),
                    true,
                ),
                one(Some(nothing), true),
            ]),
        );
    }
}

#[test]
fn the_round_trip_graph_draws_its_line_where_its_samples_put_it() {
    let theme = Theme::new(Scheme::Dark, Accent::DEFAULT);
    let role = Role::Status(Quality::Good.status(), StatusRole::Text);
    let drawn = |millis: u64| {
        let mut trips = RoundTrips::default();
        let trip = screens::RoundTrip { took: Duration::from_millis(millis), on: screens::MeasuredOn::Route };
        (0..screens::KEPT_LOOKS).for_each(|_| trips.push(Some(trip)));
        let graph = trips.graph();
        let words = GraphWords {
            scale: ["50".into(), "25".into(), "0".into()],
            minutes: graph.minutes.iter().map(|minute| minute.ago.to_string()).collect(),
        };
        let graph = line_graph::<Said, Renderer>(graph, words, role, design::ROUND_TRIP_TRACE);
        testing::render(&theme, container(graph), Size::new(276.0, 84.0), 1.0)
    };
    let rgb = |pixel: [u8; 4]| [pixel[0], pixel[1], pixel[2]];
    let colour = |role: Role| {
        let c = theme.palette().color(role);
        [c.r, c.g, c.b]
    };
    let (line, ground) = (colour(role), colour(Role::Surface(Surface::Base)));
    let distance = |a: [u8; 3], b: [u8; 3]| a.iter().zip(b).map(|(a, b)| u32::from(a.abs_diff(b))).sum::<u32>();
    let on_line = |image: &Image, y: u32| {
        let pixel = rgb(image.pixel(100, y));
        distance(pixel, line) * 2 < distance(pixel, ground)
    };
    let column = |image: &Image| (0..64).map(|y| image.pixel(100, y)).collect::<Vec<_>>();
    // 10 of a 50 ms top is a fifth of the way up the 62.5 px between the line's insets: its centre 50.75 px down.
    let low = drawn(10);
    assert!(on_line(&low, 50), "{:?} {line:?}", column(&low));
    assert!(!on_line(&low, 47) && !on_line(&low, 54), "the line is only there: {:?}", column(&low));
    for y in [20, 57] {
        assert!(distance(rgb(low.pixel(100, y)), ground) < 8, "nothing above or under it: {:?}", column(&low));
    }
    // A round trip of nothing, as on loopback, still shows its whole line, over the bottom gridline.
    let none = drawn(0);
    assert!(on_line(&none, 63), "a loopback's line is drawn whole: {:?}", column(&none));
}

#[test]
fn the_connection_popover_is_a_dialog_named_for_a_screen_reader() {
    let mut sim = sim(stats_popover::<Said, Renderer>(
        label("Connection"),
        None,
        &RoundTrips::default(),
        false,
        None,
        |_| Said::Pressed,
        &WORDS,
    ));
    let found = semantics(&mut sim);
    assert!(
        found.iter().any(|each| each.role == SemanticRole::Dialog && each.label.as_str() == "Connection"),
        "{found:?}"
    );
}

#[test]
fn the_connection_bars_are_one_stop_of_the_keyboard_named_connection() {
    let mut sim = sim(FocusScope::new(Column::with_children([bars(Some(Status::Success), false)])));
    let found = semantics(&mut sim);
    assert!(found.iter().any(|each| each.role == SemanticRole::Button && each.label.as_str() == "Connection"));
    tab(&mut sim, false);
    press(&mut sim, Named::Enter);
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Pressed]);
}
