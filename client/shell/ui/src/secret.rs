//! A field for a password, an invite or a token: our own widget, not iced's editor, so the typed text lives in one
//! place, a [`Secret`] that zeroes itself and never prints, and no operation, clipboard write or input method is
//! ever given it.
//!
//! Hidden, the field draws a paragraph of bullets, which holds no secret. Shown (the eye), it draws a paragraph of the
//! real text, built only while shown and only again when the text changes, and dropped when it is hidden: its glyph
//! buffer, and those of the paragraphs that measure where the caret stands while shown (each dropped as it is
//! measured), are the only copies of the text outside a [`Secret`], owned by the text renderer, which cannot zero
//! them. A pasted text is read from the clipboard's own copy, which is not ours to zero either, straight into the
//! buffer.

use crate::drawn::Drawn;
use crate::fonts;
use crate::input::padding;
use crate::{Button, Icon, Labelled, Renderer, Theme};
use design::{
    BORDER_WIDTH, ButtonVariant, CONTROL_HEIGHT, INPUT_RADIUS, InputState, Label, SemanticRole, Semantics, TypeStyle,
};
use iced_core::keyboard::{self, Key, Modifiers, key::Named};
use iced_core::shell::Tracking;
use iced_core::text::{self, Paragraph, Shaping, Text, Wrapping};
use iced_core::widget::operation::{Focusable, Operation};
use iced_core::widget::{Meta, Tree, tree};
use iced_core::{
    Border, Element, Event, Layout, Length, Padding, Pixels, Point, Rectangle, Shell, Size, Widget, alignment,
    clipboard, layout, mouse, renderer, window,
};
use iced_widget::container;
use terms::Secret;
use words::{Phrase, Words};

/// What stands in for each character while the text is hidden.
const BULLET: char = '\u{2022}';

/// A labelled secure field whose text no operation, clipboard or input method is given, even while it is shown.
pub struct SecretInput<'a, Message> {
    label: Label,
    placeholder: String,
    value: &'a Secret,
    on_input: Option<Box<dyn Fn(Secret) -> Message + 'a>>,
    on_submit: Option<Message>,
    invalid: bool,
    revealed: bool,
    trailing: f32,
}

impl<'a, Message: Clone + 'a> SecretInput<'a, Message> {
    /// A field named by `label` over `value`, drawn as bullets. It is disabled until it has [`SecretInput::on_input`].
    pub fn new(label: Label, value: &'a Secret) -> Self {
        SecretInput {
            label,
            placeholder: String::new(),
            value,
            on_input: None,
            on_submit: None,
            invalid: false,
            revealed: false,
            trailing: 0.0,
        }
    }

    /// Room kept clear at the field's end for a control laid over it (Show password).
    pub fn trailing_room(mut self, room: f32) -> Self {
        self.trailing = room;
        self
    }

    /// What it shows while empty.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// What it sends as the text changes: the new text, already a [`Secret`]. Without it the field is disabled.
    pub fn on_input(mut self, on_input: impl Fn(Secret) -> Message + 'a) -> Self {
        self.on_input = Some(Box::new(on_input));
        self
    }

    pub(crate) fn on_submit(mut self, message: Message) -> Self {
        self.on_submit = Some(message);
        self
    }

    /// What it holds is refused: drawn with the danger edge.
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// Show the text instead of bullets. It is the screen's state, off until the person asks; a
    /// shown secret is still never copied, read by an operation, or sent to an input method.
    pub fn revealed(mut self, revealed: bool) -> Self {
        self.revealed = revealed;
        self
    }

    /// The field as an element, labelled for a screen reader and reached by Tab.
    pub fn view<R: Renderer + 'a>(self) -> Element<'a, Message, Theme, R> {
        let disabled = self.on_input.is_none();
        let semantics = Semantics { role: SemanticRole::SecretInput, label: self.label };
        let field = SecretField {
            placeholder: self.placeholder,
            value: self.value,
            on_input: self.on_input,
            on_submit: self.on_submit,
            invalid: self.invalid,
            revealed: self.revealed,
            trailing: self.trailing,
        };
        let field = Labelled::new(field, semantics);
        (if disabled { field.without_focus() } else { field })._boxed()
    }
}

