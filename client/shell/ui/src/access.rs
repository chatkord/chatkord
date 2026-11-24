//! The interface as a screen reader reads it: an AccessKit tree made from what each control reports of itself in an
//! operation, so it holds only what the components already say, and nothing a screen states twice.
//!
//! A control's node is nested under the control that reported it, through the traversal it opens; its id is derived
//! from its parent's, its role's kind, its label and its place among siblings with the same two, so a control keeps
//! its id from one turn to the next however much its siblings change.

use crate::focus::Covered;
use crate::identity;
use accesskit::{Action, HasPopup, NodeId, Rect, Role, Toggled, TreeId, TreeInfo, TreeUpdate};
use design::{Label, Live, Popup, SemanticRole, Semantics};
use iced_core::widget::operation::{Focusable, Scrollable, Text, TextInput};
use iced_core::widget::{Id, Operation};
use iced_core::{Rectangle, Size, Vector};
use std::any::Any;
use std::collections::HashMap;
use words::without_isolation;

/// Where a control is and which focusable it is, for carrying out what a screen reader asks of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Control {
    /// Where it is drawn, in the window's logical pixels.
    pub bounds: Rectangle,
    /// Its place among the interface's focusables in the order iced's focus operations walk them; none when it takes
    /// no focus now.
    pub focus: Option<usize>,
}

/// Every control of a tree by its node.
pub type Controls = HashMap<NodeId, Control>;

/// The tree of everything `walk` reports, in a window `size` big in logical pixels drawn at `scale` physical pixels
/// to one, named `title`. Every node of the tree is in it, so it serves as the first update as well as any later one.
pub fn tree(size: Size, scale: f32, title: Option<&Label>, walk: impl FnOnce(&mut dyn Operation)) -> TreeUpdate {
    tree_with_controls(size, scale, title, walk).0
}

/// The same tree, and where each of its controls is, so a request for a node can be carried out.
pub fn tree_with_controls(
    size: Size,
    scale: f32,
    title: Option<&Label>,
    walk: impl FnOnce(&mut dyn Operation),
) -> (TreeUpdate, Controls) {
    let mut collect = Collect::new(size, scale, title);
    walk(&mut collect);
    collect.finish()
}

/// The operation that focuses the `target`-th focusable of the interface, and takes the focus from the rest.
pub struct FocusAt {
    target: usize,
    seen: usize,
}

impl FocusAt {
    /// Focuses the focusable at `target`, counted as iced's own focus operations count.
    pub fn new(target: usize) -> Self {
        FocusAt { target, seen: 0 }
    }
}

impl Operation for FocusAt {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn focusable(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
        if self.seen == self.target {
            state.focus();
        } else {
            state.unfocus();
        }
        self.seen += 1;
    }
}

/// A node being made: its own facts, then the children reported under it.
struct Making {
    id: NodeId,
    node: accesskit::Node,
    /// What the control said of itself; none for the window.
    role: Option<SemanticRole>,
    /// It takes the focus now, or would once the layer over it is gone.
    focusable: bool,
    /// Where it is, as the tree states.
    bounds: Rectangle,
    /// Its place among the focusables, once it has one.
    focus: Option<usize>,
    children: Vec<NodeId>,
    /// How many children so far share each role's kind and label, by their hash.
    seen: HashMap<u64, u32>,
}

/// Where a traversal reports: under which node, and how far its content is scrolled.
#[derive(Clone, Copy)]
struct Frame {
    parent: usize,
    scrolled: Vector,
}

/// The operation that makes the tree.
struct Collect {
    window: NodeId,
    made: Vec<Making>,
    frames: Vec<Frame>,
    /// The control just reported, whose traversal, if it opens one next, holds its content.
    armed: Option<usize>,
    /// How far a scrollable just reported moved its content, for the traversal that holds it.
    scrolling: Option<Vector>,
    focused: Option<usize>,
    /// How many focusables were reported so far.
    focusables: usize,
}

/// The window's node, whose id every other is derived from.
const WINDOW: usize = 0;

impl Collect {
    fn new(size: Size, scale: f32, title: Option<&Label>) -> Self {
        let mut node = accesskit::Node::new(Role::Window);
        node.set_bounds(rect(Rectangle::with_size(size)));
        // Bounds are the interface's logical pixels; the window's scale makes them the physical ones AccessKit reads.
        node.set_transform(accesskit::Affine::scale(f64::from(scale)));
        if let Some(title) = title {
            node.set_label(without_isolation(title.as_str()));
        }
        let id = identity::window_id();
        let window = Making {
            id,
            node,
            role: None,
            focusable: false,
            bounds: Rectangle::with_size(size),
            focus: None,
            children: Vec::new(),
            seen: HashMap::new(),
        };
        Collect {
            window: id,
            made: vec![window],
            frames: vec![Frame { parent: WINDOW, scrolled: Vector::ZERO }],
            armed: None,
            scrolling: None,
            focused: None,
            focusables: 0,
        }
    }

