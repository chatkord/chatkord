//! What the server and its members are: its greeting, name and picture, a member's name and colour.

use super::*;
use terms::FileError;

/// Which switches a server has on and what it takes in files, as a `ServerChanged` says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offers {
    /// Its switches.
    pub served: Served,
    /// What it takes in files.
    pub limits: terms::FileLimits,
}

/// What the server and its members are: its greeting, name and picture, a member's name and colour.
#[derive(Clone, Debug)]
pub enum ServerMessage {
    /// What the server greeted this session with: its name, colour and picture.
    Greeted {
        /// Its name, colour and picture.
        identity: ServerIdentity,
        /// What it takes in files.
        limits: terms::FileLimits,
    },
    /// The switches the server's greeting said are on: told with the greeting, before the sign-in.
    Serves(Served),
    /// The release the server named in its greeting; none when it named none.
    Built(Option<terms::ServerBuild>),
    /// The protocol version the server spoke in its greeting.
    Spoke(proto::chatkord_v1::ProtoVersion),
    /// The server's name, colour or picture, what it offers or what it takes in files changed, here or on another
    /// device.
    ServerChanged {
        /// Its name, colour and picture.
        identity: ServerIdentity,
        /// Its switches and caps now.
        offers: Offers,
    },
    /// The host has this picture ready to paint.
    PictureReady(FileId),
    /// This device's cache has no copy of this picture.
    PictureMissing(FileId),
    /// The server's copy of this picture could not be had.
    PictureUnfetched(FileId, FileError),
    /// The host holds this picture a message or the emoji show, ready to paint.
    FileReady(FileId),
    /// This picture could not be had this run.
    FileUnfetched(FileId, FileError),
    /// The host let go of this picture, held for this server, to stay within what it holds.
    LetGo(FileId),
    /// A view drew these pictures and the host holds none of them.
    Missing(Vec<FileId>),
    /// Someone chose the name others see, or went back to their username.
    Renamed {
        /// Their id at the server.
        user_id: MemberId,
        /// The name others see now: the chosen one, else the username.
        name: String,
    },
    /// Someone picked the colour others see them in, or went back to none.
    Recoloured {
        /// Their id at the server.
        user_id: MemberId,
        /// The colour picked; none for none.
        colour: Option<proto::rules::Colour>,
    },
    /// A role was made, renamed, recoloured, moved or deleted, or given or taken a word.
    RolesChanged,
    /// The roles in their order, or why there is no list.
    RolesListed(Result<Vec<RoleEntry>, LinkError>),
    /// A channel's overwrites, or why there is no list.
    Overwrites {
        /// The channel.
        channel: ChannelId,
        /// Its overwrites, the roles' in the roles' order first, then the members'.
        listed: Result<Vec<proto::chatkord_v1::ChannelOverwrite>, LinkError>,
    },
    /// The server's answer to a moderation; the window's to read.
    Moderated {
        /// Who it was about.
        user_id: MemberId,
        /// What was asked.
        act: Moderation,
        /// The server's answer.
        answer: Result<(), LinkError>,
    },
}

impl Place {
    /// The release the server named in its greeting this session; none before it greets or when it named none.
    pub fn build(&self) -> Option<&terms::ServerBuild> {
        self.build.as_ref()
    }

    /// The protocol version the server spoke in its greeting this session; none before it greets.
    pub fn protocol(&self) -> Option<proto::chatkord_v1::ProtoVersion> {
        self.protocol
    }

    /// The server's picture, once the host has it ready to paint; until then its initial is shown.
    pub fn picture(&self) -> Option<FileId> {
        self.server.picture.filter(|picture| self.painted == Some(*picture))
    }

    /// What makes the server's picture ready to paint, when it has one the host has not said it has.
    pub(crate) fn to_paint(&self) -> Option<Ask> {
        self.server
            .picture
            .filter(|picture| self.painted != Some(*picture))
            .map(|picture| Ask::LookUpPicture { picture })
    }

    /// The server said what it is now: an open Overview follows what nobody is changing.
    pub(super) fn follow_server(&mut self) {
        let Status::Signed(signed) = &mut self.status else { return };
        if let Some(open) = opened::<Overview>(&mut signed.page) {
            open.follow(&self.server);
        }
    }

