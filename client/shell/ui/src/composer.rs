//! A field of several lines over text the screen holds: it grows with its lines up to a bound, then scrolls. iced's
//! editor keeps its text outside the widget, so this one keeps it in its own state and reports each edit, as the
//! single-line field does; Enter submits, Shift+Enter breaks the line, Escape is the screen's. Its cursor is reported
//! in Unicode scalar values of the text, and put where the screen says each time the screen moves it.

use crate::{Labelled, Renderer, Theme, fonts};
use design::{InputState, Label, SemanticRole, Semantics, TypeStyle};
use iced_core::keyboard::{self, key};
use iced_core::text::editor::{Action, Cursor, Motion};
use iced_core::text::parser::PlainText;
use iced_core::widget::tree::{self, Tree};
use iced_core::widget::{Meta, Operation};
use iced_core::{
    Background, Border, Element, Event, Layout, Length, Padding, Pixels, Rectangle, Shell, Size, Widget, layout, mouse,
    renderer, shell,
};
use iced_widget::text_editor::{self, Binding, Content, KeyPress, TextEditor};

/// A field of up to `rows` visible lines over `text`, the text the screen holds.
pub struct Composer<'a, Message> {
    label: Label,
    placeholder: String,
    text: &'a str,
    rows: u16,
    style: TypeStyle,
    padding: Padding,
    caret: Option<usize>,
    caret_moved: u64,
    focus_asked: u64,
    on_change: Option<Box<dyn Fn(String) -> Message + 'a>>,
    on_caret: Option<Box<dyn Fn(usize) -> Message + 'a>>,
    on_submit: Option<Message>,
    on_escape: Option<Message>,
}

impl<'a, Message: Clone + 'a> Composer<'a, Message> {
    /// A field named by `label` over `text`; it is disabled until it has [`Composer::on_change`].
    pub fn new(label: Label, text: &'a str) -> Self {
        Composer {
            label,
            placeholder: String::new(),
            text,
            rows: design::COMPOSER_ROWS,
            style: TypeStyle::Body,
            padding: Padding::ZERO,
            caret: None,
            caret_moved: 0,
            focus_asked: 0,
            on_change: None,
            on_caret: None,
            on_submit: None,
            on_escape: None,
        }
    }

    /// What it shows while empty.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Its text's style.
    pub fn style(mut self, style: TypeStyle) -> Self {
        self.style = style;
        self
    }

    /// The room between its edge and its text.
    pub fn padding(mut self, padding: Padding) -> Self {
        self.padding = padding;
        self
    }

    /// The most lines it shows before it scrolls.
    pub fn rows(mut self, rows: u16) -> Self {
        self.rows = rows;
        self
    }

    /// What it sends as the text changes; without it the field is disabled.
    pub fn on_change(mut self, on_change: impl Fn(String) -> Message + 'a) -> Self {
        self.on_change = Some(Box::new(on_change));
        self
    }

    /// The field takes the keyboard each time `asked` changes, as a reply begun asks of it.
    pub fn focus_when(mut self, asked: u64) -> Self {
        self.focus_asked = asked;
        self
    }

    /// Where the screen puts the cursor, in scalars of the text (none at its end), each time `moved` changes: the
    /// screen moved it, as a completion does.
    pub fn caret(mut self, at: Option<usize>, moved: u64) -> Self {
        self.caret = at;
        self.caret_moved = moved;
        self
    }

    /// What it sends when the cursor moves or the text changes: the cursor's place, in scalars of the text.
    pub fn on_caret(mut self, on_caret: impl Fn(usize) -> Message + 'a) -> Self {
        self.on_caret = Some(Box::new(on_caret));
        self
    }

    /// What it sends on Enter.
    pub fn on_submit(mut self, message: Message) -> Self {
        self.on_submit = Some(message);
        self
    }

    /// What it sends on Escape.
    pub fn on_escape(mut self, message: Message) -> Self {
        self.on_escape = Some(message);
        self
    }

    /// The field as an element, labelled for a screen reader and reached by Tab.
    pub fn view<R: Renderer + 'static>(self) -> Element<'a, Message, Theme, R> {
        let semantics = Semantics { role: SemanticRole::TextInput, label: self.label.clone() };
        let disabled = self.on_change.is_none();
        let labelled = Labelled::new(self, semantics);
        (if disabled { labelled.without_focus() } else { labelled })._boxed()
    }

    fn editor<'b, R: Renderer>(&'b self, content: &'b Content<R>) -> TextEditor<'b, PlainText, Inner, Theme, R> {
        let style = self.style;
        let editor = TextEditor::new(content)
            .placeholder(self.placeholder.as_str())
            .font(fonts::font(style))
            .size(Pixels(style.size()))
            .line_height(iced_core::text::LineHeight::Absolute(Pixels(style.line_height())))
            .padding(self.padding)
            .height(Length::Shrink)
            .class(EditorClass)
            .key_binding(binding);
        if self.on_change.is_some() { editor.on_action(Inner::Act) } else { editor }
    }

    fn most_height(&self) -> f32 {
        f32::from(self.rows) * self.style.line_height() + self.padding.top + self.padding.bottom
    }
}