    fn frame(&self) -> Frame {
        self.frames.last().copied().unwrap_or(Frame { parent: WINDOW, scrolled: Vector::ZERO })
    }

    /// The node a focusable just reported belongs to: the control that reported it, or the control whose content it is
    /// when that control does not take the focus itself (a field inside its label).
    fn focus_owner(&self) -> Option<usize> {
        self.armed.or_else(|| {
            let parent = self.frame().parent;
            let making = self.made.get(parent)?;
            (making.role.is_some_and(SemanticRole::operated) && !making.focusable).then_some(parent)
        })
    }

    fn add(&mut self, semantics: &Semantics, bounds: Rectangle) {
        let frame = self.frame();
        let Some(parent) = self.made.get_mut(frame.parent) else { return };
        let kind = std::mem::discriminant(&semantics.role);
        let nth = parent.seen.entry(identity::sibling_key(kind, &semantics.label)).or_default();
        let id = identity::child_id(parent.id, kind, &semantics.label, *nth);
        *nth += 1;
        parent.children.push(id);
        let shown = Rectangle { x: bounds.x - frame.scrolled.x, y: bounds.y - frame.scrolled.y, ..bounds };
        self.made.push(Making {
            id,
            node: node(semantics, shown),
            role: Some(semantics.role),
            focusable: false,
            bounds: shown,
            focus: None,
            children: Vec::new(),
            seen: HashMap::new(),
        });
        self.armed = Some(self.made.len() - 1);
    }

    /// The control a text field reported belongs to, when it said it is a plain one: a secret field, or one that said
    /// nothing, gives no value.
    fn plain_field(&self) -> Option<usize> {
        let owner = self.armed.unwrap_or(self.frame().parent);
        self.made.get(owner).filter(|making| making.role == Some(SemanticRole::TextInput)).map(|_| owner)
    }

    /// Adds a plain text under the control it is in, unless that control already says it or is operated.
    fn add_text(&mut self, text: &str, bounds: Rectangle) {
        let frame = self.frame();
        let Some(parent) = self.made.get_mut(frame.parent) else { return };
        let spoken = without_isolation(text);
        let text = spoken.trim();
        if text.is_empty() || parent.role.is_some_and(SemanticRole::operated) || parent.node.label() == Some(text) {
            return;
        }
        let kind = std::mem::discriminant(&Role::Label);
        let nth = parent.seen.entry(identity::sibling_key(kind, text)).or_default();
        let id = identity::child_id(parent.id, kind, text, *nth);
        *nth += 1;
        parent.children.push(id);
        let shown = Rectangle { x: bounds.x - frame.scrolled.x, y: bounds.y - frame.scrolled.y, ..bounds };
        let mut node = accesskit::Node::new(Role::Label);
        node.set_value(text);
        node.set_bounds(rect(shown));
        self.made.push(Making {
            id,
            node,
            role: None,
            focusable: false,
            bounds: shown,
            focus: None,
            children: Vec::new(),
            seen: HashMap::new(),
        });
    }

    /// `index` is the place of the focusable just reported among all; none for a control only covered.
    fn take_focus(&mut self, focused: bool, index: Option<usize>) {
        let Some(owner) = self.focus_owner() else { return };
        if let Some(making) = self.made.get_mut(owner) {
            making.focusable = true;
            making.focus = making.focus.or(index);
        }
        if focused {
            self.focused = Some(owner);
        }
    }

    fn finish(self) -> (TreeUpdate, Controls) {
        let root = self.window;
        let focus = self.focused.and_then(|at| self.made.get(at)).map_or(root, |making| making.id);
        let controls = self
            .made
            .iter()
            .filter(|making| making.role.is_some())
            .map(|making| (making.id, Control { bounds: making.bounds, focus: making.focus }))
            .collect();
        let nodes = self
            .made
            .into_iter()
            .map(|Making { id, mut node, role, focusable, children, .. }| {
                node.set_children(children);
                if focusable {
                    node.add_action(Action::Focus);
                    if role.is_some_and(pressed) {
                        node.add_action(Action::Click);
                    }
                } else if role.is_some_and(SemanticRole::operated) {
                    node.set_disabled();
                }
                (id, node)
            })
            .collect();
        let update = TreeUpdate {
            nodes,
            tree: Some(TreeInfo { root, toolkit_name: Some("iced".into()), toolkit_version: None }),
            tree_id: TreeId::ROOT,
            focus,
        };
        (update, controls)
    }
}