    /// The server said what it offers: an open Overview shows the switches as they are, and a page of what it no
    /// longer offers closes.
    fn follow_switches(&mut self) {
        let offered = Page::offered(self.grants());
        let Status::Signed(signed) = &mut self.status else { return };
        if signed.page.as_ref().and_then(OpenPage::settings).is_some_and(|open| !offered.contains(&open.page)) {
            signed.page = None;
        }
        if let Some(open) = opened::<Overview>(&mut signed.page) {
            open.offered(&self.served);
        }
    }

    /// What the server and its members are.
    pub(super) fn on_server(&mut self, message: ServerMessage, at: Moment) -> Vec<Ask> {
        match message {
            ServerMessage::Greeted { identity, limits } => {
                self.limits = limits;
                self.greeted();
                self.server.greeted(identity);
                self.follow_server();
                self.to_paint().into_iter().collect()
            }
            ServerMessage::Serves(served) => {
                self.served = served;
                Vec::new()
            }
            ServerMessage::Built(build) => {
                self.build = build;
                Vec::new()
            }
            ServerMessage::Spoke(version) => {
                self.protocol = Some(version);
                Vec::new()
            }
            ServerMessage::ServerChanged { identity, offers } => {
                self.server.said(identity.clone());
                let Offers { served, limits } = offers;
                let was = std::mem::replace(&mut self.served, served);
                self.limits = limits;
                self.follow_switches();
                let reads = self.reads_newly_offered(&was);
                self.follow_server();
                let remember = Ask::RememberServer { server: self.server.url.clone(), identity };
                self.to_paint().into_iter().chain([remember]).chain(reads).collect()
            }
            ServerMessage::PictureReady(picture) => {
                if self.server.picture == Some(picture) {
                    self.painted = Some(picture);
                }
                self.files.insert(picture, super::emoji::Fetched::Ready);
                Vec::new()
            }
            // Not on this device: the server has it, unless it has changed picture since.
            ServerMessage::PictureMissing(picture) => {
                if self.server.picture == Some(picture) {
                    vec![Ask::FetchPicture { picture, kind: proto::files::FileKind::ServerPicture }]
                } else if self.files.get(&picture) == Some(&super::emoji::Fetched::Asked) {
                    vec![Ask::FetchPicture { picture, kind: proto::files::FileKind::ProfilePicture }]
                } else {
                    Vec::new()
                }
            }
            // The initials stay; the next greeting or change asks again.
            ServerMessage::PictureUnfetched(picture, _) => {
                if self.server.picture != Some(picture) {
                    self.files.insert(picture, super::emoji::Fetched::Failed);
                }
                Vec::new()
            }
            ServerMessage::FileReady(file) => {
                self.files.insert(file, super::emoji::Fetched::Ready);
                Vec::new()
            }
            ServerMessage::FileUnfetched(file, _) => {
                self.files.insert(file, super::emoji::Fetched::Failed);
                Vec::new()
            }
            ServerMessage::LetGo(file) => self.let_go(file),
            ServerMessage::Missing(files) => self.missing(files),
            ServerMessage::Recoloured { user_id, colour } => {
                let Status::Signed(signed) = &mut self.status else { return Vec::new() };
                if let Some(standing) = self.standing.as_mut().filter(|standing| standing.user_id == user_id) {
                    standing.colour = colour;
                    if let Some(open) = signed.page.as_mut().and_then(OpenPage::account_mut) {
                        open.recoloured(colour);
                    }
                }
                recoloured(signed, &user_id, colour, at.now);
                self.follow_person(&user_id, |person| person.colour = colour);
                Vec::new()
            }
            ServerMessage::Renamed { user_id, name } => {
                let Status::Signed(signed) = &mut self.status else { return Vec::new() };
                if let Some(standing) = self.standing.as_mut().filter(|standing| standing.user_id == user_id) {
                    standing.shown = Some(name.clone());
                    if let Some(open) = signed.page.as_mut().and_then(OpenPage::account_mut) {
                        open.renamed(&standing.username, &name);
                    }
                }
                renamed(signed, &user_id, &name, at.now);
                self.follow_person(&user_id, |person| person.name.clone_from(&name));
                Vec::new()
            }
            // The place's roles are read again; the members page lists the members again, whose places may have moved.
            ServerMessage::RolesChanged => {
                let members = self.showing(Page::Members).then_some(Ask::ListMembers);
                [ops::list_roles()].into_iter().chain(members).collect()
            }
            ServerMessage::RolesListed(listed) => {
                self.roles_read(listed);
                Vec::new()
            }
            ServerMessage::Overwrites { channel, listed } => {
                let Status::Signed(signed) = &mut self.status else { return Vec::new() };
                let read = match listed {
                    Ok(entries) => {
                        Loaded::Ready(entries.iter().filter_map(crate::channel_settings::Mark::read).collect())
                    }
                    Err(failed) => Loaded::Failed(failed),
                };
                signed.overwrites.insert(channel, read);
                Vec::new()
            }
            // The window says what came of it.
            ServerMessage::Moderated { .. } => Vec::new(),
        }
    }
}

