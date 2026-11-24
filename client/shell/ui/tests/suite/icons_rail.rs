//! Icons, icon-only buttons and the server rail. A rail item is drawn in any colour a server may name, so its contrast
//! is measured from the pixels it draws across colours, not from the roles it is made of.

use crate::common;

use common::{focusable_total, label, press, semantics, sim, tab};
use design::{
    Accent, AccentRole, AvatarSize, ButtonVariant, Content, IconSize, Oklch, Ratio, Rgb, Role, Scheme, SemanticRole,
    Surface,
};
use iced_core::keyboard::key::Named;
use iced_core::{Element, Length, Point, Size};
use iced_widget::{Column, container};
use screens::RailState;
use ui::testing::{self, Image, Renderer, Scene};
use ui::{Button, FocusScope, Icon, IconClass, RailItem, Surfaced, Theme, icon};

/// The English the views are drawn in, for as long as they borrow it.
static WORDS: std::sync::LazyLock<words::Words> = std::sync::LazyLock::new(words::Words::english);

type El<'a, M> = Element<'a, M, Theme, Renderer>;

fn count_near(image: &Image, colour: Rgb, tolerance: u8) -> usize {
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

fn rgb(pixel: [u8; 4]) -> Rgb {
    Rgb::new(pixel[0], pixel[1], pixel[2])
}

#[test]
fn every_icon_file_is_a_variant_and_every_variant_has_its_file() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icons");
    let mut files: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|entry| entry.unwrap().path().file_stem().map(|stem| stem.to_string_lossy().into_owned()))
        .filter(|stem| !stem.starts_with("LICENSE"))
        .collect();
    files.sort();
    files.dedup();
    let mut variants: Vec<String> = Icon::ALL.iter().map(|icon| icon.file_stem().to_string()).collect();
    variants.sort();
    assert_eq!(files, variants);
    assert_eq!(Icon::ALL.len(), 84);
}

/// Each icon draws something, in the colour of the role it was asked for, at every size, in both schemes.
#[test]
fn every_icon_draws_in_its_roles_colour_at_every_size() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        for glyph in Icon::ALL {
            for size in IconSize::ALL {
                let tone = Role::Content(Content::Primary);
                let on = container(icon(glyph, size, IconClass::Role(tone)))
                    .center(Length::Fill)
                    .class(Surfaced::On(Surface::Raised));
                let side = size.px() + 8.0;
                let image = testing::render::<()>(&theme, on, Size::new(side, side), 1.0);
                let want = theme.palette().color(tone);
                let surface = theme.palette().color(Role::Surface(Surface::Chrome));
                let drawn = image
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|p| {
                        p[0].abs_diff(surface.r) > 12 || p[1].abs_diff(surface.g) > 12 || p[2].abs_diff(surface.b) > 12
                    })
                    .count();
                assert!(drawn > 6, "{glyph:?} at {size:?} in {scheme:?} drew nothing");
                // A thin stroke at 16 px never reaches full coverage, but its strongest pixels are near its colour.
                assert!(count_near(&image, want, 70) > 0, "{glyph:?} at {size:?} in {scheme:?} is not in {want:?}");
            }
        }
    }
}

#[test]
fn the_icon_set_is_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let row: Vec<_> = Icon::ALL
            .iter()
            .map(|glyph| {
                let on = container(icon(*glyph, IconSize::L, IconClass::Role(Role::Content(Content::Primary))))
                    .center(Length::Fill)
                    .class(Surfaced::On(Surface::Raised));
                testing::render::<()>(&theme, on, Size::new(32.0, 32.0), 1.0)
            })
            .collect();
        ui::refs!().assert(&format!("icons-{scheme:?}").to_lowercase(), &testing::side_by_side(&row));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Pressed;

fn mute<'a>(enabled: bool) -> El<'a, Pressed> {
    let mut button = Button::new(label("Mute microphone")).icon_only(Icon::Mic).variant(ButtonVariant::Ghost);
    if enabled {
        button = button.on_press(Pressed);
    }
    button.view()
}

#[test]
fn an_icon_only_button_has_its_name_is_reached_by_tab_and_pressed_by_space() {
    let mut sim = sim(FocusScope::new(mute(true)));
    let found = semantics(&mut sim);
    assert_eq!((found[0].role, found[0].label.as_str()), (SemanticRole::Button, "Mute microphone"));
    assert_eq!(focusable_total(&mut sim), 1);
    tab(&mut sim, false);
    press(&mut sim, Named::Space);
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Pressed]);
    assert_eq!(focusable_total(&mut common::sim(FocusScope::new(mute(false)))), 0);
}

