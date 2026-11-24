//! A row of a list: something to the left, a name, something to the right. Pressed and reached like a button, it shows
//! that it is selected by weight and an edge as well as by its fill.

use crate::button::{ButtonClass, RowTone, actionable};
use crate::typography::{coloured, line, text};
use crate::{Icon, IconClass, Renderer, Surfaced, TextClass, Theme, icon};
use design::{
    AccentRole, CHECK_BOX, COLOUR_DOT, Content, IconSize, Label, Rgb, Role, SemanticRole, Semantics, Space, TypeStyle,
};
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Row, Space as Gap, button, container};

/// A row of a list named by its label, a control once it has [`ListRow::on_press`].
pub struct ListRow<'a, Message, R> {
    label: Label,
    leading: Option<Element<'a, Message, Theme, R>>,
    trailing: Option<Element<'a, Message, Theme, R>>,
    selected: bool,
    tone: RowTone,
    on_press: Option<Message>,
    role: SemanticRole,
    tooltip: bool,
    /// The name wraps onto more lines instead of being cut with an ellipsis.
    wrapped: bool,
    /// What the tooltip says where it is not the name: what a press does.
    tip: Option<Label>,
    /// What is written right after the name, and heard after it.
    note: Option<String>,
    /// How the name stands out from the rest of a quiet list.
    standing: Standing,
    /// What a screen reader hears where it is more than the name: its unread mark, its mentions.
    heard: Option<Label>,
}

/// How a row's name stands out from the rest of its list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Standing {
    /// As its tone draws it.
    #[default]
    Plain,
    /// Something new in it: strong and in the primary colour.
    Unread,
    /// Muted: in the colour of what cannot be used now.
    Faint,
}

impl<'a, Message: Clone + 'a, R: Renderer + 'a> ListRow<'a, Message, R> {
    /// A row named by `label`, which is also its tooltip and what a screen reader says.
    pub fn new(label: Label) -> Self {
        ListRow {
            label,
            leading: None,
            trailing: None,
            selected: false,
            tone: RowTone::Plain,
            on_press: None,
            role: SemanticRole::ListItem,
            tooltip: true,
            wrapped: false,
            tip: None,
            note: None,
            standing: Standing::Plain,
            heard: None,
        }
    }

    /// Something new in it, as a channel with unread messages: its name strong and in the primary colour.
    pub fn unread(mut self, unread: bool) -> Self {
        if unread {
            self.standing = Standing::Unread;
        }
        self
    }

    /// Muted, as a muted channel: its name faint, whatever else it is.
    pub fn faint(mut self, faint: bool) -> Self {
        if faint {
            self.standing = Standing::Faint;
        }
        self
    }

    /// What a screen reader hears of it in place of its name: the name with what its marks say.
    pub fn heard(mut self, heard: Label) -> Self {
        self.heard = Some(heard);
        self
    }

    /// What stands before the name: an icon, a picture.
    pub fn leading(mut self, element: impl Into<Element<'a, Message, Theme, R>>) -> Self {
        self.leading = Some(element.into());
        self
    }

    /// What stands after the name, at the row's end.
    pub fn trailing(mut self, element: impl Into<Element<'a, Message, Theme, R>>) -> Self {
        self.trailing = Some(element.into());
        self
    }

    /// An item of a menu: said as one, and without a tooltip (a menu is short and its items are whole).
    pub fn menu_item(mut self) -> Self {
        self.role = SemanticRole::MenuItem;
        self.tooltip = false;
        self
    }

    /// One choice of a menu of which one is taken: said as one and whether it is taken, drawn on the accent's tint
    /// with a check in the accent when it is, and without a tooltip.
    pub fn menu_choice(mut self, chosen: bool) -> Self {
        self.role = SemanticRole::MenuItemRadio { chosen };
        self.tooltip = false;
        if chosen {
            self.tone = RowTone::Chosen;
        }
        self.leading = Some(tick(chosen));
        self
    }

    /// `mark` beside what already stands before the name: a role's dot after a choice's check.
    pub fn after_mark(mut self, mark: Element<'a, Message, Theme, R>) -> Self {
        let before = self.leading.take();
        let marks = Row::with_children(before.into_iter().chain([mark]))
            .spacing(Space::S.px() + Space::Xxs.px())
            .align_y(Alignment::Center);
        self.leading = Some(marks._boxed());
        self
    }

    /// One choice of a menu of which any are taken, in the colour it is shown in (none for none): said as one and
    /// whether it is ticked, its box filled with a check in the accent when it is, and without a tooltip.
    pub fn menu_check(mut self, checked: bool, colour: Option<Rgb>) -> Self {
        self.role = SemanticRole::MenuItemCheckbox { checked };
        self.tooltip = false;
        let marks = Row::with_children([check_box(checked), colour_dot(colour)])
            .spacing(Space::S.px() + Space::Xxs.px())
            .align_y(Alignment::Center);
        self.leading = Some(marks._boxed());
        self
    }

    /// A note right after the name, smaller and in the secondary colour, heard after the name as one: "Default" beside
    /// the device the system's default follows, the language Match system says.
    pub fn noted(mut self, note: &str) -> Self {
        self.note = Some(note.to_string());
        self
    }

    /// One of a few choices, with its mark: said as an option and whether it is the one taken, without a tooltip.
    pub fn radio(mut self, chosen: bool) -> Self {
        self.role = SemanticRole::Radio { chosen };
        self.tooltip = false;
        self.leading = Some(crate::radio::radio_dot(chosen));
        self
    }

    /// A name that is a device's, and as long as one: it wraps, and the row grows, rather than losing its end.
    pub fn wrapped(mut self) -> Self {
        self.wrapped = true;
        self
    }