/// A password field with its Show password / Hide password button at its right end; `on_reveal` is what the button
/// sends, and `input` says itself whether its text is shown.
pub(crate) fn password_field<'a, Message: Clone + 'a, R: Renderer + 'a>(
    input: SecretInput<'a, Message>,
    on_reveal: Message,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let shown = input.revealed;
    let toggle = if shown { Phrase::HidePassword } else { Phrase::ShowPassword };
    let reveal = Button::new(words.label(toggle))
        .icon_only(if shown { Icon::EyeOff } else { Icon::Eye })
        .variant(ButtonVariant::Ghost)
        .on_press(on_reveal)
        .view::<R>();
    let inside = container(reveal).align_right(Length::Fill).center_y(Length::Fill);
    crate::over_field::over_field(input.trailing_room(design::CONTROL_HEIGHT).view::<R>(), inside._boxed())
}

/// The widget: a single line of typed text, its caret and selection, drawn from the design's input look.
struct SecretField<'a, Message> {
    placeholder: String,
    value: &'a Secret,
    on_input: Option<Box<dyn Fn(Secret) -> Message + 'a>>,
    on_submit: Option<Message>,
    invalid: bool,
    revealed: bool,
    trailing: f32,
}

/// What the paragraph that draws the text was built from, so it is built again only when that changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Built {
    #[default]
    Nothing,
    /// This many bullets.
    Bullets(usize),
    /// The real text, as it was after this many edits.
    Shown(u64),
}

/// What the paragraph drawn must be built from: bullets while hidden, which say only how many, and the text while
/// shown, which changes only with the text.
fn built_for(shown: bool, edits: u64, characters: usize) -> Built {
    if shown { Built::Shown(edits) } else { Built::Bullets(characters) }
}

/// Where the caret and the anchor stood, in which text, which `x` of each was measured for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Measured {
    edits: u64,
    caret: usize,
    anchor: usize,
    shown: bool,
}

/// What the widget keeps between frames: the text, only as a [`Secret`], and where the caret and the selection stand
/// in it, as character indices. The selection is the characters between `anchor` and `caret`.
struct State<P: Paragraph> {
    typed: Secret,
    /// Counts every change of the text, so a shown paragraph is built again only when it changed.
    edits: u64,
    caret: usize,
    anchor: usize,
    focused: bool,
    window_focused: bool,
    dragging: bool,
    /// Our last message to the screen, until the screen has processed it: its text is not ours to overwrite before.
    pending: Option<Tracking>,
    paragraph: P,
    built: Built,
    placeholder: P,
    measured: Measured,
    caret_x: f32,
    anchor_x: f32,
    scroll: f32,
    /// How the last frame drew the field's edge.
    drawn: Drawn<InputState>,
}

impl<P: Paragraph> State<P> {
    fn new() -> Self {
        State {
            typed: Secret::new(String::new()),
            edits: 0,
            caret: 0,
            anchor: 0,
            focused: false,
            window_focused: true,
            dragging: false,
            pending: None,
            paragraph: P::default(),
            built: Built::Nothing,
            placeholder: P::default(),
            measured: Measured::default(),
            caret_x: 0.0,
            anchor_x: 0.0,
            scroll: 0.0,
            drawn: Drawn::default(),
        }
    }

    /// How many characters it holds.
    fn len(&self) -> usize {
        self.typed.expose().chars().count()
    }

    /// The characters selected, as a range of indices; empty while none are.
    fn selected(&self) -> std::ops::Range<usize> {
        self.caret.min(self.anchor)..self.caret.max(self.anchor)
    }

    /// The text taken whole from the screen, caret at its end.
    fn adopt(&mut self, text: &str) {
        self.typed = Secret::new(text.to_owned());
        self.edits += 1;
        self.caret = self.len();
        self.anchor = self.caret;
    }

    /// The characters from `from` to `to` replaced by `with`, which comes straight from the key or the clipboard into
    /// the new buffer; the old one zeroes as it goes.
    fn replace(&mut self, from: usize, to: usize, with: &str) {
        let text = self.typed.expose();
        let (start, end) = (byte_of(text, from), byte_of(text, to));
        let mut next = String::with_capacity(text.len() - (end - start) + with.len());
        next.extend(text.chars().take(from));
        next.push_str(with);
        next.extend(text.chars().skip(to));
        self.typed = Secret::new(next);
        self.edits += 1;
        self.caret = from + with.chars().count();
        self.anchor = self.caret;
    }

