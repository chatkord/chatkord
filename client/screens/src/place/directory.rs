//! Every member as every member sees them (`people`): read page by page after each sign-in and kept current by what
//! the server tells, so a message's author and the members list are named by one owner.

use super::*;
use proto::status::Shown;
use std::collections::HashMap;

/// A member as the directory names them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Named {
    /// The name others see.
    pub name: String,
    /// The colour they picked, if any.
    pub colour: Option<proto::rules::Colour>,
    /// Every role they hold, the default one included, by id, in no promised order.
    pub roles: Vec<RoleId>,
    /// The picture they chose; none for initials.
    pub picture: Option<proto::files::FileId>,
}

/// How a member is shown as the wire states it: an unspecified status, or one this build does not know, is a server not
/// keeping the protocol, and reads as offline.
pub(crate) fn shown_of(status: i32) -> Shown {
    proto::chatkord_v1::Status::try_from(status).ok().and_then(Shown::from_wire).unwrap_or(Shown::Offline)
}

/// One page of the members, read at the edge.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PeoplePage {
    /// Each member by their id, and how they were shown at the page's snapshot; one out of shape is passed over.
    pub entries: Vec<(MemberId, Named, Shown)>,
    /// Where the next page starts; none on the last.
    pub next: Option<crate::Cursor>,
    /// The generation of the presence snapshot the page was read at.
    pub presence: u64,
}

impl From<&proto::chatkord_v1::People> for PeoplePage {
    fn from(people: &proto::chatkord_v1::People) -> PeoplePage {
        PeoplePage {
            entries: people.entries.iter().filter_map(person_of).collect(),
            next: people.next.clone().map(crate::Cursor::from),
            presence: people.presence,
        }
    }
}

/// A person as the wire states them: an empty id is a server not keeping the protocol, and is passed over, as is a
/// role named by an empty id.
pub fn person_of(person: &proto::chatkord_v1::Person) -> Option<(MemberId, Named, Shown)> {
    let user_id = MemberId::parse(&person.user_id)?;
    let colour = wire_colour(person.colour.as_deref());
    let roles = person.role_ids.iter().filter_map(|id| RoleId::parse(id)).collect();
    let shown = shown_of(person.status);
    // A picture out of shape is a server not keeping the protocol: the person keeps their initials.
    let picture = person.picture.as_deref().and_then(|id| proto::files::FileId::parse(id).ok());
    Some((user_id, Named { name: person.display_name.clone(), colour, roles, picture }, shown))
}

/// How a member is shown, and the presence generation that was told at: a later telling of an earlier generation is
/// older news, and is passed over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Told {
    state: Shown,
    at: u64,
}

/// How a telling of presence weighs against what is known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Newer {
    /// A page read at a generation: as new as a change told of that generation.
    OrSame,
    /// A change told of a generation: news only past what is known.
    Strictly,
}

/// The members, as last read and since told.
#[derive(Clone, Debug, Default)]
pub struct Directory {
    known: HashMap<MemberId, Named>,
    /// A listing under way: filled page by page, it replaces `known` whole once its last page comes, so a member who
    /// left meanwhile is not kept.
    reading: Option<crate::Paged<HashMap<MemberId, Named>>>,
    /// How each member is shown, apart from the listing: a change told while a page of an older snapshot is still to
    /// come must outlive that page.
    presence: HashMap<MemberId, Told>,
    /// Every member known, by name as the members column orders them: put in order again whenever `known` changes,
    /// so a view of the column sorts nothing.
    by_name: Vec<MemberId>,
    /// Those of `by_name` shown online, in its order: grouped again whenever it or a presence changes.
    online: Vec<MemberId>,
    /// Those of `by_name` shown offline, in its order.
    offline: Vec<MemberId>,
}

impl Directory {
    /// The member `user_id`, as last read or told.
    pub fn person(&self, user_id: &MemberId) -> Option<&Named> {
        self.known.get(user_id)
    }

    /// How `user_id` is shown, as last told; offline until told.
    pub fn presence(&self, user_id: &MemberId) -> Shown {
        self.presence.get(user_id).map_or(Shown::Offline, |told| told.state)
    }