    /// The row chosen now: drawn strong, with an edge and a fill.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// A row of the side pane: its name quiet until it is selected or the pointer is on it.
    pub fn quiet(mut self) -> Self {
        self.tone = RowTone::Quiet;
        self
    }

    /// An action that removes or leaves something: its name in the danger colour and the strong weight.
    pub fn danger(mut self) -> Self {
        self.tone = RowTone::Danger;
        self
    }

    /// The tooltip says what a press does, where the name alone does not.
    pub fn tip(mut self, tip: Label) -> Self {
        self.tip = Some(tip);
        self
    }

    /// Without it the row is not a control: it is shown, and Tab passes it.
    pub fn on_press(mut self, message: Message) -> Self {
        self.on_press = Some(message);
        self
    }

    /// The row as an element: a control reached by Tab once it can be pressed.
    pub fn view(self) -> Element<'a, Message, Theme, R> {
        let menu_item = matches!(
            self.role,
            SemanticRole::MenuItem | SemanticRole::MenuItemRadio { .. } | SemanticRole::MenuItemCheckbox { .. }
        );
        let strong = self.selected || self.tone == RowTone::Danger || self.standing == Standing::Unread;
        let style = match (menu_item, strong) {
            (false, false) => TypeStyle::Body,
            (false, true) => TypeStyle::BodyStrong,
            (true, false) => TypeStyle::Small,
            (true, true) => TypeStyle::SmallStrong,
        };
        let name = if self.wrapped {
            text(self.label.as_str().to_string(), style)
        } else {
            line(self.label.as_str().to_string(), style)
        };
        let name = match self.standing {
            Standing::Plain => name,
            Standing::Unread => name.class(TextClass::Content(Content::Primary)),
            Standing::Faint => name.class(TextClass::Content(Content::Disabled)),
        };
        let named = match &self.note {
            Some(note) => {
                let note = coloured(note.clone(), TypeStyle::Caption, TextClass::Content(Content::Secondary));
                Row::with_children([name._boxed(), note._boxed()])
                    .spacing(Space::S.px())
                    .align_y(Alignment::Center)
                    .width(Length::Fill)
                    ._boxed()
            }
            None => name.width(Length::Fill)._boxed(),
        };
        let mut cells: Vec<Element<'a, Message, Theme, R>> = Vec::new();
        cells.extend(self.leading);
        cells.push(named);
        cells.extend(self.trailing);
        let content = Row::with_children(cells).spacing(Space::S.px()).align_y(Alignment::Center);
        let inner = button::Button::new(content)
            .class(ButtonClass::Row { selected: self.selected, tone: self.tone })
            // A row is a target's height: its line and the padding round it. A menu's item is a little taller and
            // set in from the menu's edge.
            .padding(if menu_item {
                Padding::from([(design::MENU_ITEM_HEIGHT - style.line_height()) / 2.0, Space::S.px() + Space::Xxs.px()])
            } else {
                Padding::from([Space::S.px() - Space::Xxs.px(), Space::S.px()])
            })
            .width(Length::Fill)
            .on_press_maybe(self.on_press.clone());
        let full_name = self.label.clone();
        let said = match (&self.heard, &self.note) {
            (Some(heard), _) => heard.clone(),
            (None, Some(note)) => {
                Label::new(&format!("{} {note}", self.label.as_str())).unwrap_or_else(|_| self.label.clone())
            }
            (None, None) => self.label.clone(),
        };
        let semantics = Semantics { role: self.role, label: said };
        let row = actionable(inner, semantics, self.on_press);
        let tip = self.tip.unwrap_or(full_name);
        if self.tooltip { crate::tooltip(row, &tip, iced_widget::tooltip::Position::Bottom) } else { row }
    }
}

/// A colour's dot, as a role is shown in it: the colour picked, or the strong edge where none was.
pub(crate) fn colour_dot<'a, Message: 'a, R: Renderer + 'a>(colour: Option<Rgb>) -> Element<'a, Message, Theme, R> {
    container(Gap::new())
        .width(Length::Fixed(COLOUR_DOT))
        .height(Length::Fixed(COLOUR_DOT))
        .class(Surfaced::Dot(colour))
        ._boxed()
}

/// The box beside a choice of which any are taken: the strong edge round the card's surface, filled with the accent
/// and its check when ticked.
fn check_box<'a, Message: 'a, R: Renderer + 'a>(checked: bool) -> Element<'a, Message, Theme, R> {
    let (fill, edge) = if checked {
        (Role::Accent(AccentRole::Fill), Role::Accent(AccentRole::Fill))
    } else {
        (Role::Surface(design::Surface::Base), Role::Border(design::Border::Strong))
    };
    let check: Element<'a, Message, Theme, R> = if checked {
        icon(Icon::Check, IconSize::Mark, IconClass::Role(Role::Accent(AccentRole::OnFill)))._boxed()
    } else {
        Gap::new()._boxed()
    };
    container(check).center(Length::Fixed(CHECK_BOX)).class(Surfaced::CheckBox { fill, edge })._boxed()
}

/// The check beside a menu's choice that is taken, or the room it would take.
fn tick<'a, Message: 'a, R: Renderer + 'a>(on: bool) -> Element<'a, Message, Theme, R> {
    let side = IconSize::S.px();
    if on {
        icon(Icon::Check, IconSize::S, IconClass::Role(Role::Accent(AccentRole::Text)))._boxed()
    } else {
        Gap::new().width(Length::Fixed(side)).height(Length::Fixed(side))._boxed()
    }
}
