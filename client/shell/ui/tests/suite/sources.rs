//! Our own picker of what to share: a tab each of Screens and Applications, cards two across, a selected one in the
//! accent's ring and Share only once one is selected, with a stand-in painting the host's pictures.

use crate::common;

use common::{semantics, sim_sized};
use design::{Accent, Role, Scheme, SemanticRole, Surface};
use iced_core::{Element, Length, Size, Widget};
use iced_widget::container;
use screens::{Painting, Source, SourceKey, SourceKind, Sources, SourcesTab};
use ui::picture::Painted;
use ui::testing::{self, Renderer};
use ui::{PickerSends, Surfaced, Theme, source_picker};

/// The English the views are drawn in, for as long as they borrow it.
static WORDS: std::sync::LazyLock<words::Words> = std::sync::LazyLock::new(words::Words::english);

type El<'a, M> = Element<'a, M, Theme, Renderer>;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Said {
    Tab(SourcesTab),
    Select(SourceKey),
    Share,
    Close,
}

const WINDOW: Size = Size::new(1280.0, 768.0);

fn listed() -> Vec<Source> {
    let screen = |key: &str, size, still| Source {
        key: SourceKey(key.into()),
        kind: SourceKind::Screen,
        title: None,
        size: Some(size),
        icon: None,
        still: Some(Painting(still)),
    };
    let app = |key: &str, title: &str, at: u64| Source {
        key: SourceKey(key.into()),
        kind: SourceKind::Application,
        title: Some(title.into()),
        size: None,
        icon: Some(Painting(100 + at)),
        still: Some(Painting(200 + at)),
    };
    vec![
        screen("m1", (3840, 2160), 1),
        screen("m2", (1080, 1920), 2),
        app("w1", "Plant swap wiki – Browser", 1),
        app("w2", "Recipes for Saturday – Notes, with a title long enough to be cut", 2),
        app("w3", "poster.png – Photo editor", 3),
        app("w4", "Terminal", 4),
        app("w5", "Main menu – Game", 5),
    ]
}

/// The host's pictures: a still is a filled block, an icon the accent's fill, so where each sits is seen.
fn painted<'a>(painted: &Painted) -> El<'a, Said> {
    let fill = match painted {
        Painted::Still(Painting(id), _) if (100..200).contains(id) => Role::Accent(design::AccentRole::Fill),
        _ => Role::Surface(Surface::Overlay),
    };
    container(iced_widget::Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .class(Surfaced::Fill { fill, text: Role::Content(design::Content::Primary), radius: design::Radius::None })
        ._boxed()
}

fn picker<'a>(sources: &Sources) -> El<'a, Said> {
    let sends = PickerSends {
        tab: Box::new(Said::Tab),
        select: Box::new(Said::Select),
        share: Said::Share,
        close: Said::Close,
    };
    let panel = source_picker::<Said, Renderer>(sources, WINDOW, sends, &|picture| painted(&picture), &WORDS);
    container(panel).center(Length::Fill).class(Surfaced::On(Surface::Base))._boxed()
}

#[test]
fn the_picker_in_each_tab_and_with_a_card_selected_is_the_reference_in_both_schemes() {
    let states = [
        ("screens", Sources::new(SourcesTab::Screens, listed(), None)),
        ("apps", Sources::new(SourcesTab::Applications, listed(), None)),
        ("chosen", Sources::new(SourcesTab::Applications, listed(), Some(SourceKey("w2".into())))),
    ];
    for scheme in Scheme::ALL {
        let theme = Theme::new(scheme, Accent::DEFAULT);
        for (name, sources) in &states {
            let image = testing::render(&theme, picker(sources), WINDOW, 1.0);
            ui::refs!().assert(&format!("picker-{name}-{scheme:?}").to_lowercase(), &image);
        }
    }
}

/// What a click on the control that says `words` sends.
fn clicked(sim: &mut common::Sim<'_, Said>, words: &str) -> Vec<Said> {
    sim.click(words).unwrap();
    sim.drain().collect()
}

#[test]
fn every_screen_is_a_card_however_many_there_are_and_the_grid_scrolls_to_them() {
    let screens: Vec<Source> = (1..=6)
        .map(|n| Source {
            key: SourceKey(format!("m{n}")),
            kind: SourceKind::Screen,
            title: None,
            size: Some((1920, 1080)),
            icon: None,
            still: Some(Painting(n)),
        })
        .collect();
    let sources = Sources::new(SourcesTab::Screens, screens, None);
    let mut sim = sim_sized(picker(&sources), WINDOW);
    let names: Vec<String> = semantics(&mut sim)
        .into_iter()
        .filter(|each| matches!(each.role, SemanticRole::Radio { .. }))
        .map(|each| each.label.as_str().to_string())
        .filter(|name| name.starts_with("Screen "))
        .collect();
    assert_eq!(names, (1..=6).map(|n| format!("Screen {n}")).collect::<Vec<_>>());
    struct Overflow(bool);
    impl iced_core::widget::Operation for Overflow {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced_core::widget::Operation)) {
            operate(self);
        }
        fn scrollable(
            &mut self,
            _: Option<&iced_core::widget::Id>,
            bounds: iced_core::Rectangle,
            content: Size,
            _: iced_core::Vector,
            _: &mut dyn iced_core::widget::operation::Scrollable,
        ) {
            self.0 |= content.height > bounds.height;
        }
    }
    let mut overflow = Overflow(false);
    sim.operate(&mut overflow);
    assert!(overflow.0, "three rows of screens are taller than the grid's room: it scrolls to them");
}

