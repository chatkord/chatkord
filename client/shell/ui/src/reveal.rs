//! A card's controls that show only when wanted, as a layer over the card. Hidden, the layer stays in the tree, so
//! Tab still reaches its controls and so shows them.

use crate::Theme;
use iced_core::widget::operation::{Operation, focusable::Focusable};
use iced_core::widget::{Tree, tree};
use iced_core::{
    Element, Event, Layout, Length, Rectangle, Shell, Size, Vector, Widget, layout, mouse, overlay, renderer,
};

/// What the widget remembers between events.
#[derive(Clone, Copy, Debug, Default)]
struct State {
    /// A control of the card or of its layer holds the focus.
    focus_within: bool,
    /// The keyboard acted last, not the pointer: a focus a click left behind shows nothing, as `:focus-visible`.
    keyboard: bool,
    /// Whether the layer was shown at the last event, so a change asks for a redraw.
    shown: bool,
}

/// `content` with `layer` over it, the layer laid out in the content's bounds.
pub(crate) struct Reveal<'a, Message, R> {
    /// The card, then its layer.
    children: [Element<'a, Message, Theme, R>; 2],
    held: bool,
    /// The pointer over the content shows the layer.
    hover: bool,
}

/// Show `layer` over `content`, and give it input, only while the pointer is over it, the keyboard put the focus on
/// the card or in the layer, or it is `held` (a popover from it is open).
pub fn reveal<'a, Message: 'a, R: renderer::Renderer + 'a>(
    content: impl Into<Element<'a, Message, Theme, R>>,
    layer: impl Into<Element<'a, Message, Theme, R>>,
    held: bool,
) -> Element<'a, Message, Theme, R> {
    Reveal { children: [content.into(), layer.into()], held, hover: true }._boxed()
}

/// Show `layer` over `content`, and give it input, only while `shown`, or while the keyboard put the focus in it: what
/// covers the whole window, which the pointer is always over.
pub fn reveal_when<'a, Message: 'a, R: renderer::Renderer + 'a>(
    content: impl Into<Element<'a, Message, Theme, R>>,
    layer: impl Into<Element<'a, Message, Theme, R>>,
    shown: bool,
) -> Element<'a, Message, Theme, R> {
    Reveal { children: [content.into(), layer.into()], held: shown, hover: false }._boxed()
}

/// Whether a control inside what it visits holds the focus.
#[derive(Default)]
struct FindFocus(bool);

impl Operation for FindFocus {
    fn focusable(&mut self, _id: Option<&iced_core::widget::Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
        self.0 |= state.is_focused();
    }

    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }
}

/// What a hidden layer lets through: the focus, so Tab reaches its controls; not its words or names, so nothing reads a
/// control nobody can see.
struct FocusOnly<'o>(&'o mut dyn Operation);

impl Operation for FocusOnly<'_> {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        self.0.traverse(&mut |inner| operate(&mut FocusOnly(inner)));
    }

    fn container(&mut self, id: Option<&iced_core::widget::Id>, bounds: Rectangle, viewport: &Rectangle) {
        self.0.container(id, bounds, viewport);
    }

    fn focusable(&mut self, id: Option<&iced_core::widget::Id>, bounds: Rectangle, state: &mut dyn Focusable) {
        self.0.focusable(id, bounds, state);
    }
}

impl<Message, R: renderer::Renderer> Reveal<'_, Message, R> {
    fn shown(&self, state: &State, layout: Layout, cursor: mouse::Cursor) -> bool {
        self.held || (state.focus_within && state.keyboard) || (self.hover && cursor.is_over(layout.bounds()))
    }

    /// Whether a control of the card or of its layer holds the focus.
    fn focus_within(&mut self, tree: &mut Tree, layout: Layout, viewport: &Rectangle, renderer: &R) -> bool {
        let mut find = FindFocus::default();
        for (child, (layout, tree)) in self.children.iter_mut().zip(layout.iter_mut(&mut tree.children)) {
            child.operate(tree, layout, viewport, renderer, &mut find);
        }
        find.0
    }
}

