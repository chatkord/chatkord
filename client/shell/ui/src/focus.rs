//! Keyboard focus for iced, which binds no Tab: [`Focus`] makes a control focusable, [`FocusScope`] moves the focus
//! round those it holds. A scope is a widget, so a test drives it, and a dialog's traps the focus by taking the key.

use crate::Theme;
use crate::identity::Lineage;
use accesskit::NodeId;
use design::Semantics;
use iced_core::keyboard::{self, Key, key::Named};
use iced_core::mouse;
use iced_core::renderer;
use iced_core::widget::operation::{self, Operation, Outcome, focusable::Focusable};
use iced_core::widget::{Tree, tree};
use iced_core::{Event, Layout, Point, Rectangle, Shell, Vector, Widget};
use std::any::Any;

/// What a focused control remembers.
#[derive(Clone, Copy, Debug, Default)]
struct State {
    focused: bool,
    /// Whether the focus shows: the keyboard put it here, a pointer did not.
    visible: bool,
}

impl Focusable for State {
    fn is_focused(&self) -> bool {
        self.focused
    }

    /// Focus handed over by a traversal is the keyboard's.
    fn focus(&mut self) {
        *self = State { focused: true, visible: true };
    }

    fn unfocus(&mut self) {
        *self = State::default();
    }
}

/// What a focused control makes of a key it takes for itself; none for one it leaves.
type NoKeys<Message> = fn(&Key) -> Option<Message>;

/// A control that can hold the keyboard focus.
pub(crate) struct Focus<Message, W, K = NoKeys<Message>> {
    content: W,
    semantics: Semantics,
    on_activate: Option<Message>,
    on_key: Option<K>,
}

impl<Message, W> Focus<Message, W> {
    /// `semantics` is what a screen reader will say of it; a control without one cannot be made.
    pub(crate) fn new(content: W, semantics: Semantics) -> Self {
        Focus { content, semantics, on_activate: None, on_key: None }
    }

    /// The keys this takes while it has the focus, each made a message by `on_key` or left to what holds it. A
    /// control that takes keys is focusable.
    pub(crate) fn on_key<K: Fn(&Key) -> Option<Message>>(self, on_key: K) -> Focus<Message, W, K> {
        let Focus { content, semantics, on_activate, .. } = self;
        Focus { content, semantics, on_activate, on_key: Some(on_key) }
    }
}

impl<Message, W, K> Focus<Message, W, K> {
    fn focusable(&self) -> bool {
        self.on_activate.is_some() || self.on_key.is_some()
    }

    /// What Space or Enter sends while this has the focus. Without it, or keys of its own, the control is not
    /// focusable: a disabled control is skipped by Tab.
    pub(crate) fn on_activate(mut self, message: Message) -> Self {
        self.on_activate = Some(message);
        self
    }
}

impl<Message, W, K> iced_core::widget::Meta for Focus<Message, W, K> {}

