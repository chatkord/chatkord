//! What a press on a message's marks opens: a web link in the browser, a Chatkord invite in Add a server's preview, a
//! channel's mark in that channel, a person's mention in their popover, a role's in who holds it.

use super::*;
use crate::roles::ListedRole;
use terms::RoleId;

/// A mark pressed in a message, or a link in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Clicked {
    /// A link, by its text, which is its target.
    Link(String),
    /// A person's mention.
    Mention(MemberId),
    /// A role's mention.
    Role(RoleId),
    /// A channel's mark.
    Channel(ChannelId),
}

/// A role's members, shown from its mention.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleShown {
    /// The server.
    pub place: PlaceId,
    /// The role.
    pub role: ListedRole,
    /// Who holds it, by id and the name others see, by name.
    pub members: Vec<(MemberId, String)>,
}

impl Window {
    /// The role's members put away.
    pub(super) fn on_close_role(&mut self) -> Vec<Ask> {
        self.desk.role_open = None;
        Vec::new()
    }

    /// A role's members, open from its mention.
    pub fn role_shown(&self) -> Option<RoleShown> {
        let (place, role) = self.desk.role_open.as_ref()?;
        let held = self.place(*place)?;
        let listed = held.role_list().role(role)?.clone();
        let mut members: Vec<(MemberId, String)> = held
            .directory()
            .people()
            .filter(|(_, person)| person.roles.contains(role))
            .map(|(id, person)| (id.clone(), person.name.clone()))
            .collect();
        members.sort_by_key(|(_, name)| name.to_lowercase());
        Some(RoleShown { place: *place, role: listed, members })
    }

    /// A press on a mark or a link in a message of `id`'s.
    pub(super) fn clicked(&mut self, id: PlaceId, clicked: Clicked, at: Moment) -> Vec<Ask> {
        self.desk.role_open = None;
        match clicked {
            Clicked::Mention(user) => self.open_person(id, user, PersonFrom::Author),
            Clicked::Role(role) => {
                self.desk.role_open = Some((id, role));
                Vec::new()
            }
            // A mark of a channel this reader cannot see is its text alone: nothing opens.
            Clicked::Channel(channel) if !self.place(id).is_some_and(|place| place.opens_channel(&channel)) => {
                Vec::new()
            }
            Clicked::Channel(channel) => {
                self.update(Message::At(id, place::Message::Discussion(place::DiscussionMessage::Open(channel))), at)
            }
            // A link of a scheme the window does not open is its text alone, as a stored 1.0 invite's is.
            Clicked::Link(link) if !chat::link_opens(&link) => Vec::new(),
            Clicked::Link(link) => self.link(id, &link, at),
        }
    }

    /// A link: an invite to Add a server's preview, any other to the browser.
    fn link(&mut self, id: PlaceId, link: &str, at: Moment) -> Vec<Ask> {
        if terms::InviteLink::parse(link).is_ok() {
            let mut todo = self.add();
            todo.extend(self.adding_door(door::Message::Where(link.to_owned()), at));
            todo.extend(self.adding_door(door::Message::Submit, at));
            return todo;
        }
        vec![Ask::At(id, place::Ask::OpenLink(link.to_owned()))]
    }
}