    /// Every member known, in no order.
    pub fn people(&self) -> impl Iterator<Item = (&MemberId, &Named)> {
        self.known.iter()
    }

    /// How many members it knows.
    pub fn len(&self) -> usize {
        self.known.len()
    }

    /// Whether it knows none.
    pub fn is_empty(&self) -> bool {
        self.known.is_empty()
    }

    /// Every member known, shown online then offline, each by name.
    pub(super) fn groups(&self) -> (&[MemberId], &[MemberId]) {
        (&self.online, &self.offline)
    }

    /// Begins a listing; answers what to ask first. A server that started again counts its generations from the
    /// start, so what is known stays only until the listing's pages say otherwise.
    fn begin(&mut self) -> Ask {
        self.reading = Some(crate::Paged::new());
        for told in self.presence.values_mut() {
            told.at = 0;
        }
        ops::list_people(None)
    }

    /// A page of the listing under way; answers the next page to ask, if any.
    fn page(&mut self, page: PeoplePage) -> Option<Ask> {
        let reading = self.reading.as_mut()?;
        let mut named = Vec::with_capacity(page.entries.len());
        for (user_id, person, state) in page.entries {
            named.push((user_id.clone(), person));
            tell(&mut self.presence, user_id, state, page.presence, Newer::OrSame);
        }
        reading.extend(named, page.next);
        let next = match reading.next().cloned() {
            Some(after) => Some(ops::list_people(Some(after))),
            None => {
                self.known = self.reading.take().map(crate::Paged::into_items).unwrap_or_default();
                self.presence.retain(|user_id, _| self.known.contains_key(user_id));
                self.order();
                None
            }
        };
        self.group();
        next
    }

    /// How each member whose status changed is shown, told of `generation`: each applied only where it is newer than
    /// what is known; a member named twice is shown as last named.
    fn told(&mut self, generation: u64, shown: Vec<(MemberId, Shown)>) {
        let states: HashMap<MemberId, Shown> = shown.into_iter().collect();
        for (user_id, state) in states {
            tell(&mut self.presence, user_id, state, generation, Newer::Strictly);
        }
        self.group();
    }

    /// Applies `change` to what is known and to a listing under way, which may have read past the member already.
    fn each(&mut self, change: impl Fn(&mut HashMap<MemberId, Named>)) {
        change(&mut self.known);
        if let Some(reading) = self.reading.as_mut() {
            change(reading.items_mut());
        }
        self.order();
    }

    /// Puts every member known in the members column's order: by name, regardless of case, then by id.
    fn order(&mut self) {
        let known = &self.known;
        let mut by_name: Vec<MemberId> = known.keys().cloned().collect();
        by_name.sort_by_cached_key(|user_id| {
            (known.get(user_id).map(|named| named.name.to_lowercase()).unwrap_or_default(), user_id.clone())
        });
        self.by_name = by_name;
        self.group();
    }

    /// Splits the members by name into those shown online and those shown offline.
    fn group(&mut self) {
        let (online, offline): (Vec<MemberId>, Vec<MemberId>) =
            self.by_name.iter().cloned().partition(|user_id| self.presence(user_id).online());
        (self.online, self.offline) = (online, offline);
    }
}

/// `user_id`'s presence told as `state` at the generation `at`, kept where it is news by `newer`.
fn tell(presence: &mut HashMap<MemberId, Told>, user_id: MemberId, state: Shown, at: u64, newer: Newer) {
    let told = Told { state, at };
    match presence.entry(user_id) {
        std::collections::hash_map::Entry::Vacant(vacant) => {
            vacant.insert(told);
        }
        std::collections::hash_map::Entry::Occupied(mut known) => {
            let news = match newer {
                Newer::OrSame => at >= known.get().at,
                Newer::Strictly => at > known.get().at,
            };
            if news {
                known.insert(told);
            }
        }
    }
}

impl Place {
    /// The members as this account sees them.
    pub fn directory(&self) -> &Directory {
        &self.directory
    }

    /// The picture `user_id` chose, which a view asks the host for and paints once it holds it, their initials until
    /// then; none for one who chose none.
    pub fn picture_of(&self, user_id: &MemberId) -> Option<proto::files::FileId> {
        self.directory.person(user_id)?.picture
    }

