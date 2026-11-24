//! What the behaviour tests share: a simulator over the software renderer, and questions to ask it.

use design::{Label, Semantics};
use iced_core::keyboard::{Key, key::Named};
use iced_core::widget::operation::{self, Operation, Outcome, focusable};
use iced_core::{Rectangle, Vector, Widget};
use iced_test::Simulator;
use ui::Theme;
use ui::testing::{Renderer, key_press};

pub(crate) type Sim<'a, Message> = Simulator<'a, Message, Theme, Renderer>;

pub(crate) fn label(text: &str) -> Label {
    Label::new(text).unwrap()
}

pub(crate) fn sim_sized<'a, Message: 'a>(
    widget: impl Widget<Message, Theme, Renderer> + 'a,
    size: iced_core::Size,
) -> Sim<'a, Message> {
    ui::testing::hermetic_fonts();
    Simulator::with_size(iced_core::Settings::default(), size, widget)
}

/// Lets the interface settle after it was built or rebuilt with a redraw, which is when a layer's opening or closing
/// is noticed.
pub(crate) fn settle<'a, Message: 'a>(sim: &mut Sim<'a, Message>) {
    sim.simulate([iced_core::Event::Window(
        iced_core::window::Event::RedrawRequested(iced_core::time::Instant::now()),
    )]);
}

pub(crate) fn sim<'a, Message: 'a>(widget: impl Widget<Message, Theme, Renderer> + 'a) -> Sim<'a, Message> {
    ui::testing::hermetic_fonts();
    Simulator::with_size(iced_core::Settings::default(), iced_core::Size::new(240.0, 200.0), widget)
}

/// The index, in view order, of the focused control, if any.
pub(crate) fn focused<'a, Message: 'a>(sim: &mut Sim<'a, Message>) -> Option<usize> {
    let mut count = focusable::count();
    sim.operate(&mut operation::black_box(&mut count));
    match Operation::<focusable::Count>::finish(&count) {
        Outcome::Some(count) => count.focused,
        _ => None,
    }
}

/// How many controls the Tab key can reach.
pub(crate) fn focusable_total<'a, Message: 'a>(sim: &mut Sim<'a, Message>) -> usize {
    let mut count = focusable::count();
    sim.operate(&mut operation::black_box(&mut count));
    match Operation::<focusable::Count>::finish(&count) {
        Outcome::Some(count) => count.total,
        _ => 0,
    }
}

pub(crate) fn tab<'a, Message: 'a>(sim: &mut Sim<'a, Message>, shift: bool) {
    sim.simulate([key_press(Key::Named(Named::Tab), shift)]);
}

pub(crate) fn press<'a, Message: 'a>(sim: &mut Sim<'a, Message>, key: Named) {
    sim.simulate([key_press(Key::Named(key), false)]);
}

/// The semantics every control in the interface exposes, in view order.
pub(crate) fn semantics<'a, Message: 'a>(sim: &mut Sim<'a, Message>) -> Vec<Semantics> {
    struct Collect(Vec<Semantics>);
    impl Operation for Collect {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            operate(self);
        }
        fn custom(&mut self, _id: Option<&iced_core::widget::Id>, _bounds: Rectangle, state: &mut dyn std::any::Any) {
            if let Some(semantics) = state.downcast_ref::<Semantics>() {
                self.0.push(spoken(semantics));
            }
        }
    }
    let mut collect = Collect(Vec::new());
    sim.operate(&mut collect);
    collect.0
}

/// Each control's semantics with the box it fills where the window shows it (scrolled content moved by its
/// scrollable), in view order.
pub(crate) fn boxes<'a, Message: 'a>(sim: &mut Sim<'a, Message>) -> Vec<(Semantics, Rectangle)> {
    #[derive(Default)]
    struct Collect {
        found: Vec<(Semantics, Rectangle)>,
        scrolled: Vector,
        scrolling: Option<Vector>,
    }
    impl Operation for Collect {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
            let outer = self.scrolled;
            self.scrolled = outer + self.scrolling.take().unwrap_or(Vector::ZERO);
            operate(self);
            self.scrolled = outer;
            self.scrolling = None;
        }
        fn scrollable(
            &mut self,
            _id: Option<&iced_core::widget::Id>,
            _bounds: Rectangle,
            _content: iced_core::Size,
            translation: Vector,
            _state: &mut dyn operation::Scrollable,
        ) {
            self.scrolling = Some(translation);
        }
        fn custom(&mut self, _id: Option<&iced_core::widget::Id>, bounds: Rectangle, state: &mut dyn std::any::Any) {
            if let Some(semantics) = state.downcast_ref::<Semantics>() {
                self.found.push((spoken(semantics), bounds - self.scrolled));
            }
        }
    }
    let mut collect = Collect::default();
    sim.operate(&mut collect);
    collect.found
}