impl Operation for Collect {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        let outer = self.frame();
        let parent = self.armed.take().unwrap_or(outer.parent);
        let scrolled = outer.scrolled + self.scrolling.take().unwrap_or(Vector::ZERO);
        self.frames.push(Frame { parent, scrolled });
        operate(self);
        self.frames.pop();
        self.armed = None;
        self.scrolling = None;
    }

    fn container(&mut self, _id: Option<&Id>, _bounds: Rectangle, _viewport: &Rectangle) {
        self.armed = None;
    }

    fn scrollable(
        &mut self,
        _id: Option<&Id>,
        _bounds: Rectangle,
        _content: Size,
        translation: Vector,
        _state: &mut dyn Scrollable,
    ) {
        self.armed = None;
        self.scrolling = Some(translation);
    }

    fn focusable(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
        let index = self.focusables;
        self.focusables += 1;
        self.take_focus(state.is_focused(), Some(index));
    }

    fn text_input(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn TextInput) {
        // iced hands over the raw text of a secure field too: only a field that said it is plain gives its value.
        if let Some(owner) = self.plain_field()
            && let Some(making) = self.made.get_mut(owner)
        {
            making.node.set_value(without_isolation(&state.text()));
        }
        self.armed = None;
    }

    fn text(&mut self, _id: Option<&Id>, bounds: Rectangle, text: &mut dyn Text) {
        if self.armed.is_none() {
            self.add_text(&text.text(), bounds);
        }
        self.armed = None;
    }

    fn custom(&mut self, _id: Option<&Id>, bounds: Rectangle, state: &mut dyn Any) {
        if let Some(semantics) = state.downcast_ref::<Semantics>() {
            self.add(semantics, bounds);
        } else if state.is::<Covered>() {
            self.take_focus(false, None);
        }
    }
}

/// Whether pressing the control's centre does what a person asks of it; a field or a slider is operated otherwise.
fn pressed(role: SemanticRole) -> bool {
    role.operated()
        && !matches!(role, SemanticRole::TextInput | SemanticRole::SecretInput | SemanticRole::Slider { .. })
}

/// A control's node, from what it says of itself, where it is.
fn node(semantics: &Semantics, bounds: Rectangle) -> accesskit::Node {
    let mut node = accesskit::Node::new(role(semantics.role));
    node.set_label(without_isolation(semantics.label.as_str()));
    node.set_bounds(rect(bounds));
    match semantics.role {
        SemanticRole::Radio { chosen: on }
        | SemanticRole::MenuItemRadio { chosen: on }
        | SemanticRole::MenuItemCheckbox { checked: on }
        | SemanticRole::Switch { on } => {
            node.set_toggled(if on { Toggled::True } else { Toggled::False });
        }
        SemanticRole::Slider { value, least, most } => {
            node.set_numeric_value(f64::from(value));
            node.set_min_numeric_value(f64::from(least));
            node.set_max_numeric_value(f64::from(most));
        }
        SemanticRole::Opens { layer, open } => {
            node.set_has_popup(match layer {
                Popup::Menu => HasPopup::Menu,
                Popup::Dialog => HasPopup::Dialog,
            });
            node.set_expanded(open);
        }
        SemanticRole::Button
        | SemanticRole::Link
        | SemanticRole::TextInput
        | SemanticRole::SecretInput
        | SemanticRole::Image
        | SemanticRole::Status
        | SemanticRole::Alert
        | SemanticRole::Dialog
        | SemanticRole::AlertDialog
        | SemanticRole::Menu
        | SemanticRole::MenuItem
        | SemanticRole::List
        | SemanticRole::ListItem
        | SemanticRole::Heading
        | SemanticRole::RadioGroup
        | SemanticRole::Table => {}
    }
    if let Some(live) = semantics.role.live() {
        node.set_live(match live {
            Live::Polite => accesskit::Live::Polite,
            Live::Assertive => accesskit::Live::Assertive,
        });
    }
    node
}

/// AccessKit's name for a role.
fn role(role: SemanticRole) -> Role {
    match role {
        SemanticRole::Button | SemanticRole::Opens { .. } => Role::Button,
        SemanticRole::Link => Role::Link,
        SemanticRole::TextInput => Role::TextInput,
        SemanticRole::SecretInput => Role::PasswordInput,
        SemanticRole::Image => Role::Image,
        SemanticRole::Status => Role::Status,
        SemanticRole::Alert => Role::Alert,
        SemanticRole::Dialog => Role::Dialog,
        SemanticRole::AlertDialog => Role::AlertDialog,
        SemanticRole::Menu => Role::Menu,
        SemanticRole::MenuItem => Role::MenuItem,
        SemanticRole::MenuItemRadio { .. } => Role::MenuItemRadio,
        SemanticRole::MenuItemCheckbox { .. } => Role::MenuItemCheckBox,
        SemanticRole::List => Role::List,
        SemanticRole::ListItem => Role::ListItem,
        SemanticRole::Heading => Role::Heading,
        SemanticRole::RadioGroup => Role::RadioGroup,
        SemanticRole::Radio { .. } => Role::RadioButton,
        SemanticRole::Switch { .. } => Role::Switch,
        SemanticRole::Table => Role::Table,
        SemanticRole::Slider { .. } => Role::Slider,
    }
}

fn rect(bounds: Rectangle) -> Rect {
    Rect::new(
        f64::from(bounds.x),
        f64::from(bounds.y),
        f64::from(bounds.x + bounds.width),
        f64::from(bounds.y + bounds.height),
    )
}