impl<Message, W, K, R> Widget<Message, Theme, R> for Focus<Message, W, K>
where
    Message: Clone,
    R: renderer::Renderer,
    W: Widget<Message, Theme, R>,
    K: Fn(&Key) -> Option<Message>,
{
    crate::wrap::forward!(size diff layout mouse_interaction overlay);

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &R,
        operation: &mut dyn Operation,
    ) {
        operation.custom(None, layout.bounds(), &mut self.semantics);
        if self.focusable() {
            let state: &mut State = tree.state.downcast_mut();
            operation.focusable(None, layout.bounds(), state);
            // Whether the focus shows, which `Focusable` cannot say, for the operations that read and set it.
            operation.custom(None, layout.bounds(), state);
        }
        // Its own traversal, so what reads the interface finds the content under the control.
        let content = &mut self.content;
        let child = &mut tree.children[0];
        operation.traverse(&mut |operation| content.operate(child, layout, viewport, renderer, operation));
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
        // A press decides whether this is focused: inside takes the focus without a ring, outside
        // gives it up. The content still sees the press (a button needs it to be pressed).
        if let Event::Mouse(mouse::Event::ButtonPressed(_)) | Event::Touch(_) = event {
            let state: &mut State = tree.state.downcast_mut();
            let was = *state;
            *state = if cursor.is_over(layout.bounds()) && self.focusable() {
                State { focused: true, visible: false }
            } else {
                State::default()
            };
            if state.focused != was.focused || state.visible != was.visible {
                shell.request_redraw();
            }
        }

        // A control whose focus shows is told the pointer is on it, except of the pointer's own events: a press goes
        // where the pointer really is.
        let seen = match event {
            Event::Mouse(_) | Event::Touch(_) => cursor,
            _ => shown_as_hovered(tree.state.downcast_ref(), layout, cursor),
        };
        self.content.update(&mut tree.children[0], event, layout, seen, renderer, shell, viewport);
        if shell.is_event_captured() {
            return;
        }

        let state: &State = tree.state.downcast_ref();
        let Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) = event else { return };
        if !state.focused {
            return;
        }
        let taken = match (key, &self.on_activate, &self.on_key) {
            (_, _, Some(on_key)) if let Some(message) = on_key(key) => Some(message),
            (Key::Named(Named::Space | Named::Enter), Some(message), _) => Some(message.clone()),
            _ => None,
        };
        if let Some(message) = taken {
            shell.publish(message);
            shell.capture_event();
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
        let seen = shown_as_hovered(tree.state.downcast_ref(), layout, cursor);
        self.content.draw(&tree.children[0], renderer, theme, style, layout, seen, viewport);
    }
}

/// The pointer as a control is told of it: over its centre while its focus shows, so it draws its own hover look.
fn shown_as_hovered(state: &State, layout: Layout, cursor: mouse::Cursor) -> mouse::Cursor {
    if state.focused && state.visible { mouse::Cursor::Available(layout.bounds().center()) } else { cursor }
}

/// What a control under an open layer reports in place of its focus: it takes the focus once the layer is gone, but
/// Tab does not reach it now.
pub(crate) struct Covered;

/// What a screen under an open menu or popover reports: all a reader reads, its controls as [`Covered`].
pub(crate) struct Covering<'o>(pub &'o mut dyn Operation);

impl Operation for Covering<'_> {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        self.0.traverse(&mut |inner| operate(&mut Covering(inner)));
    }

    fn container(&mut self, id: Option<&iced_core::widget::Id>, bounds: Rectangle, viewport: &Rectangle) {
        self.0.container(id, bounds, viewport);
    }

    fn scrollable(
        &mut self,
        id: Option<&iced_core::widget::Id>,
        bounds: Rectangle,
        content: iced_core::Size,
        translation: iced_core::Vector,
        state: &mut dyn operation::Scrollable,
    ) {
        self.0.scrollable(id, bounds, content, translation, state);
    }

    fn focusable(&mut self, id: Option<&iced_core::widget::Id>, bounds: Rectangle, _state: &mut dyn Focusable) {
        self.0.custom(id, bounds, &mut Covered);
    }

    fn custom(&mut self, id: Option<&iced_core::widget::Id>, bounds: Rectangle, state: &mut dyn Any) {
        if !state.is::<State>() {
            self.0.custom(id, bounds, state);
        }
    }
}

/// A region the Tab key moves the focus around in.
pub struct FocusScope<W> {
    content: W,
    arrows: bool,
}

impl<W> FocusScope<W> {
    /// A scope over `content` that Tab and Shift+Tab move through, the arrow keys not.
    pub fn new(content: W) -> Self {
        FocusScope { content, arrows: false }
    }

    /// The Up and Down arrow keys move the focus too, as in a menu.
    pub fn arrows(mut self) -> Self {
        self.arrows = true;
        self
    }
}

impl<W> iced_core::widget::Meta for FocusScope<W> {}

/// Which way the focus moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Forward,
    Backward,
}

/// The index the focus moves to, wrapping, from `focused` among `total` focusable controls.
fn target(step: Step, focused: Option<usize>, total: usize) -> Option<usize> {
    let last = total.checked_sub(1)?;
    Some(match (step, focused) {
        (Step::Forward, None) => 0,
        (Step::Forward, Some(at)) if at >= last => 0,
        (Step::Forward, Some(at)) => at + 1,
        (Step::Backward, None) => last,
        (Step::Backward, Some(0)) => last,
        (Step::Backward, Some(at)) => at - 1,
    })
}