    /// The selection, or `from..to` where none, deleted.
    fn delete(&mut self, from: usize, to: usize) {
        let selected = self.selected();
        let (from, to) = if selected.is_empty() { (from, to) } else { (selected.start, selected.end) };
        self.replace(from, to, "");
    }

    /// The caret moved to `to`, the selection kept growing from where it began while `extend`.
    fn go(&mut self, to: usize, extend: bool) {
        self.caret = to.min(self.len());
        if !extend {
            self.anchor = self.caret;
        }
    }
}

impl<P: Paragraph> Focusable for State<P> {
    fn is_focused(&self) -> bool {
        self.focused
    }

    fn focus(&mut self) {
        self.focused = true;
        self.window_focused = true;
        self.go(self.len(), false);
    }

    fn unfocus(&mut self) {
        self.focused = false;
        self.dragging = false;
        self.anchor = self.caret;
    }
}

/// The byte where the character at `index` starts, or the text's end.
fn byte_of(text: &str, index: usize) -> usize {
    text.char_indices().nth(index).map_or(text.len(), |(at, _)| at)
}

/// How far back a word's start lies from `at`: the space before, then the word.
fn word_before(text: &str, at: usize) -> usize {
    let after = text.chars().count().saturating_sub(at);
    let mut chars = text.chars().rev().skip(after).peekable();
    let mut back = 0;
    while chars.next_if(|each| !each.is_alphanumeric()).is_some() {
        back += 1;
    }
    while chars.next_if(|each| each.is_alphanumeric()).is_some() {
        back += 1;
    }
    at - back
}

/// How far on the next word's start lies from `at`: the rest of this word, then the space after.
fn word_after(text: &str, at: usize) -> usize {
    let mut chars = text.chars().skip(at).peekable();
    let mut on = 0;
    while chars.next_if(|each| each.is_alphanumeric()).is_some() {
        on += 1;
    }
    while chars.next_if(|each| !each.is_alphanumeric()).is_some() {
        on += 1;
    }
    at + on
}

/// What a key did to the field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pressed {
    /// Nothing of the field's: another control's key.
    Passed,
    /// The text changed.
    Edited,
    /// The caret or the selection moved.
    Moved,
    /// Enter.
    Submitted,
    /// The clipboard's text is wanted.
    Paste,
    /// Copy or Cut, which a secret never does: the key is taken and nothing is written or cut.
    Refused,
}

/// `key` pressed with `modifiers`, producing `typed`, applied to `state`.
fn press<P: Paragraph>(state: &mut State<P>, key: &Key, modifiers: Modifiers, typed: Option<&str>) -> Pressed {
    let (caret, len) = (state.caret, state.len());
    match key {
        Key::Named(Named::Backspace) => {
            let to = if modifiers.jump() { word_before(state.typed.expose(), caret) } else { caret.saturating_sub(1) };
            state.delete(to, caret);
            Pressed::Edited
        }
        Key::Named(Named::Delete) if modifiers.shift() => Pressed::Refused,
        Key::Named(Named::Delete) => {
            let to = if modifiers.jump() { word_after(state.typed.expose(), caret) } else { (caret + 1).min(len) };
            state.delete(caret, to);
            Pressed::Edited
        }
        Key::Named(Named::ArrowLeft) => {
            let to = match (state.selected().is_empty(), modifiers.shift(), modifiers.jump()) {
                (false, false, _) => state.selected().start,
                (_, _, true) => word_before(state.typed.expose(), caret),
                (_, _, false) => caret.saturating_sub(1),
            };
            state.go(to, modifiers.shift());
            Pressed::Moved
        }
        Key::Named(Named::ArrowRight) => {
            let to = match (state.selected().is_empty(), modifiers.shift(), modifiers.jump()) {
                (false, false, _) => state.selected().end,
                (_, _, true) => word_after(state.typed.expose(), caret),
                (_, _, false) => caret + 1,
            };
            state.go(to, modifiers.shift());
            Pressed::Moved
        }
        Key::Named(Named::Home) => {
            state.go(0, modifiers.shift());
            Pressed::Moved
        }
        Key::Named(Named::End) => {
            state.go(len, modifiers.shift());
            Pressed::Moved
        }
        Key::Named(Named::Enter) => Pressed::Submitted,
        Key::Named(Named::Insert) if modifiers.control() => Pressed::Refused,
        Key::Named(Named::Insert) if modifiers.shift() => Pressed::Paste,
        Key::Character(character) if modifiers.command() => match character.to_ascii_lowercase().as_str() {
            "a" => {
                state.anchor = 0;
                state.caret = len;
                Pressed::Moved
            }
            "c" | "x" => Pressed::Refused,
            "v" => Pressed::Paste,
            _ => Pressed::Passed,
        },
        Key::Named(_) | Key::Character(_) | Key::Unidentified => match typed {
            Some(typed) if !modifiers.command() && !typed.is_empty() && !typed.chars().any(char::is_control) => {
                let selected = state.selected();
                let (from, to) = if selected.is_empty() { (caret, caret) } else { (selected.start, selected.end) };
                state.replace(from, to, typed);
                Pressed::Edited
            }
            Some(_) | None => Pressed::Passed,
        },
    }
}

