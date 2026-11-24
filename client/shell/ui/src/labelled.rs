//! A control's name, for what reads the interface other than by sight, wrapped round a widget that is not a button (a
//! field, a picture) so it cannot be made without one; [`crate::Focus`] carries the same for what it wraps.

use crate::Theme;
use crate::wrap::{Filtered, Hide};
use design::Semantics;
use iced_core::widget::{Meta, Operation};
use iced_core::{Layout, Rectangle, Widget, renderer};

pub(crate) struct Labelled<W> {
    content: W,
    semantics: Semantics,
    hide: Hide,
}

impl<W> Labelled<W> {
    pub(crate) fn new(content: W, semantics: Semantics) -> Self {
        Labelled { content, semantics, hide: Hide::default() }
    }

    /// The Tab key passes over it: for a control that is disabled.
    pub(crate) fn without_focus(mut self) -> Self {
        self.hide.focus = true;
        self
    }
}

impl<W> Meta for Labelled<W> {}

impl<Message, W, R> Widget<Message, Theme, R> for Labelled<W>
where
    R: renderer::Renderer,
    W: Widget<Message, Theme, R>,
{
    crate::wrap::forward!(size diff layout update mouse_interaction draw overlay);

    fn operate(
        &mut self,
        tree: &mut iced_core::widget::Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &R,
        operation: &mut dyn Operation,
    ) {
        operation.custom(None, layout.bounds(), &mut self.semantics);
        // Its own traversal, so what reads the interface finds the content under what names it.
        let (content, child, hide) = (&mut self.content, &mut tree.children[0], self.hide);
        operation.traverse(&mut |inner| {
            content.operate(child, layout, viewport, renderer, &mut Filtered { inner, hide });
        });
    }
}
