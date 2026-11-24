//! The shared components in every state they show, each reached the way a person reaches it: pictures against the
//! references, behaviour through the simulator, and what is painted against what the look names.

use crate::common;

use common::{focusable_total, label, press, semantics, sim, tab};
use design::{Accent, AccentRole, AvatarSize, Elevation, Presence, Role, Scheme, SemanticRole, Space, Status, Surface};
use iced_core::keyboard::key::Named;
use iced_core::{Element, Length, Point, Size, Widget};
use iced_widget::{Column, container};
use screens::{Percent, RestartOffer, UpdateNotice, initials, presence_phrase};
use std::time::Duration;
use ui::testing::{self, Image, Renderer, Scene};
use ui::{AvatarKind, FocusScope, ListRow, Surfaced, Theme, avatar, badge, panel, tooltip, update_notice};

/// The English the views are drawn in, for as long as they borrow it.
static WORDS: std::sync::LazyLock<words::Words> = std::sync::LazyLock::new(words::Words::english);

type El<'a, M> = Element<'a, M, Theme, Renderer>;

/// An element on a raised surface, padded, centred in a cell.
fn cell<'a, M: 'a>(theme: &Theme, element: El<'a, M>, size: Size, scene: &Scene) -> Image {
    let on = container(element).padding(Space::S.px()).center(Length::Fill).class(Surfaced::On(Surface::Raised));
    testing::render_scene(theme, on, size, 1.0, scene)
}

/// How many pixels are within `tolerance` of a colour in every channel: small text never reaches its colour exactly.
fn near(image: &Image, colour: design::Rgb, tolerance: u8) -> usize {
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

fn count_of(image: &Image, colour: design::Rgb) -> usize {
    image.rgba.as_chunks::<4>().0.iter().filter(|pixel| **pixel == [colour.r, colour.g, colour.b, 255]).count()
}

#[test]
fn avatars_are_the_references_and_a_servers_is_in_its_own_colour() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        for (name, text) in [("latin", "Ada Lovelace"), ("greek", "Ελένη Παπά"), ("accent", "e\u{301}lan vital")]
        {
            let mut row = Vec::new();
            for kind in [AvatarKind::Person, AvatarKind::Server(Accent::DEFAULT.seed())] {
                // The tile's two sizes are drawn only on a tile.
                for size in
                    AvatarSize::ALL.into_iter().filter(|size| !matches!(size, AvatarSize::Tile | AvatarSize::Alone))
                {
                    row.push(cell::<()>(
                        &theme,
                        avatar(label(text), kind, size),
                        Size::new(80.0, 64.0),
                        &Scene::default(),
                    ));
                }
            }
            ui::refs!().assert(&format!("avatar-{name}-{scheme:?}").to_lowercase(), &testing::side_by_side(&row));
        }
        let picked = design::Rgb::hex(0x3D7FD6);
        let server = cell::<()>(
            &theme,
            avatar(label("Server S"), AvatarKind::Server(picked), AvatarSize::L),
            Size::new(80.0, 80.0),
            &Scene::default(),
        );
        assert!(count_of(&server, picked) > 1000, "a server's picture is its colour as picked");
        let person = cell::<()>(
            &theme,
            avatar(label("Ada"), AvatarKind::Person, AvatarSize::L),
            Size::new(80.0, 80.0),
            &Scene::default(),
        );
        let disc = theme.palette().color(design::plain_avatar().fill);
        assert!(count_of(&person, disc) > 1000, "a person's is the plain avatar's disc");
    }
}

#[test]
fn an_avatar_says_its_name_and_its_initials_are_a_whole_letters() {
    let mut sim = sim::<()>(avatar::<(), Renderer>(label("Ελένη Παπά"), AvatarKind::Person, AvatarSize::M));
    let found = semantics(&mut sim);
    assert_eq!((found[0].role, found[0].label.as_str()), (SemanticRole::Image, "Ελένη Παπά"));
    assert_eq!(initials("Ελένη Παπά"), "ΕΠ");
}

/// Each status's dot, plain and ringed over a picture: told by its shape as well as its colour, the away dot's top
/// left and the do not disturb dot's middle cut in the ground, the invisible one a ring with the ground inside.
#[test]
fn every_status_dot_is_the_reference_and_its_shape_says_it() {
    use screens::StatusDot;
    use ui::{person_with_status, status_mark_dot};
    const DOTS: [(StatusDot, design::StatusMark); 5] = [
        (StatusDot::Online, design::StatusMark::Online),
        (StatusDot::Away, design::StatusMark::Away),
        (StatusDot::DoNotDisturb, design::StatusMark::DoNotDisturb),
        (StatusDot::Offline, design::StatusMark::Offline),
        (StatusDot::Hollow, design::StatusMark::Hollow),
    ];
    let side = design::PRESENCE_DOT;
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let ground = theme.palette().color(Role::Surface(Surface::Raised));
        let mut row = Vec::new();
        for (dot, mark) in DOTS {
            let fill = theme.palette().color(design::status_mark(mark).fill);
            let plain =
                container(status_mark_dot::<(), Renderer>(dot, Surface::Raised)).class(Surfaced::On(Surface::Raised));
            let image = testing::render(&theme, plain, Size::new(side, side), 1.0);
            let colour = |pixel: [u8; 4]| [pixel[0], pixel[1], pixel[2]];
            let (fill, ground) = ([fill.r, fill.g, fill.b], [ground.r, ground.g, ground.b]);
            let (middle, left_middle, top_left) = (image.pixel(5, 5), image.pixel(1, 5), image.pixel(2, 2));
            match mark {
                design::StatusMark::Online | design::StatusMark::Offline => {
                    assert_eq!((colour(middle), colour(top_left)), (fill, fill), "{dot:?}: whole");
                }
                design::StatusMark::Away => {
                    assert_eq!(colour(top_left), ground, "{dot:?}: notched");
                    assert_eq!(colour(image.pixel(7, 7)), fill, "{dot:?}: the rest its colour");
                }
                design::StatusMark::DoNotDisturb => {
                    assert_eq!(colour(middle), ground, "{dot:?}: barred");
                    assert_eq!(colour(image.pixel(5, 2)), fill, "{dot:?}: its colour round the bar");
                }
                design::StatusMark::Hollow => {
                    assert_eq!(colour(middle), ground, "{dot:?}: hollow");
                    assert_eq!(colour(left_middle), fill, "{dot:?}: its ring");
                }
            }
            row.push(cell::<()>(
                &theme,
                status_mark_dot(dot, Surface::Raised),
                Size::new(40.0, 40.0),
                &Scene::default(),
            ));
            let ringed = person_with_status::<(), Renderer>(
                label("Ada Lovelace"),
                AvatarKind::Person,
                None,
                Some(dot),
                Surface::Raised,
            );
            row.push(cell::<()>(&theme, ringed, Size::new(56.0, 56.0), &Scene::default()));
        }
        ui::refs!().assert(&format!("status-dots-{scheme:?}").to_lowercase(), &testing::side_by_side(&row));
    }
}

