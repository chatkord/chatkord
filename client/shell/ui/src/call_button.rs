//! The round controls of a call, and the bar they float in over it. A control's icon says how things are (crossed out
//! is off) and its label what a press does ("Unmute"), which is also its tooltip and what a screen reader says.

use crate::button::{ButtonClass, actionable};
use crate::{Icon, IconClass, Renderer, Surfaced, Theme, icon, text, tooltip};
use design::{
    Border, CALL_ARROW_WIDTH, CALL_BUTTON, CallControl, FAULT_BADGE, HIT_TARGET, IconSize, Interaction, LEAVE_WIDTH,
    Label, Popup, Radius, Role, STAGE_BAR_PAD, SemanticRole, Semantics, TypeStyle, call_button, fault_badge,
};
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::tooltip::Position;
use iced_widget::{Row, button, container};

/// Where a call control stands: in the call bar, large, or in the side pane, a hit target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallSize {
    /// In the call bar: the large control.
    Bar,
    /// In the side pane: the size of a hit target.
    Pane,
}

/// A round control of a call, named by its label, with an arrow beside it when it chooses between things.
pub struct CallButton<Message> {
    label: Label,
    glyph: Icon,
    control: CallControl,
    size: CallSize,
    wide: bool,
    on_press: Option<Message>,
    /// A button, or one that opens a layer over the screen.
    role: SemanticRole,
    /// An arrow beside it that opens what it chooses between.
    menu: Option<Arrow<Message>>,
    /// Why its last use failed, which is its tooltip beside the badge it wears.
    fault: Option<Label>,
}

/// The arrow beside a control that opens the menu of what it chooses between.
struct Arrow<Message> {
    label: Label,
    message: Message,
    /// Whether its menu is open.
    open: bool,
}

impl<'a, Message: Clone + 'a> CallButton<Message> {
    /// A round control with this icon, named by `label`. It is dimmed until it has [`CallButton::on_press`].
    pub fn new(label: Label, glyph: Icon, control: CallControl) -> Self {
        CallButton {
            label,
            glyph,
            control,
            size: CallSize::Bar,
            wide: false,
            on_press: None,
            role: SemanticRole::Button,
            menu: None,
            fault: None,
        }
    }

    /// Where it stands, which sets its size; the call bar's unless said.
    pub fn size(mut self, size: CallSize) -> Self {
        self.size = size;
        self
    }

    /// Leave is wider than it is tall, in the call bar.
    pub fn wide(mut self) -> Self {
        self.wide = true;
        self
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

    /// An arrow beside the control, named `label`, which sends `message` to open a menu, open while `open`; dimmed
    /// with the control.
    pub fn menu(mut self, label: Label, message: Message, open: bool) -> Self {
        self.menu = Some(Arrow { label, message, open });
        self
    }

    /// Held off by someone else, as a microphone an administrator muted for everyone, which `said` names as its label
    /// and tooltip: on the danger ground whatever its icon says of the person's own choice.
    pub fn held(mut self, said: Label) -> Self {
        self.label = said;
        self.control = CallControl::Off;
        self
    }

    /// Its last use failed, for the reason `said`: it is as at rest with a badge, and says the reason as its tooltip.
    pub fn failed(mut self, said: Label) -> Self {
        self.fault = Some(said);
        self.control = CallControl::Failed;
        self
    }

    /// The control as an element, its label in a tooltip, with its arrow beside it when it has a menu.
    pub fn view<R: Renderer + 'a>(self) -> Element<'a, Message, Theme, R> {
        let active = self.on_press.is_some();
        let (side, glyph_size) = match self.size {
            CallSize::Bar => (CALL_BUTTON, IconSize::Bar),
            CallSize::Pane => (HIT_TARGET, IconSize::S),
        };
        let width = if self.wide { LEAVE_WIDTH } else { side };
        let tip = self.fault.clone().unwrap_or_else(|| self.label.clone());
        let main = round(
            Semantics { role: self.role, label: self.label },
            &tip,
            self.glyph,
            self.control,
            (width, side),
            glyph_size,
            self.on_press,
        );
        let main = if self.fault.is_some() { badged(main) } else { main };
        match self.menu {
            Some(Arrow { label, message, open }) => {
                let arrow = round(
                    Semantics { role: SemanticRole::Opens { layer: Popup::Menu, open }, label: label.clone() },
                    &label,
                    Icon::ChevronDown,
                    self.control,
                    (CALL_ARROW_WIDTH, side),
                    IconSize::S,
                    active.then_some(message),
                );
                Row::with_children([main, arrow]).align_y(Alignment::Center)._boxed()
            }
            None => main,
        }
    }
}

/// One round control, `(width, height)` big, named by its label in its tooltip and to a screen reader.
fn round<'a, Message: Clone + 'a, R: Renderer + 'a>(
    semantics: Semantics,
    tip: &Label,
    glyph: Icon,
    control: CallControl,
    (width, height): (f32, f32),
    glyph_size: IconSize,
    on_press: Option<Message>,
) -> Element<'a, Message, Theme, R> {
    let look = call_button(control, if on_press.is_some() { Interaction::Idle } else { Interaction::Disabled });
    // The pointer changes the fill, never the icon: its colour is the look's at rest, or disabled.
    let content = container(icon(glyph, glyph_size, IconClass::Role(look.icon)))
        .center_x(Length::Fixed(width))
        .center_y(Length::Fixed(height));
    let inner = button::Button::new(content)
        .class(ButtonClass::Call(control))
        .padding(0)
        .width(Length::Fixed(width))
        .height(Length::Fixed(height))
        .on_press_maybe(on_press.clone());
    let control = actionable(inner, semantics, on_press);
    tooltip(control, tip, Position::Top)
}

/// `control` with the badge of a failed use at its top right corner.
fn badged<'a, Message: 'a, R: Renderer + 'a>(
    control: Element<'a, Message, Theme, R>,
) -> Element<'a, Message, Theme, R> {
    let look = fault_badge();
    let badge = container(text("!", TypeStyle::CaptionStrong))
        .center(Length::Fixed(FAULT_BADGE))
        .class(Surfaced::Fill { fill: look.fill, text: look.icon, radius: Radius::Full });
    let corner = container(badge).align_right(Length::Fill).align_top(Length::Fill);
    iced_widget::Stack::with_children([control, corner._boxed()])._boxed()
}

/// The call bar: its controls in a pill that floats over the call.
pub fn stage_bar<'a, Message: 'a, R: Renderer + 'a>(
    controls: Vec<Element<'a, Message, Theme, R>>,
) -> Element<'a, Message, Theme, R> {
    container(Row::with_children(controls).spacing(STAGE_BAR_PAD).align_y(Alignment::Center))
        .padding(STAGE_BAR_PAD)
        .class(Surfaced::StageBar)
        ._boxed()
}

/// A hairline between groups of controls in the call bar.
pub fn bar_divider<'a, Message: 'a, R: Renderer + 'a>() -> Element<'a, Message, Theme, R> {
    let line = container(iced_widget::Space::new())
        .width(Length::Fixed(design::BORDER_WIDTH))
        .height(Length::Fixed(design::CALL_BAR_DIVIDER_HEIGHT))
        .class(Surfaced::Fill {
            fill: Role::Border(Border::Subtle),
            text: Role::Content(design::Content::Primary),
            radius: Radius::None,
        });
    container(line).padding(Padding::from([0.0, design::Space::Xxs.px()]))._boxed()
}