    /// The name others see `user_id` by: as the members list says, else as the voice room they are in says.
    pub fn name_of(&self, user_id: &MemberId) -> Option<String> {
        let seated = || match self.lobby()? {
            Lobby::Ready(listed) => listed
                .voice
                .iter()
                .flat_map(|channel| channel.people.iter())
                .find(|one| one.user_id == *user_id)
                .map(|one| one.name.clone()),
            Lobby::Loading | Lobby::Failed(_) => None,
        };
        self.directory.person(user_id).map(|person| person.name.clone()).or_else(seated)
    }

    /// The members are read again from the start, where the server keeps them; the list known so far stays meanwhile.
    pub(super) fn list_people(&mut self) -> Ask {
        self.directory.begin()
    }

    /// The members, as the server lists them and tells of them.
    pub(super) fn on_people(&mut self, message: PeopleMessage) -> Vec<Ask> {
        match message {
            PeopleMessage::Page(Ok(page)) => self.directory.page(page).into_iter().collect(),
            // The list known stays; the next sign-in reads it again.
            PeopleMessage::Page(Err(_)) => {
                self.directory.reading = None;
                Vec::new()
            }
            PeopleMessage::Arrived { user_id, person } => {
                self.directory.each(|known| {
                    known.insert(user_id.clone(), person.clone());
                });
                // Told arriving, never signed in yet: whatever presence was told stands.
                tell(&mut self.directory.presence, user_id, Shown::Offline, 0, Newer::Strictly);
                self.directory.group();
                Vec::new()
            }
            PeopleMessage::Pictured { user_id, picture } => {
                self.follow_person(&user_id, |person| person.picture = picture);
                if self.me() == Some(&user_id)
                    && let Status::Signed(signed) = &mut self.status
                    && let Some(open) = signed.page.as_mut().and_then(OpenPage::account_mut)
                {
                    open.pictured(picture);
                }
                Vec::new()
            }
            PeopleMessage::Left(user_id) => {
                self.talk_member_left(&user_id);
                self.directory.each(|known| {
                    known.remove(&user_id);
                });
                self.directory.presence.remove(&user_id);
                Vec::new()
            }
            PeopleMessage::Presence { generation, shown } => {
                self.directory.told(generation, shown);
                Vec::new()
            }
            PeopleMessage::Roles { user_id, roles } => {
                self.follow_person(&user_id, |person| person.roles.clone_from(&roles));
                self.held_roles(&user_id, &roles);
                Vec::new()
            }
        }
    }

    /// A member's new name, colour or roles, kept where the directory has them.
    pub(super) fn follow_person(&mut self, user_id: &MemberId, change: impl Fn(&mut Named)) {
        self.directory.each(|known| {
            if let Some(person) = known.get_mut(user_id) {
                change(person);
            }
        });
    }
}

