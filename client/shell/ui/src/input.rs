//! A single-line text field, styled from `design::input` and never without the label a screen reader says. Its value
//! and whether it is invalid are the screen's: the field reports what is typed and draws the edge.

use crate::wrap::{Filtered, Hide};
use crate::{Labelled, Renderer, Theme, fonts};
use design::{
    CONTROL_HEIGHT, INPUT_RADIUS, InputLook, InputState, Label, Radius, SemanticRole, Semantics, Space, TypeStyle,
};
use iced_core::widget::{Meta, Tree, tree};
use iced_core::{
    Border, Element, Event, InputMethod, Layout, Length, Padding, Pixels, Rectangle, Shell, Widget, mouse, renderer,
    window,
};
use iced_widget::text_input;

/// A field's class: whether what it holds is refused, and its corners.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputClass {
    pub(crate) invalid: bool,
    pub(crate) radius: Radius,
}

impl Default for InputClass {
    fn default() -> Self {
        InputClass { invalid: false, radius: INPUT_RADIUS }
    }
}

/// A labelled single-line field over a value the screen holds.
pub struct TextInput<'a, Message> {
    label: Label,
    placeholder: String,
    value: &'a str,
    on_input: Option<Box<dyn Fn(String) -> Message + 'a>>,
    on_submit: Option<Message>,
    invalid: bool,
    style: TypeStyle,
    leading: f32,
    trailing: f32,
    radius: Radius,
    align: iced_core::text::Alignment,
}

impl<'a, Message: Clone + 'a> TextInput<'a, Message> {
    /// A field named by `label` over `value`, the text the screen holds. It is disabled until it has
    /// [`TextInput::on_input`].
    pub fn new(label: Label, value: &'a str) -> Self {
        TextInput {
            label,
            placeholder: String::new(),
            value,
            on_input: None,
            on_submit: None,
            invalid: false,
            style: TypeStyle::Body,
            leading: 0.0,
            trailing: 0.0,
            radius: INPUT_RADIUS,
            align: iced_core::text::Alignment::Left,
        }
    }

    /// The type the value is set in: [`TypeStyle::Mono`] for an address.
    pub fn style(mut self, style: TypeStyle) -> Self {
        self.style = style;
        self
    }

    /// Room kept clear at the field's start for a mark laid over it (a search's magnifier).
    pub fn leading_room(mut self, room: f32) -> Self {
        self.leading = room;
        self
    }

    /// Its corners, where a field is not shaped as every other ([`INPUT_RADIUS`]).
    pub fn radius(mut self, radius: Radius) -> Self {
        self.radius = radius;
        self
    }

    /// Room kept clear at the field's end for a control laid over it (Paste, Show password).
    pub fn trailing_room(mut self, room: f32) -> Self {
        self.trailing = room;
        self
    }

    /// What it shows while empty and nothing is composed in it.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// What it sends as the text changes. Without it the field is disabled.
    pub fn on_input(mut self, on_input: impl Fn(String) -> Message + 'a) -> Self {
        self.on_input = Some(Box::new(on_input));
        self
    }

    /// What it sends when Enter is pressed in it.
    pub fn on_submit(mut self, message: Message) -> Self {
        self.on_submit = Some(message);
        self
    }

    /// A number: set at the field's end, as numbers are, so its digits line up with the end of the row.
    pub fn number(mut self) -> Self {
        self.align = iced_core::text::Alignment::Right;
        self
    }

    /// What it holds is refused: drawn with the danger edge.
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// The field as an element, labelled for a screen reader and reached by Tab.
    pub fn view<R: Renderer + 'a>(self) -> Element<'a, Message, Theme, R> {
        let semantics = Semantics { role: SemanticRole::TextInput, label: self.label.clone() };
        let disabled = self.on_input.is_none();
        let quiet = Quiet {
            empty: self.value.is_empty(),
            invalid: self.invalid,
            padding: padding(self.style, self.leading, self.trailing),
            content: field(
                &self.placeholder,
                self.value,
                Field {
                    invalid: self.invalid,
                    secure: false,
                    style: self.style,
                    leading: self.leading,
                    trailing: self.trailing,
                    radius: self.radius,
                    align: self.align,
                },
                self.on_input,
                self.on_submit,
            ),
        };
        let labelled = Labelled::new(quiet, semantics);
        (if disabled { labelled.without_focus() } else { labelled })._boxed()
    }
}

/// What differs between the fields built here.
#[derive(Clone, Copy)]
pub(crate) struct Field {
    pub invalid: bool,
    pub secure: bool,
    pub style: TypeStyle,
    /// Room at the start for a mark laid over the field.
    pub leading: f32,
    /// Room at the end for a control laid over the field.
    pub trailing: f32,
    pub radius: Radius,
    /// Where the text stands across the field.
    pub align: iced_core::text::Alignment,
}

