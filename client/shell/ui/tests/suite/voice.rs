//! The voice tile, the grid of tiles and the share card in every state they show, with the ring, the words and the
//! caption's contrast over the brightest and darkest pictures measured from the pixels.

use crate::common;

use common::{semantics, sim};
use design::{Accent, Content, Ratio, Rgb, Role, Scheme, SemanticRole, Surface};
use iced_core::{Element, Length, Size};
use iced_widget::container;
use screens::{Connection, Route, ShareCard, ShareState, Tile};
use ui::testing::{self, Image, Renderer, Scene};
use ui::{Surfaced, Theme, share_card, voice_grid, voice_tile};

/// The English the views are drawn in, for as long as they borrow it.
static WORDS: std::sync::LazyLock<words::Words> = std::sync::LazyLock::new(words::Words::english);

type El<'a> = Element<'a, (), Theme, Renderer>;

const TILE: (f32, f32) = (216.0, 130.0);

fn person(edit: impl FnOnce(&mut Tile)) -> Tile {
    let mut tile = Tile::new("Ada Lovelace");
    edit(&mut tile);
    tile
}

fn states() -> Vec<(&'static str, Tile)> {
    vec![
        ("online", person(|_| {})),
        ("speaking", person(|t| t.speaking = true)),
        ("muted", person(|t| t.muted = true)),
        ("deafened", person(|t| t.deafened = true)),
        ("connecting", person(|t| t.connection = Connection::Connecting)),
        ("failed", person(|t| t.connection = Connection::Failed)),
        ("reconnecting", person(|t| t.connection = Connection::Reconnecting)),
    ]
}

/// A picture the size of the space it is given, all in `role`'s colour.
fn picture<'a>(role: Role) -> El<'a> {
    use iced_core::Widget as _;
    container(iced_widget::Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .class(Surfaced::Fill { fill: role, text: role, radius: design::Radius::None })
        ._boxed()
}

fn render_tile(theme: &Theme, tile: &Tile, picture: Option<El<'_>>) -> Image {
    render_tile_in(theme, tile, picture, false)
}

fn render_tile_in(theme: &Theme, tile: &Tile, picture: Option<El<'_>>, alone: bool) -> Image {
    let on = container(voice_tile::<(), Renderer>(tile, TILE, picture, None, alone, None, &WORDS))
        .center(Length::Fill)
        .class(Surfaced::On(Surface::Base));
    testing::render_scene(theme, on, Size::new(TILE.0 + 16.0, TILE.1 + 16.0), 1.0, &Scene::default())
}

fn near(image: &Image, colour: Rgb, tolerance: u8) -> usize {
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

#[test]
fn every_tile_state_is_the_reference_in_both_schemes() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let mut row: Vec<_> = states().iter().map(|(_, tile)| render_tile(&theme, tile, None)).collect();
        let sharing = person(|t| t.sharing = true);
        row.push(render_tile(&theme, &sharing, Some(picture(Role::Surface(Surface::Overlay)))));
        let mine = person(|t| (t.own, t.colour) = (true, design::PersonColour::Picked(design::Rgb::hex(0x3D7FD6))));
        row.push(render_tile(&theme, &mine, None));
        let alone = true;
        row.push(render_tile_in(&theme, &mine, None, alone));
        let muted_deafened_sharing = person(|t| (t.muted, t.deafened, t.sharing) = (true, true, true));
        row.push(render_tile(&theme, &muted_deafened_sharing, None));
        ui::refs!().assert(&format!("tile-{scheme:?}").to_lowercase(), &testing::side_by_side(&row));
    }
}

/// The ring is the speaking colour and only someone heard has it; the word is there as well, so speaking is not told
/// by colour alone.
#[test]
fn speaking_is_a_ring_in_the_speaking_colour_and_a_word_and_nothing_else_has_the_ring() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let ring = theme.palette().color(Role::Speaking);
        let base = theme.palette().color(Role::Surface(Surface::Base));
        assert!(Ratio::between(ring, base).meets(Ratio::NON_TEXT));
        for (name, tile) in states() {
            let drawn = near(&render_tile(&theme, &tile, None), ring, 0);
            if name == "speaking" {
                assert!(drawn > 200, "a ring in {ring:?}: {drawn} pixels in {scheme:?}");
            } else {
                assert_eq!(drawn, 0, "{name} has no ring in {scheme:?}");
            }
        }
        let speaking = person(|t| t.speaking = true);
        let mut sim = sim::<()>(voice_tile::<(), Renderer>(&speaking, TILE, None, None, false, None, &WORDS));
        let found = semantics(&mut sim);
        let said: Vec<_> = found.iter().map(|each| (each.role, each.label.as_str().to_string())).collect();
        assert!(said.contains(&(SemanticRole::ListItem, "Ada Lovelace, Speaking".into())), "{said:?}");
    }
}

/// The name over a picture is read on the caption's layer whatever the picture is: the brightest and the darkest
/// the theme can give, the pixels of the label against the pixels of the layer under it.
#[test]
fn the_name_over_the_brightest_and_the_darkest_picture_is_read_at_4_5_to_1() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let on_scrim = theme.palette().color(Role::OnScrim);
        for tone in [Role::Content(Content::Primary), Role::Surface(Surface::Inset), Role::Surface(Surface::Raised)] {
            let tile = person(|t| t.sharing = true);
            let image = render_tile(&theme, &tile, Some(picture(tone)));
            // The name's pill is inset from the tile's bottom left: the scrim is its darkest pixels, the label its
            // brightest.
            let (top, bottom) = (TILE.1 as u32 - 26, TILE.1 as u32 - 4);
            let strip: Vec<Rgb> = (top..bottom)
                .flat_map(|y| (24..image.size.width / 2).map(move |x| (x, y)))
                .map(|(x, y)| {
                    let p = image.pixel(x, y);
                    Rgb::new(p[0], p[1], p[2])
                })
                .collect();
            let label_pixel = strip.iter().copied().max_by(|a, b| a.luminance().total_cmp(&b.luminance())).unwrap();
            let layer = strip.iter().copied().min_by(|a, b| a.luminance().total_cmp(&b.luminance())).unwrap();
            assert!(
                near(&image, on_scrim, 0) > 20,
                "the label is the on-scrim colour {on_scrim:?} in {scheme:?} over {tone:?}"
            );
            assert!(
                Ratio::between(label_pixel, layer).meets(Ratio::TEXT),
                "label {label_pixel:?} over layer {layer:?} (picture {tone:?}) in {scheme:?}"
            );
        }
    }
}

