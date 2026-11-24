//! A column whose children keep their state by key wherever it moved. iced's `keyed::Column` keeps it by place while
//! the count holds, so a list sliding over its rows would hand one message the selection and hover of another. As
//! the pointer lets go it says which rows hold selected text, so the list keeps them built while they are selected.

use std::collections::HashMap;
use std::hash::Hash;

use iced_core::layout::{self, flex};
use iced_core::widget::operation::Text as TextState;
use iced_core::widget::tree::{self, Tree};
use iced_core::widget::{Meta, Operation};
use iced_core::{
    Alignment, Element, Event, Layout, Length, Padding, Rectangle, Shell, Size, Vector, Widget, mouse, overlay,
    renderer,
};

/// Children laid out top to bottom, each keeping its state under its key.
pub(crate) struct Rows<'a, K, Message, Theme, Renderer> {
    keys: Vec<K>,
    children: Vec<Element<'a, Message, Theme, Renderer>>,
    on_selected: Option<Box<dyn Fn(Vec<K>) -> Message + 'a>>,
}

impl<'a, K, Message, Theme, Renderer> Rows<'a, K, Message, Theme, Renderer> {
    /// The children in order, each under its key; two children under one key share nothing but the first's state.
    pub(crate) fn new(children: impl IntoIterator<Item = (K, Element<'a, Message, Theme, Renderer>)>) -> Self {
        let (keys, children) = children.into_iter().unzip();
        Self { keys, children, on_selected: None }
    }

    /// What it sends as the pointer lets go, when the rows holding selected text are not those it last said.
    pub(crate) fn on_selected(mut self, on_selected: impl Fn(Vec<K>) -> Message + 'a) -> Self {
        self.on_selected = Some(Box::new(on_selected));
        self
    }
}

struct State<K> {
    keys: Vec<K>,
    cache: flex::Cache,
    /// The rows last said to hold selected text.
    selected: Vec<K>,
}

/// Whether any text under a widget is selected.
#[derive(Default)]
struct Holds(bool);

impl Operation for Holds {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn text(&mut self, _id: Option<&iced_core::widget::Id>, _bounds: Rectangle, state: &mut dyn TextState) {
        self.0 |= state.copy().is_some_and(|copied| !copied.is_empty());
    }
}

impl<K, Message, Theme, Renderer> Meta for Rows<'_, K, Message, Theme, Renderer> {}

impl<K, Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Rows<'_, K, Message, Theme, Renderer>
where
    K: Clone + Eq + Hash + 'static,
    Renderer: iced_core::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State<K>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State { keys: self.keys.clone(), cache: flex::Cache::default(), selected: Vec::new() })
    }

    fn diff(&mut self, tree: &mut Tree) {
        let Tree { state, children, .. } = tree;
        let state = state.downcast_mut::<State<K>>();
        let mut before: HashMap<K, Tree> = state.keys.drain(..).zip(children.drain(..)).collect();
        for (key, child) in self.keys.iter().zip(&mut self.children) {
            let mut kept = before.remove(key).unwrap_or_else(|| Tree::new(&*child));
            kept.diff(child);
            children.push(kept);
        }
        state.keys.clone_from(&self.keys);
    }

    fn size(&self) -> Size<Length> {
        Size { width: Length::Fill, height: Length::Shrink }
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        let state = tree.state.downcast_mut::<State<K>>();
        tree.size = flex::resolve(
            flex::Axis::Vertical,
            renderer,
            limits,
            Length::Fill,
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
        let let_go = matches!(event, Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)));
        let Some(on_selected) = self.on_selected.as_ref().filter(|_| let_go) else { return };
        let Tree { state, children, .. } = tree;
        let mut selected = Vec::new();
        for ((key, child), (layout, child_tree)) in
            self.keys.iter().zip(&mut self.children).zip(layout.iter_mut(children))
        {
            let mut holds = Holds::default();
            child.operate(child_tree, layout, viewport, renderer, &mut holds);
            if holds.0 {
                selected.push(key.clone());
            }
        }
        let state = state.downcast_mut::<State<K>>();
        if state.selected != selected {
            shell.publish(on_selected(selected.clone()));
            state.selected = selected;
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
            if layout.bounds().intersects(viewport) {
                child.draw(state, renderer, theme, style, layout, cursor, viewport);
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Renderer;
    use iced_core::Point;
    use iced_test::Simulator;

    fn rows(words: &[&'static str]) -> Element<'static, Vec<usize>, crate::Theme, Renderer> {
        let rows = words.iter().enumerate().map(|(key, said)| {
            let row = crate::text(*said, design::TypeStyle::Message).selectable(true);
            (key, row._boxed())
        });
        Rows::new(rows).on_selected(|keys| keys)._boxed()
    }

    fn drag(sim: &mut Simulator<'_, Vec<usize>, crate::Theme, Renderer>, from: Point, to: Point) {
        sim.point_at(from);
        sim.simulate([Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))]);
        sim.point_at(to);
        sim.simulate([Event::Mouse(mouse::Event::CursorMoved { position: to })]);
        sim.simulate([Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))]);
    }

    #[test]
    fn letting_go_of_a_drag_says_which_rows_hold_the_selection() {
        crate::testing::hermetic_fonts();
        let line = design::TypeStyle::Message.line_height();
        let mut sim = Simulator::with_size(
            iced_core::Settings::default(),
            Size::new(400.0, 200.0),
            rows(&["the first message", "the second message", "the third message"]),
        );
        drag(&mut sim, Point::new(2.0, line / 2.0), Point::new(60.0, line * 1.5));
        assert_eq!(sim.drain().last(), Some(vec![0, 1]), "the rows the drag crossed");
        drag(&mut sim, Point::new(300.0, line * 2.5), Point::new(300.0, line * 2.5));
        assert_eq!(sim.drain().last(), Some(Vec::new()), "a press elsewhere lets the selection go");
    }
}
