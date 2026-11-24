//! A row's words and its buttons side by side while the words keep room to be read, else the buttons on a line of
//! their own under them: a narrow window never squeezes a name to one letter a line.

use crate::Theme;
use iced_core::widget::Tree;
use iced_core::widget::operation::Operation;
use iced_core::{
    Element, Event, Layout, Length, Rectangle, Shell, Size, Vector, Widget, layout, mouse, overlay, renderer,
};

pub(crate) struct Beside<'a, Message, R> {
    /// The words, then the buttons.
    children: [Element<'a, Message, Theme, R>; 2],
    /// The least width the words keep beside the buttons.
    least: f32,
    spacing: f32,
    /// How far in the buttons start on their own line, so they sit under the words and not under what leads them.
    indent: f32,
}

/// `main` with `actions` at its right end while `main` keeps `least` wide, else `actions` under it, `indent` in.
pub(crate) fn beside<'a, Message: 'a, R: renderer::Renderer + 'a>(
    main: impl Into<Element<'a, Message, Theme, R>>,
    actions: impl Into<Element<'a, Message, Theme, R>>,
    least: f32,
    spacing: f32,
    indent: f32,
) -> Element<'a, Message, Theme, R> {
    Beside { children: [main.into(), actions.into()], least, spacing, indent }._boxed()
}

impl<Message, R: renderer::Renderer> iced_core::widget::Meta for Beside<'_, Message, R> {}

impl<Message, R: renderer::Renderer> Widget<Message, Theme, R> for Beside<'_, Message, R> {
    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut self.children);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &R, limits: &layout::Limits) {
        let width = limits.max.width;
        let [main, actions] = &mut self.children;
        // The words and the buttons are as tall as they are: a row given little height is not to squeeze them into it.
        let loose = |width: f32| layout::Limits::new(Size::ZERO, Size::new(width, f32::INFINITY));
        actions.layout(&mut tree.children[1], renderer, &loose(width));
        let wanted = tree.children[1].size;
        let left = width - wanted.width - self.spacing;
        if left >= self.least {
            main.layout(&mut tree.children[0], renderer, &loose(left));
            let words = tree.children[0].size;
            let height = words.height.max(wanted.height);
            tree.children[0].translation = Vector::new(0.0, (height - words.height) / 2.0);
            tree.children[1].translation = Vector::new(width - wanted.width, (height - wanted.height) / 2.0);
            tree.size = Size::new(width, height);
        } else {
            main.layout(&mut tree.children[0], renderer, &loose(width));
            let words = tree.children[0].size;
            actions.layout(&mut tree.children[1], renderer, &loose((width - self.indent).max(0.0)));
            let below = tree.children[1].size;
            tree.children[0].translation = Vector::ZERO;
            tree.children[1].translation = Vector::new(self.indent, words.height + self.spacing);
            tree.size = Size::new(width, words.height + self.spacing + below.height);
        }
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
            for (child, (layout, tree)) in self.children.iter_mut().zip(layout.iter_mut(&mut tree.children)) {
                child.operate(tree, layout, viewport, renderer, operation);
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
        for (child, (layout, tree)) in self.children.iter_mut().zip(layout.iter_mut(&mut tree.children)) {
            child.update(tree, event, layout, cursor, renderer, shell, viewport);
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
        self.children
            .iter()
            .zip(layout.iter(&tree.children))
            .map(|(child, (layout, tree))| child.mouse_interaction(tree, layout, cursor, viewport, renderer))
            .max()
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
        for (child, (layout, tree)) in self.children.iter().zip(layout.iter(&tree.children)) {
            child.draw(tree, renderer, theme, style, layout, cursor, viewport);
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