/// What the inner editor tells this field.
#[derive(Clone, Debug)]
enum Inner {
    Act(Action),
    Submit,
    Escape,
}

/// Enter submits and Escape is the screen's; Shift+Enter breaks the line, as in every chat composer.
fn binding(press: KeyPress) -> Option<Binding<Inner>> {
    if !press.is_focused {
        return None;
    }
    match press.key.as_ref() {
        keyboard::Key::Named(key::Named::Enter) if !press.modifiers.shift() => Some(Binding::Custom(Inner::Submit)),
        keyboard::Key::Named(key::Named::Escape) => Some(Binding::Custom(Inner::Escape)),
        _ => Binding::from_key_press(press),
    }
}

/// The field's own text, the editor over it, the screen's last move of its cursor and the cursor last reported.
struct Held<R: Renderer> {
    content: Content<R>,
    text: String,
    moved: u64,
    reported: Option<usize>,
    /// The screen's last ask to take the keyboard, once taken.
    focused_for: u64,
    claim: crate::focus::Claim,
}

impl<R: Renderer> Held<R> {
    fn new(text: &str, caret: Option<usize>, moved: u64) -> Held<R> {
        let mut held = Held {
            content: Content::with_text(text),
            text: text.to_owned(),
            moved,
            reported: None,
            focused_for: 0,
            claim: crate::focus::Claim::default(),
        };
        held.put_caret(caret);
        held
    }

    /// The cursor at `caret` scalars into the text, or at its end.
    fn put_caret(&mut self, caret: Option<usize>) {
        match caret.and_then(|caret| position_of(&self.content, caret)) {
            Some(position) => self.content.move_to(Cursor { position, selection: None }),
            None => self.content.perform(Action::Move(Motion::DocumentEnd)),
        }
    }

    /// The screen's text, taken when it is not what this field last reported: a send cleared it, or a failed one put
    /// it back.
    fn follow(&mut self, text: &str) {
        if self.text != text {
            self.content = Content::with_text(text);
            self.content.perform(Action::Move(Motion::DocumentEnd));
            text.clone_into(&mut self.text);
        }
    }
}

impl<Message> Meta for Composer<'_, Message> {}