#[test]
fn every_badge_is_the_reference_and_its_word_is_painted_in_its_colour() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let mut row = Vec::new();
        for presence in Presence::ALL {
            let image = cell::<()>(&theme, badge(presence, &WORDS), Size::new(160.0, 40.0), &Scene::default());
            let look = design::badge(presence);
            let fill = theme.palette().color(look.fill);
            let word = theme.palette().color(look.text);
            assert!(count_of(&image, fill) > 300, "{presence:?}: its colour is behind it");
            assert!(near(&image, word, 40) > 5, "{presence:?}: its word is in the colour that reads on it");
            row.push(image);
        }
        ui::refs!().assert(&format!("badge-{scheme:?}").to_lowercase(), &testing::side_by_side(&row));
    }
}

#[test]
fn a_badge_says_its_word_to_a_screen_reader() {
    for presence in Presence::ALL {
        let mut sim = sim::<()>(badge::<(), Renderer>(presence, &WORDS));
        let found = semantics(&mut sim);
        assert_eq!(
            (found[0].role, found[0].label.as_str()),
            (SemanticRole::Status, &*WORDS.phrase(presence_phrase(presence)))
        );
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Picked {
    Voice,
    Text,
}

fn row<'a>(name: &str, selected: bool, enabled: bool) -> El<'a, Picked> {
    let mut row = ListRow::<Picked, Renderer>::new(label(name))
        .selected(selected)
        .leading(avatar(label(name), AvatarKind::Person, AvatarSize::S))
        .trailing(badge(Presence::Online, &WORDS));
    if enabled {
        row = row.on_press(Picked::Voice);
    }
    row.view()
}

const ROW: Size = Size::new(260.0, 52.0);

#[test]
fn a_list_row_in_every_state_is_the_reference_in_both_schemes() {
    let centre = Point::new(ROW.width / 2.0, ROW.height / 2.0);
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let cells = [
            cell(&theme, row("Voice channel V", false, true), ROW, &Scene::default()),
            cell(&theme, row("Voice channel V", false, true), ROW, &Scene::hovering(centre)),
            cell(&theme, row("Voice channel V", false, true), ROW, &Scene::pressing(centre)),
            cell(&theme, row("Voice channel V", true, true), ROW, &Scene::default()),
            cell(&theme, row("Voice channel V", true, true), ROW, &Scene::hovering(centre)),
            cell(&theme, row("Voice channel V", false, false), ROW, &Scene::default()),
        ];
        ui::refs!().assert(&format!("row-{scheme:?}").to_lowercase(), &testing::side_by_side(&cells));
        let hover = theme.palette().color(Role::Surface(Surface::Hover));
        assert!(count_of(&cells[1], hover) > 2000, "a hovered row is filled with the hover surface in {scheme:?}");
    }
}

fn rows<'a>(disabled_middle: bool) -> FocusScope<Column<El<'a, Picked>>> {
    FocusScope::new(Column::with_children([row("Lobby", false, true), row("Games", false, !disabled_middle), {
        let mut last = ListRow::<Picked, Renderer>::new(label("Music")).on_press(Picked::Text);
        last = last.selected(false);
        last.view()
    }]))
}

#[test]
fn rows_are_reached_by_tab_in_order_pressed_by_space_and_a_disabled_one_is_skipped() {
    let mut sim = sim(rows(true));
    assert_eq!(focusable_total(&mut sim), 2);
    tab(&mut sim, false);
    press(&mut sim, Named::Space);
    tab(&mut sim, false);
    press(&mut sim, Named::Enter);
    assert_eq!(
        sim.drain().collect::<Vec<_>>(),
        [Picked::Voice, Picked::Text],
        "Lobby then Music; Games was passed over"
    );
    let mut sim = common::sim(rows(false));
    let names: Vec<_> =
        semantics(&mut sim).into_iter().map(|each| (each.role, each.label.as_str().to_string())).collect();
    assert_eq!(
        names,
        [
            (SemanticRole::ListItem, "Lobby".into()),
            (SemanticRole::Image, "Lobby".into()),
            (SemanticRole::Status, "Online".into()),
            (SemanticRole::ListItem, "Games".into()),
            (SemanticRole::Image, "Games".into()),
            (SemanticRole::Status, "Online".into()),
            (SemanticRole::ListItem, "Music".into()),
        ]
    );
}

/// A name too long for its row stays on one line and ends in an ellipsis, so every row is as tall as the
/// rest and nothing spills; its whole is in the row's label and shown in a tooltip.
#[test]
fn a_long_name_keeps_the_row_one_line_tall_and_inside_it() {
    let theme = Theme::new(Scheme::Dark, Accent::DEFAULT);
    let base = theme.palette().color(Role::Surface(Surface::Base));
    let size = Size::new(300.0, 52.0);
    let width = 220.0;
    let wrapped = |name: &str| {
        let row = container(row(name, false, true)).width(Length::Fixed(width));
        testing::render_scene(
            &theme,
            container(row).padding(Space::S.px()).class(Surfaced::On(Surface::Raised)),
            size,
            1.0,
            &Scene::default(),
        )
    };
    let short = wrapped("Lobby");
    for name in ["x".repeat(110), "Α".repeat(80), "Καλημέρα κόσμε ".repeat(7), "مرحبا بالعالم ".repeat(8)]
    {
        let long = wrapped(&name);
        assert_eq!(long.size, short.size);
        let beyond = 2 * Space::S.px() as u32 + width as u32;
        for y in 0..long.size.height {
            for x in beyond..long.size.width {
                assert_eq!(long.pixel(x, y), [base.r, base.g, base.b, 255], "{:?} spilled at {x},{y}", name.get(..4));
            }
        }
    }
}

#[test]
fn panels_at_each_height_are_the_references_and_light_shows_height_by_shadow() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let mut row = Vec::new();
        for elevation in [Elevation::Flat, Elevation::Raised, Elevation::Overlay] {
            let content: El<'_, ()> = design_text("Panel");
            let on = container(panel(content, elevation))
                .padding(Space::L.px())
                .center(Length::Fill)
                .class(Surfaced::On(Surface::Base));
            row.push(testing::render::<()>(&theme, on, Size::new(140.0, 100.0), 1.0));
        }
        ui::refs!().assert(&format!("panel-{scheme:?}").to_lowercase(), &testing::side_by_side(&row));
    }
}

fn design_text<'a>(words: &str) -> El<'a, ()> {
    ui::text(words.to_string(), design::TypeStyle::Body)._boxed()
}

#[test]
fn a_tooltip_shows_after_its_delay_and_not_before() {
    let theme = Theme::new(Scheme::Dark, Accent::DEFAULT);
    let size = Size::new(220.0, 90.0);
    let target = || -> El<'static, ()> {
        tooltip(design_text("Mute"), &label("Mute microphone"), iced_widget::tooltip::Position::Bottom)
    };
    let wrap = |scene: &Scene| {
        let on = container(target()).padding(Space::L.px()).class(Surfaced::On(Surface::Raised));
        testing::render_scene(&theme, on, size, 1.0, scene)
    };
    let over = Point::new(Space::L.px() + 10.0, Space::L.px() + 8.0);
    let rest = wrap(&Scene::default());
    let early = wrap(&Scene { settle: Duration::from_millis(50), ..Scene::hovering(over) });
    let late = wrap(&Scene { settle: design::TOOLTIP_DELAY + Duration::from_millis(100), ..Scene::hovering(over) });
    assert!(rest == early, "nothing shows before the delay");
    assert!(rest != late, "its tip shows after it");
    ui::refs!().assert("tooltip-dark", &late);
}

