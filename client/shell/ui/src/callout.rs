//! The callout: one box wherever a note or a refusal is boxed. The surface colour in a hairline edge, which is the
//! status colour when it has a status; a dot only with a status, an optional title, the sentence, at most one action
//! as a pill at the right and a ✕ only where it can be put away. It decides nothing: `screens` says what it holds.

use crate::labels::{label, phrase};
use crate::status_dot::status_dot;
use crate::{Button, Icon, Labelled, Renderer, Surfaced, Theme, text};
use design::{ButtonVariant, CALLOUT_PAD, Label, SemanticRole, Semantics, Space, TypeStyle};
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Column, Row, container};
use screens::Callout;
use words::{Phrase, Words};

/// What a callout sends, and what it holds besides its sentence.
pub struct CalloutParts<'a, Message, R: Renderer> {
    /// What the action's pill sends, where the callout offers one.
    pub act: Option<Message>,
    /// What the ✕ sends, where the callout may be put away.
    pub away: Option<Message>,
    /// A part of the page the box holds under the sentence: a server's fingerprint.
    pub under: Option<Element<'a, Message, Theme, R>>,
}

impl<Message, R: Renderer> Default for CalloutParts<'_, Message, R> {
    fn default() -> Self {
        CalloutParts { act: None, away: None, under: None }
    }
}

/// `shown` as a box as wide as the room it is in, said in `words`.
pub fn callout<'a, Message: Clone + 'a, R: Renderer + 'a>(
    shown: &Callout,
    parts: CalloutParts<'a, Message, R>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let said = words.say(&shown.said);
    let title = shown.title.as_ref().map(|title| words.say(title));
    let named: Label = label(&title.as_deref().map_or_else(|| said.clone(), |title| format!("{title}. {said}")));
    let mut lines: Vec<Element<'a, Message, Theme, R>> = Vec::new();
    lines.extend(title.map(|title| text(title, TypeStyle::SmallStrong)._boxed()));
    lines.push(text(said, TypeStyle::Small)._boxed());
    lines.extend(parts.under);
    // The dot stands level with the first line of the text, whatever the box holds under it.
    let dot = shown.kind.map(|kind| {
        container(status_dot(kind)).padding(Padding { top: Space::Xs.px() + Space::Xxs.px(), ..Padding::ZERO })._boxed()
    });
    let text = Column::with_children(lines).spacing(Space::Xxs.px()).width(Length::Fill)._boxed();
    let told = Row::with_children(dot.into_iter().chain([text])).spacing(Space::M.px()).align_y(Alignment::Start);
    let mut cells: Vec<Element<'a, Message, Theme, R>> = vec![told.width(Length::Fill)._boxed()];
    cells.extend(shown.action.as_ref().zip(parts.act).map(|(action, press)| {
        Button::new(label(&words.say(&action.said())))
            .variant(ButtonVariant::Primary)
            .compact()
            .on_press(press)
            .view::<R>()
    }));
    cells.extend(shown.dismissable.then_some(parts.away).flatten().map(|press| {
        Button::new(phrase(Phrase::Dismiss, words))
            .icon_only(Icon::Close)
            .variant(ButtonVariant::Ghost)
            .compact()
            .on_press(press)
            .view::<R>()
    }));
    let row = Row::with_children(cells).spacing(Space::M.px()).align_y(Alignment::Center);
    let boxed = container(row).padding(CALLOUT_PAD).width(Length::Fill).class(Surfaced::Callout(shown.kind));
    let role = shown.kind.map_or(SemanticRole::Status, SemanticRole::telling);
    Labelled::new(boxed, Semantics { role, label: named })._boxed()
}

/// `content` with the callouts a page keeps above it, each with a ✕ sending what `away` makes of its subject. While
/// one may be put away, Escape puts away the first such before it does anything else.
pub(crate) fn callouts_above<'a, Message: Clone + 'a, R: Renderer + 'a>(
    shown: &[screens::Notice],
    width: f32,
    away: impl Fn(screens::Subject) -> Message,
    content: Element<'a, Message, Theme, R>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    if shown.is_empty() {
        return content;
    }
    let boxes = shown.iter().map(|one| {
        let parts = CalloutParts { act: None, away: Some(away(one.subject)), under: None };
        callout(&one.callout(), parts, words)
    });
    let top = container(Column::with_children(boxes).spacing(Space::S.px())).width(Length::Fill.max(width));
    let column = Column::with_children([top._boxed(), content]).spacing(Space::M.px());
    match screens::notice::escapes(shown) {
        Some(subject) => crate::on_escape(column, away(subject), None)._boxed(),
        None => column._boxed(),
    }
}