/// Focuses the focusable control at one index, showing it or not, and unfocuses the others.
struct FocusAt {
    index: usize,
    shown: bool,
    seen: usize,
}

impl Operation for FocusAt {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn focusable(&mut self, _id: Option<&iced_core::widget::Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
        if self.seen == self.index {
            state.focus();
        } else {
            state.unfocus();
        }
        self.seen += 1;
    }

    fn custom(&mut self, _id: Option<&iced_core::widget::Id>, _bounds: Rectangle, state: &mut dyn Any) {
        if let Some(state) = state.downcast_mut::<State>()
            && state.focused
        {
            state.visible = self.shown;
        }
    }
}

/// Focuses the control whose identity is `id` (the access tree's, which survives the interface being built anew),
/// showing its ring or not, and unfocuses the others: when no control has it, none is focused.
struct FocusOn {
    id: NodeId,
    shown: bool,
    lineage: Lineage,
    /// The control a focusable just met belongs to.
    owner: Option<NodeId>,
    /// A control was focused: a second of the same identity is not.
    done: bool,
}

impl Operation for FocusOn {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        self.lineage.enter();
        operate(self);
        self.lineage.leave();
    }

    fn container(&mut self, _id: Option<&iced_core::widget::Id>, _bounds: Rectangle, _viewport: &Rectangle) {
        self.lineage.release();
    }

    fn scrollable(
        &mut self,
        _id: Option<&iced_core::widget::Id>,
        _bounds: Rectangle,
        _content: iced_core::Size,
        _translation: Vector,
        _state: &mut dyn operation::Scrollable,
    ) {
        self.lineage.release();
    }

    fn text_input(
        &mut self,
        _id: Option<&iced_core::widget::Id>,
        _bounds: Rectangle,
        _: &mut dyn operation::TextInput,
    ) {
        self.lineage.release();
    }

    fn text(&mut self, _id: Option<&iced_core::widget::Id>, _bounds: Rectangle, _: &mut dyn operation::Text) {
        self.lineage.release();
    }

    fn focusable(&mut self, _id: Option<&iced_core::widget::Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
        self.owner = self.lineage.focusable();
        if !self.done && self.owner == Some(self.id) {
            self.done = true;
            state.focus();
        } else {
            state.unfocus();
        }
    }

    fn custom(&mut self, _id: Option<&iced_core::widget::Id>, _bounds: Rectangle, state: &mut dyn Any) {
        if let Some(semantics) = state.downcast_ref::<Semantics>() {
            self.lineage.report(semantics);
        } else if let Some(state) = state.downcast_mut::<State>()
            && state.focused
        {
            state.visible = self.shown;
        }
    }
}

/// Where the focus is among the focusable controls an operation walks, in view order, and whether it shows.
#[derive(Debug, Default)]
#[cfg_attr(not(feature = "testing"), expect(unreachable_pub, reason = "exported through `testing` only"))]
pub struct FindFocus {
    seen: usize,
    found: Option<(usize, bool)>,
    /// The same control by what it is, for finding it again once the interface is built anew.
    who: Option<(NodeId, bool)>,
    lineage: Lineage,
    /// The control the last focusable belongs to.
    owner: Option<NodeId>,
    bounds: Option<Rectangle>,
    /// How far what is walked now is drawn from where it is laid out: the scroll of every list around it.
    scrolled: Vector,
    /// The scroll a list just said, which its own walk takes.
    entering: Option<Vector>,
}

impl FindFocus {
    /// Where the focus is: its place among the focusable controls, from 0, and whether its ring shows; `None`
    /// when nothing has it.
    #[cfg(feature = "testing")]
    pub fn found(&self) -> Option<(usize, bool)> {
        self.found
    }

    /// The control that has the focus by its identity, and whether its ring shows; `None` when nothing has it.
    pub(crate) fn identity(&self) -> Option<(NodeId, bool)> {
        self.who
    }