/// What the server says of its members.
#[derive(Clone, Debug)]
pub enum PeopleMessage {
    /// A page of the listing, or why it could not be had.
    Page(Result<PeoplePage, LinkError>),
    /// Someone became a member.
    Arrived {
        /// Their id at the server.
        user_id: MemberId,
        /// How they are shown.
        person: Named,
    },
    /// Someone is no longer a member, by their id.
    Left(MemberId),
    /// Someone chose the picture others see them by, or went back to their initials.
    Pictured {
        /// Their id at the server.
        user_id: MemberId,
        /// The picture; none for none.
        picture: Option<proto::files::FileId>,
    },
    /// Whose status changed, signing in on a first device and out of a last one included, as of a presence generation.
    Presence {
        /// The snapshot's generation.
        generation: u64,
        /// Each member whose status changed, and to what.
        shown: Vec<(MemberId, Shown)>,
    },
    /// The roles a member holds changed.
    Roles {
        /// Their id at the server.
        user_id: MemberId,
        /// Every role they hold now, the default one included.
        roles: Vec<RoleId>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{Clock, accepted, chatting, flow, member, tell};
    use crate::harness::{sends, wire};
    use crate::place;
    use crate::window::Window;
    use proto::rules::Grant;
    use std::time::Instant;
    use terms::Ended;

    const ADA: &str = "ada";
    const BO: &str = "bo";

    fn person(name: &str) -> Named {
        Named { name: name.into(), colour: None, roles: Vec::new(), picture: None }
    }

    fn page(entries: Vec<(&str, Shown)>, presence: u64) -> PeoplePage {
        let entries = entries.into_iter().map(|(id, state)| (member(id), person(id), state)).collect();
        PeoplePage { entries, next: None, presence }
    }

    #[test]
    fn presence_told_before_an_older_page_is_kept_and_a_page_as_new_as_a_change_stands() {
        let mut directory = Directory::default();
        directory.begin();
        directory.told(5, vec![(member(BO), Shown::Online)]);
        assert!(directory.page(page(vec![(BO, Shown::Offline), (ADA, Shown::Online)], 4)).is_none());
        assert_eq!(directory.presence(&member(BO)), Shown::Online, "told at 5, the page read at 4 is older");
        assert_eq!(directory.presence(&member(ADA)), Shown::Online);
        directory.told(4, vec![(member(ADA), Shown::Offline)]);
        assert_eq!(directory.presence(&member(ADA)), Shown::Online, "a change of the page's own age is not news");
        directory.told(6, vec![(member(ADA), Shown::Offline)]);
        assert_eq!(directory.presence(&member(ADA)), Shown::Offline);
        directory.begin();
        assert!(directory.page(page(vec![(BO, Shown::Offline)], 1)).is_none());
        assert_eq!(
            directory.presence(&member(BO)),
            Shown::Offline,
            "a server started again counts from 1: a listing begun anew takes its pages"
        );
        assert_eq!(directory.presence(&member(ADA)), Shown::Offline, "a member no longer listed is no one");
    }

    #[test]
    fn a_listing_replaces_the_list_whole_and_keeps_what_was_told_while_it_ran() {
        let mut directory = Directory::default();
        directory.known.insert(member(BO), person("Bo, gone since"));
        directory.begin();
        let first = PeoplePage {
            entries: vec![(member(ADA), person("Ada"), Shown::Online)],
            next: Some(ADA.into()),
            presence: 1,
        };
        let next = directory.page(first);
        assert!(matches!(&next, Some(ask) if sends(ask, wire::ListPeople { after: Some(ADA.into()) })), "{next:?}");
        assert_eq!(
            directory.person(&member(BO)).map(|bo| bo.name.as_str()),
            Some("Bo, gone since"),
            "kept until the end"
        );
        directory.each(|known| {
            if let Some(ada) = known.get_mut(&member(ADA)) {
                ada.name = "Ada Lovelace".into();
            }
        });
        assert!(directory.page(PeoplePage::default()).is_none());
        assert_eq!(directory.person(&member(BO)), None, "a member who left before the listing is gone after it");
        assert_eq!(
            directory.person(&member(ADA)).map(|ada| ada.name.as_str()),
            Some("Ada Lovelace"),
            "a rename mid-listing stays"
        );
    }

    #[test]
    fn the_roster_lists_who_is_online_then_offline_by_name_and_someone_in_a_call_keeps_their_status() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = crate::harness::holding(&clock, proto::rules::Grants::default());
        let entries = vec![
            (member("mira"), person("mira"), Shown::Offline),
            (member(BO), person("Bo"), Shown::Offline),
            (member("grace"), person("grace"), Shown::Online),
            (member(ADA), person("Ada"), Shown::Online),
        ];
        let page = PeoplePage { entries, next: None, presence: 2 };
        tell(&mut window, &clock, owls, place::Message::People(PeopleMessage::Page(Ok(page))));
        let seat = place::Occupant {
            user_id: member("mira"),
            name: "mira".into(),
            colour: None,
            state: choices::State::default(),
            sharing: false,
            camera: false,
            held: false,
            place: proto::rules::Place::BOTTOM,
        };
        let channel_id = crate::harness::channel_id("lounge");
        tell(
            &mut window,
            &clock,
            owls,
            place::Message::Lobby(place::LobbyMessage::Occupancy { channel_id, people: vec![seat] }),
        );
        let roster = window.open().unwrap().roster();
        let names = |group: RosterGroup| {
            let lines = roster.lines.iter().skip_while(|line| **line != RosterLine::Heading(group, 2)).skip(1);
            lines
                .map_while(|line| match line {
                    RosterLine::Member(row) => Some(row.name.to_owned()),
                    RosterLine::Heading(..) => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(names(RosterGroup::Online), ["Ada", "grace"]);
        assert_eq!(names(RosterGroup::Offline), ["Bo", "mira"], "an invisible member in a call stays offline here");
    }

    #[test]
    fn a_status_told_sets_a_members_whole_and_an_older_generation_changes_nothing() {
        let mut directory = Directory::default();
        directory.begin();
        assert!(directory.page(page(vec![(BO, Shown::Offline), (ADA, Shown::Online)], 1)).is_none());
        directory.told(2, vec![(member(ADA), Shown::Away)]);
        assert_eq!(directory.presence(&member(ADA)), Shown::Away);
        directory.told(3, vec![(member(BO), Shown::DoNotDisturb)]);
        assert_eq!(directory.presence(&member(BO)), Shown::DoNotDisturb, "signed in, as the status says");
        directory.told(4, vec![(member(ADA), Shown::Offline)]);
        assert_eq!(directory.presence(&member(ADA)), Shown::Offline, "signed out of a last device");
        directory.told(3, vec![(member(ADA), Shown::Online)]);
        assert_eq!(directory.presence(&member(ADA)), Shown::Offline, "an older generation is not newer");
    }

    #[test]
    fn a_person_is_read_by_their_status_and_one_not_said_is_offline() {
        use proto::chatkord_v1::Status;
        let wire = |status: Status| proto::chatkord_v1::Person {
            user_id: ADA.into(),
            display_name: "Ada".into(),
            status: status as i32,
            ..Default::default()
        };
        let shown = |person| person_of(&person).map(|(_, _, shown)| shown);
        assert_eq!(shown(wire(Status::Away)), Some(Shown::Away));
        assert_eq!(shown(wire(Status::Online)), Some(Shown::Online));
        assert_eq!(shown(wire(Status::Offline)), Some(Shown::Offline), "invisible");
        assert_eq!(shown(wire(Status::Unspecified)), Some(Shown::Offline), "a server not keeping the protocol");
        assert_eq!(shown_of(99), Shown::Offline, "a status this build does not know");
    }

    #[test]
    fn the_roster_keeps_its_order_as_members_arrive_are_renamed_and_leave() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = crate::harness::holding(&clock, proto::rules::Grants::default());
        let entries =
            vec![(member("mira"), person("mira"), Shown::Offline), (member(BO), person("Bo"), Shown::Offline)];
        let page = PeoplePage { entries, next: None, presence: 2 };
        tell(&mut window, &clock, owls, place::Message::People(PeopleMessage::Page(Ok(page))));
        let arrived = PeopleMessage::Arrived { user_id: member("cy"), person: person("Cy") };
        tell(&mut window, &clock, owls, place::Message::People(arrived));
        let renamed = place::ServerMessage::Renamed { user_id: member(BO), name: "Zed".into() };
        tell(&mut window, &clock, owls, place::Message::Server(renamed));
        let offline = |window: &Window| -> Vec<String> {
            let roster = window.open().unwrap().roster();
            let rows = roster.lines.iter().filter_map(|line| match line {
                RosterLine::Member(row) if !row.presence.online() => Some(row.name.to_owned()),
                RosterLine::Member(_) | RosterLine::Heading(..) => None,
            });
            rows.collect()
        };
        assert_eq!(offline(&window), ["Cy", "mira", "Zed"]);
        tell(&mut window, &clock, owls, place::Message::People(PeopleMessage::Left(member("mira"))));
        assert_eq!(offline(&window), ["Cy", "Zed"]);
    }

    #[test]
    fn a_members_picture_drawn_is_looked_up_once_fetched_when_missing_and_followed_when_changed() {
        const OWL: proto::files::FileId = proto::files::FileId::from_bytes([7; proto::files::FileId::LEN]);
        let clock = Clock(Instant::now());
        let (mut window, owls) = crate::harness::holding(&clock, proto::rules::Grants::default());
        let people = |message| place::Message::People(message);
        let arrived =
            PeopleMessage::Arrived { user_id: member(BO), person: Named { picture: Some(OWL), ..person("Bo") } };
        assert!(tell(&mut window, &clock, owls, people(arrived)).is_empty(), "nothing until a view draws it");
        assert_eq!(window.open().unwrap().picture_of(&member(BO)), Some(OWL), "for a view to ask the host for");
        let drawn = || place::Message::Server(place::ServerMessage::Missing(vec![OWL]));
        let todo = tell(&mut window, &clock, owls, drawn());
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, place::Ask::LookUpPicture { picture })] if *picture == OWL),
            "the cache first: {todo:?}"
        );
        assert!(tell(&mut window, &clock, owls, drawn()).is_empty(), "asked once a run");
        let missing = place::Message::Server(place::ServerMessage::PictureMissing(OWL));
        let todo = tell(&mut window, &clock, owls, missing);
        assert!(
            matches!(
                &todo[..],
                [crate::Ask::At(_, place::Ask::FetchPicture { picture, kind: proto::files::FileKind::ProfilePicture })]
                    if *picture == OWL
            ),
            "{todo:?}"
        );
        tell(&mut window, &clock, owls, place::Message::Server(place::ServerMessage::PictureReady(OWL)));
        assert!(tell(&mut window, &clock, owls, drawn()).is_empty(), "held");
        let removed = PeopleMessage::Pictured { user_id: member(BO), picture: None };
        tell(&mut window, &clock, owls, people(removed));
        assert_eq!(window.open().unwrap().picture_of(&member(BO)), None, "back to initials");
    }

    #[test]
    fn a_page_with_no_listing_under_way_is_passed_over() {
        let mut directory = Directory::default();
        let late = PeoplePage { entries: vec![(member(ADA), person("Ada"), Shown::Online)], next: None, presence: 1 };
        assert!(directory.page(late).is_none());
        assert!(directory.is_empty());
    }

    #[test]
    fn a_sign_in_reads_the_members_page_by_page_and_what_the_server_tells_keeps_them_current() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        let named = |name: &str| place::Named { name: name.into(), colour: None, roles: Vec::new(), picture: None };
        let page = |entries: Vec<(&str, &str)>, next: Option<&str>| {
            let entries = entries.into_iter().map(|(id, name)| (member(id), named(name), Shown::Offline)).collect();
            place::Message::People(place::PeopleMessage::Page(Ok(place::PeoplePage {
                entries,
                next: next.map(Into::into),
                presence: 1,
            })))
        };
        let ended = Ended { silent: true, ..Ended::default() };
        tell(&mut window, &clock, owls, place::Message::Session(place::SessionMessage::Disconnected(ended)));
        tell(&mut window, &clock, owls, place::Message::Session(place::SessionMessage::Reconnect(1)));
        let todo = tell(&mut window, &clock, owls, flow(accepted([Grant::ChannelRead].into_iter().collect())));
        assert!(
            todo.iter()
                .any(|done| matches!(done, crate::Ask::At(_, ask) if sends(ask, wire::ListPeople { after: None }))),
            "{todo:?}"
        );
        let todo = tell(
            &mut window,
            &clock,
            owls,
            page(vec![(crate::harness::ADA, "Ada"), ("grace", "grace")], Some("grace")),
        );
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, ask)] if sends(ask, wire::ListPeople { after: Some("grace".into()) })),
            "{todo:?}"
        );
        assert!(tell(&mut window, &clock, owls, page(vec![("mira", "mira")], None)).is_empty());
        let name = |window: &Window, id: &str| {
            window.places()[0].directory().person(&member(id)).map(|person| person.name.clone())
        };
        assert_eq!(name(&window, "grace").as_deref(), Some("grace"));
        let arrived = place::PeopleMessage::Arrived { user_id: member("bo"), person: named("Bo") };
        tell(&mut window, &clock, owls, place::Message::People(arrived));
        tell(&mut window, &clock, owls, place::Message::People(place::PeopleMessage::Left(member("mira"))));
        let renamed = place::ServerMessage::Renamed { user_id: member("grace"), name: "Grace H".into() };
        tell(&mut window, &clock, owls, place::Message::Server(renamed));
        assert_eq!((name(&window, "bo"), name(&window, "mira")), (Some("Bo".into()), None));
        assert_eq!(name(&window, "grace").as_deref(), Some("Grace H"));
    }
}