/// The iced field with our font, size, padding and class: the one place a field is assembled, so a
/// plain and a secret one cannot drift apart.
pub(crate) fn field<'a, Message: Clone + 'a>(
    placeholder: &str,
    value: &'a str,
    options: Field,
    on_input: Option<Box<dyn Fn(String) -> Message + 'a>>,
    on_submit: Option<Message>,
) -> text_input::TextInput<'a, Message, Theme> {
    let style = options.style;
    let mut field = text_input::TextInput::new(placeholder.to_string(), value)
        .class(InputClass { invalid: options.invalid, radius: options.radius })
        .secure(options.secure)
        .font(fonts::font(style))
        .size(Pixels(style.size()))
        .line_height(iced_core::text::LineHeight::Absolute(Pixels(style.line_height())))
        .padding(padding(style, options.leading, options.trailing))
        .align_x(options.align)
        .width(Length::Fill);
    if let Some(on_input) = on_input {
        field = field.on_input(on_input);
    }
    if let Some(message) = on_submit {
        field = field.on_submit(message);
    }
    field
}

/// The room between a field's edge and its text, with `leading` and `trailing` kept clear at its ends.
pub(crate) fn padding(style: TypeStyle, leading: f32, trailing: f32) -> Padding {
    let vertical = (CONTROL_HEIGHT - style.line_height()) / 2.0;
    Padding { top: vertical, bottom: vertical, left: Space::M.px() + leading, right: Space::M.px() + trailing }
}

/// A plain field that hides its placeholder while an input method composes in it: iced draws the
/// placeholder whenever the value is empty, and the composing text covers only its own width.
struct Quiet<W> {
    content: W,
    empty: bool,
    invalid: bool,
    padding: Padding,
}

/// Whether an input method is composing text in the field, as the field last asked for one.
#[derive(Default)]
struct Composing(bool);

impl<W> Meta for Quiet<W> {}

fn composes(input_method: &InputMethod) -> bool {
    matches!(input_method, InputMethod::Enabled { preedit: Some(preedit), .. } if !preedit.content.is_empty())
}

impl<Message, W, R> Widget<Message, Theme, R> for Quiet<W>
where
    R: renderer::Renderer,
    W: Widget<Message, Theme, R>,
{
    crate::wrap::forward!(size diff layout mouse_interaction overlay);

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &R,
        operation: &mut dyn iced_core::widget::Operation,
    ) {
        let hide = Hide { placeholder: self.empty, ..Hide::default() };
        self.content.operate(
            &mut tree.children[0],
            layout,
            viewport,
            renderer,
            &mut Filtered { inner: operation, hide },
        );
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Composing>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Composing::default())
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
        let asked_before = !matches!(shell.input_method(), InputMethod::Disabled);
        self.content.update(&mut tree.children[0], event, layout, cursor, renderer, shell, viewport);
        // The field asks for an input method only as it is drawn; one asked before it is another field's.
        if let Event::Window(window::Event::RedrawRequested(_)) = event {
            tree.state.downcast_mut::<Composing>().0 = !asked_before && composes(shell.input_method());
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
        self.content.draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
        if self.empty && tree.state.downcast_ref::<Composing>().0 {
            let fill = theme.color(design::input(InputState::Focused, self.invalid).fill);
            let bounds = layout.bounds().shrink(self.padding);
            // A layer of its own: within one, text is drawn over quads.
            renderer.with_layer(bounds, |renderer| {
                renderer.fill_quad(renderer::Quad { bounds, ..renderer::Quad::default() }, fill);
            });
        }
    }
}

impl text_input::Catalog for Theme {
    type Class<'a> = InputClass;

    fn default<'a>() -> Self::Class<'a> {
        InputClass::default()
    }

    fn style(&self, class: &Self::Class<'_>, status: text_input::Status) -> text_input::Style {
        let state = match status {
            text_input::Status::Active => InputState::Idle,
            text_input::Status::Hovered => InputState::Hover,
            text_input::Status::Focused { .. } => InputState::Focused,
            text_input::Status::Disabled => InputState::Disabled,
        };
        let InputLook { fill, text, placeholder, border, border_width } = design::input(state, class.invalid);
        text_input::Style {
            background: iced_core::Background::Color(self.color(fill)),
            border: Border { color: self.color(border), width: border_width, radius: class.radius.px().into() },
            placeholder: self.color(placeholder),
            value: self.color(text),
            selection: crate::color::translucent(self.palette().selection()),
        }
    }
}
