//! A watched share: the picture with its card beside it once the viewer knows what it is, the card flagging a
//! software decode, and the picture keeping the room when there is no card yet.

use crate::common;

use common::{semantics, sim_sized};
use design::{Accent, Role, Scheme, SemanticRole, Surface};
use iced_core::{Element, Length, Size, Widget};
use iced_widget::container;
use screens::{Route, ShareCard, ShareState};
use ui::testing::{self, Image, Renderer};
use ui::{Surfaced, Theme, watch};

/// The English the views are drawn in, for as long as they borrow it.
static WORDS: std::sync::LazyLock<words::Words> = std::sync::LazyLock::new(words::Words::english);

const WINDOW: Size = Size::new(720.0, 300.0);

fn picture<'a>() -> Element<'a, (), Theme, Renderer> {
    container(iced_widget::Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .class(Surfaced::Fill {
            fill: Role::Surface(Surface::Inset),
            text: Role::Surface(Surface::Inset),
            radius: design::Radius::None,
        })
        ._boxed()
}

/// The sharer's id, and the name the place shows them by.
const ADA: &str = "ada";
const ADA_NAME: &str = "Ada";

fn card(decoder: Route) -> ShareCard {
    ShareCard {
        who: screens::harness::member(ADA),
        width: 1920,
        height: 1080,
        frame_rate: Some(60),
        dropped: 4,
        codec: "H.264".into(),
        decoder,
        refused: 0,
        freezes: (0, std::time::Duration::ZERO),
        state: ShareState::Live,
    }
}

fn render(theme: &Theme, card: Option<&ShareCard>) -> Image {
    testing::render::<()>(
        theme,
        watch::view::<(), Renderer>("watching Ada".into(), card.map(|card| (card, ADA_NAME)), picture(), &WORDS),
        WINDOW,
        1.0,
    )
}

#[test]
fn the_card_is_beside_the_picture_once_there_is_one_and_the_picture_keeps_the_rest() {
    let mut sim = sim_sized(watch::view::<(), Renderer>("watching Ada".into(), None, picture(), &WORDS), WINDOW);
    assert!(semantics(&mut sim).is_empty(), "no card, nothing said of one");
    let with = card(Route::Hardware { via: "VA-API".into() });
    let mut sim = sim_sized(
        watch::view::<(), Renderer>("watching Ada".into(), Some((&with, ADA_NAME)), picture(), &WORDS),
        WINDOW,
    );
    let found = semantics(&mut sim);
    assert!(found.iter().any(|each| each.role == SemanticRole::Status
        && each.label.as_str().contains("1920")
        && each.label.as_str().starts_with(ADA_NAME)));
}

#[test]
fn the_watch_screen_is_the_reference_and_a_software_decode_is_flagged() {
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        let hardware = card(Route::Hardware { via: "VA-API".into() });
        let software = card(Route::Software);
        let row = [render(&theme, None), render(&theme, Some(&hardware)), render(&theme, Some(&software))];
        ui::refs!().assert(&format!("watch-{scheme:?}").to_lowercase(), &testing::side_by_side(&row));
        let warning = theme.palette().color(Role::Status(design::Status::Warning, design::StatusRole::Text));
        let count = |image: &Image| {
            (0..image.size.height)
                .flat_map(|y| (0..image.size.width).map(move |x| (x, y)))
                .filter(|&(x, y)| image.pixel(x, y) == [warning.r, warning.g, warning.b, 255])
                .count()
        };
        assert_eq!(count(&row[1]), 0, "hardware carries no warning");
        assert!(count(&row[2]) > 10, "software is marked in {scheme:?}");
    }
}
