//! A choice of a few: its name, then pills on a rounded track, the chosen one raised. Each pill is reached by Tab and
//! taken by Space or Enter like a button, and a screen reader hears a group of options with the chosen one marked.

use crate::button::{ButtonClass, actionable};
use crate::{Labelled, Renderer, Surfaced, TextClass, Theme, coloured, text};
use design::TypeStyle;
use design::{Content, HIT_TARGET, Label, PillKind, Radius, SMALL_TARGET, SemanticRole, Semantics, Space, Surface};
use iced_core::text::Wrapping;
use iced_core::{Element, Length, Padding, Widget};
use iced_widget::{Column, Row, button, container};

/// The options of a choice named `name`, each with what taking it sends; `chosen` is the index of the one taken.
/// `fill` spreads the track across its line, the pills sharing it; without it the track is as wide as its words.
pub fn choice<'a, Message: Clone + 'a, R: Renderer + 'a>(
    name: Label,
    options: Vec<(Label, Message)>,
    chosen: usize,
    fill: bool,
) -> Element<'a, Message, Theme, R> {
    let width = if fill { Length::Fill } else { Length::Shrink };
    let heading = coloured(name.as_str().to_string(), TypeStyle::Label, TextClass::Content(Content::Primary));
    let group = Column::with_children([heading._boxed(), pills(options, chosen, width)])
        .spacing(Space::Xs.px() + Space::Xxs.px())
        .width(width);
    Labelled::new(group, Semantics { role: SemanticRole::RadioGroup, label: name })._boxed()
}

/// The pills of a choice alone, as wide as their words, for a page that heads it with its own section heading.
pub(crate) fn choice_track<'a, Message: Clone + 'a, R: Renderer + 'a>(
    name: Label,
    options: Vec<(Label, Message)>,
    chosen: usize,
) -> Element<'a, Message, Theme, R> {
    // Where its options do not all fit, the track scrolls across rather than shrink one or run past its room.
    let track = iced_widget::scrollable(pills(options, chosen, Length::Shrink))
        .direction(iced_widget::scrollable::Direction::Horizontal(crate::theme::scroll_bar()))
        .width(Length::Shrink);
    Labelled::new(track, Semantics { role: SemanticRole::RadioGroup, label: name })._boxed()
}

/// One option of a [`verdicts`] track: its words, how it looks chosen, and what taking it sends; none while it is not
/// offered, with why in `why_not`, shown when the pointer rests on it.
pub(crate) struct Verdict<Message> {
    /// Its words, which a screen reader says too.
    pub(crate) label: Label,
    /// Denied, Kept or Allowed: how it looks chosen.
    pub(crate) kind: PillKind,
    /// What taking it sends; none while it is not offered.
    pub(crate) message: Option<Message>,
    /// Why it is not offered.
    pub(crate) why_not: Option<Label>,
}

/// A small track of what to do to something named `name` (deny, leave, allow), the `chosen` one tinted as its kind
/// says; an option not offered is drawn disabled.
pub(crate) fn verdicts<'a, Message: Clone + 'a, R: Renderer + 'a>(
    name: Label,
    options: Vec<Verdict<Message>>,
    chosen: usize,
) -> Element<'a, Message, Theme, R> {
    let pills = options.into_iter().enumerate().map(|(index, verdict)| {
        let sides = Space::S.px() + Space::Xxs.px();
        let chosen = index == chosen;
        let pill =
            pill_offered(verdict.kind, verdict.label, chosen, verdict.message, Length::Shrink, sides, SMALL_TARGET);
        match verdict.why_not {
            Some(why) => crate::tooltip(pill, &why, iced_widget::tooltip::Position::Top),
            None => pill,
        }
    });
    let track = container(Row::with_children(pills).spacing(Space::Xxs.px()))
        .padding(Space::Xs.px())
        .class(Surfaced::Framed(Surface::Raised, Radius::Full));
    Labelled::new(track, Semantics { role: SemanticRole::RadioGroup, label: name })._boxed()
}

/// One of a few chips of which one is chosen, each drawn as `content` (a mark, a name, a note) on a framed pill, the
/// chosen one on the accent's tint; `label` is what a screen reader says of it.
pub(crate) fn chip<'a, Message: Clone + 'a, R: Renderer + 'a>(
    label: Label,
    content: Element<'a, Message, Theme, R>,
    chosen: bool,
    message: Message,
) -> Element<'a, Message, Theme, R> {
    let inner = button::Button::new(content)
        .class(ButtonClass::Pill { kind: PillKind::Page, chosen })
        .padding(Padding::from([Space::Xxs.px(), Space::M.px()]))
        .on_press(message.clone());
    let pressed = actionable(inner, Semantics { role: SemanticRole::Radio { chosen }, label }, Some(message));
    container(pressed).class(Surfaced::Framed(Surface::Raised, Radius::Full))._boxed()
}