/// The text a paragraph is laid out in: one line, as wide as it needs, in the field's type.
struct Face {
    size: Pixels,
    line_height: text::LineHeight,
    font: iced_core::Font,
    hint_factor: Option<f32>,
}

impl Face {
    fn of(renderer: &impl text::Renderer) -> Face {
        let style = TypeStyle::Body;
        Face {
            size: Pixels(style.size()),
            line_height: text::LineHeight::Absolute(Pixels(style.line_height())),
            font: fonts::font(style),
            hint_factor: renderer.hint_factor(),
        }
    }

    fn paragraph<P: Paragraph>(&self, content: &str) -> P {
        P::with_text(Text {
            content,
            bounds: Size::INFINITE,
            size: self.size,
            line_height: self.line_height,
            font: self.font,
            align_x: text::Alignment::Default,
            align_y: alignment::Vertical::Top,
            shaping: Shaping::Advanced,
            wrapping: Wrapping::None,
            ellipsis: text::Ellipsis::None,
            hint_factor: self.hint_factor,
        })
    }

    /// How far across the text the character at `index` starts: measured on a paragraph of what precedes it, which is
    /// dropped as it is measured.
    fn across<P: Paragraph>(&self, typed: &str, index: usize, shown: bool) -> f32 {
        if shown {
            // `byte_of` gives a character's start, where a split always holds.
            let before = typed.split_at_checked(byte_of(typed, index)).map_or(typed, |(before, _)| before);
            self.paragraph::<P>(before).min_width()
        } else {
            self.paragraph::<P>(&BULLET.to_string().repeat(index)).min_width()
        }
    }
}

impl<Message> SecretField<'_, Message> {
    /// How the field's edge is drawn, focused or not, with the pointer as `cursor` says.
    fn status(&self, focused: bool, bounds: Rectangle, cursor: mouse::Cursor) -> InputState {
        if self.on_input.is_none() {
            InputState::Disabled
        } else if focused {
            InputState::Focused
        } else if cursor.is_over(bounds) {
            InputState::Hover
        } else {
            InputState::Idle
        }
    }
}

impl<Message> Meta for SecretField<'_, Message> {}