    /// Where the control that has the focus is drawn.
    pub(crate) fn bounds(&self) -> Option<Rectangle> {
        self.bounds
    }
}

impl Operation for FindFocus {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        let entered = self.entering.take().unwrap_or(Vector::ZERO);
        self.scrolled += entered;
        self.lineage.enter();
        operate(self);
        self.lineage.leave();
        self.scrolled -= entered;
    }

    fn container(&mut self, _id: Option<&iced_core::widget::Id>, _bounds: Rectangle, _viewport: &Rectangle) {
        self.lineage.release();
    }

    fn text_input(
        &mut self,
        _id: Option<&iced_core::widget::Id>,
        _bounds: Rectangle,
        _: &mut dyn operation::TextInput,
    ) {
        self.lineage.release();
    }

    fn text(&mut self, _id: Option<&iced_core::widget::Id>, _bounds: Rectangle, _: &mut dyn operation::Text) {
        self.lineage.release();
    }

    // A list says its scroll, then walks its content, which is laid out as if it were not scrolled.
    fn scrollable(
        &mut self,
        _id: Option<&iced_core::widget::Id>,
        _bounds: Rectangle,
        _content: iced_core::Size,
        translation: Vector,
        _state: &mut dyn operation::Scrollable,
    ) {
        self.lineage.release();
        self.entering = Some(translation);
    }

    fn focusable(&mut self, _id: Option<&iced_core::widget::Id>, _bounds: Rectangle, _state: &mut dyn Focusable) {
        self.owner = self.lineage.focusable();
        self.seen += 1;
    }

    fn custom(&mut self, _id: Option<&iced_core::widget::Id>, bounds: Rectangle, state: &mut dyn Any) {
        if let Some(semantics) = state.downcast_ref::<Semantics>() {
            self.lineage.report(semantics);
        } else if let Some(state) = state.downcast_ref::<State>()
            && state.focused
        {
            self.found = Some((self.seen - 1, state.visible));
            self.who = self.owner.map(|owner| (owner, state.visible));
            self.bounds = Some(bounds - self.scrolled);
        }
    }
}

/// Where the control under a point is drawn: the innermost focusable control whose box holds it, as it is drawn, every
/// list's scroll included. A press over an open layer opens another popover without focusing what it pressed, since a
/// layer shuts the screen's controls away from the focus.
pub(crate) struct FindAt {
    point: Point,
    bounds: Option<Rectangle>,
    scrolled: Vector,
    entering: Option<Vector>,
}

impl FindAt {
    /// Looks for the control under `point`, in the walked interface's own coordinates.
    pub(crate) fn new(point: Point) -> Self {
        Self { point, bounds: None, scrolled: Vector::ZERO, entering: None }
    }

    /// Where the control under the point is drawn; none where no control is.
    pub(crate) fn bounds(&self) -> Option<Rectangle> {
        self.bounds
    }
}

impl Operation for FindAt {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        let entered = self.entering.take().unwrap_or(Vector::ZERO);
        self.scrolled += entered;
        operate(self);
        self.scrolled -= entered;
    }

    fn scrollable(
        &mut self,
        _id: Option<&iced_core::widget::Id>,
        _bounds: Rectangle,
        _content: iced_core::Size,
        translation: Vector,
        _state: &mut dyn operation::Scrollable,
    ) {
        self.entering = Some(translation);
    }

    // A control inside another is walked after it, so the last that holds the point is the innermost.
    fn custom(&mut self, _id: Option<&iced_core::widget::Id>, bounds: Rectangle, state: &mut dyn Any) {
        let drawn = bounds - self.scrolled;
        if state.downcast_ref::<State>().is_some() && drawn.contains(self.point) {
            self.bounds = Some(drawn);
        }
    }
}

/// The index, in view order, of the focused focusable control of whatever `operate` walks, if any.
pub(crate) fn focused_index(operate: impl FnOnce(&mut dyn Operation)) -> Option<usize> {
    let mut count = operation::focusable::count();
    operate(&mut operation::black_box(&mut count));
    match Operation::<operation::focusable::Count>::finish(&count) {
        Outcome::Some(counted) => counted.focused,
        _ => None,
    }
}