/// What the notice's Restart sends here.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Restart;

fn notice(shown: UpdateNotice) -> El<'static, Restart> {
    update_notice::<Restart, Renderer>(shown, Restart, &WORDS)
}

const NOTICE: Size = Size::new(360.0, 110.0);

fn shown() -> [UpdateNotice; 4] {
    [
        UpdateNotice::Downloading(Percent::new(64)),
        UpdateNotice::Ready(RestartOffer::Offered),
        UpdateNotice::Ready(RestartOffer::DeferredUntilVoiceEnds),
        UpdateNotice::Failed,
    ]
}

#[test]
fn the_update_notice_in_every_state_is_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let row: Vec<_> =
            shown().into_iter().map(|shown| cell(&theme, notice(shown), NOTICE, &Scene::default())).collect();
        ui::refs!().assert(&format!("update-notice-{scheme:?}").to_lowercase(), &testing::side_by_side(&row));
    }
}

#[test]
fn a_restart_is_offered_only_when_offered_and_it_is_never_taken_for_the_person() {
    for shown in shown() {
        let mut sim = sim(FocusScope::new(notice(shown)));
        let offered = shown == UpdateNotice::Ready(RestartOffer::Offered);
        assert_eq!(focusable_total(&mut sim), usize::from(offered), "{shown:?}");
        // The notice never takes the focus: nothing is focused until a person asks.
        assert_eq!(common::focused(&mut sim), None);
        let found = semantics(&mut sim);
        assert_eq!(found[0].role, SemanticRole::Status);
        assert_eq!(found[0].label.as_str(), WORDS.phrase(shown.phrase()));
        if offered {
            tab(&mut sim, false);
            press(&mut sim, Named::Space);
            assert_eq!(sim.drain().collect::<Vec<_>>(), [Restart]);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Pressed {
    Act,
    Away,
    Held(bool),
}

/// A toast of `kind`, offering `action` when asked.
fn a_toast(kind: Status, action: bool) -> El<'static, Pressed> {
    let mut telling = screens::Telling::of(
        kind,
        words::Sentence::Said("You’re back. You left Lounge when the connection dropped.".into()),
    );
    if action {
        telling = telling.offering(screens::Action::Rejoin("Lounge".into()));
    }
    let toast = screens::harness::toast(telling, std::time::Instant::now());
    let sends = ui::ToastSends { act: Pressed::Act, away: Pressed::Away, held: |held| Pressed::Held(held) };
    ui::toast::<Pressed, Renderer>(&toast, &sends, &WORDS)
}

const STRIP: Size = Size::new(640.0, 56.0);

#[test]
fn a_toast_of_every_kind_with_and_without_its_action_is_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let column: Vec<_> = [
            (Status::Info, false),
            (Status::Info, true),
            (Status::Warning, false),
            (Status::Success, true),
            (Status::Danger, true),
        ]
        .into_iter()
        .map(|(kind, action)| cell(&theme, a_toast(kind, action), STRIP, &Scene::default()))
        .collect();
        ui::refs!().assert(&format!("toast-{scheme:?}").to_lowercase(), &testing::stacked(&column));
    }
}

#[test]
fn a_toast_is_a_status_whose_action_and_cross_are_the_only_things_to_press() {
    let plain = || sim(FocusScope::new(a_toast(Status::Info, false)));
    assert_eq!(focusable_total(&mut plain()), 0, "plain news has nothing to press");
    let mut with_both = sim(FocusScope::new(a_toast(Status::Success, true)));
    assert_eq!(focusable_total(&mut with_both), 2);
    assert_eq!(common::focused(&mut with_both), None, "it never takes the focus itself");
    let found = semantics(&mut with_both);
    assert_eq!(found[0].role, SemanticRole::Status);
    assert_eq!(found[0].label.as_str(), "You’re back. You left Lounge when the connection dropped.");
    tab(&mut with_both, false);
    press(&mut with_both, Named::Space);
    tab(&mut with_both, false);
    press(&mut with_both, Named::Space);
    assert_eq!(with_both.drain().collect::<Vec<_>>(), [Pressed::Act, Pressed::Away]);
    let failed = semantics(&mut sim(FocusScope::new(a_toast(Status::Danger, false))));
    assert_eq!(failed[0].role, SemanticRole::Alert, "a failure is read out at once");
}

#[test]
fn a_toast_with_a_dot_is_in_the_colour_of_its_kind_and_a_plain_one_has_none() {
    let theme = Theme::new(Scheme::Dark, Accent::DEFAULT);
    for kind in Status::ALL {
        let dot = theme.palette().color(Role::Status(kind, design::StatusRole::Fill));
        let image = cell(&theme, a_toast(kind, false), STRIP, &Scene::default());
        let painted = count_of(&image, dot);
        assert_eq!(painted > 20, kind != Status::Info, "{kind:?}: {painted}");
    }
}

#[test]
fn a_state_line_of_each_kind_is_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let column: Vec<_> = [
            (Status::Warning, "Connection to Night Owls lost. Reconnecting…"),
            (Status::Danger, "An admin muted you for everyone. You can talk again when they undo it."),
        ]
        .into_iter()
        .map(|(kind, said)| {
            let line = screens::StateLine { kind, said: words::Sentence::Said(said.into()) };
            let shown = ui::state_line::<(), Renderer>(&line, &WORDS);
            testing::render(&theme, container(shown).class(Surfaced::On(Surface::Base)), Size::new(640.0, 40.0), 1.0)
        })
        .collect();
        ui::refs!().assert(&format!("state-line-{scheme:?}").to_lowercase(), &testing::stacked(&column));
    }
}

/// A callout of `kind`, with its title, action and ✕ as asked.
fn a_callout(kind: Option<Status>, titled: bool, action: bool, away: bool) -> El<'static, Pressed> {
    let mut shown = screens::Callout::new(words::Sentence::Said("You can’t change Moderators.".into()));
    shown.kind = kind;
    if titled {
        shown = shown.titled(words::Sentence::Said("Not allowed".into()));
    }
    if action {
        shown = shown.offering(screens::Action::InvitePeople);
    }
    if away {
        shown = shown.dismissable();
    }
    let parts = ui::CalloutParts { act: Some(Pressed::Act), away: Some(Pressed::Away), under: None };
    ui::callout::<Pressed, Renderer>(&shown, parts, &WORDS)
}

const BOX: Size = Size::new(560.0, 96.0);

#[test]
fn a_callout_of_every_status_with_and_without_title_action_and_cross_is_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let column: Vec<_> = [
            (None, false, false, false),
            (None, true, true, true),
            (Some(Status::Warning), false, false, true),
            (Some(Status::Danger), true, true, true),
        ]
        .into_iter()
        .map(|(kind, titled, action, away)| cell(&theme, a_callout(kind, titled, action, away), BOX, &Scene::default()))
        .collect();
        ui::refs!().assert(&format!("callout-{scheme:?}").to_lowercase(), &testing::stacked(&column));
    }
}

