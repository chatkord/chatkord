//! The button: a stock iced button restyled from `design::button`, wrapped in [`Focus`] so the keyboard reaches it,
//! and never without a label, which is its text and what a screen reader says.

use crate::{Focus, Icon, IconClass, Renderer, Theme, icon, tooltip};
use design::{
    ButtonVariant, CONTROL_HEIGHT, IconSize, Interaction, Label, Popup, SemanticRole, Semantics, Space, TypeStyle,
};
use iced_core::text::Wrapping;
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::tooltip::Position;
use iced_widget::{Row, button};

/// What a button-shaped control is drawn as: a button for what it is for and whether its work is under
/// way, or a row of a list, selected or not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonClass {
    /// A button, drawn by its variant's look.
    Button {
        /// What it is for, which sets its colours.
        variant: ButtonVariant,
        /// Its work is under way: its label ends in an ellipsis and it takes no input.
        loading: bool,
    },
    /// A row of a list, the whole row its hit target.
    Row {
        /// The row chosen now: drawn strong, with an edge and a fill.
        selected: bool,
        /// How its name is set while it is not selected.
        tone: RowTone,
    },
    /// An item of the server rail: the server's picture is all it draws.
    Rail,
    /// A rail item that is no server, the gear or the "+": its face is a quiet row's, filled under the pointer.
    RailFace {
        /// The page it opens is the one shown.
        current: bool,
    },
    /// A colour picker's swatch: the swatch is all it draws.
    Swatch,
    /// The search pill in the top bar: the pill is all it draws, under the pointer as at rest.
    SearchPill,
    /// A pill among a few: a choice on its track, or a page of a list of pages.
    Pill {
        /// On a track or in a page list, which sets how the chosen one looks.
        kind: design::PillKind,
        /// The one chosen among its pills.
        chosen: bool,
    },
    /// A round control of a call.
    Call(design::CallControl),
    /// The track of a switch: the knob it holds is all it draws besides its colour.
    Switch {
        /// Whether the switch is on, which sets the track's colour.
        on: bool,
    },
    /// A card chosen by a press: a source in our own picker.
    Card {
        /// The card chosen now.
        selected: bool,
    },
    /// A field that opens a menu of choices: drawn as a text field is.
    Field,
    /// A reaction under a message, a pill with an edge, one's own ringed in the accent.
    Chip {
        /// This account reacted with it.
        mine: bool,
    },
}

/// How a row's name is set while nothing selects it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowTone {
    /// In the primary content colour.
    Plain,
    /// A row of the side pane, quiet until the pointer is on it.
    Quiet,
    /// An action that removes or leaves something.
    Danger,
    /// The choice of a menu that is taken: on the accent's tint.
    Chosen,
}

impl Default for ButtonClass {
    fn default() -> Self {
        ButtonClass::Button { variant: ButtonVariant::Secondary, loading: false }
    }
}

/// An iced button made reachable by the keyboard: the one place a button-shaped control is wrapped, so a
/// plain button and a list row cannot differ in how Tab, Space and Enter reach them.
pub(crate) fn actionable<'a, Message: Clone + 'a, R: Renderer + 'a, W>(
    inner: iced_widget::Button<'a, Message, W, Theme>,
    semantics: Semantics,
    on_press: Option<Message>,
) -> Element<'a, Message, Theme, R>
where
    W: Widget<Message, Theme, R> + 'a,
{
    let mut focus = Focus::new(inner, semantics);
    if let Some(message) = on_press {
        focus = focus.on_activate(message);
    }
    focus._boxed()
}

/// `shown` as a press that opens a popover, with no padding of its own and no look until the pointer is on it, named
/// `named` to a screen reader.
pub(crate) fn opening<'a, Message: Clone + 'a, R: Renderer + 'a>(
    shown: Element<'a, Message, Theme, R>,
    open: Message,
    named: Label,
) -> Element<'a, Message, Theme, R> {
    let inner = button(shown)
        .class(ButtonClass::Row { selected: false, tone: RowTone::Plain })
        .padding(0)
        .on_press(open.clone());
    let semantics = Semantics { role: SemanticRole::Opens { layer: Popup::Dialog, open: false }, label: named };
    actionable(inner, semantics, Some(open))
}

/// A picture that takes a pointer press and nothing else: no control for a screen reader, no stop in the focus order,
/// for what another control beside it already does by keyboard.
pub fn pointer_press<'a, Message: Clone + 'a, R: Renderer + 'a>(
    content: Element<'a, Message, Theme, R>,
    on_press: Message,
) -> Element<'a, Message, Theme, R> {
    iced_widget::mouse_area(content).on_press(on_press).interaction(iced_core::mouse::Interaction::Pointer)._boxed()
}

/// How tall a button is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Height {
    /// A control's, with the body's type.
    Control,
    /// A hit target's, with a label's type.
    Target,
    /// Smaller than a hit target: a glyph on a card in the strip.
    Small,
}

/// A button with a label, in a variant of the design, an icon before the label or in its place.
pub struct Button<'a, Message> {
    label: Label,
    icon: Option<Icon>,
    icon_only: bool,
    /// An icon after the label: the chevron of a button that opens a menu.
    after: Option<Icon>,
    variant: ButtonVariant,
    on_press: Option<Message>,
    /// A button, or one that opens a layer over the screen.
    role: SemanticRole,
    loading: bool,
    height: Height,
    width: Length,
    _lifetime: std::marker::PhantomData<&'a ()>,
}

