//! A height a popup grows to and no further, its content then scrolling: a chooser, a list of pins.

use crate::Theme;
use iced_core::widget::{Meta, Tree};
use iced_core::{Element, Size, Widget, layout, renderer};

/// `content`, laid out no taller than `most` logical pixels.
pub(crate) fn at_most<'a, Message: 'a, R: renderer::Renderer + 'a>(
    content: impl Widget<Message, Theme, R> + 'a,
    most: f32,
) -> Element<'a, Message, Theme, R> {
    AtMost { content, most }._boxed()
}

struct AtMost<W> {
    content: W,
    most: f32,
}

impl<W> Meta for AtMost<W> {}

impl<Message, W, R> Widget<Message, Theme, R> for AtMost<W>
where
    R: renderer::Renderer,
    W: Widget<Message, Theme, R>,
{
    crate::wrap::forward!(size diff operate update mouse_interaction draw overlay);

    fn layout(&mut self, tree: &mut Tree, renderer: &R, limits: &layout::Limits) {
        let bounds = limits.bounds();
        let limits = layout::Limits::new(limits.min, Size::new(bounds.width, bounds.height.min(self.most)));
        self.content.layout(&mut tree.children[0], renderer, &limits);
        tree.size = tree.children[0].size;
    }
}