impl<Message, R: renderer::Renderer> iced_core::widget::Meta for Reveal<'_, Message, R> {}

impl<Message, R: renderer::Renderer> Widget<Message, Theme, R> for Reveal<'_, Message, R> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut self.children);
    }

    fn size(&self) -> Size<Length> {
        self.children[0].size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &R, limits: &layout::Limits) {
        let [content, layer] = &mut self.children;
        content.layout(&mut tree.children[0], renderer, limits);
        let size = tree.children[0].size;
        layer.layout(&mut tree.children[1], renderer, &layout::Limits::new(Size::ZERO, size));
        tree.size = size;
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &R,
        operation: &mut dyn Operation,
    ) {
        let shown = self.held || tree.state.downcast_ref::<State>().shown;
        operation.traverse(&mut |operation| {
            let mut children = self.children.iter_mut().zip(layout.iter_mut(&mut tree.children));
            if let Some((content, (layout, tree))) = children.next() {
                content.operate(tree, layout, viewport, renderer, operation);
            }
            if let Some((layer, (layout, tree))) = children.next() {
                if shown {
                    layer.operate(tree, layout, viewport, renderer, operation);
                } else {
                    layer.operate(tree, layout, viewport, renderer, &mut FocusOnly(operation));
                }
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
        let focus_within = self.focus_within(tree, layout, viewport, renderer);
        let state = tree.state.downcast_mut::<State>();
        state.focus_within = focus_within;
        match event {
            Event::Keyboard(iced_core::keyboard::Event::KeyPressed { .. }) => state.keyboard = true,
            Event::Mouse(mouse::Event::ButtonPressed(_)) | Event::Touch(_) => state.keyboard = false,
            _ => {}
        }
        let shown = self.shown(state, layout, cursor);
        {
            let mut children = layout.iter_mut(&mut tree.children);
            let (Some((content_layout, content_tree)), Some((layer_layout, layer_tree))) =
                (children.next(), children.next())
            else {
                return;
            };
            let [content, layer] = &mut self.children;
            // The event that hides the layer reaches it too, so what it holds (a tooltip) sees the pointer go.
            if shown || state.shown {
                layer.update(layer_tree, event, layer_layout, cursor, renderer, shell, viewport);
            }
            if !shell.is_event_captured() {
                content.update(content_tree, event, content_layout, cursor, renderer, shell, viewport);
            }
        }

        // The keyboard may have moved the focus in or out of the layer with this event.
        let focus_within = self.focus_within(tree, layout, viewport, renderer);
        tree.state.downcast_mut::<State>().focus_within = focus_within;
        let shown = self.shown(tree.state.downcast_ref(), layout, cursor);
        let state = tree.state.downcast_mut::<State>();
        if shown != state.shown {
            state.shown = shown;
            shell.request_redraw();
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
        // The layer is on top, and counts only while it is shown.
        let shown = self.shown(tree.state.downcast_ref(), layout, cursor);
        self.children
            .iter()
            .zip(layout.iter(&tree.children))
            .enumerate()
            .rev()
            .filter(|(at, _)| *at == 0 || shown)
            .map(|(_, (child, (layout, tree)))| child.mouse_interaction(tree, layout, cursor, viewport, renderer))
            .find(|interaction| *interaction != mouse::Interaction::None)
            .unwrap_or_default()
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
        let mut children = self.children.iter().zip(layout.iter(&tree.children));
        if let Some((content, (layout, tree))) = children.next() {
            content.draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
        if self.shown(tree.state.downcast_ref(), layout, cursor)
            && let Some((layer, (layout, tree))) = children.next()
        {
            renderer.with_layer(*viewport, |renderer| {
                layer.draw(tree, renderer, theme, style, layout, cursor, viewport);
            });
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
        overlay::from_children(&mut self.children, tree, layout, renderer, viewport, translation, window)
    }
}
