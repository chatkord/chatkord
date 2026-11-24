//! The marks that isolate a placeable's direction (U+2066 to U+2069) take no room where the views shape text, and a
//! line with no right-to-left letter carries none.

use design::TypeStyle;
use iced_core::text::Wrapping;
use iced_core::widget::Tree;
use iced_core::{Size, Widget, layout};
use ui::Theme;
use ui::testing::{self, Renderer};
use words::{Sentence, Words, without_isolation};

/// The width `said` takes on one line in the views' own text, shaped as the views shape it.
fn width_of(said: &str) -> f32 {
    testing::hermetic_fonts();
    let renderer = Renderer::new(ui::fonts::settings());
    let mut line = ui::text(said.to_string(), TypeStyle::Body).wrapping(Wrapping::None);
    let mut tree = Tree::new::<(), Theme, Renderer>(&line);
    let limits = layout::Limits::new(Size::ZERO, Size::INFINITE);
    Widget::<(), Theme, Renderer>::layout(&mut line, &mut tree, &renderer, &limits);
    tree.size.width
}

#[test]
fn isolated_words_measure_as_plain() {
    let words = Words::english();
    for sentence in [Sentence::MessagePerson("עדה".into()), Sentence::Welcome("מועדון ספרים".into())] {
        let isolated = words.say(&sentence);
        let plain = without_isolation(&isolated);
        assert_ne!(isolated, plain, "the catalog isolates a right-to-left placeable: {plain}");
        assert!((width_of(&isolated) - width_of(&plain)).abs() < f32::EPSILON, "{plain}");
    }
    let latin = words.say(&Sentence::ScreenOf("Ada".into()));
    assert_eq!(latin, without_isolation(&latin), "a line with no right-to-left letter carries no marks");
}
