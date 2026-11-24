//! A card floating at a point the core gave it, over what lies under it there and nothing elsewhere: a drag anywhere on
//! it that its own controls do not take sends where its top left is going.

use crate::{Renderer, Theme};
use iced_core::widget::{Operation, Tree, tree};
use iced_core::{
    Element, Event, Layout, Length, Point, Rectangle, Shell, Size, Vector, Widget, layout, mouse, overlay, renderer,
};

/// `card` with its top left at `at` in the room it is given, which it fills; a drag on it sends `on_move` with the
/// point its top left is dragged to.
pub(crate) fn floating<'a, Message: 'a, R: Renderer + 'a>(
    card: Element<'a, Message, Theme, R>,
    at: Point,
    on_move: impl Fn(Point) -> Message + 'a,
) -> Element<'a, Message, Theme, R> {
    Floating { card: [card], at: Vector::new(at.x, at.y), on_move: Box::new(on_move) }._boxed()
}

struct Floating<'a, Message, R> {
    card: [Element<'a, Message, Theme, R>; 1],
    at: Vector,
    on_move: Box<dyn Fn(Point) -> Message + 'a>,
}

/// Where in the card the pointer took hold of it, while a press that began on it is held.
#[derive(Default)]
struct Held(Option<Vector>);

impl<Message, R: Renderer> iced_core::widget::Meta for Floating<'_, Message, R> {}

impl<Message, R: Renderer> Widget<Message, Theme, R> for Floating<'_, Message, R> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Held>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Held::default())
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut self.card);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &R, limits: &layout::Limits) {
        tree.size = layout::atomic(limits, Length::Fill, Length::Fill);
        self.card[0].layout(&mut tree.children[0], renderer, &layout::Limits::new(Size::ZERO, tree.size));
        tree.children[0].translation = self.at;
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &R,
        operation: &mut dyn Operation,
    ) {
        operation.traverse(&mut |operation| {
            if let Some((layout, tree)) = layout.iter_mut(&mut tree.children).next() {
                self.card[0].operate(tree, layout, viewport, renderer, operation);
            }
        });
    }

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
        let origin = layout.bounds().position();
        let Some((card_layout, card_tree)) = layout.iter_mut(&mut tree.children).next() else { return };
        let card = card_layout.bounds();
        self.card[0].update(card_tree, event, card_layout, cursor, renderer, shell, viewport);
        if shell.is_event_captured() {
            return;
        }
        let held = tree.state.downcast_mut::<Held>();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(point) = cursor.position_over(card) {
                    held.0 = Some(point - card.position());
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if let Some(grip) = held.0 {
                    let to = *position - grip;
                    shell.publish((self.on_move)(Point::new(to.x - origin.x, to.y - origin.y)));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if held.0.take().is_some() => {
                shell.capture_event();
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &R,
    ) -> mouse::Interaction {
        if tree.state.downcast_ref::<Held>().0.is_some() {
            return mouse::Interaction::Grabbing;
        }
        let Some((card_layout, card_tree)) = layout.iter(&tree.children).next() else {
            return mouse::Interaction::None;
        };
        // Outside the card, nothing: what lies under the rest of its room keeps the pointer.
        if !cursor.is_over(card_layout.bounds()) {
            return mouse::Interaction::None;
        }
        match self.card[0].mouse_interaction(card_tree, card_layout, cursor, viewport, renderer) {
            mouse::Interaction::None | mouse::Interaction::Idle => mouse::Interaction::Grab,
            interaction => interaction,
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut R,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if let Some((layout, tree)) = layout.iter(&tree.children).next() {
            self.card[0].draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout,
        renderer: &R,
        viewport: &Rectangle,
        translation: Vector,
        window: Size,
    ) -> Vec<overlay::Element<'b, Message, Theme, R>> {
        overlay::from_children(&mut self.card, tree, layout, renderer, viewport, translation, window)
    }
}