#[test]
fn a_callout_has_a_cross_only_where_it_can_be_put_away_and_an_action_only_where_it_offers_one() {
    let mut none = sim(FocusScope::new(a_callout(Some(Status::Warning), false, false, false)));
    assert_eq!(focusable_total(&mut none), 0);
    let mut both = sim(FocusScope::new(a_callout(None, true, true, true)));
    assert_eq!(focusable_total(&mut both), 2);
    tab(&mut both, false);
    press(&mut both, Named::Space);
    tab(&mut both, false);
    press(&mut both, Named::Space);
    assert_eq!(both.drain().collect::<Vec<_>>(), [Pressed::Act, Pressed::Away]);
}

#[test]
fn a_callout_edge_is_in_the_colour_of_its_status() {
    let theme = Theme::new(Scheme::Dark, Accent::DEFAULT);
    for kind in Status::ALL {
        let edge = theme.palette().color(design::notice_edge(kind));
        let image = cell(&theme, a_callout(Some(kind), false, false, false), BOX, &Scene::default());
        assert!(count_of(&image, edge) > 200, "{kind:?}");
    }
}

#[test]
fn a_field_message_is_a_refusal_with_an_alert_circle_or_help_with_none() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let row = |tone| {
            let said = screens::FieldSaid {
                tone,
                said: words::Sentence::Said("That username or password doesn’t match.".into()),
            };
            cell(
                &theme,
                container(ui::field_said::<(), Renderer>(&said, &WORDS)).width(Length::Fill)._boxed(),
                Size::new(420.0, 40.0),
                &Scene::default(),
            )
        };
        let (refusal, help) = (row(screens::FieldTone::Refusal), row(screens::FieldTone::Help));
        ui::refs!().assert(&format!("field-message-{scheme:?}").to_lowercase(), &testing::stacked(&[refusal, help]));
    }
}

#[test]
fn a_centre_panel_in_each_mark_with_and_without_its_pills_is_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let panel = |mark, title: &str, body: Option<&str>, offers: Vec<screens::Offer>| {
            let shown = screens::CentrePanel {
                mark,
                title: Some(words::Sentence::Said(title.into())),
                body: body.map(|body| words::Sentence::Said(body.into())),
                offers,
            };
            let drawn = ui::centre::<Pressed, Renderer>(&shown, |_| Pressed::Act, &WORDS);
            testing::render(&theme, container(drawn).class(Surfaced::On(Surface::Base)), Size::new(520.0, 300.0), 1.0)
        };
        let row = [
            panel(
                screens::Mark::Waiting,
                "Connecting to Book club…",
                Some("Pick another server in the list at any time."),
                vec![],
            ),
            panel(
                screens::Mark::Offline,
                "Book club isn’t answering",
                Some("Trying again in 12 seconds."),
                vec![screens::Offer::TryNow],
            ),
            panel(
                screens::Mark::Failed,
                "Search didn’t answer.",
                None,
                vec![screens::Offer::TryNow, screens::Offer::ShowChannels],
            ),
        ];
        ui::refs!().assert(&format!("centre-panel-{scheme:?}").to_lowercase(), &testing::side_by_side(&row));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Written(bool);

/// Copy puts its text on the clipboard once, and says whether that took when the window answers.
#[test]
fn a_clipboard_writer_writes_once_and_says_how_it_went() {
    let answered = |result| iced_core::Event::Clipboard(iced_core::clipboard::Event::Written(result));
    let redraw = iced_core::Event::Window(iced_core::window::Event::RedrawRequested(iced_core::time::Instant::now()));
    let seen = testing::interact(
        container(ui::clipboard_writer("chat.example.invalid", Written)),
        Size::new(100.0, 40.0),
        Point::ORIGIN,
        &[redraw.clone(), redraw.clone(), answered(Ok(()))],
    );
    assert_eq!(seen.clipboard, [iced_core::clipboard::Content::Text("chat.example.invalid".into())]);
    assert_eq!(seen.messages, [Written(true)]);
    let failed = testing::interact(
        container(ui::clipboard_writer("chat.example.invalid", Written)),
        Size::new(100.0, 40.0),
        Point::ORIGIN,
        &[redraw, answered(Err(iced_core::clipboard::Error::ClipboardUnavailable))],
    );
    assert_eq!(failed.messages, [Written(false)]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Took(usize);

fn uses<'a>(chosen: usize, fill: bool) -> El<'a, Took> {
    let options = ["1 person", "5 people", "25 people"].iter().enumerate().map(|(i, said)| (label(said), Took(i)));
    ui::choice(label("Can be used by"), options.collect(), chosen, fill)
}

const CHOICE: Size = Size::new(420.0, 76.0);

/// A choice in each state: the chosen pill raised, one under the pointer, one pressed; filling its line or not.
#[test]
fn a_choice_in_every_state_is_the_reference_in_both_schemes() {
    let over = Point::new(CHOICE.width * 5.0 / 6.0, 56.0);
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let cells = [
            cell(&theme, uses(1, true), CHOICE, &Scene::default()),
            cell(&theme, uses(1, true), CHOICE, &Scene::hovering(over)),
            cell(&theme, uses(1, true), CHOICE, &Scene::pressing(over)),
            cell(&theme, uses(0, false), CHOICE, &Scene::default()),
        ];
        ui::refs!().assert(&format!("choice-{scheme:?}").to_lowercase(), &testing::side_by_side(&cells));
        let raised = theme.palette().color(Role::Surface(Surface::Inset));
        assert!(count_of(&cells[0], raised) > 1500, "the chosen pill is raised in {scheme:?}");
    }
}

/// A screen reader hears the group by its name and which option is taken; Tab reaches each, Space takes one.
#[test]
fn a_choice_says_which_is_taken_and_each_option_is_taken_by_the_keyboard() {
    let mut sim = sim(FocusScope::new(container(uses(1, true))));
    let said: Vec<_> =
        semantics(&mut sim).into_iter().map(|each| (each.role, each.label.as_str().to_string())).collect();
    assert_eq!(
        said,
        [
            (SemanticRole::RadioGroup, "Can be used by".into()),
            (SemanticRole::Radio { chosen: false }, "1 person".into()),
            (SemanticRole::Radio { chosen: true }, "5 people".into()),
            (SemanticRole::Radio { chosen: false }, "25 people".into()),
        ]
    );
    assert_eq!(focusable_total(&mut sim), 3);
    tab(&mut sim, false);
    tab(&mut sim, false);
    tab(&mut sim, false);
    press(&mut sim, Named::Space);
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Took(2)]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Asked {
    Page(usize),
    Close,
    Copy,
    Stop,
}

fn invites_table<'a>(rows: usize) -> El<'a, Asked> {
    let row = |note: &str, used: &str, ends: &str| {
        vec![
            ui::text(note.to_string(), design::TypeStyle::Small)._boxed(),
            ui::text(used.to_string(), design::TypeStyle::Small)._boxed(),
            ui::text(ends.to_string(), design::TypeStyle::Small)._boxed(),
            container(
                ui::Button::new(label("Stop"))
                    .variant(design::ButtonVariant::Danger)
                    .compact()
                    .on_press(Asked::Stop)
                    .view(),
            )
            .align_right(Length::Fill)
            ._boxed(),
        ]
    };
    let all = vec![row("for Bo", "2 of 5", "1 November"), row("No note", "0 of 1", "27 October")];
    let empty = ui::text("No invites working right now.".to_string(), design::TypeStyle::Body);
    ui::table(
        label("Invites"),
        ["Note", "Used", "Ends", ""].map(str::to_string).to_vec(),
        all.into_iter().take(rows).collect(),
        Some(empty._boxed()),
    )
}

fn settings<'a>(rows: usize) -> El<'a, Asked> {
    let pages = ["Invites", "Members"]
        .iter()
        .enumerate()
        .map(|(i, said)| ui::SettingsPage { label: label(said), current: i == 0, message: Asked::Page(i) })
        .collect();
    let pages = vec![ui::SettingsGroup { heading: None, pages }];
    let new =
        ui::Button::new(label("New invite")).variant(design::ButtonVariant::Primary).compact().on_press(Asked::Page(0));
    let page = Column::with_children([
        invites_table(rows),
        ui::copy_field("https://chat.example.invalid/join#Kx7fQ2".into(), label("Copy"), Asked::Copy),
    ])
    .spacing(Space::L.px());
    ui::settings_frame(
        &label("Server S"),
        pages,
        label("Invites"),
        Some(new.view()),
        page._boxed(),
        ui::SettingsWays {
            close: (label("Close"), Asked::Close),
            shown: screens::frame::SettingsShown::Beside,
            back: (label("Back"), Asked::Close),
        },
    )
}

const SETTINGS: Size = Size::new(900.0, 420.0);

/// A server's settings: the pages beside the open one, a table with and without rows, a value to copy.
#[test]
fn settings_with_a_table_are_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let cells = [
            testing::render_scene(&theme, settings(2), SETTINGS, 1.0, &Scene::default()),
            testing::render_scene(&theme, settings(0), SETTINGS, 1.0, &Scene::default()),
        ];
        ui::refs!().assert(&format!("settings-{scheme:?}").to_lowercase(), &testing::side_by_side(&cells));
    }
}