/// A control's semantics as a screen reader is told them: its label without the marks isolating a placeable's
/// direction, as the accessibility tree states it.
fn spoken(semantics: &Semantics) -> Semantics {
    let label =
        Label::new(&words::without_isolation(semantics.label.as_str())).unwrap_or_else(|_| semantics.label.clone());
    Semantics { label, ..semantics.clone() }
}

/// A text as a person reads it: matched without the marks isolating a placeable's direction, which are drawn as
/// nothing.
pub(crate) struct Plain<'a>(pub &'a str);

impl iced_selector::Selector for Plain<'_> {
    type Output = iced_selector::Text;

    fn select(&mut self, candidate: iced_selector::Candidate<'_>) -> Option<Self::Output> {
        let wanted = words::without_isolation(self.0);
        match candidate {
            iced_selector::Candidate::TextInput { id, bounds, visible_bounds, state }
                if words::without_isolation(&state.text()) == wanted =>
            {
                Some(iced_selector::Text::Input { id: id.cloned(), bounds, visible_bounds })
            }
            iced_selector::Candidate::Text { id, bounds, visible_bounds, content }
                if words::without_isolation(content) == wanted =>
            {
                Some(iced_selector::Text::Raw { id: id.cloned(), bounds, visible_bounds })
            }
            _ => None,
        }
    }

    fn description(&self) -> String {
        format!("text read as {:?}", self.0)
    }
}

/// Where the focus is, in view order, and whether it shows.
pub(crate) fn focus_shown<'a, Message: 'a>(sim: &mut Sim<'a, Message>) -> Option<(usize, bool)> {
    let mut find = ui::testing::FindFocus::default();
    sim.operate(&mut find);
    find.found()
}

/// The tree a screen reader is handed of the interface, in a window `size` big at a scale of one.
pub(crate) fn access_tree<'a, Message: 'a>(sim: &mut Sim<'a, Message>, size: iced_core::Size) -> accesskit::TreeUpdate {
    ui::access::tree(size, 1.0, None, |operation| sim.operate(operation))
}

/// The one node of `tree` named `name`.
pub(crate) fn node_named<'t>(tree: &'t accesskit::TreeUpdate, name: &str) -> &'t (accesskit::NodeId, accesskit::Node) {
    let mut named = tree.nodes.iter().filter(|(_, node)| node.label() == Some(name));
    let found = named.next().unwrap_or_else(|| panic!("no node named {name:?}"));
    assert!(named.next().is_none(), "more than one node named {name:?}");
    found
}

/// The node `child` is under.
pub(crate) fn parent_of(tree: &accesskit::TreeUpdate, child: accesskit::NodeId) -> Option<accesskit::NodeId> {
    tree.nodes.iter().find(|(_, node)| node.children().contains(&child)).map(|(id, _)| *id)
}

/// A tree AccessKit takes whole: every id once, every node but the root under exactly one other and reached from the
/// root, and the focus on one of them.
pub(crate) fn well_formed(tree: &accesskit::TreeUpdate) {
    let root = tree.tree.as_ref().expect("a whole tree names its root").root;
    let ids: std::collections::HashSet<_> = tree.nodes.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids.len(), tree.nodes.len(), "every id is a node's own");
    let mut parents = std::collections::HashMap::new();
    for (id, node) in &tree.nodes {
        for child in node.children() {
            assert!(ids.contains(child), "{id:?} holds {child:?}, which is not in the tree");
            assert!(parents.insert(*child, *id).is_none(), "{child:?} is under two nodes");
        }
    }
    for id in &ids {
        let mut at = *id;
        for _ in 0..ids.len() {
            if at == root {
                break;
            }
            at = *parents.get(&at).unwrap_or_else(|| panic!("{id:?} is not reached from the root"));
        }
        assert_eq!(at, root, "{id:?} is under a loop, not the root");
    }
    assert!(ids.contains(&tree.focus), "the focus is on a node of the tree");
}
