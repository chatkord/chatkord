//! Who sees a channel and who writes in it, as the create dialog and the channel's settings both show it: one card of
//! switches, Private channel and, for a text channel, Read-only, each with what it does; while one is on, who it lets
//! in, each with an × where they may be taken off, and the add menu under them.

use crate::add_menu::{AddSaid, Hang, add_menu};
use crate::labels::{label, phrase};
use crate::whom::Naming;
use crate::{Button, Icon, Renderer, Surfaced, TextClass, Theme, coloured};
use design::{ButtonVariant, Content, HIT_TARGET, Radius, Space, Surface, TypeStyle};
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Column, Row, container};
use screens::channel_settings::{AccessView, Listed, Removal, Switch};
use screens::{Access, Whom};
use words::{Phrase, Sentence, Words};

/// What the card sends, of one of its accesses.
#[derive(Clone, Debug)]
pub(crate) enum AccessSaid {
    /// Its switch taken.
    Switch(Access),
    /// Its add menu opened, or closed while it is open.
    Adding(Access),
    /// The add menu's search, as typed.
    Query(String),
    /// A role or member let in.
    Allow(Access, Whom),
    /// A role or member taken off.
    Remove(Access, Whom),
}

/// The card of `views`, each a switch set off from the one above by a hairline; `query` is the add menu's search, and
/// `on` makes what each part does a message.
pub(crate) fn access_card<'a, Message: Clone + 'a, R: Renderer + 'a>(
    views: Vec<AccessView>,
    query: &'a str,
    naming: &Naming<'_>,
    on: impl Fn(AccessSaid) -> Message + Copy + 'a,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let mut parts: Vec<Element<'a, Message, Theme, R>> = Vec::new();
    for view in views {
        if !parts.is_empty() {
            parts.push(crate::panel_parts::hairline());
        }
        parts.push(access(view, query, naming, on, words));
    }
    container(Column::with_children(parts))
        .width(Length::Fill)
        .class(Surfaced::Framed(Surface::Raised, Radius::Xl))
        ._boxed()
}

/// One access: its switch, and while it is on, who it lets in and the add menu.
fn access<'a, Message: Clone + 'a, R: Renderer + 'a>(
    view: AccessView,
    query: &'a str,
    naming: &Naming<'_>,
    on: impl Fn(AccessSaid) -> Message + Copy + 'a,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let AccessView { access, on: switched, listed, adding } = view;
    let (name, line, caption) = match access {
        Access::Private => (Phrase::PrivateChannel, Phrase::PrivateChannelLine, Phrase::WhoCanSee),
        Access::ReadOnly => (Phrase::ReadOnly, Phrase::ReadOnlyLine, Phrase::WhoCanWrite),
    };
    let side = Space::M.px() + Space::Xxs.px();
    let switch = crate::switch(
        &phrase(name, words),
        Some(&words.phrase(line)),
        switched == Switch::On,
        on(AccessSaid::Switch(access)),
    );
    let switch = container(switch).padding(Padding::from([Space::M.px(), side]));
    if switched == Switch::Off {
        return switch._boxed();
    }
    let mut under: Vec<Element<'a, Message, Theme, R>> = vec![
        coloured(words.phrase(caption).to_string(), TypeStyle::CaptionStrong, TextClass::Content(Content::Secondary))
            ._boxed(),
    ];
    if !listed.is_empty() {
        let mut rows: Vec<Element<'a, Message, Theme, R>> = Vec::new();
        for row in listed {
            if !rows.is_empty() {
                rows.push(crate::panel_parts::hairline());
            }
            rows.push(listed_row(access, row, naming, on, words));
        }
        let rows = container(Column::with_children(rows))
            .width(Length::Fill)
            .class(Surfaced::Framed(Surface::Base, Radius::M));
        under.push(rows._boxed());
    }
    under.push(add_menu(
        phrase(Phrase::AddRoleOrMember, words),
        adding,
        query,
        Hang::Start,
        naming,
        move |said| match said {
            AddSaid::Toggle => on(AccessSaid::Adding(access)),
            AddSaid::Query(typed) => on(AccessSaid::Query(typed)),
            AddSaid::Pick(whom) => on(AccessSaid::Allow(access, whom)),
        },
        words,
    ));
    let under = Column::with_children(under).spacing(Space::S.px()).padding(Padding {
        top: 0.0,
        right: side,
        bottom: side,
        left: side,
    });
    Column::with_children([switch._boxed(), under._boxed()])._boxed()
}