/// The settings say their pages and which is open, the table its name; Close, a page, Stop and Copy each send theirs.
#[test]
fn settings_say_their_pages_and_send_what_is_pressed() {
    let mut sim = common::sim_sized(FocusScope::new(container(settings(1))), SETTINGS);
    let said: Vec<_> =
        semantics(&mut sim).into_iter().map(|each| (each.role, each.label.as_str().to_string())).collect();
    for expected in [
        (SemanticRole::RadioGroup, "Server S".to_string()),
        (SemanticRole::Radio { chosen: true }, "Invites".into()),
        (SemanticRole::Radio { chosen: false }, "Members".into()),
        (SemanticRole::Heading, "Invites".into()),
        (SemanticRole::Table, "Invites".into()),
        (SemanticRole::Button, "Close".into()),
    ] {
        assert!(said.contains(&expected), "{expected:?} in {said:?}");
    }
    for _ in 0..focusable_total(&mut sim) {
        tab(&mut sim, false);
        press(&mut sim, Named::Enter);
    }
    let sent: Vec<_> = sim.drain().collect();
    for each in [Asked::Page(0), Asked::Page(1), Asked::Stop, Asked::Copy, Asked::Close] {
        assert!(sent.contains(&each), "{each:?} in {sent:?}");
    }
}

/// A section says its heading and holds its action at the right of it; a row says its title and its line, and its
/// control sends what it is given; a danger row is set apart from a plain one only by its colour, not its size.
#[test]
fn a_settings_section_says_its_heading_and_its_rows_send_what_is_pressed() {
    let action = ui::Button::new(label("Play a test sound")).compact().on_press(Asked::Page(1)).view();
    let row = |title: &str, line: Option<&str>, press: &str, sends: Asked, tone: ui::RowTone| {
        ui::settings_row(
            title.to_string(),
            line.map(str::to_string),
            ui::Button::new(label(press)).compact().on_press(sends).view(),
            tone,
        )
    };
    let card = Column::with_children([
        row("Sign out of Server S", Some("On this device."), "Sign out", Asked::Page(2), ui::RowTone::Plain),
        row("Leave Server S", None, "Leave", Asked::Close, ui::RowTone::Danger),
    ]);
    let section = ui::settings_section(label("Sign-in"), Some(action), card._boxed());
    let mut sim = common::sim_sized(FocusScope::new(ui::settings_body(vec![section])), Size::new(700.0, 300.0));
    let said: Vec<_> =
        semantics(&mut sim).into_iter().map(|each| (each.role, each.label.as_str().to_string())).collect();
    for expected in [
        (SemanticRole::Heading, "Sign-in".to_string()),
        (SemanticRole::Button, "Play a test sound".into()),
        (SemanticRole::Button, "Sign out".into()),
        (SemanticRole::Button, "Leave".into()),
    ] {
        assert!(said.contains(&expected), "{expected:?} in {said:?}");
    }
    let boxes = common::boxes(&mut sim);
    let at = |name: &str| boxes.iter().find(|(each, _)| each.label.as_str() == name).map(|(_, at)| *at);
    let (heading, action) = (at("Sign-in").expect("heading"), at("Play a test sound").expect("action"));
    assert!(
        action.x > heading.x + heading.width / 2.0,
        "the action is at the right of the heading: {heading:?} {action:?}"
    );
    let (plain, danger) = (at("Sign out").expect("plain"), at("Leave").expect("danger"));
    assert_eq!(plain.height, danger.height, "rows keep one control height");
    for _ in 0..focusable_total(&mut sim) {
        tab(&mut sim, false);
        press(&mut sim, Named::Enter);
    }
    let sent: Vec<_> = sim.drain().collect();
    for each in [Asked::Page(1), Asked::Page(2), Asked::Close] {
        assert!(sent.contains(&each), "{each:?} in {sent:?}");
    }
}

/// A compact button is one hit target tall whatever it carries, an icon too (the icon is taller than its line), and a
/// full one one control tall.
#[test]
fn a_button_is_as_tall_as_its_size_says_with_or_without_an_icon() {
    let buttons = Column::with_children([
        ui::Button::new(label("Close")).icon(ui::Icon::Close).compact().on_press(Asked::Close).view(),
        ui::Button::new(label("Dismiss")).icon_only(ui::Icon::Close).compact().on_press(Asked::Close).view(),
        ui::Button::new(label("New invite")).compact().on_press(Asked::Close).view(),
        ui::Button::new(label("Leave")).icon(ui::Icon::Close).on_press(Asked::Close).view(),
    ]);
    let mut sim = common::sim_sized(buttons, Size::new(300.0, 300.0));
    let tall: Vec<(String, f32)> = common::boxes(&mut sim)
        .into_iter()
        .filter(|(each, _)| each.role == SemanticRole::Button)
        .map(|(each, bounds)| (each.label.as_str().to_string(), bounds.height))
        .collect();
    let (hit, control) = (design::HIT_TARGET, design::CONTROL_HEIGHT);
    assert_eq!(
        tall,
        [("Close".into(), hit), ("Dismiss".into(), hit), ("New invite".into(), hit), ("Leave".into(), control)]
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Turned {
    Joining,
}

fn joining_switch<'a>(on: bool) -> El<'a, Turned> {
    ui::switch(
        &label("Join calls muted"),
        Some("Your microphone starts off. Unmute it when you are ready."),
        on,
        Turned::Joining,
    )
}

/// A switch off and on, with its name and what it does: its track in the strong edge colour or the accent, and its
/// knob at that end.
#[test]
fn a_switch_is_the_reference_in_both_schemes_and_its_state_is_told_by_colour_as_well_as_place() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let size = Size::new(420.0, 64.0);
        let (off, on) = (
            cell(&theme, joining_switch(false), size, &Scene::default()),
            cell(&theme, joining_switch(true), size, &Scene::default()),
        );
        ui::refs!()
            .assert(&format!("switch-{scheme:?}").to_lowercase(), &testing::side_by_side(&[off.clone(), on.clone()]));
        let palette = theme.palette();
        let (strong, accent) =
            (palette.color(Role::Border(design::Border::Strong)), palette.color(Role::Accent(AccentRole::Fill)));
        assert!(count_of(&off, strong) > 300, "the off track is the strong edge colour in {scheme:?}");
        assert_eq!(count_of(&off, accent), 0, "and no accent");
        assert!(count_of(&on, accent) > 300, "the on track is the accent in {scheme:?}");
        assert_eq!(count_of(&on, strong), 0, "and no edge colour");
    }
}