#[test]
fn icon_buttons_in_each_state_are_the_reference_and_the_icon_follows_the_look() {
    let centre = Point::new(30.0, 30.0);
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let cell = |scene: &Scene, enabled: bool| {
            let on =
                container(FocusScope::new(mute(enabled))).center(Length::Fill).class(Surfaced::On(Surface::Raised));
            testing::render_scene(&theme, on, Size::new(60.0, 60.0), 1.0, scene)
        };
        let idle = cell(&Scene::default(), true);
        let disabled = cell(&Scene::default(), false);
        let row = [
            idle.clone(),
            cell(&Scene::hovering(centre), true),
            cell(&Scene::pressing(centre), true),
            disabled.clone(),
            cell(&Scene::tabbing(false), true),
        ];
        ui::refs!().assert(&format!("icon-button-{scheme:?}").to_lowercase(), &testing::side_by_side(&row));
        let primary = theme.palette().color(Role::Content(Content::Primary));
        let off = theme.palette().color(Role::Content(Content::Disabled));
        assert!(count_near(&idle, primary, 30) > 8, "its icon is the text colour");
        assert!(count_near(&disabled, off, 30) > 8, "a disabled one's is the disabled colour");
        assert!(
            count_near(&disabled, primary, 30) == 0
                || count_near(&disabled, primary, 30) < count_near(&disabled, off, 30)
        );
    }
}

fn item<'a>(open: bool, state: RailState, colour: Rgb) -> El<'a, Pressed> {
    RailItem::new(label("Server S"), colour).open(open).state(state).on_press(Pressed).view::<Renderer>(&WORDS)
}

/// The colour of a server that picked none.
fn unpicked() -> Rgb {
    Accent::DEFAULT.seed()
}

const CELL: Size = Size::new(84.0, 72.0);

fn on_chrome<'a>(element: El<'a, Pressed>) -> impl iced_core::Widget<Pressed, Theme, Renderer> + 'a {
    container(element).center(Length::Fill).class(Surfaced::On(Surface::Chrome))
}

#[test]
fn a_rail_item_in_each_state_is_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let cells = [
            (false, RailState::Online, unpicked()),
            (true, RailState::Online, unpicked()),
            (false, RailState::Connecting, unpicked()),
            (false, RailState::NotAnswering, unpicked()),
            (true, RailState::SignedOut, unpicked()),
            (true, RailState::Connecting, Rgb::hex(0xE5A50A)),
        ]
        .map(|(open, state, colour)| testing::render(&theme, on_chrome(item(open, state, colour)), CELL, 1.0));
        ui::refs!().assert(&format!("rail-{scheme:?}").to_lowercase(), &testing::side_by_side(&cells));
    }
}

#[test]
fn a_rail_item_is_reached_by_tab_pressed_by_space_and_says_what_its_marks_say() {
    let rail = |state| {
        FocusScope::new(Column::with_children([
            item(true, state, unpicked()),
            item(false, RailState::Online, unpicked()),
        ]))
    };
    let mut sim = sim(rail(RailState::Online));
    assert_eq!(focusable_total(&mut sim), 2);
    tab(&mut sim, false);
    tab(&mut sim, false);
    press(&mut sim, Named::Space);
    assert_eq!(sim.drain().collect::<Vec<_>>(), [Pressed]);
    let first = |state| {
        let mut sim = common::sim(rail(state));
        semantics(&mut sim)
            .into_iter()
            .find(|each| each.role == SemanticRole::ListItem)
            .map(|each| each.label.as_str().to_string())
            .unwrap()
    };
    assert_eq!(first(RailState::Online), "Server S");
    assert_eq!(first(RailState::NewPassword), "Server S", "a temporary password: the rail says nothing more");
    assert_eq!(
        first(RailState::NotAnswering),
        "Server S, not answering",
        "a dot alone says it to no one who cannot see it"
    );
    assert_eq!(first(RailState::SignedOut), "Server S, signed out");
    assert_eq!(first(RailState::Connecting), "Server S, connecting");
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Rail {
    Open,
    Menu,
    Add,
    Settings,
}

/// A left click opens the server, a right click asks for its menu, and the "+" after the servers adds one.
#[test]
fn a_rail_item_opens_its_menu_on_a_right_click_and_the_plus_adds_a_server() {
    let rail = || {
        FocusScope::new(Column::with_children([
            RailItem::new(label("Server S"), unpicked())
                .on_press(Rail::Open)
                .on_menu(Rail::Menu)
                .view::<Renderer>(&WORDS),
            ui::add_tile::<Rail, Renderer>(&label("Add a server"), Rail::Add),
        ]))
    };
    let middle = Point::new(30.0, RailItem::<Rail>::HEIGHT / 2.0);
    let below = Point::new(30.0, RailItem::<Rail>::HEIGHT * 1.5);
    let seen = |at, events: &[iced_core::Event]| testing::interact(rail(), Size::new(60.0, 200.0), at, events).messages;
    assert_eq!(seen(middle, &testing::right_click()), [Rail::Menu]);
    assert_eq!(seen(middle, &testing::click()), [Rail::Open]);
    assert_eq!(seen(below, &testing::click()), [Rail::Add]);
    let mut sim = sim(rail());
    let named: Vec<_> = semantics(&mut sim)
        .into_iter()
        .filter(|each| each.role != SemanticRole::Image)
        .map(|each| (each.role, each.label.as_str().to_string()))
        .collect();
    assert_eq!(named, [(SemanticRole::ListItem, "Server S".into()), (SemanticRole::Button, "Add a server".into())]);
}

#[test]
fn the_plus_is_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let image = testing::render(
            &theme,
            on_chrome(ui::add_tile::<Pressed, Renderer>(&label("Add a server"), Pressed)),
            CELL,
            1.0,
        );
        ui::refs!().assert(&format!("rail-add-{scheme:?}").to_lowercase(), &image);
    }
}