/// How many focusable controls `operate` walks.
pub(crate) fn focusable_count(operate: impl FnOnce(&mut dyn Operation)) -> usize {
    let mut count = operation::focusable::count();
    operate(&mut operation::black_box(&mut count));
    match Operation::<operation::focusable::Count>::finish(&count) {
        Outcome::Some(counted) => counted.total,
        _ => 0,
    }
}

/// Focuses the control at `index` of whatever `operate` walks, and unfocuses the rest.
pub(crate) fn focus_index(operate: impl FnOnce(&mut dyn Operation), index: usize, shown: bool) {
    operate(&mut FocusAt { index, shown, seen: 0 });
}

/// Focuses the control of whatever `operate` walks that is `id` (as [`FindFocus::identity`] found it), and unfocuses
/// the rest.
pub(crate) fn focus_on(operate: impl FnOnce(&mut dyn Operation), id: NodeId, shown: bool) {
    operate(&mut FocusOn { id, shown, lineage: Lineage::default(), owner: None, done: false });
}

/// Where a field holds the keyboard, unfocuses every other control `operate` walks: one a press focused before the
/// field took the keyboard by itself (opened to be typed in) would otherwise take the field's Enter or Space first.
/// A field that took it by itself since the last key ([`Claim`]) keeps it over a field focused before it, so one
/// field alone hears what is typed.
fn yield_to_field(mut operate: impl FnMut(&mut dyn Operation)) {
    let mut found = Focused::default();
    operate(&mut found);
    let Some(keeps) = found.claimed.or(found.field) else { return };
    let others: Vec<usize> = found.focused.into_iter().filter(|at| *at != keeps).collect();
    if !others.is_empty() {
        operate(&mut Unfocus { at: 0, others });
    }
}

/// A field's mark that it took the keyboard by itself, shown to be typed in or asked to by the screen, reported by
/// its widget through [`Operation::custom`] before the field: the next key's scope gives the field the keyboard alone.
#[derive(Debug, Default)]
pub(crate) struct Claim(bool);

impl Claim {
    /// The field just took the keyboard.
    pub(crate) fn raise(&mut self) {
        self.0 = true;
    }
}

/// The focusable controls an operation meets, in order: which hold the focus, which of them is a field, and which
/// field claimed the keyboard.
#[derive(Default)]
struct Focused {
    at: usize,
    focused: Vec<usize>,
    field: Option<usize>,
    claimed: Option<usize>,
    /// A field said itself as one before its focusable: the next focusable is that field.
    next_is_field: bool,
    /// The last focusable met, for a field that says itself after it (iced's editor).
    last: Option<(usize, Rectangle)>,
    /// A claim was met: the next field is its claimant.
    claiming: bool,
}

impl Focused {
    fn field_at(&mut self, at: usize) {
        let claimed = std::mem::take(&mut self.claiming);
        if self.focused.last() == Some(&at) {
            self.field = Some(at);
            if claimed {
                self.claimed = Some(at);
            }
        }
    }
}

impl Operation for Focused {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn text_input(&mut self, _id: Option<&iced_core::widget::Id>, bounds: Rectangle, _: &mut dyn operation::TextInput) {
        match self.last.take() {
            Some((at, met)) if met == bounds => self.field_at(at),
            _ => self.next_is_field = true,
        }
    }

    fn focusable(&mut self, _id: Option<&iced_core::widget::Id>, bounds: Rectangle, state: &mut dyn Focusable) {
        if state.is_focused() {
            self.focused.push(self.at);
        }
        if std::mem::take(&mut self.next_is_field) {
            self.last = None;
            self.field_at(self.at);
        } else {
            self.last = Some((self.at, bounds));
        }
        self.at += 1;
    }

    fn custom(&mut self, _id: Option<&iced_core::widget::Id>, _bounds: Rectangle, state: &mut dyn Any) {
        if let Some(Claim(raised)) = state.downcast_mut::<Claim>()
            && std::mem::take(raised)
        {
            self.claiming = true;
        }
    }
}