/// A name too long for its tile stays on one line and inside it; the tile is as large as every other.
#[test]
fn a_long_name_keeps_the_tile_its_size_and_ends_in_an_ellipsis() {
    let theme = Theme::new(Scheme::Dark, Accent::DEFAULT);
    let short = render_tile(&theme, &person(|_| {}), None);
    for name in ["x".repeat(110), "Α".repeat(80), "Καλημέρα κόσμε ".repeat(7), "مرحبا بالعالم ".repeat(8)]
    {
        let long = render_tile(&theme, &Tile::new(name), None);
        assert_eq!(long.size, short.size);
        let base = theme.palette().color(Role::Surface(Surface::Base));
        let margin = ((long.size.width as f32 - TILE.0) / 2.0) as u32 - 1;
        for y in 0..long.size.height {
            for x in (0..margin).chain(long.size.width - margin..long.size.width) {
                assert_eq!(long.pixel(x, y), [base.r, base.g, base.b, 255], "spilled at {x},{y}");
            }
        }
    }
}

fn crowd(count: usize) -> Vec<Tile> {
    (0..count)
        .map(|n| {
            let mut tile = Tile::new(format!("Guest {}", n + 1));
            tile.speaking = n == 1;
            tile.muted = n == 2;
            tile
        })
        .collect()
}

#[test]
fn the_grid_for_each_count_is_the_reference_and_a_crowd_scrolls_instead_of_shrinking() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let size = Size::new(640.0, 400.0);
        let mut row = Vec::new();
        for count in [1, 2, 4, 7] {
            let people = crowd(count);
            row.push(testing::render::<()>(
                &theme,
                voice_grid::<_, (), Renderer>(
                    &people.iter().collect::<Vec<_>>(),
                    size.width,
                    size.height,
                    |person, size, alone| voice_tile(person, size, None, None, alone, None, &WORDS),
                ),
                size,
                1.0,
            ));
        }
        ui::refs!().assert(&format!("grid-{scheme:?}").to_lowercase(), &testing::side_by_side(&row));
        // Too many people for the space: the tiles stay as wide as the least worth drawing and the grid scrolls.
        let many = crowd(40);
        let small = Size::new(480.0, 360.0);
        let image = testing::render::<()>(
            &theme,
            voice_grid::<_, (), Renderer>(
                &many.iter().collect::<Vec<_>>(),
                small.width,
                small.height,
                |person, size, alone| voice_tile(person, size, None, None, alone, None, &WORDS),
            ),
            small,
            1.0,
        );
        ui::refs!().assert(&format!("grid-crowd-{scheme:?}").to_lowercase(), &image);
    }
}

/// The sharer's id, and the name the place shows them by.
const ADA: &str = "ada";
const ADA_NAME: &str = "Ada";

fn card(decoder: Route, state: ShareState) -> ShareCard {
    ShareCard {
        who: screens::harness::member(ADA),
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
fn the_share_card_in_each_state_is_the_reference_and_a_software_path_is_flagged() {
    let hardware = || Route::Hardware { via: "VA-API".into() };
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let cards = [
            card(hardware(), ShareState::Live),
            card(Route::Software, ShareState::Live),
            card(hardware(), ShareState::Reconnecting),
            // No rate measured yet.
            ShareCard { frame_rate: None, ..card(Route::Software, ShareState::Live) },
        ];
        let row: Vec<_> = cards
            .iter()
            .map(|card| {
                let on = container(share_card::<(), Renderer>(card, ADA_NAME, &WORDS))
                    .center(Length::Fill)
                    .class(Surfaced::On(Surface::Base));
                testing::render::<()>(&theme, on, Size::new(300.0, 160.0), 1.0)
            })
            .collect();
        ui::refs!().assert(&format!("share-card-{scheme:?}").to_lowercase(), &testing::side_by_side(&row));
        let warning = theme.palette().color(Role::Status(design::Status::Warning, design::StatusRole::Text));
        assert_eq!(near(&row[0], warning, 0), 0, "a hardware path carries no warning colour");
        assert!(
            near(&row[1], warning, 0) > 10,
            "a decode that fell back to software is marked in the warning colour in {scheme:?}"
        );
    }
}

#[test]
fn the_share_card_says_what_it_shows_to_a_screen_reader() {
    let card = ShareCard { dropped: 3, ..card(Route::Software, ShareState::Live) };
    let mut sim = sim::<()>(share_card::<(), Renderer>(&card, ADA_NAME, &WORDS));
    let names: Vec<_> = semantics(&mut sim)
        .into_iter()
        .map(|each| (each.role, words::without_isolation(each.label.as_str())))
        .collect();
    assert!(
        names.contains(&(
            SemanticRole::Status,
            "Ada\u{2019}s stream, 1920 \u{d7} 1080 \u{b7} 60 fps, 3 pictures not painted".into()
        )),
        "{names:?}"
    );
}
