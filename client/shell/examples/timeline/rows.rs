//! A column whose children keep their state by key wherever it moved. iced's `keyed_column` keeps it by place while
//! the count holds, so a sliding window would hand a message the selection and hover of the one that was there.

use std::collections::HashMap;

use iced_winit::core::layout::{self, flex};
use iced_winit::core::widget::tree::{self, Tree};
use iced_winit::core::widget::{Meta, Operation};
use iced_winit::core::{
    Alignment, Element, Event, Layout, Length, Padding, Rectangle, Shell, Size, Vector, Widget, mouse, overlay,
    renderer,
};

pub(crate) struct Rows<'a, Message, Theme, Renderer> {
    keys: Vec<usize>,
    children: Vec<Element<'a, Message, Theme, Renderer>>,
    width: Length,
}

impl<'a, Message, Theme, Renderer> Rows<'a, Message, Theme, Renderer> {
    pub(crate) fn new(children: impl IntoIterator<Item = (usize, Element<'a, Message, Theme, Renderer>)>) -> Self {
        let (keys, children) = children.into_iter().unzip();
        Self { keys, children, width: Length::Shrink }
    }

    pub(crate) fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }
}

struct State {
    keys: Vec<usize>,
    cache: flex::Cache,
}

impl<Message, Theme, Renderer> Meta for Rows<'_, Message, Theme, Renderer> {}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Rows<'_, Message, Theme, Renderer>
where
    Renderer: iced_winit::core::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State { keys: self.keys.clone(), cache: flex::Cache::default() })
    }

    fn diff(&mut self, tree: &mut Tree) {
        let Tree { state, children, .. } = tree;
        let state = state.downcast_mut::<State>();
        let mut before: HashMap<usize, Tree> = state.keys.drain(..).zip(children.drain(..)).collect();
        for (key, child) in self.keys.iter().zip(&mut self.children) {
            let mut kept = before.remove(key).unwrap_or_else(|| Tree::new(&*child));
            kept.diff(child);
            children.push(kept);
        }
        state.keys.clone_from(&self.keys);
    }

    fn size(&self) -> Size<Length> {
        Size { width: self.width, height: Length::Shrink }
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        let state = tree.state.downcast_mut::<State>();
        tree.size = flex::resolve(
            flex::Axis::Vertical,
            renderer,
            limits,
            self.width,
            Length::Shrink,
            Padding::ZERO,
            0.0,
            Alignment::Start,
            &mut tree.children,
            &mut self.children,
            &mut state.cache,
        );
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds(), viewport);
        operation.traverse(&mut |operation| {
            for (child, (layout, state)) in self.children.iter_mut().zip(layout.iter_mut(&mut tree.children)) {
                child.operate(state, layout, viewport, renderer, operation);
            }
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        renderer: &Renderer,
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
        renderer: &Renderer,
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
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        for (child, (layout, state)) in self.children.iter().zip(layout.iter(&tree.children)) {
            child.draw(state, renderer, theme, style, layout, cursor, viewport);
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
        window: Size,
    ) -> Vec<overlay::Element<'b, Message, Theme, Renderer>> {
        overlay::from_children(&mut self.children, tree, layout, renderer, viewport, translation, window)
    }
}