/// Unfocuses the focusable controls at `others`, counted as [`Focused`] counts them.
struct Unfocus {
    at: usize,
    others: Vec<usize>,
}

impl Operation for Unfocus {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn focusable(&mut self, _id: Option<&iced_core::widget::Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
        if self.others.contains(&self.at) {
            state.unfocus();
        }
        self.at += 1;
    }
}

/// Unfocuses every focusable control `operate` walks.
pub(crate) fn clear_focus(operate: impl FnOnce(&mut dyn Operation)) {
    operate(&mut operation::black_box(&mut operation::focusable::unfocus::<()>()));
}

impl<Message, W, R> Widget<Message, Theme, R> for FocusScope<W>
where
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
        if let Event::Keyboard(keyboard::Event::KeyPressed { .. }) = event {
            let (content, child) = (&mut self.content, &mut tree.children[0]);
            yield_to_field(|operation| content.operate(child, layout, viewport, renderer, operation));
        }
        self.content.update(&mut tree.children[0], event, layout, cursor, renderer, shell, viewport);
        if shell.is_event_captured() {
            return;
        }
        let Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(key), modifiers, .. }) = event else {
            return;
        };
        let step = match key {
            Named::Tab if modifiers.shift() => Step::Backward,
            Named::Tab => Step::Forward,
            Named::ArrowDown if self.arrows => Step::Forward,
            Named::ArrowUp if self.arrows => Step::Backward,
            _ => return,
        };
        let content = &mut self.content;
        let child = &mut tree.children[0];
        let mut walk = |operation: &mut dyn Operation| content.operate(child, layout, viewport, renderer, operation);
        let focused = focused_index(&mut walk);
        let total = focusable_count(&mut walk);
        if let Some(index) = target(step, focused, total) {
            focus_index(&mut walk, index, true);
            shell.request_redraw();
            shell.capture_event();
        }
    }
}

/// `content`, whose first control takes the focus once, as it is first shown: a field opened to be typed in at once
/// (a message edited in place), as a layer's first control takes it when the layer opens.
pub(crate) fn take_focus<W>(content: W) -> TakeFocus<W> {
    TakeFocus { content }
}

pub(crate) struct TakeFocus<W> {
    content: W,
}

/// Whether the focus was given to the control in it, and its claim to the keyboard.
#[derive(Debug, Default)]
struct Taken {
    given: bool,
    claim: Claim,
}

/// Puts the caret at the end of the first field it meets, so what is typed adds to what is there.
#[derive(Default)]
struct CaretAtEnd {
    moved: bool,
}

impl Operation for CaretAtEnd {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn text_input(
        &mut self,
        _id: Option<&iced_core::widget::Id>,
        _bounds: Rectangle,
        state: &mut dyn operation::TextInput,
    ) {
        if !self.moved {
            state.move_cursor_to_end();
            self.moved = true;
        }
    }
}

impl<W> iced_core::widget::Meta for TakeFocus<W> {}

impl<Message, W, R> Widget<Message, Theme, R> for TakeFocus<W>
where
    R: renderer::Renderer,
    W: Widget<Message, Theme, R>,
{
    crate::wrap::forward!(size diff layout mouse_interaction draw overlay);

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Taken>()
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &R,
        operation: &mut dyn Operation,
    ) {
        operation.custom(None, layout.bounds(), &mut tree.state.downcast_mut::<Taken>().claim);
        self.content.operate(&mut tree.children[0], layout, viewport, renderer, operation);
    }

    fn state(&self) -> tree::State {
        tree::State::new(Taken::default())
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
        if !tree.state.downcast_ref::<Taken>().given {
            let (content, child) = (&mut self.content, &mut tree.children[0]);
            focus_index(|operation| content.operate(child, layout, viewport, renderer, operation), 0, false);
            content.operate(child, layout, viewport, renderer, &mut CaretAtEnd::default());
            let taken = tree.state.downcast_mut::<Taken>();
            taken.given = true;
            taken.claim.raise();
            shell.request_redraw();
        }
        self.content.update(&mut tree.children[0], event, layout, cursor, renderer, shell, viewport);
    }
}