/// A switch is one stop for Tab, said as a switch named for what it turns on and whether it is on; Space and Enter
/// send.
#[test]
fn a_switch_is_reached_by_tab_said_with_its_state_and_taken_by_space_and_enter() {
    for on in [false, true] {
        let mut sim = common::sim_sized(FocusScope::new(container(joining_switch(on))), Size::new(420.0, 64.0));
        assert_eq!(focusable_total(&mut sim), 1);
        let said: Vec<_> =
            semantics(&mut sim).into_iter().map(|each| (each.role, each.label.as_str().to_string())).collect();
        assert!(said.contains(&(SemanticRole::Switch { on }, "Join calls muted".to_string())), "{said:?}");
        tab(&mut sim, false);
        press(&mut sim, Named::Space);
        press(&mut sim, Named::Enter);
        assert_eq!(sim.drain().collect::<Vec<_>>(), [Turned::Joining, Turned::Joining], "on={on}");
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Slid {
    Moved(u16),
    Kept(u16),
}

fn volume_at<'a>(value: u16) -> El<'a, Slid> {
    container(ui::slider(label("Volume"), 0..=100, value, Slid::Moved, Slid::Kept)).width(Length::Fixed(200.0))._boxed()
}

/// A slider is its rail in the accent up to the handle and the strong edge colour past it, in both schemes.
#[test]
fn a_slider_is_the_reference_in_both_schemes_and_fills_up_to_its_handle() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let size = Size::new(216.0, 40.0);
        let cells: Vec<_> = [0, 40, 100].map(|value| cell(&theme, volume_at(value), size, &Scene::default())).into();
        ui::refs!().assert(&format!("slider-{scheme:?}").to_lowercase(), &testing::side_by_side(&cells));
        let palette = theme.palette();
        let (strong, accent) =
            (palette.color(Role::Border(design::Border::Strong)), palette.color(Role::Accent(AccentRole::Fill)));
        assert!(count_of(&cells[0], accent) > 100, "an empty slider still shows its handle in {scheme:?}");
        assert!(count_of(&cells[0], strong) > count_of(&cells[1], strong), "less rail is left past 40 than past 0");
        assert!(count_of(&cells[2], accent) > count_of(&cells[1], accent), "and more is filled at 100");
    }
}

/// A drag sends each value it passes and keeps one, where it is let go; a press on the rail goes there.
#[test]
fn a_slider_dragged_sends_where_it_is_and_keeps_where_it_is_let_go() {
    use iced_core::mouse;
    let mut sim = common::sim_sized(FocusScope::new(volume_at(50)), Size::new(200.0, 40.0));
    let (_, rail) = common::boxes(&mut sim).into_iter().next().expect("the slider is a control");
    let at = |fraction: f32| Point::new(rail.x + 8.0 + fraction * (rail.width - 16.0), rail.center_y());
    sim.point_at(at(0.0));
    sim.simulate([iced_core::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))]);
    sim.point_at(at(0.25));
    sim.simulate([iced_core::Event::Mouse(mouse::Event::CursorMoved { position: at(0.25) })]);
    sim.point_at(Point::new(rail.x + rail.width + 40.0, rail.center_y()));
    sim.simulate([iced_core::Event::Mouse(mouse::Event::CursorMoved {
        position: Point::new(rail.x + rail.width + 40.0, rail.center_y()),
    })]);
    sim.simulate([iced_core::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))]);
    assert_eq!(
        sim.drain().collect::<Vec<_>>(),
        [Slid::Moved(0), Slid::Moved(25), Slid::Moved(100), Slid::Kept(100)],
        "past the end is the end"
    );
}

