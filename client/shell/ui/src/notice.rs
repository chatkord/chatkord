//! A notice: one sentence about the area it sits at the top of, a dot in the colour of its kind, at most one action and
//! a ✕ when it can be put away. It is in the flow, never over what is below it, and never takes the focus.

use crate::labels::{label, phrase};
use crate::{Button, Icon, Labelled, Renderer, Surfaced, Theme, text};
use design::{
    ButtonVariant, Label, NOTICE_HEIGHT, Radius, Role, SemanticRole, Semantics, Space, Status, StatusRole, TypeStyle,
};
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Column, Row, Space as Gap, container};
use screens::{Action, Notice, Subject};
use words::{Phrase, Words};

/// A notice of a kind. `said` is its sentence, `action` a control beside it, and `dismiss` the ✕'s name and the message
/// it sends: none for a notice that is a state and goes when the state is over.
pub fn notice<'a, Message: Clone + 'a, R: Renderer + 'a>(
    kind: Status,
    said: Label,
    action: Option<Element<'a, Message, Theme, R>>,
    dismiss: Option<(Label, Message)>,
) -> Element<'a, Message, Theme, R> {
    let colour = Role::Status(kind, StatusRole::Fill);
    let dot = container(Gap::new())
        .width(Length::Fixed(Space::S.px()))
        .height(Length::Fixed(Space::S.px()))
        .class(Surfaced::Fill { fill: colour, text: colour, radius: Radius::Full });
    let sentence = text(said.as_str().to_string(), TypeStyle::Small).width(Length::Fill);
    let mut cells: Vec<Element<'a, Message, Theme, R>> = vec![dot._boxed(), sentence._boxed()];
    cells.extend(action);
    cells.extend(dismiss.map(|(name, message)| {
        Button::new(name).icon_only(Icon::Close).variant(ButtonVariant::Ghost).compact().on_press(message).view::<R>()
    }));
    let row = Row::with_children(cells).spacing(Space::M.px() - Space::Xxs.px()).align_y(Alignment::Center);
    let boxed = container(row)
        .padding(Padding { top: Space::Xs.px(), bottom: Space::Xs.px(), left: Space::M.px(), right: Space::Xs.px() })
        .width(Length::Fill)
        .height(Length::Fixed(NOTICE_HEIGHT))
        .align_y(Alignment::Center)
        .class(Surfaced::Notice(kind));
    Labelled::new(boxed, Semantics { role: SemanticRole::telling(kind), label: said })._boxed()
}

/// The room `count` notices take, one under another.
pub(crate) fn notices_height(count: usize) -> f32 {
    match count {
        0 => 0.0,
        n => n as f32 * NOTICE_HEIGHT + (n - 1) as f32 * Space::S.px(),
    }
}

/// An area's notices, one under another, each a person may put away with a ✕ sending what `away` makes of its subject.
pub(crate) fn notices<'a, Message: Clone + 'a, R: Renderer + 'a>(
    shown: &[Notice],
    away: impl Fn(Subject) -> Message,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    list(shown, away, |_| None, words)
}

/// The same, and a notice that offers an action has its button beside the sentence, sending what `act` makes of it.
pub fn notices_acting<'a, Message: Clone + 'a, R: Renderer + 'a>(
    shown: &[Notice],
    away: impl Fn(Subject) -> Message,
    act: impl Fn(Action) -> Message,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    list(shown, away, |action| Some(act(action)), words)
}

fn list<'a, Message: Clone + 'a, R: Renderer + 'a>(
    shown: &[Notice],
    away: impl Fn(Subject) -> Message,
    act: impl Fn(Action) -> Option<Message>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let rows = shown.iter().map(|one| {
        let action = one.action.and_then(|action| {
            let press = act(action)?;
            Some(
                Button::new(phrase(action.phrase(), words))
                    .variant(ButtonVariant::Secondary)
                    .compact()
                    .on_press(press)
                    .view::<R>(),
            )
        });
        let dismiss = one.dismissable().then(|| (phrase(Phrase::Dismiss, words), away(one.subject)));
        notice(one.kind, label(&words.say(&one.said)), action, dismiss)
    });
    Column::with_children(rows).spacing(Space::S.px()).width(Length::Fill)._boxed()
}

/// `content`, with the area's notices above it as wide as it is. While one may be put away, Escape puts away the first
/// such before it does anything else.
pub(crate) fn above<'a, Message: Clone + 'a, R: Renderer + 'a>(
    shown: &[Notice],
    width: f32,
    away: impl Fn(Subject) -> Message,
    content: Element<'a, Message, Theme, R>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    if shown.is_empty() {
        return content;
    }
    let top = container(notices(shown, &away, words)).width(Length::Fill.max(width));
    let column = Column::with_children([top._boxed(), content]).spacing(Space::M.px());
    match screens::notice::escapes(shown) {
        Some(subject) => crate::on_escape(column, away(subject), None)._boxed(),
        None => column._boxed(),
    }
}