impl<'a, Message: Clone + 'a> Button<'a, Message> {
    /// A button with this label. It is disabled until it has [`Button::on_press`].
    pub fn new(label: Label) -> Self {
        Button {
            label,
            icon: None,
            icon_only: false,
            after: None,
            variant: ButtonVariant::Secondary,
            on_press: None,
            role: SemanticRole::Button,
            loading: false,
            height: Height::Control,
            width: Length::Shrink,
            _lifetime: std::marker::PhantomData,
        }
    }

    /// What it sends when pressed, or activated by Space or Enter.
    pub fn on_press(mut self, message: Message) -> Self {
        self.on_press = Some(message);
        self
    }

    /// A press opens `layer` over the screen, which is showing while `open`, as a screen reader is told.
    pub fn opens(mut self, layer: Popup, open: bool) -> Self {
        self.role = SemanticRole::Opens { layer, open };
        self
    }

    /// An icon before the label.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// An icon after the label, the small size: the chevron of a button that opens a menu.
    pub fn icon_after(mut self, icon: Icon) -> Self {
        self.after = Some(icon);
        self
    }

    /// Shows only the icon: the label is then what a screen reader says and what a tooltip shows.
    pub fn icon_only(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self.icon_only = true;
        self
    }

    /// What it is for, which sets its look; secondary unless said.
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Its action is under way: it takes no input and its label says so.
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    /// The height of a hit target rather than a control, with a label's size: a button inside a field.
    pub fn compact(mut self) -> Self {
        self.height = Height::Target;
        self
    }

    /// Smaller than a hit target, for a glyph alone on a card in the strip under a pinned one.
    pub fn small(mut self) -> Self {
        self.height = Height::Small;
        self
    }

    pub(crate) fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// The button as an element, reached by Tab and pressed by Space or Enter; an icon-only one shows its label in
    /// a tooltip.
    pub fn view<R: Renderer + 'a>(self) -> Element<'a, Message, Theme, R> {
        // The main and the destructive action are set strong; a link is as small as the line it sits in.
        let style = match self.variant {
            _ if self.height != Height::Control => TypeStyle::Label,
            design::ButtonVariant::Primary | design::ButtonVariant::Danger => TypeStyle::BodyStrong,
            design::ButtonVariant::Secondary
            | design::ButtonVariant::Ghost
            | design::ButtonVariant::Quiet
            | design::ButtonVariant::OnScrim
            | design::ButtonVariant::OnPicture => TypeStyle::Body,
            design::ButtonVariant::Link => TypeStyle::Small,
        };
        let shown =
            if self.loading { format!("{}\u{2026}", self.label.as_str()) } else { self.label.as_str().to_string() };
        let active = !self.loading && self.on_press.is_some();
        // An icon is in the colour of the look's text, which changes with being disabled, never with the pointer.
        let tone = design::button(self.variant, if active { Interaction::Idle } else { Interaction::Disabled }).text;
        // An icon alone is the small size, as the design draws a glyph button; beside words it matches the line.
        let glyph_size = if self.icon_only { IconSize::S } else { IconSize::M };
        let mut cells: Vec<Element<'a, Message, Theme, R>> = Vec::new();
        if let Some(glyph) = self.icon {
            cells.push(icon(glyph, glyph_size, IconClass::Role(tone))._boxed());
        }
        if !self.icon_only {
            // A label stays on one line: a button is as wide as its words.
            cells.push(match self.variant {
                design::ButtonVariant::Link => crate::typography::underlined(shown, style)._boxed(),
                design::ButtonVariant::Primary
                | design::ButtonVariant::Secondary
                | design::ButtonVariant::Danger
                | design::ButtonVariant::Ghost
                | design::ButtonVariant::Quiet
                | design::ButtonVariant::OnScrim
                | design::ButtonVariant::OnPicture => {
                    crate::typography::text(shown, style).wrapping(Wrapping::None)._boxed()
                }
            });
            cells.extend(self.after.map(|glyph| icon(glyph, IconSize::S, IconClass::Role(tone))._boxed()));
        }
        let spacing = if self.after.is_some() { Space::Xs.px() } else { Space::S.px() };
        let content = Row::with_children(cells).spacing(spacing).align_y(Alignment::Center);
        let content = iced_widget::container(content).center_x(self.width);
        let height = match self.height {
            Height::Control => CONTROL_HEIGHT,
            Height::Target => design::HIT_TARGET,
            Height::Small => design::SMALL_TARGET,
        };
        let content_height =
            if self.icon.is_some() { style.line_height().max(glyph_size.px()) } else { style.line_height() };
        let padding = match (self.icon_only, self.variant) {
            // A link is its words, inline.
            (false, design::ButtonVariant::Link) => Padding::ZERO,
            // An icon alone: centred both ways in a circle as wide as the button is tall.
            (true, _) => Padding::from((height - glyph_size.px()) / 2.0),
            // The one main action is a little wider than the rest, as the design draws it.
            (false, design::ButtonVariant::Primary) => {
                Padding::from([(height - content_height) / 2.0, Space::L.px() + Space::Xs.px()])
            }
            (false, _) => Padding::from([(height - content_height) / 2.0, Space::L.px()]),
        };
        let inner = button::Button::new(content)
            .class(ButtonClass::Button { variant: self.variant, loading: self.loading })
            // iced sets the content at the top: the padding centres it in a control of the one height.
            .padding(padding)
            .width(self.width)
            .height(Length::Shrink)
            .on_press_maybe(if active { self.on_press.clone() } else { None });
        let full_name = self.label.clone();
        let control = actionable(
            inner,
            Semantics { role: self.role, label: self.label },
            if active { self.on_press } else { None },
        );
        if self.icon_only { tooltip(control, &full_name, Position::Bottom) } else { control }
    }
}