fn pills<'a, Message: Clone + 'a, R: Renderer + 'a>(
    options: Vec<(Label, Message)>,
    chosen: usize,
    width: Length,
) -> Element<'a, Message, Theme, R> {
    let pills = options.into_iter().enumerate().map(|(index, (label, message))| {
        pill(PillKind::Choice, label, index == chosen, message, width, Space::M.px() + Space::Xxs.px(), HIT_TARGET)
    });
    container(Row::with_children(pills).spacing(Space::Xxs.px()))
        .padding(Space::Xs.px())
        .width(width)
        .class(Surfaced::Framed(Surface::Raised, Radius::Full))
        ._boxed()
}

/// One pill: its words in the strong weight when chosen, said as an option and whether it is the one taken.
pub(crate) fn pill<'a, Message: Clone + 'a, R: Renderer + 'a>(
    kind: PillKind,
    label: Label,
    chosen: bool,
    message: Message,
    width: Length,
    sides: f32,
    height: f32,
) -> Element<'a, Message, Theme, R> {
    pill_offered(kind, label, chosen, Some(message), width, sides, height)
}

/// [`pill`], disabled while there is no `message`.
fn pill_offered<'a, Message: Clone + 'a, R: Renderer + 'a>(
    kind: PillKind,
    label: Label,
    chosen: bool,
    message: Option<Message>,
    width: Length,
    sides: f32,
    height: f32,
) -> Element<'a, Message, Theme, R> {
    let style = match (kind, chosen) {
        (PillKind::Choice, false) => TypeStyle::Small,
        (PillKind::Choice, true) => TypeStyle::SmallStrong,
        (PillKind::Page, false) => TypeStyle::Body,
        (PillKind::Page, true) => TypeStyle::BodyStrong,
        (PillKind::Denied | PillKind::Kept | PillKind::Allowed, false) => TypeStyle::Caption,
        (PillKind::Denied | PillKind::Kept | PillKind::Allowed, true) => TypeStyle::CaptionStrong,
    };
    let words = text(label.as_str().to_string(), style).wrapping(Wrapping::None);
    let content = match kind {
        PillKind::Choice | PillKind::Denied | PillKind::Kept | PillKind::Allowed => {
            container(words).center_x(Length::Fill)
        }
        PillKind::Page => container(words).width(Length::Fill),
    };
    let inner = button::Button::new(content)
        .class(ButtonClass::Pill { kind, chosen })
        .padding(Padding::from([(height - style.line_height()) / 2.0, sides]))
        .width(width)
        .on_press_maybe(message.clone());
    actionable(inner, Semantics { role: SemanticRole::Radio { chosen }, label }, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Renderer;
    use iced_core::{Point, Rectangle, Size};
    use iced_test::Simulator;

    #[derive(Clone, Debug, PartialEq)]
    enum Said {
        Allow,
    }

    fn label(text: &str) -> Label {
        Label::new(text).expect("a label")
    }

    fn boxes_of(sim: &mut Simulator<'_, Said, Theme, Renderer>) -> Vec<(Semantics, Rectangle)> {
        struct Boxes(Vec<(Semantics, Rectangle)>);
        impl iced_core::widget::Operation for Boxes {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced_core::widget::Operation)) {
                operate(self);
            }
            fn custom(
                &mut self,
                _id: Option<&iced_core::widget::Id>,
                bounds: Rectangle,
                state: &mut dyn std::any::Any,
            ) {
                if let Some(said) = state.downcast_ref::<Semantics>() {
                    self.0.push((said.clone(), bounds));
                }
            }
        }
        let mut boxes = Boxes(Vec::new());
        sim.operate(&mut boxes);
        boxes.0
    }

    #[test]
    fn a_verdict_not_offered_sends_nothing_and_the_chosen_one_is_said_chosen() {
        crate::testing::hermetic_fonts();
        let verdict = |said: &str, kind, message| Verdict { label: label(said), kind, message, why_not: None };
        let track = verdicts::<Said, Renderer>(
            label("See channels"),
            vec![
                Verdict {
                    why_not: Some(label("You would no longer see this channel.")),
                    ..verdict("Deny", PillKind::Denied, None)
                },
                verdict("Inherit", PillKind::Kept, None),
                verdict("Allow", PillKind::Allowed, Some(Said::Allow)),
            ],
            1,
        );
        let mut sim = Simulator::with_size(iced_core::Settings::default(), Size::new(400.0, 120.0), track);
        let boxes = boxes_of(&mut sim);
        let found = |name: &str| boxes.iter().find(|(said, _)| said.label.as_str() == name).cloned().expect(name);
        assert_eq!(found("Inherit").0.role, SemanticRole::Radio { chosen: true });
        assert_eq!(found("Deny").0.role, SemanticRole::Radio { chosen: false });
        for (name, sent) in [("Deny", vec![]), ("Allow", vec![Said::Allow])] {
            let (_, at) = found(name);
            sim.point_at(Point::new(at.center_x(), at.center_y()));
            sim.simulate(crate::testing::click());
            assert_eq!(sim.drain().collect::<Vec<_>>(), sent, "{name}");
        }
    }
}
