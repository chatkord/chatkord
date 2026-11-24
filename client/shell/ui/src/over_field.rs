//! A field with what is laid over it: a unit, a clear or show button, marks at its ends.

use crate::{Renderer, Theme};
use iced_core::widget::Tree;
use iced_core::{Element, Event, Layout, Length, Rectangle, Shell, Widget, mouse, shell::Bus};
use iced_widget::stack;

/// `field` with `over` laid over it, as tall as the field. iced's stack hands an event to no layer under one once
/// anything in the window took it, so a field in one never hears that a press on another field moved the focus: its
/// layers hear the event as if no other widget had taken it.
pub(crate) fn over_field<'a, Message: 'a, R: Renderer + 'a>(
    field: Element<'a, Message, Theme, R>,
    over: Element<'a, Message, Theme, R>,
) -> Element<'a, Message, Theme, R> {
    Apart { content: stack([field, over]).height(Length::Shrink) }._boxed()
}

/// Its content's events, taken or not by what came before it.
struct Apart<W> {
    content: W,
}

impl<W> iced_core::widget::Meta for Apart<W> {}

impl<Message, W, R> Widget<Message, Theme, R> for Apart<W>
where
    R: iced_core::Renderer,
    W: Widget<Message, Theme, R>,
{
    crate::wrap::forward!(size diff layout operate mouse_interaction draw overlay);

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        renderer: &R,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let mut bus = Bus::new();
        let mut own = shell.local(&mut bus);
        self.content.update(&mut tree.children[0], event, layout, cursor, renderer, &mut own, viewport);
        shell.merge(own, std::convert::identity);
    }
}

#[cfg(test)]
mod tests {
    use super::over_field;
    use crate::testing::Renderer;
    use crate::{FocusScope, TextInput, Theme};
    use design::Label;
    use iced_core::{Element, Length, Point, Size, Widget};
    use iced_test::Simulator;
    use iced_widget::{Row, container, text};

    #[derive(Clone, Debug, PartialEq)]
    struct Typed(u8, String);

    fn field(at: u8, value: &str) -> Element<'_, Typed, Theme, Renderer> {
        let input = TextInput::new(Label::new("Size").unwrap(), value)
            .on_input(move |typed| Typed(at, typed))
            .trailing_room(40.0)
            .view::<Renderer>();
        let unit = container(text("MB")).align_right(Length::Fill).center_y(Length::Fill)._boxed();
        container(over_field(input, unit)).width(Length::Fixed(200.0))._boxed()
    }

    #[test]
    fn a_press_on_one_field_takes_the_keyboard_from_another_with_marks_over_it() {
        crate::testing::hermetic_fonts();
        let row = Row::with_children([field(0, "100"), field(1, "1024")]).spacing(20.0);
        let mut sim: Simulator<'_, Typed, Theme, Renderer> =
            Simulator::with_size(iced_core::Settings::default(), Size::new(480.0, 80.0), FocusScope::new(row));
        sim.point_at(Point::new(300.0, 15.0));
        sim.simulate(crate::testing::click());
        sim.point_at(Point::new(60.0, 15.0));
        sim.simulate(crate::testing::click());
        sim.simulate(crate::testing::typing("5"));
        let typed: Vec<Typed> = sim.into_messages().collect();
        assert!(matches!(&typed[..], [Typed(0, _)]), "the field pressed last, alone: {typed:?}");
    }
}