/// `content`, which sends `left` as the focus leaves the control in it that held it (a press elsewhere, Tab): a field
/// that saves what is typed in it when it is left.
pub(crate) fn on_leave<W, Message>(content: W, left: Message) -> OnLeave<W, Message> {
    OnLeave { content, left }
}

pub(crate) struct OnLeave<W, Message> {
    content: W,
    left: Message,
}

/// Whether the control in it held the focus after the last event it saw.
#[derive(Clone, Copy, Debug, Default)]
struct Held(bool);

impl<W, Message> iced_core::widget::Meta for OnLeave<W, Message> {}

impl<Message, W, R> Widget<Message, Theme, R> for OnLeave<W, Message>
where
    Message: Clone,
    R: renderer::Renderer,
    W: Widget<Message, Theme, R>,
{
    crate::wrap::forward!(size diff layout operate mouse_interaction draw overlay);

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Held>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Held::default())
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
        self.content.update(&mut tree.children[0], event, layout, cursor, renderer, shell, viewport);
        let (content, child) = (&mut self.content, &mut tree.children[0]);
        let focused =
            focused_index(|operation| content.operate(child, layout, viewport, renderer, operation)).is_some();
        let held = tree.state.downcast_mut::<Held>();
        if held.0 && !focused {
            shell.publish(self.left.clone());
        }
        held.0 = focused;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct List;

    impl operation::Scrollable for List {
        fn snap_to(
            &mut self,
            _: operation::scrollable::RelativeOffset<Option<f32>>,
            _: operation::Animation,
            _: Rectangle,
            _: iced_core::Size,
        ) {
        }

        fn scroll_to(
            &mut self,
            _: operation::scrollable::AbsoluteOffset<Option<f32>>,
            _: operation::Animation,
            _: Rectangle,
            _: iced_core::Size,
        ) {
        }

        fn scroll_by(
            &mut self,
            _: operation::scrollable::AbsoluteOffset,
            _: operation::Animation,
            _: Rectangle,
            _: iced_core::Size,
        ) {
        }
    }

    /// What opens beside the focused control is placed where that control is drawn: in a scrolled list, its place
    /// less the scroll, and outside the list as it is laid out.
    #[test]
    fn the_focused_control_of_a_scrolled_list_is_found_where_it_is_drawn() {
        let laid = Rectangle::new(iced_core::Point::new(40.0, 500.0), iced_core::Size::new(36.0, 36.0));
        let focused = |find: &mut dyn Operation| {
            let mut state = State { focused: true, visible: false };
            find.focusable(None, laid, &mut state);
            find.custom(None, laid, &mut state);
        };
        let mut inside = FindFocus::default();
        inside.scrollable(None, Rectangle::default(), iced_core::Size::ZERO, Vector::new(0.0, 180.0), &mut List);
        inside.traverse(&mut |find| focused(find));
        assert_eq!(inside.bounds(), Some(Rectangle { y: 320.0, ..laid }));
        let mut after = FindFocus::default();
        after.scrollable(None, Rectangle::default(), iced_core::Size::ZERO, Vector::new(0.0, 180.0), &mut List);
        after.traverse(&mut |_| {});
        focused(&mut after);
        assert_eq!(after.bounds(), Some(laid), "the list's scroll ends with the list");
    }

    #[test]
    fn the_focus_moves_around_and_wraps() {
        assert_eq!(target(Step::Forward, None, 3), Some(0));
        assert_eq!(target(Step::Forward, Some(0), 3), Some(1));
        assert_eq!(target(Step::Forward, Some(2), 3), Some(0), "wraps past the last");
        assert_eq!(target(Step::Backward, None, 3), Some(2));
        assert_eq!(target(Step::Backward, Some(2), 3), Some(1));
        assert_eq!(target(Step::Backward, Some(0), 3), Some(2), "wraps before the first");
        assert_eq!(target(Step::Forward, None, 0), None, "nothing to focus");
        assert_eq!(target(Step::Forward, Some(0), 1), Some(0), "one control keeps it");
    }

    #[derive(Clone, Debug, PartialEq)]
    enum Said {
        Typed(String),
        Left,
    }

    #[test]
    fn leaving_the_field_says_so_once_and_staying_in_it_does_not() {
        use crate::testing::Renderer;
        use iced_core::{Length, Point, Size};
        use iced_test::Simulator;
        crate::testing::hermetic_fonts();
        let field = crate::TextInput::new(design::Label::new("Role name").expect("a label"), "Moderators")
            .on_input(Said::Typed)
            .view::<Renderer>();
        let page = iced_widget::Column::with_children([
            on_leave(field, Said::Left)._boxed(),
            iced_widget::Space::new().height(Length::Fixed(200.0))._boxed(),
        ]);
        let mut sim: Simulator<'_, Said, Theme, Renderer> =
            Simulator::with_size(iced_core::Settings::default(), Size::new(320.0, 260.0), page);
        let click = |sim: &mut Simulator<'_, Said, Theme, Renderer>, at: Point| {
            sim.point_at(at);
            sim.simulate(crate::testing::click());
        };
        click(&mut sim, Point::new(100.0, 20.0));
        click(&mut sim, Point::new(120.0, 20.0));
        assert_eq!(sim.drain().collect::<Vec<_>>(), [], "a press in the field keeps it");
        click(&mut sim, Point::new(100.0, 200.0));
        assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Left]);
        click(&mut sim, Point::new(100.0, 220.0));
        assert_eq!(sim.drain().collect::<Vec<_>>(), [], "once");
    }

    #[test]
    fn a_field_shown_to_be_typed_in_takes_the_focus_once() {
        use crate::testing::Renderer;
        use iced_core::{Length, Point, Size};
        use iced_test::Simulator;
        crate::testing::hermetic_fonts();
        let field = crate::Composer::new(design::Label::new("Edit message").expect("a label"), "Hello")
            .on_change(Said::Typed)
            .view::<Renderer>();
        let page = iced_widget::Column::with_children([
            take_focus(field)._boxed(),
            iced_widget::Space::new().height(Length::Fixed(200.0))._boxed(),
        ]);
        let mut sim: Simulator<'_, Said, Theme, Renderer> =
            Simulator::with_size(iced_core::Settings::default(), Size::new(320.0, 260.0), page);
        sim.point_at(Point::new(100.0, 200.0));
        sim.simulate(crate::testing::typing("!"));
        assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Typed("Hello!".into())], "typed with no press first");
        sim.simulate(crate::testing::click());
        sim.simulate(crate::testing::typing("?"));
        assert_eq!(sim.drain().collect::<Vec<_>>(), [], "a press elsewhere keeps the focus away: it is taken once");
    }

    #[test]
    fn a_field_that_takes_the_focus_has_it_over_the_first_control_of_the_dialog_it_opens_in() {
        use crate::testing::Renderer;
        use iced_core::{Length, Size};
        use iced_test::Simulator;
        crate::testing::hermetic_fonts();
        let first =
            crate::Button::new(design::Label::new("Text").expect("a label")).on_press(Said::Left).view::<Renderer>();
        let field = crate::TextInput::new(design::Label::new("Channel name").expect("a label"), "")
            .on_input(Said::Typed)
            .view::<Renderer>();
        let panel = iced_widget::Column::with_children([first, take_focus(field)._boxed()])._boxed();
        let under = iced_widget::Space::new().width(Length::Fill).height(Length::Fill)._boxed();
        let screen = crate::dialog(under, Some(panel), Said::Left);
        let mut sim: Simulator<'_, Said, Theme, Renderer> =
            Simulator::with_size(iced_core::Settings::default(), Size::new(480.0, 360.0), screen);
        // A window draws the dialog before anything is typed in it.
        sim.simulate([iced_core::Event::Window(iced_core::window::Event::RedrawRequested(
            iced_core::time::Instant::now(),
        ))]);
        sim.simulate(crate::testing::typing("a"));
        assert_eq!(sim.drain().collect::<Vec<_>>(), [Said::Typed("a".into())], "typed into the field, not the button");
    }
}