/// A slider is one stop for Tab, said with its value and range; the arrows step it by one, the page keys by ten and
/// Home and End go to the ends, each step kept, and a key past an end stays there.
#[test]
fn a_slider_is_reached_by_tab_said_with_its_value_and_stepped_by_the_keys() {
    let mut sim = common::sim_sized(FocusScope::new(volume_at(50)), Size::new(200.0, 40.0));
    assert_eq!(focusable_total(&mut sim), 1);
    let said: Vec<_> =
        semantics(&mut sim).into_iter().map(|each| (each.role, each.label.as_str().to_string())).collect();
    let role = SemanticRole::Slider { value: 50, least: 0, most: 100 };
    assert!(said.contains(&(role, "Volume".to_string())), "{said:?}");
    press(&mut sim, Named::ArrowRight);
    assert!(sim.drain().next().is_none(), "nothing moves it before it has the focus");
    tab(&mut sim, false);
    for key in [Named::ArrowRight, Named::ArrowUp, Named::ArrowLeft, Named::ArrowDown, Named::PageUp, Named::PageDown] {
        press(&mut sim, key);
    }
    press(&mut sim, Named::Home);
    press(&mut sim, Named::End);
    assert_eq!(
        sim.drain().collect::<Vec<_>>(),
        [51, 51, 49, 49, 60, 40, 0, 100].map(Slid::Kept),
        "each from the value it was drawn at"
    );
    let mut ends = common::sim_sized(FocusScope::new(volume_at(100)), Size::new(200.0, 40.0));
    tab(&mut ends, false);
    press(&mut ends, Named::ArrowRight);
    assert_eq!(ends.drain().collect::<Vec<_>>(), [Slid::Kept(100)], "taken at the end, so the focus stays");
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Heard {
    Moved(&'static str, u16),
    Kept(&'static str, u16),
    Mute(&'static str),
}

/// Audio controls with Bo's voice at 40 % and the sound of Grace's screen muted, or with no one else in the call.
fn audio_controls_of<'a>(empty: bool) -> El<'a, Heard> {
    let row = |name: &str, colour, share, slider: &str, percent: u16, key: &'static str| {
        let muted = percent == 0;
        ui::SoundRow {
            name: label(name),
            colour: design::PersonColour::Swatch(colour),
            picture: None,
            share,
            words: ui::VolumeWords {
                slider: label(slider),
                mute: label(if muted { "Unmute for me" } else { "Mute for me" }),
                figure: words::Words::english().percent(percent),
            },
            percent,
            muted,
            on_change: Box::new(move |percent| Heard::Moved(key, percent)),
            on_commit: Box::new(move |percent| Heard::Kept(key, percent)),
            on_mute: Heard::Mute(key),
        }
    };
    let rows = if empty {
        Vec::new()
    } else {
        vec![
            row("Bo", design::Swatch::Teal, false, "Bo", 40, "bo"),
            row("Grace", design::Swatch::Clay, true, "Sound from Grace’s stream", 0, "grace"),
        ]
    };
    let words = ui::AudioWords { title: label("Audio controls"), empty: "No one else is in the call.".into() };
    ui::audio_controls(words, rows)
}

/// Audio controls draws each sound on its row, a share's with a screen's glyph, and says so with no one else there.
#[test]
fn audio_controls_is_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let size = Size::new(392.0, 180.0);
        let cells: Vec<_> =
            [false, true].map(|empty| cell(&theme, audio_controls_of(empty), size, &Scene::default())).into();
        ui::refs!().assert(&format!("audio-controls-{scheme:?}").to_lowercase(), &testing::side_by_side(&cells));
    }
}

/// Each row is two stops for Tab, Mute for me named by what it will do and the slider by whose sound it is; each sends
/// for its own sound.
#[test]
fn audio_controls_names_each_sound_and_its_mute_and_each_row_sends_its_own() {
    let mut sim = common::sim_sized(FocusScope::new(audio_controls_of(false)), Size::new(392.0, 180.0));
    assert_eq!(focusable_total(&mut sim), 4);
    let said: Vec<_> =
        semantics(&mut sim).into_iter().map(|each| (each.role, each.label.as_str().to_string())).collect();
    for expected in [
        (SemanticRole::Dialog, "Audio controls"),
        (SemanticRole::Button, "Mute for me"),
        (SemanticRole::Slider { value: 40, least: 0, most: 100 }, "Bo"),
        (SemanticRole::Button, "Unmute for me"),
        (SemanticRole::Slider { value: 0, least: 0, most: 100 }, "Sound from Grace’s stream"),
    ] {
        assert!(said.contains(&(expected.0, expected.1.to_string())), "{expected:?} in {said:?}");
    }
    tab(&mut sim, false);
    press(&mut sim, Named::Enter);
    tab(&mut sim, false);
    press(&mut sim, Named::ArrowRight);
    tab(&mut sim, false);
    press(&mut sim, Named::Space);
    tab(&mut sim, false);
    press(&mut sim, Named::End);
    assert_eq!(
        sim.drain().collect::<Vec<_>>(),
        [Heard::Mute("bo"), Heard::Kept("bo", 41), Heard::Mute("grace"), Heard::Kept("grace", 100)]
    );
}

fn level_at<'a>(level: u8) -> El<'a, Turned> {
    container(ui::level_bar::<Turned, Renderer>(level)).width(Length::Fixed(200.0))._boxed()
}

/// The level bar fills in the success colour in proportion, from empty to full.
#[test]
fn a_level_bar_fills_in_proportion_in_the_success_colour() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let size = Size::new(232.0, 40.0);
        let cells: Vec<_> = [0, 40, 100].map(|level| cell(&theme, level_at(level), size, &Scene::default())).into();
        ui::refs!().assert(&format!("level-bar-{scheme:?}").to_lowercase(), &testing::side_by_side(&cells));
        let success = theme.palette().color(Role::Status(Status::Success, design::StatusRole::Fill));
        let filled: Vec<usize> = cells.iter().map(|image| count_of(image, success)).collect();
        assert_eq!(filled[0], 0, "nothing at 0 in {scheme:?}");
        assert!(filled[1] > 0 && filled[2] > filled[1] * 2, "40 fills less than a half of 100 does, {filled:?}");
    }
}

fn grouped<'a>() -> El<'a, Asked> {
    let page = |said: &str, current: bool, at: usize| ui::SettingsPage {
        label: label(said),
        current,
        message: Asked::Page(at),
    };
    ui::settings_frame(
        &label("Server S"),
        vec![
            ui::SettingsGroup { heading: None, pages: vec![page("Account", false, 0)] },
            ui::SettingsGroup { heading: Some(label("This device")), pages: vec![page("Voice and video", true, 1)] },
        ],
        label("Voice and video"),
        None,
        ui::text("The page.".to_string(), design::TypeStyle::Body)._boxed(),
        ui::SettingsWays {
            close: (label("Close"), Asked::Close),
            shown: screens::frame::SettingsShown::Beside,
            back: (label("Back"), Asked::Close),
        },
    )
}

/// The pages sit in groups: the server's name over the first, a heading in capitals over the next, each group said as
/// a group of options with the open one marked.
#[test]
fn settings_pages_in_groups_are_the_reference_and_each_group_says_its_name() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let image = testing::render_scene(&theme, grouped(), Size::new(700.0, 260.0), 1.0, &Scene::default());
        ui::refs!().assert(&format!("settings-groups-{scheme:?}").to_lowercase(), &image);
    }
    let mut sim = common::sim_sized(FocusScope::new(container(grouped())), Size::new(700.0, 260.0));
    let said: Vec<_> =
        semantics(&mut sim).into_iter().map(|each| (each.role, each.label.as_str().to_string())).collect();
    for expected in [
        (SemanticRole::RadioGroup, "Server S".to_string()),
        (SemanticRole::Radio { chosen: false }, "Account".into()),
        (SemanticRole::RadioGroup, "This device".into()),
        (SemanticRole::Radio { chosen: true }, "Voice and video".into()),
    ] {
        assert!(said.contains(&expected), "{expected:?} in {said:?}");
    }
}

/// The waiting circle keeps the window drawing while it turns, and holds still, asking for nothing, when the system
/// asks for less motion.
#[test]
fn the_spinner_holds_still_when_the_system_asks_for_less_motion() {
    let turning = Theme::new(Scheme::Dark, Accent::DEFAULT);
    let moving = testing::redraws(&turning, ui::spinner::<(), Renderer>(Role::OnScrim), Size::new(40.0, 40.0), 3);
    assert!(moving.iter().all(|asked| *asked == iced_core::window::RedrawRequest::NextFrame), "{moving:?}");
    let still = turning.with_motion(design::MotionPreference::Reduced);
    let asked = testing::redraws(&still, ui::spinner::<(), Renderer>(Role::OnScrim), Size::new(40.0, 40.0), 3);
    assert_eq!(asked.last(), Some(&iced_core::window::RedrawRequest::Wait), "{asked:?}");
}