impl<'a, Message: Clone + 'a, R: Renderer + 'static> Widget<Message, Theme, R> for Composer<'a, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Held<R>>()
    }

    fn state(&self) -> tree::State {
        let mut held = Held::<R>::new(self.text, self.caret, self.caret_moved);
        held.focused_for = self.focus_asked;
        tree::State::new(held)
    }

    fn diff(&mut self, tree: &mut Tree) {
        let Tree { state, children, .. } = tree;
        let held = state.downcast_mut::<Held<R>>();
        held.follow(self.text);
        if held.moved != self.caret_moved {
            held.moved = self.caret_moved;
            held.put_caret(self.caret);
            held.reported = self.caret;
        }
        let mut editor = self.editor(&held.content);
        match children.first_mut() {
            Some(child) => child.diff(&mut editor),
            None => children.push(Tree::new(&editor)),
        }
    }

    fn size(&self) -> Size<Length> {
        Size { width: Length::Fill, height: Length::Shrink }
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &R, limits: &layout::Limits) {
        let most = limits.bounds();
        let limits = layout::Limits::new(Size::ZERO, Size::new(most.width, most.height.min(self.most_height())));
        let Tree { state, children, size, .. } = tree;
        let held = state.downcast_mut::<Held<R>>();
        if let Some(child) = children.first_mut() {
            self.editor(&held.content).layout(child, renderer, &limits);
            *size = child.size;
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
        let Tree { state, children, .. } = tree;
        let held = state.downcast_mut::<Held<R>>();
        operation.custom(None, layout.bounds(), &mut held.claim);
        if let Some(child) = children.first_mut() {
            self.editor(&held.content).operate(child, layout, viewport, renderer, operation);
        }
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
        let Tree { state, children, .. } = tree;
        let held = state.downcast_mut::<Held<R>>();
        let Some(child) = children.first_mut() else { return };
        if held.focused_for != self.focus_asked {
            held.focused_for = self.focus_asked;
            let editor = &mut self.editor(&held.content);
            crate::focus::focus_index(
                |operation| editor.operate(child, layout, viewport, renderer, operation),
                0,
                false,
            );
            held.claim.raise();
            shell.request_redraw();
        }
        let mut bus = shell::Bus::new();
        // Relayed as iced's own component relays its view's shell: what the editor said is this field's to act on.
        {
            let mut inner = Shell::local(shell, &mut bus);
            self.editor(&held.content).update(child, event, layout, cursor, renderer, &mut inner, viewport);
            if inner.is_event_captured() {
                shell.capture_event();
            }
            shell.request_redraw_at(inner.redraw_request());
            shell.request_input_method(inner.input_method());
            shell.clipboard_mut().merge(inner.clipboard_mut());
            shell.invalidate(inner.invalidation());
        }
        let mut edited = false;
        for (said, _) in bus.drain() {
            match said {
                Inner::Act(action) => {
                    edited |= action.is_edit();
                    held.content.perform(action);
                }
                Inner::Submit => {
                    if let Some(message) = self.on_submit.clone() {
                        shell.publish(message);
                    }
                }
                Inner::Escape => {
                    if let Some(message) = self.on_escape.clone() {
                        shell.publish(message);
                    }
                }
            }
        }
        if edited && let Some(on_change) = &self.on_change {
            held.text = held.content.text();
            shell.publish(on_change(held.text.clone()));
        }
        if let Some(on_caret) = &self.on_caret {
            let caret = scalars_before(&held.content);
            if edited || held.reported != Some(caret) {
                held.reported = Some(caret);
                shell.publish(on_caret(caret));
            }
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
        let held = tree.state.downcast_ref::<Held<R>>();
        tree.children.first().map_or_else(mouse::Interaction::default, |child| {
            self.editor(&held.content).mouse_interaction(child, layout, cursor, viewport, renderer)
        })
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
        let held = tree.state.downcast_ref::<Held<R>>();
        if let Some(child) = tree.children.first() {
            self.editor(&held.content).draw(child, renderer, theme, style, layout, cursor, viewport);
        }
    }
}

/// How many scalars of the text stand before the cursor.
fn scalars_before<R: Renderer>(content: &Content<R>) -> usize {
    let at = content.cursor().position;
    content
        .lines()
        .take(at.line + 1)
        .enumerate()
        .map(|(index, line)| {
            if index == at.line {
                line.text.get(..at.index).unwrap_or(&line.text).chars().count()
            } else {
                line.text.chars().count() + line.ending.as_str().chars().count()
            }
        })
        .sum()
}

/// Where the cursor stands `caret` scalars into the text: its line and the byte in it; none past the text's end.
fn position_of<R: Renderer>(content: &Content<R>, caret: usize) -> Option<iced_core::text::Position> {
    let mut left = caret;
    for (line, held) in content.lines().enumerate() {
        let length = held.text.chars().count();
        if left <= length {
            let index = held.text.char_indices().nth(left).map_or(held.text.len(), |(byte, _)| byte);
            return Some(iced_core::text::Position { line, index });
        }
        left -= length + held.ending.as_str().chars().count();
    }
    None
}

/// The editor's own style: no fill and no edge of its own, the frame round the composer has them.
#[derive(Clone, Copy, Debug, Default)]
pub struct EditorClass;

impl text_editor::Catalog for Theme {
    type Class<'a> = EditorClass;

    fn default<'a>() -> Self::Class<'a> {
        EditorClass
    }

    fn style(&self, _class: &Self::Class<'_>, status: text_editor::Status) -> text_editor::Style {
        let state = match status {
            text_editor::Status::Active => InputState::Idle,
            text_editor::Status::Hovered => InputState::Hover,
            text_editor::Status::Focused { .. } => InputState::Focused,
            text_editor::Status::Disabled => InputState::Disabled,
        };
        let look = design::input(state, false);
        text_editor::Style {
            background: Background::Color(iced_core::Color::TRANSPARENT),
            border: Border::default(),
            placeholder: self.color(look.placeholder),
            value: self.color(look.text),
            selection: crate::color::translucent(self.palette().selection()),
        }
    }
}