/// The gear at the rail's foot: quiet on the chrome, and on a chosen row's fill while the settings it opens are shown.
#[test]
fn the_gear_is_the_reference_in_both_schemes_and_a_press_opens_the_settings() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        for current in [false, true] {
            let gear = ui::gear_tile::<Pressed, Renderer>(&label("Settings"), current, Pressed);
            let image = testing::render(&theme, on_chrome(gear), CELL, 1.0);
            let shown = if current { "current" } else { "idle" };
            ui::refs!().assert(&format!("rail-gear-{shown}-{scheme:?}").to_lowercase(), &image);
        }
    }
    let gear = || FocusScope::new(ui::gear_tile::<Rail, Renderer>(&label("Settings"), false, Rail::Settings));
    let middle = Point::new(30.0, RailItem::<Rail>::HEIGHT / 2.0);
    assert_eq!(testing::interact(gear(), Size::new(60.0, 60.0), middle, &testing::click()).messages, [Rail::Settings]);
    let mut sim = sim(gear());
    let named: Vec<_> =
        semantics(&mut sim).into_iter().map(|each| (each.role, each.label.as_str().to_string())).collect();
    assert_eq!(named, [(SemanticRole::Button, "Settings".into())]);
}

/// The gear and the "+" are no server, so the pointer on one fills its face as it fills a pane's row, and no bar is
/// drawn at the rail's edge.
#[test]
fn the_gear_and_the_plus_fill_under_the_pointer_with_no_bar_beside_them() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let hover = design::quiet_row(false, design::Interaction::Hover).fill.expect("a hovered row is filled");
        let fill = theme.palette().color(hover);
        let bar = theme.palette().color(Role::Content(Content::Primary));
        let centre = Point::new(CELL.width / 2.0, CELL.height / 2.0);
        // Inside the face, clear of its glyph and of the "+"'s dashed edge.
        let face_x = (centre.x - AvatarSize::Rail.px() / 2.0 + 8.0) as u32;
        let bar_x = ((CELL.width - design::RAIL_WIDTH) / 2.0 + design::RAIL_BAR.0 / 2.0) as u32;
        type Tile = fn() -> Element<'static, Pressed, Theme, Renderer>;
        let tiles: [(&str, Tile); 2] = [
            ("gear", || ui::gear_tile(&label("Settings"), false, Pressed)),
            ("plus", || ui::add_tile(&label("Add a server"), Pressed)),
        ];
        for (name, tile) in tiles {
            let shown = |scene: &Scene| testing::render_scene(&theme, on_chrome(tile()), CELL, 1.0, scene);
            let (idle, hovered) = (shown(&Scene::default()), shown(&Scene::hovering(centre)));
            let at = |image: &Image| image.pixel(face_x, centre.y as u32)[..3].to_vec();
            assert_ne!(at(&idle), [fill.r, fill.g, fill.b], "{name} idle is quiet in {scheme:?}");
            assert_eq!(at(&hovered), [fill.r, fill.g, fill.b], "{name} filled under the pointer in {scheme:?}");
            let barred = (0..hovered.size.height).filter(|y| hovered.pixel(bar_x, *y)[..3] == [bar.r, bar.g, bar.b]);
            assert_eq!(barred.count(), 0, "no bar beside the {name} in {scheme:?}");
        }
    }
}

