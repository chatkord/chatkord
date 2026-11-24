//! A search: a field with a magnifier at its start and, while it holds anything, a button at its end that clears it.
//! Escape clears it too while the focus is in it, and is left to the page once it is empty.

use crate::{Button, Icon, IconClass, Renderer, TextInput, Theme, icon};
use design::{ButtonVariant, Content, IconSize, Label, Role, SEARCH_RADIUS, Space};
use iced_core::keyboard::{self, Key, key::Named};
use iced_core::widget::{Meta, Tree};
use iced_core::{Alignment, Element, Event, Layout, Length, Padding, Rectangle, Shell, Widget, mouse, renderer};
use iced_widget::{Row, container};

/// A search field named `name` for a screen reader, showing `placeholder` while empty, over `value`, the text the
/// screen holds. Each change is sent through `on_input`.
///
/// While `value` is not empty a button named `clear` ("Clear search") sits at the field's end and sends `on_clear`,
/// as does Escape while the focus is in the field; an Escape on an empty field is not taken, so the page's own works.
pub fn search_field<'a, Message: Clone + 'a, R: Renderer + 'a>(
    name: Label,
    placeholder: &str,
    clear: Label,
    value: &'a str,
    on_input: impl Fn(String) -> Message + 'a,
    on_clear: Message,
) -> Element<'a, Message, Theme, R> {
    let held = !value.is_empty();
    let trailing = if held { design::HIT_TARGET + Space::Xs.px() } else { 0.0 };
    let input = TextInput::new(name, value)
        .placeholder(placeholder)
        .on_input(on_input)
        .radius(SEARCH_RADIUS)
        .leading_room(IconSize::S.px() + Space::Xs.px() + Space::Xxs.px())
        .trailing_room(trailing)
        .view::<R>();
    let magnifier = icon(Icon::Search, IconSize::S, IconClass::Role(Role::Content(Content::Secondary)));
    let mut marks: Vec<Element<'a, Message, Theme, R>> =
        vec![magnifier._boxed(), iced_widget::Space::new().width(Length::Fill)._boxed()];
    if held {
        marks.push(
            Button::new(clear)
                .icon_only(Icon::Close)
                .variant(ButtonVariant::Ghost)
                .compact()
                .on_press(on_clear.clone())
                .view::<R>(),
        );
    }
    let marks = Row::with_children(marks).align_y(Alignment::Center).padding(Padding {
        left: Space::M.px(),
        right: Space::Xs.px(),
        ..Padding::ZERO
    });
    let field =
        crate::over_field::over_field(input, container(marks).width(Length::Fill).center_y(Length::Fill)._boxed());
    ClearOnEscape { content: field, on_clear: held.then_some(on_clear) }._boxed()
}

/// A field that Escape clears while the focus is in it and it holds anything.
struct ClearOnEscape<W, Message> {
    content: W,
    /// What clears it; none while it is empty.
    on_clear: Option<Message>,
}

impl<W, Message> Meta for ClearOnEscape<W, Message> {}

impl<Message, W, R> Widget<Message, Theme, R> for ClearOnEscape<W, Message>
where
    Message: Clone,
    R: renderer::Renderer,
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
        let escape =
            matches!(event, Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(Named::Escape), .. }));
        if let (true, Some(on_clear)) = (escape, &self.on_clear) {
            let (content, child) = (&mut self.content, &mut tree.children[0]);
            // Asked before the field sees the key, which would let its focus go.
            let focused = crate::focus::focused_index(|operation| {
                content.operate(child, layout, viewport, renderer, operation);
            });
            if focused.is_some() {
                shell.publish(on_clear.clone());
                shell.capture_event();
                return;
            }
        }
        self.content.update(&mut tree.children[0], event, layout, cursor, renderer, shell, viewport);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Renderer, key_press};
    use design::Semantics;
    use iced_core::event::Status;
    use iced_core::{Point, Size};
    use iced_test::Simulator;

    #[derive(Clone, Debug, PartialEq)]
    enum Said {
        Typed(String),
        Cleared,
    }

    type Sim<'a> = Simulator<'a, Said, Theme, Renderer>;

    fn search(value: &str) -> Sim<'_> {
        crate::testing::hermetic_fonts();
        let field = search_field::<Said, Renderer>(
            Label::new("Search members").unwrap(),
            "Search by name or username",
            Label::new("Clear search").unwrap(),
            value,
            Said::Typed,
            Said::Cleared,
        );
        Simulator::with_size(iced_core::Settings::default(), Size::new(320.0, 60.0), field)
    }

    /// The names of the controls a screen reader is told of, in view order.
    fn names(sim: &mut Sim<'_>) -> Vec<String> {
        struct Names(Vec<String>);
        impl iced_core::widget::Operation for Names {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced_core::widget::Operation)) {
                operate(self);
            }
            fn custom(
                &mut self,
                _id: Option<&iced_core::widget::Id>,
                _bounds: Rectangle,
                state: &mut dyn std::any::Any,
            ) {
                if let Some(said) = state.downcast_ref::<Semantics>() {
                    self.0.push(said.label.as_str().to_string());
                }
            }
        }
        let mut names = Names(Vec::new());
        sim.operate(&mut names);
        names.0
    }

    fn click(sim: &mut Sim<'_>, at: Point) {
        sim.point_at(at);
        sim.simulate(crate::testing::click());
    }

    fn escape(sim: &mut Sim<'_>) -> Status {
        sim.simulate([key_press(Key::Named(Named::Escape), false)])[0]
    }

    /// A point on the field's words, clear of its marks.
    const ON_WORDS: Point = Point::new(120.0, 20.0);
    /// The clear button, at the field's end.
    const ON_CLEAR: Point = Point::new(320.0 - Space::Xs.px() - design::HIT_TARGET / 2.0, 20.0);

    #[test]
    fn the_clear_button_shows_only_while_it_holds_text_and_clears_it() {
        assert_eq!(names(&mut search("")), ["Search members"]);
        let mut held = search("gr");
        assert_eq!(names(&mut held), ["Search members", "Clear search"]);
        click(&mut held, ON_CLEAR);
        assert_eq!(held.drain().collect::<Vec<_>>(), [Said::Cleared]);
    }

    #[test]
    fn escape_clears_a_focused_field_and_is_left_to_the_page_once_it_is_empty() {
        let mut held = search("gr");
        assert_eq!(escape(&mut held), Status::Ignored, "the focus is elsewhere");
        click(&mut held, ON_WORDS);
        assert_eq!(escape(&mut held), Status::Captured);
        assert_eq!(held.drain().collect::<Vec<_>>(), [Said::Cleared]);
        let mut empty = search("");
        click(&mut empty, ON_WORDS);
        assert_eq!(escape(&mut empty), Status::Ignored, "the page's own Escape works");
        assert_eq!(empty.drain().collect::<Vec<_>>(), []);
    }

    #[test]
    fn what_is_typed_is_sent() {
        let mut sim = search("");
        click(&mut sim, ON_WORDS);
        sim.simulate(crate::testing::typing("g"));
        assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Typed("g".to_string())]);
    }
}
