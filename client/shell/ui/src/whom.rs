//! A role or a member as a channel's lists show them: a role by its colour's dot, a member by their picture in their
//! colour, and this account as "You", its picture still its own.

use crate::labels::{label, phrase};
use crate::{AvatarKind, Held, Renderer, Theme, avatar_pictured};
use design::{AvatarSize, Label};
use iced_core::{Element, Length, Widget};
use iced_widget::container;
use proto::files::FileId;
use screens::channel_settings::{Look, Shown};
use words::{Phrase, Words};

/// How a channel's lists name the roles and members in them, for this account.
pub(crate) struct Naming<'w> {
    words: &'w Words,
    /// This account's own name, whose initials its picture shows beside "You".
    own: Label,
    /// A person's picture the host holds ready, by its file.
    pictures: &'w dyn Fn(FileId) -> Option<Held>,
}

impl<'w> Naming<'w> {
    /// The names for this account at `place`, its picture's initials from the name others see it by, "You" while
    /// that is not known.
    pub(crate) fn of(place: &screens::Place, words: &'w Words, pictures: &'w dyn Fn(FileId) -> Option<Held>) -> Self {
        let own = place.me().and_then(|me| place.name_of(me));
        let own = own.map_or_else(|| phrase(Phrase::YouListed, words), |own| label(&own));
        Naming { words, own, pictures }
    }

    /// The names for an account others see by `own`, away from any place.
    #[cfg(test)]
    pub(crate) fn detached(own: Option<String>, words: &'w Words) -> Self {
        let own = own.map_or_else(|| phrase(Phrase::YouListed, words), |own| label(&own));
        Naming { words, own, pictures: &|_| None }
    }

    /// What a row says: "You", or the name.
    pub(crate) fn name(&self, shown: &Shown) -> Label {
        match shown {
            Shown::You => phrase(Phrase::YouListed, self.words),
            Shown::Name(name) => label(name),
        }
    }

    /// Who a sentence names: this account by the name others see it by, anyone else by their name.
    pub(crate) fn called(&self, shown: &Shown) -> Label {
        match shown {
            Shown::You => self.own.clone(),
            Shown::Name(name) => label(name),
        }
    }

    /// The mark before a name: a role's dot in a picture's room, or a member's picture.
    pub(crate) fn mark<'a, Message: 'a, R: Renderer + 'a>(
        &self,
        look: Look,
        shown: &Shown,
    ) -> Element<'a, Message, Theme, R> {
        let size = AvatarSize::Xs;
        match look {
            Look::Role(colour) => container(crate::list_row::colour_dot(colour.map(screens::colour::rgb)))
                .center(Length::Fixed(size.px()))
                ._boxed(),
            Look::Person(colour, picture) => {
                avatar_pictured(self.called(shown), AvatarKind::Called(colour), size, picture.and_then(self.pictures))
            }
        }
    }
}