impl<Message, R> Widget<Message, Theme, R> for SecretField<'_, Message>
where
    Message: Clone,
    R: text::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State<R::Paragraph>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::<R::Paragraph>::new())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(CONTROL_HEIGHT))
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &R, limits: &layout::Limits) {
        tree.size = limits.resolve(Length::Fill, Length::Fixed(CONTROL_HEIGHT), Size::new(0.0, CONTROL_HEIGHT));
        let state = tree.state.downcast_mut::<State<R::Paragraph>>();
        // The screen's text wins, unless a change of ours is still on its way to it.
        let settled = state.pending.as_ref().is_none_or(Tracking::is_processed);
        if settled && state.typed.expose() != self.value.expose() {
            state.adopt(self.value.expose());
        }
        let face = Face::of(renderer);
        let typed = state.typed.expose();
        let want = built_for(self.revealed, state.edits, typed.chars().count());
        if state.built != want {
            // Replaced, so the paragraph of the real text is dropped the moment the text is hidden.
            state.paragraph = match want {
                Built::Nothing => R::Paragraph::default(),
                Built::Bullets(count) => face.paragraph(&BULLET.to_string().repeat(count)),
                Built::Shown(_) => face.paragraph(typed),
            };
            state.built = want;
        }
        state.placeholder = if typed.is_empty() && !self.placeholder.is_empty() {
            face.paragraph(&self.placeholder)
        } else {
            R::Paragraph::default()
        };
        let measured = Measured { edits: state.edits, caret: state.caret, anchor: state.anchor, shown: self.revealed };
        if state.measured != measured {
            state.caret_x = face.across::<R::Paragraph>(typed, state.caret, self.revealed);
            state.anchor_x = if state.anchor == state.caret {
                state.caret_x
            } else {
                face.across::<R::Paragraph>(typed, state.anchor, self.revealed)
            };
            state.measured = measured;
        }
        let room = tree.size.width - horizontal(&padded(self.trailing));
        if state.caret_x - state.scroll > room {
            state.scroll = state.caret_x - room;
        } else if state.caret_x < state.scroll {
            state.scroll = state.caret_x;
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        _viewport: &Rectangle,
        _renderer: &R,
        operation: &mut dyn Operation,
    ) {
        // Focus only: the text is offered to no operation, a selector, a test or a screen reader.
        let state = tree.state.downcast_mut::<State<R::Paragraph>>();
        operation.focusable(None, layout.bounds(), state);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        _renderer: &R,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let Some(on_input) = &self.on_input else { return };
        let state = tree.state.downcast_mut::<State<R::Paragraph>>();
        let bounds = layout.bounds();
        let padding = padded(self.trailing);
        let mut edited = false;
        match event {
            Event::Window(window::Event::Focused) => state.window_focused = true,
            Event::Window(window::Event::Unfocused) => state.window_focused = false,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(at) = cursor.position_in(bounds) {
                    state.focused = true;
                    state.window_focused = true;
                    state.dragging = true;
                    let to = index_at(state, at.x - padding.left, self.revealed);
                    state.go(to, false);
                    shell.capture_event();
                    shell.invalidate_layout();
                    shell.request_redraw();
                } else if state.focused {
                    state.unfocus();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => state.dragging = false,
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.dragging => {
                if let Some(at) = cursor.position_in(bounds) {
                    let to = index_at(state, at.x - padding.left, self.revealed);
                    state.go(to, true);
                    shell.capture_event();
                    shell.invalidate_layout();
                    shell.request_redraw();
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, text, .. })
                if state.focused && state.window_focused =>
            {
                match press(state, key, *modifiers, text.as_deref()) {
                    Pressed::Passed => {}
                    Pressed::Edited => {
                        edited = true;
                        shell.capture_event();
                    }
                    Pressed::Moved => {
                        shell.capture_event();
                        shell.invalidate_layout();
                        shell.request_redraw();
                    }
                    Pressed::Submitted => {
                        if let Some(message) = &self.on_submit {
                            shell.publish(message.clone());
                        }
                        shell.capture_event();
                    }
                    Pressed::Paste => {
                        shell.read_clipboard(clipboard::Kind::Text);
                        shell.capture_event();
                    }
                    Pressed::Refused => shell.capture_event(),
                }
            }
            Event::Clipboard(clipboard::Event::Read(Ok(content))) if state.focused && state.window_focused => {
                if let clipboard::Content::Text(pasted) = content.as_ref() {
                    // One line, read from the clipboard's own copy straight into the buffer.
                    let line = pasted.split(['\n', '\r']).next().unwrap_or_default();
                    let selected = state.selected();
                    let (from, to) =
                        if selected.is_empty() { (state.caret, state.caret) } else { (selected.start, selected.end) };
                    state.replace(from, to, line);
                    edited = true;
                    shell.capture_event();
                }
            }
            _ => {}
        }
        if edited {
            state.pending = Some(shell.publish_and_track(on_input(Secret::new(state.typed.expose().to_owned()))));
            shell.invalidate_layout();
            shell.request_redraw();
        }
        let now = self.status(state.focused, bounds, cursor);
        state.drawn.follow(event, now, shell);
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut R,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State<R::Paragraph>>();
        let bounds = layout.bounds();
        let look = design::input(self.status(state.focused, bounds, cursor), self.invalid);
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: Border {
                    color: theme.color(look.border),
                    width: look.border_width,
                    radius: INPUT_RADIUS.px().into(),
                },
                ..renderer::Quad::default()
            },
            theme.color(look.fill),
        );
        let area = bounds.shrink(padded(self.trailing));
        let Some(clip) = area.intersection(viewport) else { return };
        let line = TypeStyle::Body.line_height();
        let top = area.y + (area.height - line) / 2.0;
        renderer.with_layer(clip, |renderer| {
            let editing = state.focused && state.window_focused;
            if editing && state.caret != state.anchor {
                let (left, right) = (state.caret_x.min(state.anchor_x), state.caret_x.max(state.anchor_x));
                let selection =
                    Rectangle::new(Point::new(area.x + left - state.scroll, top), Size::new(right - left, line));
                renderer.fill_quad(
                    renderer::Quad { bounds: selection, ..renderer::Quad::default() },
                    crate::color::translucent(theme.palette().selection()),
                );
            }
            if state.typed.expose().is_empty() {
                renderer.fill_paragraph(
                    &state.placeholder,
                    Point::new(area.x, top),
                    theme.color(look.placeholder),
                    clip,
                );
            } else {
                let at = Point::new(area.x - state.scroll, top);
                renderer.fill_paragraph(&state.paragraph, at, theme.color(look.text), clip);
            }
            if editing {
                let caret = Rectangle::new(
                    Point::new(area.x + state.caret_x - state.scroll, top),
                    Size::new(BORDER_WIDTH, line),
                );
                renderer
                    .fill_quad(renderer::Quad { bounds: caret, ..renderer::Quad::default() }, theme.color(look.text));
            }
        });
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &R,
    ) -> mouse::Interaction {
        match (cursor.is_over(layout.bounds()), self.on_input.is_some()) {
            (true, true) => mouse::Interaction::Text,
            (true, false) | (false, _) => mouse::Interaction::default(),
        }
    }
}

