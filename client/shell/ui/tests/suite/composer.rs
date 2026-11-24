//! The composer: what it reports as it is typed in, and how tall it grows.

use crate::common;

use common::label;
use design::TypeStyle;
use iced_core::keyboard::{Key, key::Named};
use iced_core::widget::Tree;
use iced_core::{Point, Size, Widget, layout};
use ui::testing::{self, Renderer, click, key_press, typing};
use ui::{Composer, Theme};

#[derive(Clone, Debug, PartialEq)]
enum Said {
    Text(String),
    Send,
    Away,
}

const ROOM: Size = Size::new(400.0, 600.0);

fn composer(text: &str) -> Composer<'_, Said> {
    Composer::new(label("Message #general"), text)
        .style(TypeStyle::Message)
        .on_change(Said::Text)
        .on_submit(Said::Send)
        .on_escape(Said::Away)
}

#[test]
fn enter_sends_shift_enter_breaks_the_line_and_each_edit_is_reported() {
    let events = [
        click().to_vec(),
        typing("!"),
        vec![
            key_press(Key::Named(Named::Enter), true),
            key_press(Key::Named(Named::Enter), false),
            key_press(Key::Named(Named::Escape), false),
        ],
    ]
    .concat();
    let seen = testing::interact(composer("hi").view::<Renderer>(), ROOM, Point::new(40.0, 10.0), &events);
    assert_eq!(
        seen.messages,
        [Said::Text("hi!".into()), Said::Text("hi!\n".into()), Said::Send, Said::Away],
        "Shift+Enter is a new line, Enter alone sends, Escape is the screen's"
    );
}

fn height(text: &str) -> f32 {
    testing::hermetic_fonts();
    let renderer = Renderer::new(ui::fonts::settings());
    let mut field = composer(text);
    let mut tree = Tree::new::<Said, Theme, Renderer>(&field);
    Widget::<Said, Theme, Renderer>::diff(&mut field, &mut tree);
    Widget::<Said, Theme, Renderer>::layout(&mut field, &mut tree, &renderer, &layout::Limits::new(Size::ZERO, ROOM));
    tree.size.height
}

#[test]
fn it_grows_with_its_lines_to_eight_then_holds() {
    let line = TypeStyle::Message.line_height();
    assert!((height("one line") - line).abs() < 0.5, "{}", height("one line"));
    assert!((height("one\ntwo\nthree") - 3.0 * line).abs() < 0.5);
    let twenty = vec!["a line"; 20].join("\n");
    assert!((height(&twenty) - f32::from(design::COMPOSER_ROWS) * line).abs() < 0.5, "{}", height(&twenty));
}