/// A role or member let in: its mark, its name, and its × where it may be taken off, or why it may not.
fn listed_row<'a, Message: Clone + 'a, R: Renderer + 'a>(
    access: Access,
    row: Listed,
    naming: &Naming<'_>,
    on: impl Fn(AccessSaid) -> Message + Copy + 'a,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let name = naming.name(&row.name);
    let remove = |button: Button<'a, Message>| button.icon_only(Icon::Close).variant(ButtonVariant::Ghost).compact();
    let said = label(&words.say(&Sentence::RemoveTarget(name.as_str().to_string())));
    let mut cells: Vec<Element<'a, Message, Theme, R>> = vec![
        naming.mark(row.look, &row.name),
        crate::line(name.as_str().to_string(), TypeStyle::Label).width(Length::Fill)._boxed(),
    ];
    match row.removal {
        Removal::Offered => {
            cells.push(remove(Button::new(said)).on_press(on(AccessSaid::Remove(access, row.whom))).view::<R>());
        }
        Removal::LocksOut => {
            let why = phrase(Phrase::WouldNotSee, words);
            cells.push(crate::tooltip(
                remove(Button::new(said)).view::<R>(),
                &why,
                iced_widget::tooltip::Position::Top,
            ));
        }
        Removal::Fixed => {}
    }
    // Each row a target's height and a little, whether or not it holds an ×.
    Row::with_children(cells)
        .spacing(Space::S.px() + Space::Xxs.px())
        .align_y(Alignment::Center)
        .height(Length::Fixed(HIT_TARGET + Space::Xs.px() + Space::Xxs.px()))
        .padding(Padding { top: 0.0, bottom: 0.0, left: Space::M.px(), right: Space::Xs.px() + Space::Xxs.px() })
        ._boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Renderer;
    use design::{SemanticRole, Semantics};
    use iced_core::{Point, Rectangle, Size};
    use iced_test::Simulator;
    use screens::channel_settings::{Listed, Look, Shown};
    use terms::{MemberId, RoleId};

    const MODS: &str = "mods";
    const ADA: &str = "ada";
    const GRACE: &str = "grace";

    #[derive(Clone, Debug)]
    struct Said(AccessSaid);

    fn member(id: &str) -> Whom {
        Whom::Member(MemberId::parse(id).expect("a member's id"))
    }

    /// Private, on, listing Ada ("You", who would no longer see it without her row), Moderators and grace.
    fn private() -> AccessView {
        let person = |id: &str| Look::Person(screens::colour_of(&MemberId::parse(id).expect("an id"), None), None);
        AccessView {
            access: Access::Private,
            on: Switch::On,
            listed: vec![
                Listed { whom: member(ADA), name: Shown::You, look: person(ADA), removal: Removal::LocksOut },
                Listed {
                    whom: Whom::Role(RoleId::parse(MODS).expect("a role's id")),
                    name: Shown::Name("Moderators".into()),
                    look: Look::Role(None),
                    removal: Removal::Offered,
                },
                Listed {
                    whom: member(GRACE),
                    name: Shown::Name("grace".into()),
                    look: person(GRACE),
                    removal: Removal::Offered,
                },
            ],
            adding: None,
        }
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
    fn a_listed_row_is_taken_off_by_its_x_but_never_one_that_would_lock_this_account_out() {
        crate::testing::hermetic_fonts();
        let words: &'static Words = Box::leak(Box::new(Words::new(words::Language::ENGLISH)));
        let place_words = Naming::detached(Some("Ada".into()), words);
        let card = access_card::<Said, Renderer>(vec![private()], "", &place_words, Said, words);
        let mut sim = Simulator::with_size(iced_core::Settings::default(), Size::new(560.0, 480.0), card);
        let boxes = boxes_of(&mut sim);
        let found = |name: &str| boxes.iter().find(|(said, _)| said.label.as_str() == name).cloned();
        assert!(found("Remove You").is_some_and(|(said, _)| said.role == SemanticRole::Button), "drawn, with why not");
        let (_, at) = found("Remove You").expect("the row's x");
        sim.point_at(Point::new(at.center_x(), at.center_y()));
        sim.simulate(crate::testing::click());
        assert!(sim.drain().next().is_none(), "Ada's row stays");
        let (_, at) = found("Remove grace").expect("grace's x");
        sim.point_at(Point::new(at.center_x(), at.center_y()));
        sim.simulate(crate::testing::click());
        let said: Vec<Said> = sim.drain().collect();
        assert!(
            matches!(&said[..], [Said(AccessSaid::Remove(Access::Private, whom))] if *whom == member(GRACE)),
            "{said:?}"
        );
        let (_, at) = found("Add a role or member").expect("the add button");
        sim.point_at(Point::new(at.center_x(), at.center_y()));
        sim.simulate(crate::testing::click());
        let said: Vec<Said> = sim.drain().collect();
        assert!(matches!(&said[..], [Said(AccessSaid::Adding(Access::Private))]), "{said:?}");
    }
}