/// `element` drawn at `size` after `steps`, each an event with the pointer where it then is: a drag, which a [`Scene`]'s
/// one pointer cannot make. `steps` is given where each control the interface names lies.
fn after_steps<'a, M: 'a>(
    theme: &Theme,
    element: El<'a, M>,
    size: Size,
    steps: impl FnOnce(&dyn Fn(&str) -> iced_core::Rectangle) -> Vec<(Point, iced_core::Event)>,
) -> Image {
    use iced_core::renderer::Headless;
    use iced_core::theme::Base;
    use iced_runtime::user_interface::{Cache, UserInterface};
    testing::hermetic_fonts();
    let on = container(element).padding(Space::S.px()).center(Length::Fill).class(Surfaced::On(Surface::Raised));
    let mut renderer = Renderer::new(ui::fonts::settings());
    let mut interface = UserInterface::build(on, size, Cache::default(), &mut renderer);
    let mut boxes = Vec::new();
    {
        struct Boxes<'b>(&'b mut Vec<(String, iced_core::Rectangle)>);
        impl iced_core::widget::Operation for Boxes<'_> {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced_core::widget::Operation)) {
                operate(self);
            }
            fn custom(
                &mut self,
                _id: Option<&iced_core::widget::Id>,
                bounds: iced_core::Rectangle,
                state: &mut dyn std::any::Any,
            ) {
                if let Some(said) = state.downcast_ref::<design::Semantics>() {
                    self.0.push((said.label.as_str().to_string(), bounds));
                }
            }
        }
        interface.operate(&renderer, &mut Boxes(&mut boxes));
    }
    let named = |name: &str| boxes.iter().find(|(said, _)| said == name).map(|(_, bounds)| *bounds).unwrap();
    let mut messages = iced_core::shell::Bus::new();
    let mut at = iced_core::mouse::Cursor::Unavailable;
    let redraw = iced_core::Event::Window(iced_core::window::Event::RedrawRequested(iced_core::time::Instant::now()));
    for (point, event) in steps(&named).into_iter().chain([(Point::ORIGIN, redraw)]) {
        if point != Point::ORIGIN {
            at = iced_core::mouse::Cursor::Available(point);
        }
        let _ = interface.update(
            &iced_core::window::Headless,
            &iced_core::shell::Waker::noop(),
            &[event],
            at,
            &mut renderer,
            &mut messages,
        );
    }
    let base = theme.base();
    interface.draw(&mut renderer, theme, &iced_core::renderer::Style { text_color: base.text_color }, at);
    let physical = iced_core::Size::new(size.width as u32, size.height as u32);
    Image { size: physical, rgba: renderer.screenshot(physical, 1.0, base.background_color) }
}

#[derive(Clone, Debug, PartialEq)]
enum Ordered {
    Chose,
    Moved(screens::Moved<usize>),
}

const ROLES: [&str; 5] = ["Owner", "Admin", "Moderators", "Regulars", "Member"];

/// A server's roles as the owner orders them: the owner's role and the default one stay where they are.
fn roles<'a>() -> El<'a, Ordered> {
    let rows = ROLES.iter().enumerate().map(|(key, name)| {
        (key, ListRow::<Ordered, Renderer>::new(label(name)).selected(key == 3).on_press(Ordered::Chose).view())
    });
    let may =
        |moved: &screens::Moved<usize>| ![0, 4].contains(&moved.key) && moved.before.is_some_and(|before| before != 0);
    let list = ui::reorder(label("Roles"), rows, may, Ordered::Moved);
    container(list)
        .width(Length::Fixed(design::ROLE_LIST_WIDTH))
        .class(Surfaced::Framed(Surface::Inset, design::Radius::M))
        ._boxed()
}

#[test]
fn a_dragged_row_shows_where_it_lands_in_both_schemes() {
    let drag = |named: &dyn Fn(&str) -> iced_core::Rectangle| {
        let from = named("Regulars").center();
        let admin = named("Admin");
        let to = Point::new(from.x, admin.y + admin.height);
        let moved = |position| iced_core::Event::Mouse(iced_core::mouse::Event::CursorMoved { position });
        vec![
            (from, moved(from)),
            (from, iced_core::Event::Mouse(iced_core::mouse::Event::ButtonPressed(iced_core::mouse::Button::Left))),
            (to, moved(to)),
        ]
    };
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let image = after_steps(&theme, roles(), Size::new(320.0, 260.0), drag);
        ui::refs!().assert(&format!("reorder-dragging-{scheme:?}").to_lowercase(), &image);
        let line = theme.palette().color(Role::Accent(AccentRole::Fill));
        assert!(
            count_of(&image, line) as f32 >= (design::ROLE_LIST_WIDTH - 2.0 * Space::S.px()) * design::DRAG_LINE,
            "the drop line, in the accent, across the list in {scheme:?}"
        );
    }
}

fn search<'a>(value: &'a str) -> El<'a, Ordered> {
    ui::search_field(
        label("Search members"),
        "Search by name or username",
        label("Clear search"),
        value,
        |_| Ordered::Chose,
        Ordered::Chose,
    )
}

#[test]
fn a_search_field_empty_holding_words_and_focused_is_the_reference_in_both_schemes() {
    const FIELD: Size = Size::new(320.0, 56.0);
    let words = Point::new(FIELD.width / 2.0, FIELD.height / 2.0);
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let cells = [
            cell(&theme, search(""), FIELD, &Scene::default()),
            cell(&theme, search("gr"), FIELD, &Scene::default()),
            cell(&theme, search("gr"), FIELD, &Scene { events: testing::click().to_vec(), ..Scene::hovering(words) }),
        ];
        ui::refs!().assert(&format!("search-field-{scheme:?}").to_lowercase(), &testing::stacked(&cells));
    }
}

/// The search pill under the pointer is the pill at rest, as the design draws it: nothing of its button shows past its
/// round ends.
#[test]
fn the_search_pill_under_the_pointer_is_drawn_as_at_rest() {
    let size = Size::new(design::SEARCH_FIELD_WIDTH, design::SEARCH_FIELD_HEIGHT);
    let pill = || -> Element<'static, (), Theme, Renderer> {
        ui::search_pill(label("Search Night Owls"), "Ctrl+K".into(), false, ())
    };
    for scheme in [Scheme::Dark, Scheme::Light] {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let rest = testing::render(&theme, pill(), size, 1.0);
        let middle = Point::new(size.width / 2.0, size.height / 2.0);
        let hovered = testing::render_scene(&theme, pill(), size, 1.0, &Scene::hovering(middle));
        let difference = testing::compare(&rest, &hovered);
        assert!(difference.is_none_or(testing::Difference::within_tolerance), "{scheme:?}: {difference:?}");
    }
}

/// A settings page scrolled keeps its heading line where it was, Close with it, and moves only what is under it, so
/// Close never stands over a field.
#[test]
fn a_scrolled_settings_page_keeps_its_heading_line_in_place() {
    let short = Size::new(SETTINGS.width, 240.0);
    let mut sim = common::sim_sized(FocusScope::new(container(settings(2))), short);
    let placed = |sim: &mut common::Sim<'_, Asked>, role: SemanticRole, said: &str| {
        common::boxes(sim)
            .into_iter()
            .find(|(each, _)| each.role == role && each.label.as_str() == said)
            .map(|(_, bounds)| bounds)
            .unwrap_or_else(|| panic!("{role:?} {said}"))
    };
    let named = [(SemanticRole::Heading, "Invites"), (SemanticRole::Button, "Close")];
    let before: Vec<_> = named.iter().map(|(role, said)| placed(&mut sim, *role, said)).collect();
    let table = placed(&mut sim, SemanticRole::Table, "Invites");
    sim.point_at(Point::new(SETTINGS.width - 200.0, short.height - 40.0));
    sim.simulate([iced_core::Event::Mouse(iced_core::mouse::Event::WheelScrolled {
        delta: iced_core::mouse::ScrollDelta::Pixels { x: 0.0, y: -120.0 },
    })]);
    common::settle(&mut sim);
    let after: Vec<_> = named.iter().map(|(role, said)| placed(&mut sim, *role, said)).collect();
    let moved = placed(&mut sim, SemanticRole::Table, "Invites");
    assert!(moved.y < table.y, "the page scrolled: {table:?} then {moved:?}");
    assert_eq!(before, after, "the heading line stays");
}
