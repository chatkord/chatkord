//! The toast: one look for every event, failures too. Reversed colours at the foot of the window's area, a status dot
//! before the sentence unless the news is plain, at most one action as a pill, and a ✕ whenever it has a dot or an
//! action. It stays while the pointer is on it. It decides nothing: `screens` says what it holds and for how long.

use crate::button::{ButtonClass, actionable};
use crate::labels::{label, phrase};
use crate::status_dot::status_dot;
use crate::{Button, Icon, IconClass, Labelled, Renderer, Surfaced, Theme, icon, text};
use design::{
    ButtonVariant, IconSize, Role, SMALL_TARGET, SemanticRole, Semantics, Space, Surface, TOAST_MOST, TypeStyle,
};
use iced_core::length::Constraint;
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Row, button, container, mouse_area};
use words::{Phrase, Words};

/// What a toast sends: its action pressed, its ✕, and the pointer arriving on it or leaving.
pub struct ToastSends<Message> {
    /// The action's pill.
    pub act: Message,
    /// The ✕.
    pub away: Message,
    /// The pointer onto it (`true`) or off it.
    pub held: fn(bool) -> Message,
}

/// `shown` as a toast, said in `words`.
pub fn toast<'a, Message: Clone + 'a, R: Renderer + 'a>(
    shown: &screens::Toast,
    sends: &ToastSends<Message>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let said = words.say(shown.said());
    let named = label(&said);
    let mut cells: Vec<Element<'a, Message, Theme, R>> = Vec::new();
    if shown.telling.dotted() {
        cells.push(status_dot(shown.kind()));
    }
    // The sentence gives way to its action and ✕ where the window is narrow, and wraps.
    cells.push(text(said, TypeStyle::Small).width(Length::Fluid(Constraint::Max))._boxed());
    cells.extend(shown.action().map(|action| {
        Button::new(label(&words.say(&action.said())))
            .variant(ButtonVariant::Primary)
            .compact()
            .on_press(sends.act.clone())
            .view::<R>()
    }));
    if shown.closable() {
        cells.push(away(phrase(Phrase::Dismiss, words), sends.away.clone()));
    }
    let row = Row::with_children(cells).spacing(Space::M.px()).align_y(Alignment::Center);
    let pill = container(row)
        .padding(Padding { top: Space::S.px(), bottom: Space::S.px(), left: Space::M.px(), right: Space::M.px() })
        .width(Length::Fluid(Constraint::Max))
        .class(Surfaced::Toast);
    let held = sends.held;
    let area = mouse_area(pill).on_enter(held(true)).on_exit(held(false));
    let area = container(area).center_x(Length::Fill.max(TOAST_MOST));
    Labelled::new(area, Semantics { role: SemanticRole::telling(shown.kind()), label: named })._boxed()
}

/// The ✕: drawn in the toast's own ink, which is the window's colour, so it reads on the reversed ground.
fn away<'a, Message: Clone + 'a, R: Renderer + 'a>(
    name: design::Label,
    press: Message,
) -> Element<'a, Message, Theme, R> {
    let ink = Role::Surface(Surface::Base);
    let glyph = container(icon(Icon::Close, IconSize::S, IconClass::Role(ink))).center(Length::Fixed(SMALL_TARGET));
    let inner = button::Button::new(glyph)
        .class(ButtonClass::Swatch)
        .padding(0)
        .width(Length::Fixed(SMALL_TARGET))
        .height(Length::Fixed(SMALL_TARGET))
        .on_press(press.clone());
    actionable(inner, Semantics { role: SemanticRole::Button, label: name }, Some(press))
}