#[test]
fn a_press_selects_a_card_and_share_sends_only_once_one_is() {
    let nothing = Sources::new(SourcesTab::Screens, listed(), None);
    let mut sim = sim_sized(picker(&nothing), WINDOW);
    let found = semantics(&mut sim);
    let cards: Vec<_> = found.iter().filter(|each| matches!(each.role, SemanticRole::Radio { .. })).collect();
    assert!(cards.iter().any(|card| card.label.as_str() == "Screen 1"), "{found:?}");
    assert!(
        found.iter().any(|each| each.role == SemanticRole::Dialog && each.label.as_str() == "Choose what to share")
    );
    assert_eq!(clicked(&mut sim, "Screen 2"), [Said::Select(SourceKey("m2".into()))]);
    assert!(clicked(&mut sim, "Share").is_empty(), "nothing selected: Share sends nothing");
    assert_eq!(clicked(&mut sim, "Applications"), [Said::Tab(SourcesTab::Applications)]);
    assert_eq!(clicked(&mut sim, "Cancel"), [Said::Close]);

    let chosen = Sources::new(SourcesTab::Screens, listed(), Some(SourceKey("m1".into())));
    let mut sim = sim_sized(picker(&chosen), WINDOW);
    assert_eq!(clicked(&mut sim, "Share"), [Said::Share]);
    let found = semantics(&mut sim);
    assert!(
        found.iter().any(|each| each.role == SemanticRole::Radio { chosen: true } && each.label.as_str() == "Screen 1")
    );
}

/// The picker over a window `size` big, as a dialog: two cards across while each keeps its least, else one, and
/// nothing past the window's sides; in the least window it is the reference.
#[test]
fn the_picker_fits_a_small_window_with_its_cards_one_or_two_across() {
    let sources = Sources::new(SourcesTab::Applications, listed(), Some(SourceKey("w2".into())));
    let least = Size::new(design::WINDOW_LEAST.0, design::WINDOW_LEAST.1);
    for (size, across) in [(least, 1), (Size::new(480.0, 360.0), 2), (WINDOW, 2)] {
        let sends = PickerSends {
            tab: Box::new(Said::Tab),
            select: Box::new(Said::Select),
            share: Said::Share,
            close: Said::Close,
        };
        let panel = source_picker::<Said, Renderer>(&sources, size, sends, &|picture| painted(&picture), &WORDS);
        let screen = container(iced_widget::Space::new()).width(Length::Fill).height(Length::Fill);
        let over = ui::dialog(screen._boxed(), Some(panel), Said::Close);
        let mut sim = common::sim_sized(over, size);
        let boxes = common::boxes(&mut sim);
        for (said, bounds) in &boxes {
            assert!(
                bounds.x >= 0.0 && bounds.x + bounds.width <= size.width,
                "{size:?}: {} at {bounds:?} is past the window's side",
                said.label.as_str()
            );
        }
        let tabs = [WORDS.phrase(words::Phrase::PickScreens), WORDS.phrase(words::Phrase::PickApplications)];
        let cards: Vec<_> = boxes
            .iter()
            .filter(|(said, _)| matches!(said.role, design::SemanticRole::Radio { .. }))
            .filter(|(said, _)| !tabs.iter().any(|tab| *tab == said.label.as_str()))
            .collect();
        let first_row = cards.iter().filter(|(_, bounds)| (bounds.y - cards[0].1.y).abs() < 1.0).count();
        assert_eq!(first_row, across, "{size:?}: cards across");
    }
    let sends = PickerSends {
        tab: Box::new(Said::Tab),
        select: Box::new(Said::Select),
        share: Said::Share,
        close: Said::Close,
    };
    let panel = source_picker::<Said, Renderer>(&sources, least, sends, &|picture| painted(&picture), &WORDS);
    let screen = container(iced_widget::Space::new()).width(Length::Fill).height(Length::Fill);
    let image = testing::render(
        &Theme::new(Scheme::Dark, Accent::DEFAULT),
        ui::dialog(screen._boxed(), Some(panel), Said::Close),
        least,
        1.0,
    );
    ui::refs!().assert("picker-least-dark", &image);
}