/// A new colour reaches wherever the person is shown: the call, the channels' lists and the members page.
pub(super) fn recoloured(
    signed: &mut SignedIn,
    user_id: &MemberId,
    colour: Option<proto::rules::Colour>,
    now: Instant,
) {
    if let Some(call) = signed.call.as_mut() {
        call.room.on(RoomEvent::Recoloured { user_id: user_id.clone(), colour }, now);
        call.refresh_tiles(now);
    }
    if let Lobby::Ready(listed) = &mut signed.lobby {
        let people = listed.voice.iter_mut().flat_map(|channel| channel.people.iter_mut());
        for person in people.filter(|person| &person.user_id == user_id) {
            person.colour = colour;
        }
    }
    if let Some(Loaded::Ready(members)) = opened::<Loaded<Vec<ListedMember>>>(&mut signed.page) {
        for member in members.iter_mut().filter(|member| &member.user_id == user_id) {
            member.colour = colour;
        }
    }
}

/// A rename reaches wherever the person is shown: the call, the channels' lists and the members page.
pub(super) fn renamed(signed: &mut SignedIn, user_id: &MemberId, name: &str, now: Instant) {
    if let Some(call) = signed.call.as_mut() {
        call.room.on(RoomEvent::Renamed { user_id: user_id.clone(), name: name.to_string() }, now);
        call.refresh_tiles(now);
    }
    if let Lobby::Ready(listed) = &mut signed.lobby {
        let people = listed.voice.iter_mut().flat_map(|channel| channel.people.iter_mut());
        for person in people.filter(|person| &person.user_id == user_id) {
            person.name = name.to_string();
        }
    }
    if let Some(Loaded::Ready(members)) = opened::<Loaded<Vec<ListedMember>>>(&mut signed.page) {
        for member in members.iter_mut().filter(|member| &member.user_id == user_id) {
            member.name = name.to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::door::{Opening, Server};
    use crate::harness::{ADA, Clock, id, owls, tell};
    use crate::place;
    use crate::window::Window;
    use std::time::Instant;
    use terms::LinkError;

    #[test]
    fn a_picture_not_on_this_device_is_fetched_and_one_that_cannot_be_had_leaves_the_initials() {
        const OWL: proto::files::FileId = proto::files::FileId::from_bytes([7; proto::files::FileId::LEN]);
        let clock = Clock(Instant::now());
        let pictured = Server { picture: Some(OWL), ..owls() };
        let (mut window, todo) = Window::launch(vec![Opening::Account(pictured, ADA.into())], None, clock.0);
        let owls_id = id(&window, 0);
        assert!(
            todo.iter().any(
                |done| matches!(done, crate::Ask::At(at, place::Ask::LookUpPicture { picture }) if *at == owls_id && *picture == OWL)
            ),
            "the cache first: {todo:?}"
        );
        let server = |message| place::Message::Server(message);
        let todo = tell(&mut window, &clock, owls_id, server(place::ServerMessage::PictureMissing(OWL)));
        assert!(
            matches!(
                &todo[..],
                [crate::Ask::At(_, place::Ask::FetchPicture { picture, kind: proto::files::FileKind::ServerPicture })]
                    if *picture == OWL
            ),
            "{todo:?}"
        );
        let unfetched = terms::FileError::Reach(LinkError::Malformed);
        let todo = tell(&mut window, &clock, owls_id, server(place::ServerMessage::PictureUnfetched(OWL, unfetched)));
        assert!(
            todo.is_empty() && window.toast().map(|toast| toast.said().in_english()).as_deref().is_none(),
            "no notice: {todo:?}"
        );
        assert_eq!(window.open().unwrap().picture(), None, "the initials stay");
        tell(&mut window, &clock, owls_id, server(place::ServerMessage::PictureReady(OWL)));
        assert_eq!(window.open().unwrap().picture(), Some(OWL));
    }

    #[test]
    fn roles_changed_reads_the_roles_again_and_the_members_where_they_show() {
        use proto::chatkord_v1::{Event, RolesChanged, event};
        use proto::rules::Grant;
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = crate::harness::holding(&clock, [Grant::ManageMembers].into_iter().collect());
        let roles = place::Message::heard(&Event {
            cursor: Vec::new(),
            kind: Some(event::Kind::RolesChanged(RolesChanged {})),
        })
        .expect("read");
        let todo = tell(&mut window, &clock, owls_id, roles.clone());
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, ask1)] if crate::harness::is(ask1, crate::harness::Op::ListRoles)),
            "the place's roles: {todo:?}"
        );
        let members = place::Message::Page(place::PageMessage::Settings(crate::Page::Members));
        let todo = tell(&mut window, &clock, owls_id, members);
        assert!(matches!(&todo[..], [crate::Ask::At(_, place::Ask::ListMembers)]), "{todo:?}");
        let todo = tell(&mut window, &clock, owls_id, roles);
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, ask1), crate::Ask::At(_, place::Ask::ListMembers)] if crate::harness::is(ask1, crate::harness::Op::ListRoles)),
            "the roles, and the members whose places may have moved: {todo:?}"
        );
    }

    #[test]
    fn a_server_changed_says_which_switches_are_off_and_the_caps_now() {
        use proto::chatkord_v1::{Event, ServerChanged, Switch as WireSwitch, event};
        use proto::rules::Switch;
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = crate::harness::holding(&clock, proto::rules::Grants::default());
        crate::harness::every_switch_on(&mut window, &clock, owls_id);
        let now = ServerChanged {
            switched_off: vec![WireSwitch::Search as i32, WireSwitch::WelcomeNote as i32],
            attachment_max_bytes: 7,
            ..ServerChanged::default()
        };
        let heard = place::Message::heard(&Event { cursor: Vec::new(), kind: Some(event::Kind::ServerChanged(now)) });
        tell(&mut window, &clock, owls_id, heard.expect("a change"));
        let served = window.place(owls_id).unwrap().served();
        assert_eq!(served, &place::Served::read(&[Switch::Search, Switch::Welcome]));
        assert!(served.on(Switch::Threads) && !served.on(Switch::Search));
        assert_eq!(window.place(owls_id).unwrap().file_limits().attachment, Some(7));
    }

    #[test]
    fn a_list_switched_on_while_signed_in_is_read_once() {
        use proto::chatkord_v1::{Event, ServerChanged, Switch as WireSwitch, event};
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = crate::harness::holding(&clock, proto::rules::Grants::default());
        crate::harness::every_switch_on(&mut window, &clock, owls_id);
        let heard = |switched_off: Vec<i32>| {
            let changed = ServerChanged { switched_off, ..ServerChanged::default() };
            place::Message::heard(&Event { cursor: Vec::new(), kind: Some(event::Kind::ServerChanged(changed)) })
                .expect("a change")
        };
        let followed = |todo: &[crate::Ask]| {
            todo.iter().filter(|ask| matches!(ask, crate::Ask::At(_, ask1) if crate::harness::is(ask1, crate::harness::Op::ListFollowed))).count()
        };
        let off = vec![WireSwitch::Threads as i32];
        assert_eq!(followed(&tell(&mut window, &clock, owls_id, heard(off))), 0, "threads switched off");
        assert_eq!(followed(&tell(&mut window, &clock, owls_id, heard(Vec::new()))), 1, "threads switched on");
        assert_eq!(followed(&tell(&mut window, &clock, owls_id, heard(Vec::new()))), 0, "and only as they come on");
    }

    #[test]
    fn a_picture_the_server_changed_since_it_was_looked_up_is_not_fetched() {
        const OWL: proto::files::FileId = proto::files::FileId::from_bytes([7; proto::files::FileId::LEN]);
        let clock = Clock(Instant::now());
        let (mut window, _) = Window::launch(vec![Opening::Account(owls(), ADA.into())], None, clock.0);
        let owls_id = id(&window, 0);
        let missing = place::Message::Server(place::ServerMessage::PictureMissing(OWL));
        assert!(tell(&mut window, &clock, owls_id, missing).is_empty());
    }
}