/// The colours a server may pick: every hue at several chromas and lightnesses, and the ones most likely to break a
/// contrast.
fn colours() -> Vec<Rgb> {
    let swept = (0..360).step_by(15).flat_map(|hue| {
        [(0.0, 0.5), (0.15, 0.6), (0.3, 0.95), (0.15, 0.2)]
            .map(move |(chroma, l)| Oklch { l, c: chroma, h: f64::from(hue) }.to_rgb())
    });
    let hostile = [0xFFFF00, 0xFF0000, 0x00FF00, 0x0000FF, 0x808080, 0x000000, 0xFFFFFF, 0x7F7F00].map(Rgb::hex);
    swept.chain(hostile).collect()
}

/// A connecting rail item's picture is the colour as picked, its initials read on it by 4.5:1, and the ring round it
/// is the window's accent ring, told from the chrome by 3:1.
#[test]
fn under_every_colour_the_rail_items_picture_is_as_picked_its_initials_read_and_its_ring_is_found() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let surface = theme.palette().color(Role::Surface(Surface::Chrome));
        let ring = theme.palette().color(Role::Accent(AccentRole::Ring));
        for fill in colours() {
            let accent = fill;
            let image =
                testing::render::<Pressed>(&theme, on_chrome(item(false, RailState::Connecting, fill)), CELL, 1.0);
            // At least half the picture, whatever its initials cover.
            let half = (AvatarSize::Rail.px() * AvatarSize::Rail.px() / 2.0) as usize;
            assert!(
                count_near(&image, fill, 0) > half,
                "the picture is the server's colour {fill:?} as picked in {scheme:?}"
            );
            assert!(
                count_near(&image, ring, 0) > 100,
                "the ring is the accent's ring {ring:?} round {accent:?} in {scheme:?}"
            );
            assert!(
                Ratio::between(ring, surface).meets(Ratio::NON_TEXT),
                "ring {ring:?} on {surface:?} under {accent:?} in {scheme:?}"
            );
            // The farthest pixel from the circle's colour is the surface or the ring; look only inside the circle.
            let inside: Vec<_> = (0..image.size.height)
                .flat_map(|y| (0..image.size.width).map(move |x| (x, y)))
                .filter(|(x, y)| {
                    let (dx, dy) = (*x as f32 - CELL.width / 2.0, *y as f32 - CELL.height / 2.0);
                    dx * dx + dy * dy < 17.0 * 17.0
                })
                .map(|(x, y)| rgb(image.pixel(x, y)))
                .collect();
            let text = inside
                .iter()
                .copied()
                .max_by(|a, b| Ratio::between(*a, fill).value().total_cmp(&Ratio::between(*b, fill).value()))
                .unwrap();
            assert!(
                Ratio::between(text, fill).meets(Ratio::TEXT),
                "initials {text:?} on {fill:?} under {accent:?} in {scheme:?}"
            );
        }
    }
}

/// A server not open shows the short bar at the rail's edge under the pointer, and the keyboard's focus is the same
/// look: the bar is the rail's hover, and nothing is drawn outside the item.
#[test]
fn a_rail_item_shows_the_short_bar_under_the_pointer_and_when_tab_reaches_it() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let one = |scene: &Scene| {
            let rail = FocusScope::new(Column::with_children([item(false, RailState::Online, unpicked())]));
            let on = container(rail).center(Length::Fill).class(Surfaced::On(Surface::Chrome));
            testing::render_scene(&theme, on, CELL, 1.0, scene)
        };
        let idle = one(&Scene::default());
        let hovered = one(&Scene::hovering(Point::new(CELL.width / 2.0, CELL.height / 2.0)));
        let tabbed = one(&Scene::tabbing(false));
        let bar = theme.palette().color(Role::Content(Content::Primary));
        let left = |image: &Image| {
            let item_left = (CELL.width - design::RAIL_WIDTH) / 2.0;
            let through_the_bar = (item_left + design::RAIL_BAR.0 / 2.0) as u32;
            (0..image.size.height).filter(|y| image.pixel(through_the_bar, *y)[..3] == [bar.r, bar.g, bar.b]).count()
        };
        assert_eq!(left(&idle), 0, "no bar on a server not open, in {scheme:?}");
        assert!(left(&hovered) > 0, "the short bar under the pointer, in {scheme:?}");
        let differs = testing::compare(&tabbed, &hovered).expect("the same size");
        assert!(differs.within_tolerance(), "tabbed looks hovered in {scheme:?}: {differs:?}");
        ui::refs!()
            .assert(&format!("rail-hover-{scheme:?}").to_lowercase(), &testing::side_by_side(&[idle, hovered, tabbed]));
    }
}