/// The room between the field's edge and its text.
fn padded(trailing: f32) -> Padding {
    padding(TypeStyle::Body, 0.0, trailing)
}

/// The width of the padding to either side.
fn horizontal(padding: &Padding) -> f32 {
    padding.left + padding.right
}

/// The character index nearest `x`, measured from the text's start in the field, as the paragraph drawn lays it out.
fn index_at<P: Paragraph>(state: &State<P>, x: f32, shown: bool) -> usize {
    let across = Point::new((x + state.scroll).max(0.0), TypeStyle::Body.line_height() / 2.0);
    let Some(hit) = state.paragraph.hit_test(across) else {
        return if x + state.scroll <= 0.0 { 0 } else { state.len() };
    };
    if shown {
        let typed = state.typed.expose();
        typed.get(..hit.index).map_or_else(|| state.len(), |head| head.chars().count())
    } else {
        hit.index / BULLET.len_utf8()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Plain = State<()>;

    /// The one place the typed text lives: a `Secret`, which zeroes itself and never prints. A `String` here would
    /// not compile.
    #[test]
    fn the_state_holds_the_text_only_as_a_secret() {
        let state = Plain::new();
        let held: &Secret = &state.typed;
        assert_eq!(held.expose(), "");
    }

    fn type_in(state: &mut Plain, text: &str) {
        for character in text.chars() {
            let key = Key::Character(character.to_string().into());
            press(state, &key, Modifiers::empty(), Some(&character.to_string()));
        }
    }

    #[test]
    fn typing_inserts_at_the_caret_and_replaces_a_selection() {
        let mut state = Plain::new();
        type_in(&mut state, "hunter2");
        assert_eq!((state.typed.expose(), state.caret), ("hunter2", 7));
        state.go(0, false);
        type_in(&mut state, "my-");
        assert_eq!(state.typed.expose(), "my-hunter2");
        press(&mut state, &Key::Character("a".into()), Modifiers::COMMAND, None);
        assert_eq!(state.selected(), 0..10);
        type_in(&mut state, "é");
        assert_eq!((state.typed.expose(), state.caret), ("é", 1), "a selection is replaced whole");
    }

    #[test]
    fn a_paste_goes_in_at_the_caret_and_is_edited_after() {
        let mut state = Plain::new();
        type_in(&mut state, "ab");
        state.go(1, false);
        state.replace(1, 1, "XYZ");
        assert_eq!((state.typed.expose(), state.caret), ("aXYZb", 4));
        press(&mut state, &Key::Named(Named::Backspace), Modifiers::empty(), None);
        assert_eq!((state.typed.expose(), state.caret), ("aXYb", 3));
        press(&mut state, &Key::Named(Named::Delete), Modifiers::empty(), None);
        assert_eq!(state.typed.expose(), "aXY");
    }

    #[test]
    fn the_arrows_home_and_end_move_and_the_control_key_moves_by_words() {
        let mut state = Plain::new();
        type_in(&mut state, "one two  three");
        let moves = |state: &mut Plain, key: Named, modifiers: Modifiers| {
            press(state, &Key::Named(key), modifiers, None);
            state.caret
        };
        assert_eq!(moves(&mut state, Named::Home, Modifiers::empty()), 0);
        assert_eq!(moves(&mut state, Named::ArrowRight, Modifiers::CTRL), 4, "to the start of the next word");
        assert_eq!(moves(&mut state, Named::ArrowRight, Modifiers::CTRL), 9);
        assert_eq!(moves(&mut state, Named::ArrowLeft, Modifiers::CTRL), 4);
        assert_eq!(moves(&mut state, Named::End, Modifiers::empty()), 14);
        assert_eq!(moves(&mut state, Named::ArrowLeft, Modifiers::SHIFT), 13);
        assert_eq!(state.selected(), 13..14, "shift extends");
        press(&mut state, &Key::Named(Named::Backspace), Modifiers::CTRL, None);
        assert_eq!(state.typed.expose(), "one two  thre", "a selection goes first, before any word");
    }

    #[test]
    fn copy_and_cut_are_taken_and_change_nothing() {
        let mut state = Plain::new();
        type_in(&mut state, "secret");
        for character in ["c", "x"] {
            let taken = press(&mut state, &Key::Character(character.into()), Modifiers::COMMAND, None);
            assert_eq!((taken, state.typed.expose()), (Pressed::Refused, "secret"));
        }
        assert_eq!(press(&mut state, &Key::Named(Named::Insert), Modifiers::CTRL, None), Pressed::Refused);
        assert_eq!(press(&mut state, &Key::Named(Named::Delete), Modifiers::SHIFT, None), Pressed::Refused);
        assert_eq!(state.typed.expose(), "secret");
    }

    #[test]
    fn the_paragraph_of_the_real_text_is_built_only_while_shown_and_on_a_change_and_gives_way_when_hidden() {
        assert_eq!(built_for(true, 3, 7), Built::Shown(3));
        assert_eq!(built_for(true, 3, 8), Built::Shown(3), "the same text is not built again");
        assert_eq!(built_for(true, 4, 7), Built::Shown(4), "an edit builds it again");
        assert_eq!(built_for(false, 4, 7), Built::Bullets(7), "hidden, only the count remains");
        assert_ne!(built_for(false, 4, 7), built_for(true, 4, 7));
    }

    #[test]
    fn a_word_is_found_by_its_characters_not_its_bytes() {
        assert_eq!(word_before("héllo wörld", 11), 6);
        assert_eq!(word_after("héllo wörld", 0), 6);
    }

    fn asked(drawn: Point, now: Point) -> iced_core::window::RedrawRequest {
        let value = Secret::new(String::new());
        let field = || SecretInput::new(Label::new("Password").unwrap(), &value).on_input(|_| ()).view();
        let size = Size::new(200.0, CONTROL_HEIGHT);
        let moved = [Event::Mouse(mouse::Event::CursorMoved { position: now })];
        crate::drawn::asked_after::<()>(field, size, drawn, now, &moved)
    }

    #[test]
    fn the_pointer_coming_onto_the_field_or_leaving_it_asks_for_a_frame() {
        use iced_core::window::RedrawRequest;
        let (on, off) = (Point::new(100.0, CONTROL_HEIGHT / 2.0), Point::new(100.0, CONTROL_HEIGHT * 3.0));
        assert_eq!(asked(off, on), RedrawRequest::NextFrame, "drawn hovered");
        assert_eq!(asked(on, off), RedrawRequest::NextFrame, "and idle again");
    }

    #[test]
    fn a_move_across_the_field_asks_nothing() {
        let along = |x: f32| Point::new(x, CONTROL_HEIGHT / 2.0);
        assert_eq!(asked(along(20.0), along(180.0)), iced_core::window::RedrawRequest::Wait);
    }
}
