//! A person's popover, one model whatever opened it (a row of the members column, a seat in the side pane, a card in
//! a call, a message's author, the account bar): their name and colour, their status, every role they hold but the
//! default, an administrator's part on someone below it in a call, in that call or not, and on the account's own
//! opened from its bar, the status menu.

use super::*;
use crate::place::Moderation;
use crate::place::ops;
use crate::status::{StatusMenu, StatusMessage, StatusWord, TOMORROW_AT};
use proto::rules::Colour;
use proto::status::{Choice, Chosen};

/// Where a person's popover was opened, which is where it is drawn beside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PersonFrom {
    /// A row of the members column: to its left.
    Members,
    /// A seat under a voice channel in the side pane: to its right.
    Seat,
    /// A card in a call.
    Card,
    /// A message's author: under the name.
    Author,
    /// The account bar at the pane's foot, where the account's own picture and name are: above it.
    AccountBar,
}

/// A person's popover, open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersonOpen {
    /// The server they are a member of.
    pub place: PlaceId,
    /// Who they are.
    pub user_id: MemberId,
    /// Where it was opened.
    pub from: PersonFrom,
    /// The status menu's row whose durations are open; none while none is.
    pub picking: Option<Choice>,
}

/// A role a person holds, as their popover shows it: a badge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleBadge {
    /// The role's name.
    pub name: String,
    /// Its colour's dot; none for a role with no colour, whose badge has no dot.
    pub colour: Option<Colour>,
}

/// What an administrator may do to the person, as their popover offers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdminPart {
    /// Mute for everyone (`Mute(true)`), or its undo (`Mute(false)`) while they are muted so.
    pub mute: Moderation,
    /// Take out of voice.
    pub take_out: Moderation,
}

/// A person's popover, as drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct PersonCard {
    /// Where it is open, and on whom.
    pub open: PersonOpen,
    /// The name others see.
    pub name: String,
    /// The colour they are shown in.
    pub colour: design::PersonColour,
    /// Their status as others are shown it; the account's own invisible choice says so on its own popover.
    pub presence: StatusWord,
    /// The account's status menu, on its own popover opened from its bar.
    pub status: Option<StatusMenu>,
    /// Every role they hold but the default, highest first, each in its colour.
    pub roles: Vec<RoleBadge>,
    /// How loud this device plays them, only on the popup opened from their card in a call, while this account hears
    /// them: that popup is their name and this alone.
    pub volume: Option<choices::Volume>,
    /// What an administrator may do to them, while this account may; none otherwise.
    pub admin: Option<AdminPart>,
    /// The "Message" button shows: the server has conversations and this is not the account's own popover.
    pub may_message: bool,
}

impl Window {
    /// The person's popover open, as drawn; none where it is not, or its person is no longer known.
    pub fn person(&self) -> Option<PersonCard> {
        let Some(menu::Opened::Person(open)) = self.desk.menu.as_ref() else { return None };
        let place = self.place(open.place)?;
        let user_id = &open.user_id;
        let named = place.directory().person(user_id);
        let seated = place.lobby().and_then(|lobby| match lobby {
            Lobby::Ready(listed) => {
                listed.voice.iter().flat_map(|channel| channel.people.iter()).find(|one| one.user_id == *user_id)
            }
            Lobby::Loading | Lobby::Failed(_) => None,
        });
        let name = named.map(|named| named.name.clone()).or_else(|| seated.map(|seat| seat.name.clone()))?;
        let colour = named.and_then(|named| named.colour).or_else(|| seated.and_then(|seat| seat.colour));
        let own = place.me() == Some(user_id);
        let chosen = own.then(|| self.times.wall.and_then(|wall| place.chosen(wall))).flatten();
        let presence = match chosen {
            Some(Chosen { choice: Choice::Invisible, .. }) => StatusWord::Invisible,
            Some(_) | None => StatusWord::of(place.directory().presence(user_id)),
        };
        let status = chosen
            .filter(|_| open.from == PersonFrom::AccountBar)
            .map(|chosen| StatusMenu::of(chosen, open.picking, self.status_ends(chosen)));
        let roles = place
            .roles_of(user_id)
            .into_iter()
            .map(|role| RoleBadge { name: role.name.clone(), colour: role.colour })
            .collect();
        let volume = (open.from == PersonFrom::Card && place.hears(user_id))
            .then(|| place.volumes().of(user_id, choices::Audio::Voice));
        let admin = place.may_moderate(user_id).then(|| AdminPart {
            mute: Moderation::Mute(!place.held(user_id).unwrap_or(false)),
            take_out: Moderation::TakeOut,
        });
        let may_message = place.offers_conversations() && !own;
        Some(PersonCard {
            open: open.clone(),
            name,
            colour: crate::people::colour_of(user_id, colour),
            presence,
            status,
            roles,
            volume,
            admin,
            may_message,
        })
    }

    /// A person pressed where they show: their popover opens, or closes when it is open on them already.
    pub(super) fn open_person(&mut self, id: PlaceId, user_id: MemberId, from: PersonFrom) -> Vec<Ask> {
        let same =
            matches!(&self.desk.menu, Some(menu::Opened::Person(open)) if open.place == id && open.user_id == user_id);
        let known = self
            .place(id)
            .filter(|place| place.signed_in())
            .is_some_and(|place| place.directory().person(&user_id).is_some() || place.held(&user_id).is_some());
        self.desk.menu = match (same, known) {
            (false, true) => Some(menu::Opened::Person(PersonOpen { place: id, user_id, from, picking: None })),
            (true, _) | (false, false) => None,
        };
        self.desk.toast = None;
        Vec::new()
    }

    /// When `chosen` ends, said where the person is; none for a choice with no end, or before the day is known.
    fn status_ends(&self, chosen: Chosen) -> Option<words::Ends> {
        let (until, today) = (chosen.until?, self.times.today?);
        Some(words::Ends { at: chat::local(until, &self.times.zone)?, today })
    }

    /// The account's status at the open server, as Settings › Notifications' Do not disturb note says it: on, until
    /// when, or where to turn it on.
    pub fn do_not_disturb_note(&self) -> Sentence {
        let quiet = self
            .times
            .wall
            .and_then(|wall| self.desk.open.and_then(|id| self.place(id)).and_then(|place| place.chosen(wall)))
            .filter(|chosen| chosen.choice == Choice::DoNotDisturb);
        match quiet {
            Some(chosen) => Sentence::DoNotDisturbOn(self.status_ends(chosen)),
            None => Sentence::Phrase(Phrase::DoNotDisturbOffNote),
        }
    }

    /// What the account bar says under the account's name at the server `id`, with its dot.
    pub fn account_line(&self, id: PlaceId) -> Option<StatusWord> {
        self.place(id).map(|place| place.account_line(self.times.wall))
    }

    /// The account's Do not disturb holds on some server now: no chime plays.
    pub(super) fn quiet_anywhere(&self) -> bool {
        self.times.wall.is_some_and(|wall| self.places.iter().any(|place| place.quiet(wall)))
    }

    /// What is done in the account's status menu, or Settings' pointer to it.
    pub(super) fn on_status(&mut self, message: StatusMessage) -> Vec<Ask> {
        match message {
            StatusMessage::Choose => self.choose_status(),
            StatusMessage::Pick(Choice::Online) => self.set_status(Chosen::ONLINE),
            StatusMessage::Pick(choice) => {
                if let Some(menu::Opened::Person(open)) = self.desk.menu.as_mut()
                    && open.from == PersonFrom::AccountBar
                {
                    open.picking = if open.picking == Some(choice) { None } else { Some(choice) };
                }
                Vec::new()
            }
            StatusMessage::For(duration) => {
                let picking = match self.desk.menu.as_ref() {
                    Some(menu::Opened::Person(open)) if open.from == PersonFrom::AccountBar => open.picking,
                    _ => None,
                };
                let (Some(choice), Some(wall)) = (picking, self.times.wall) else { return Vec::new() };
                let tomorrow = chat::tomorrow_at(wall, &self.times.zone, TOMORROW_AT);
                self.set_status(Chosen { choice, until: duration.until(wall, tomorrow) })
            }
        }
    }

    /// `chosen` asked of the server whose account popover is open, which closes; nothing where none is, or its account
    /// is not signed in.
    fn set_status(&mut self, chosen: Chosen) -> Vec<Ask> {
        let Some(menu::Opened::Person(open)) = self.desk.menu.as_ref() else { return Vec::new() };
        if open.from != PersonFrom::AccountBar {
            return Vec::new();
        }
        let id = open.place;
        if !self.place(id).is_some_and(|place| place.signed_in()) {
            return Vec::new();
        }
        self.desk.menu = None;
        vec![Ask::At(id, ops::set_status(chosen))]
    }

    /// Settings › Notifications' "Choose a status": this device's settings close and the account's own popover opens
    /// on the open server, from its bar.
    fn choose_status(&mut self) -> Vec<Ask> {
        let Some(id) = self.desk.open else { return Vec::new() };
        let Some(me) = self.place(id).filter(|place| place.signed_in()).and_then(|place| place.me().cloned()) else {
            return Vec::new();
        };
        self.hardware.device = None;
        self.desk.menu = Some(menu::Opened::Person(PersonOpen {
            place: id,
            user_id: me,
            from: PersonFrom::AccountBar,
            picking: None,
        }));
        Vec::new()
    }

    /// The server's answer to a status asked: a refusal is said in a toast.
    pub(super) fn status_set(&mut self, id: PlaceId, answer: &Result<(), LinkError>) -> Vec<Ask> {
        let Err(refused) = answer else { return Vec::new() };
        self.tell(id, Telling::of(design::Status::Danger, words::failure::said(refused)))
    }

    /// An administrator's entry of the open popover: asked of the server, and the popover closes. One the popover no
    /// longer offers (the person or the account's standing changed under it) does nothing.
    pub(super) fn moderate(&mut self, act: Moderation) -> Vec<Ask> {
        let Some(card) = self.person() else { return Vec::new() };
        if !card.admin.is_some_and(|admin| admin.mute == act || admin.take_out == act) {
            return Vec::new();
        }
        self.desk.menu = None;
        vec![Ask::At(card.open.place, ops::moderate(card.open.user_id, act))]
    }

    /// The server's answer to a moderation: a refusal is said in a toast, where the person cannot see it.
    pub(super) fn moderated(
        &mut self,
        id: PlaceId,
        user_id: &MemberId,
        act: Moderation,
        answer: &Result<(), LinkError>,
    ) -> Vec<Ask> {
        let Err(refused) = answer else { return Vec::new() };
        let name =
            self.place(id).and_then(|place| place.name_of(user_id)).unwrap_or_else(|| user_id.as_str().to_owned());
        let left = matches!(
            refused,
            LinkError::Refused(proto::chatkord_v1::ErrorCode::InvalidRequest | proto::chatkord_v1::ErrorCode::NotFound)
        );
        let said = match (left, act) {
            (true, Moderation::Mute(_)) => Sentence::NotMutedLeft(name),
            (true, Moderation::TakeOut) => Sentence::NotTakenOutLeft(name),
            (false, _) => words::failure::said(refused),
        };
        self.tell(id, Telling::of(design::Status::Danger, said))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{
        ADMIN_PLACE, Clock, default_role_entry, member, placed_holding, refused, role_entry, role_id, tell,
    };
    use crate::harness::{sends, wire};
    use proto::chatkord_v1::{ErrorCode, RoleEntry};
    use proto::rules::{Grants, Place as RolePlace};

    /// Someone in Lounge, which this account is not in, as the server's occupancy says.
    fn seat(user_id: &str, name: &str, at: RolePlace, held: bool) -> place::Occupant {
        place::Occupant {
            user_id: member(user_id),
            name: name.into(),
            colour: None,
            state: State::default(),
            sharing: false,
            camera: false,
            held,
            place: at,
        }
    }

    /// Ada at `at` holding `grants`, in no call, with Bo (a member), Cy (an admin) and Di (the owner) in Lounge.
    fn beside_a_call(clock: &Clock, at: RolePlace, grants: Grants, bo_held: bool) -> (Window, PlaceId) {
        let (mut window, owls) = placed_holding(clock, at, grants);
        let people = vec![
            seat("bo", "Bo", RolePlace::BOTTOM, bo_held),
            seat("cy", "Cy", ADMIN_PLACE, false),
            seat("di", "Di", RolePlace::OWNER, false),
        ];
        let occupancy = place::LobbyMessage::Occupancy { channel_id: crate::harness::channel_id("lounge"), people };
        tell(&mut window, clock, owls, place::Message::Lobby(occupancy));
        (window, owls)
    }

    fn moderates() -> Grants {
        Grants::from_words(&["MODERATE_VOICE"])
    }

    fn open(window: &mut Window, clock: &Clock, owls: PlaceId, who: &str) {
        window.update(Message::Person(owls, member(who), PersonFrom::Seat), clock.at());
    }

    #[test]
    fn a_seat_in_the_pane_outside_the_call_is_moderated_through_the_place_s_ask() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = beside_a_call(&clock, ADMIN_PLACE, moderates(), false);
        assert!(window.open().unwrap().call().is_none(), "this account is in no call");
        open(&mut window, &clock, owls, "bo");
        let card = window.person().expect("Bo's popover");
        // In a call but told no presence: a seat in voice is no presence of its own.
        assert_eq!((card.name.as_str(), card.presence), ("Bo", StatusWord::Offline));
        assert_eq!(card.admin, Some(AdminPart { mute: Moderation::Mute(true), take_out: Moderation::TakeOut }));
        let todo = window.update(Message::Moderate(Moderation::Mute(true)), clock.at());
        assert!(
            matches!(&todo[..], [Ask::At(at, ask)]
                if *at == owls && sends(ask, wire::VoiceMuteMember { user_id: "bo".into(), muted: true })),
            "{todo:?}"
        );
        assert!(window.person().is_none(), "the popover closed");
        assert!(window.update(Message::Moderate(Moderation::TakeOut), clock.at()).is_empty(), "nothing is open");

        let (mut window, owls) = beside_a_call(&clock, ADMIN_PLACE, moderates(), true);
        open(&mut window, &clock, owls, "bo");
        assert_eq!(window.person().and_then(|card| card.admin).map(|admin| admin.mute), Some(Moderation::Mute(false)));
        assert!(window.update(Message::Moderate(Moderation::Mute(true)), clock.at()).is_empty(), "not offered now");
        let todo = window.update(Message::Moderate(Moderation::TakeOut), clock.at());
        assert!(
            matches!(&todo[..], [Ask::At(_, ask)] if crate::harness::is(ask, crate::harness::Op::VoiceTakeOut)),
            "{todo:?}"
        );

        for (who, at) in [("cy", ADMIN_PLACE), ("di", ADMIN_PLACE), ("cy", RolePlace::OWNER)] {
            let (mut window, owls) = beside_a_call(&clock, at, moderates(), false);
            open(&mut window, &clock, owls, who);
            let offered = window.person().and_then(|card| card.admin).is_some();
            assert_eq!(offered, at.is_owner() && who == "cy", "{who} for an admin at {at:?}: only below the asker");
        }
        let (mut window, owls) = beside_a_call(&clock, ADMIN_PLACE, Grants::default(), false);
        open(&mut window, &clock, owls, "bo");
        assert_eq!(window.person().map(|card| card.admin), Some(None), "no word to act on them");
        open(&mut window, &clock, owls, "bo");
        assert!(window.person().is_none(), "a second press closes it");
    }

    #[test]
    fn a_refused_moderation_is_said_in_a_toast() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = beside_a_call(&clock, ADMIN_PLACE, moderates(), false);
        let answered = |act, code| {
            let told = place::ServerMessage::Moderated { user_id: member("bo"), act, answer: Err(refused(code)) };
            Message::At(owls, place::Message::Server(told))
        };
        let toast = |window: &Window| window.toast().map(|toast| toast.said().in_english());
        window.update(answered(Moderation::Mute(true), ErrorCode::NotFound), clock.at());
        assert_eq!(toast(&window).as_deref(), Some("Bo couldn’t be muted for everyone: they left the call."));
        window.update(answered(Moderation::TakeOut, ErrorCode::InvalidRequest), clock.at());
        assert_eq!(toast(&window).as_deref(), Some("Bo couldn’t be taken out of voice: they left the call."));
        window.update(answered(Moderation::TakeOut, ErrorCode::NotAuthorized), clock.at());
        assert_eq!(toast(&window).as_deref(), Some("Your account may not do that."));
        let (mut window, owls) = beside_a_call(&clock, ADMIN_PLACE, moderates(), false);
        let done = place::ServerMessage::Moderated { user_id: member("bo"), act: Moderation::TakeOut, answer: Ok(()) };
        window.update(Message::At(owls, place::Message::Server(done)), clock.at());
        assert_eq!(toast(&window), None, "the pane shows what went through");
    }

    #[test]
    fn the_popover_names_every_held_known_role_highest_first_the_default_only_alone_and_follows_a_change() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = beside_a_call(&clock, ADMIN_PLACE, moderates(), false);
        let roles = vec![
            RoleEntry { colour: Some("#FF8A5B".into()), ..role_entry("1", "Owner", RolePlace::OWNER, &[]) },
            RoleEntry { colour: Some("#7CB7FF".into()), ..role_entry("2", "Admin", RolePlace::at(10), &[]) },
            RoleEntry { colour: Some("#F2C14E".into()), ..role_entry("3", "Game hosts", RolePlace::at(11), &[]) },
            role_entry("4", "Helpers", RolePlace::at(12), &[]),
            default_role_entry("5", "Member", RolePlace::at(13), &[]),
        ];
        tell(&mut window, &clock, owls, place::Message::Server(place::ServerMessage::RolesListed(Ok(roles))));
        let grace = place::Named {
            name: "grace".into(),
            colour: None,
            roles: ["5", "4", "gone", "3"].into_iter().map(role_id).collect(),
            picture: None,
        };
        let page = place::PeoplePage {
            entries: vec![(member("grace"), grace, proto::status::Shown::Offline)],
            next: None,
            presence: 3,
        };
        tell(&mut window, &clock, owls, place::Message::People(place::PeopleMessage::Page(Ok(page))));
        window.update(Message::Person(owls, member("grace"), PersonFrom::Members), clock.at());
        let card = window.person().expect("grace's popover");
        let badges: Vec<(&str, Option<String>)> = card
            .roles
            .iter()
            .map(|badge| (badge.name.as_str(), badge.colour.map(|colour| colour.to_string())))
            .collect();
        assert_eq!(
            badges,
            [("Game hosts", Some("#F2C14E".to_string())), ("Helpers", None)],
            "the roles' order, not the member's; the default and a role the list does not hold left out"
        );
        assert_eq!((card.presence, card.admin), (StatusWord::Offline, None));
        let raised = place::PeopleMessage::Roles { user_id: member("grace"), roles: vec![role_id("2"), role_id("5")] };
        tell(&mut window, &clock, owls, place::Message::People(raised));
        let online = place::PeopleMessage::Presence {
            generation: 4,
            shown: vec![(member("grace"), proto::status::Shown::Online)],
        };
        tell(&mut window, &clock, owls, place::Message::People(online));
        let card = window.person().expect("still open");
        assert_eq!(card.roles.iter().map(|badge| badge.name.as_str()).collect::<Vec<_>>(), ["Admin"]);
        assert_eq!(card.presence, StatusWord::Online);
        let lowered = place::PeopleMessage::Roles { user_id: member("grace"), roles: vec![role_id("5")] };
        tell(&mut window, &clock, owls, place::Message::People(lowered));
        let card = window.person().expect("still open");
        let badges: Vec<_> = card.roles.iter().map(|badge| (badge.name.as_str(), badge.colour)).collect();
        assert_eq!(badges, [("Member", None)], "a member holding no other role is shown with the default one");
    }

    #[test]
    fn the_popup_of_a_call_card_offers_the_volume_of_someone_heard_and_no_other_popover_does() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = crate::harness::moderator_in_call(&clock, ADMIN_PLACE, moderates());
        window.update(Message::Person(owls, member("bo"), PersonFrom::Card), clock.at());
        let card = window.person().expect("Bo's popup");
        assert_eq!(card.volume, Some(choices::Volume::FULL));
        window.update(Message::Person(owls, member("bo"), PersonFrom::Members), clock.at());
        window.update(Message::Person(owls, member("bo"), PersonFrom::Members), clock.at());
        let card = window.person().expect("Bo's popover");
        assert_eq!(card.volume, None, "the member popover has no volume");
        window.update(Message::Person(owls, member("ada"), PersonFrom::Card), clock.at());
        assert_eq!(window.person().map(|card| card.volume), Some(None), "one's own volume is not offered");
    }

    #[test]
    fn the_account_bar_opens_one_s_own_popover_with_no_message_and_no_volume() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = crate::harness::moderator_in_call(&clock, ADMIN_PLACE, moderates());
        crate::harness::people_listed(&mut window, &clock, owls);
        window.update(Message::Person(owls, member("ada"), PersonFrom::AccountBar), clock.at());
        let card = window.person().expect("one's own popover");
        assert_eq!((card.open.from, card.name.as_str()), (PersonFrom::AccountBar, "Ada"));
        assert!(!card.may_message, "one does not message oneself");
        assert_eq!((card.volume, card.admin), (None, None));
        assert_eq!(card.presence, StatusWord::Online);
        window.update(Message::Person(owls, member("ada"), PersonFrom::AccountBar), clock.at());
        assert!(window.person().is_none(), "a second press closes it");
    }

    mod status {
        use super::*;
        use crate::harness::{ADA, WALL, chatting};
        use crate::status::{StatusFor, StatusMessage, StatusWord};
        use proto::status::{Choice, Chosen};

        /// Ada signed in, the day known, her account's popover open from its bar.
        fn own_popover(clock: &Clock) -> (Window, PlaceId) {
            let (mut window, owls) = chatting(clock);
            crate::harness::people_listed(&mut window, clock, owls);
            let today = chat::local(WALL, &chat::TimeZone::UTC).map(|at| at.date()).expect("a day");
            window.update(Message::Today(today), clock.at());
            open(&mut window, clock, owls);
            (window, owls)
        }

        fn open(window: &mut Window, clock: &Clock, owls: PlaceId) {
            window.update(Message::Person(owls, member(ADA), PersonFrom::AccountBar), clock.at());
        }

        fn chosen(window: &mut Window, clock: &Clock, owls: PlaceId, chosen: Chosen) {
            tell(window, clock, owls, place::Message::Talk(place::TalkMessage::StatusChosen(chosen)));
        }

        fn asked(todo: &[Ask]) -> Vec<Chosen> {
            todo.iter()
                .filter_map(|done| match done {
                    Ask::At(_, ask) => match ask.op() {
                        Some(crate::harness::Body::SetStatus(set)) => Some(Chosen {
                            choice: proto::status::Choice::from_wire(
                                wire::StatusChoice::try_from(set.choice).expect("a choice"),
                            )
                            .expect("a choice"),
                            until: set.until,
                        }),
                        _ => None,
                    },
                    _ => None,
                })
                .collect()
        }

        #[test]
        fn the_accounts_own_popover_lists_four_statuses_the_chosen_one_marked_with_its_end() {
            let clock = Clock(Instant::now());
            let (mut window, owls) = own_popover(&clock);
            let menu = window.person().and_then(|card| card.status).expect("the status menu");
            let rows: Vec<_> = menu.rows.iter().map(|row| (row.choice, row.chosen)).collect();
            assert_eq!(
                rows,
                [
                    (Choice::Online, true),
                    (Choice::Away, false),
                    (Choice::DoNotDisturb, false),
                    (Choice::Invisible, false)
                ]
            );
            assert!(menu.rows.iter().all(|row| row.until.is_none()) && menu.durations.is_empty());
            chosen(&mut window, &clock, owls, Chosen { choice: Choice::DoNotDisturb, until: Some(WALL + 3_600) });
            let menu = window.person().and_then(|card| card.status).expect("still open");
            let marked = menu.rows.iter().find(|row| row.chosen).expect("one marked");
            assert_eq!(marked.choice, Choice::DoNotDisturb);
            let until = marked.until.as_ref().map(|said| words::Words::english().said(said));
            assert_eq!(until.as_deref(), Some("Until 13:00"), "an hour after the wall clock, where the person is");
            window.update(Message::Person(owls, member("bo"), PersonFrom::Members), clock.at());
            assert_eq!(window.person().and_then(|card| card.status), None, "no one else's popover has it");
        }

        #[test]
        fn online_is_asked_at_once_with_no_end_and_each_duration_asks_its_end() {
            let clock = Clock(Instant::now());
            let (mut window, owls) = own_popover(&clock);
            let todo = window.update(Message::Status(StatusMessage::Pick(Choice::Online)), clock.at());
            assert_eq!(asked(&todo), [Chosen::ONLINE], "no end with Online");
            assert!(window.person().is_none(), "the popover closes");
            let tomorrow = chat::tomorrow_at(WALL, &chat::TimeZone::UTC, crate::status::TOMORROW_AT);
            for (choice, duration, until) in [
                (Choice::Away, StatusFor::OneHour, Some(WALL + 3_600)),
                (Choice::DoNotDisturb, StatusFor::EightHours, Some(WALL + 8 * 3_600)),
                (Choice::DoNotDisturb, StatusFor::Tomorrow, tomorrow),
                (Choice::Invisible, StatusFor::UntilOff, None),
            ] {
                open(&mut window, &clock, owls);
                let todo = window.update(Message::Status(StatusMessage::Pick(choice)), clock.at());
                assert!(asked(&todo).is_empty(), "{choice:?}: its durations open, nothing is asked yet");
                let menu = window.person().and_then(|card| card.status).expect("open");
                assert_eq!(menu.durations, StatusFor::ALL);
                assert!(menu.rows.iter().any(|row| row.choice == choice && row.picking));
                let todo = window.update(Message::Status(StatusMessage::For(duration)), clock.at());
                assert_eq!(asked(&todo), [Chosen { choice, until }], "{duration:?}");
            }
            assert!(tomorrow.is_some_and(|at| at > WALL));
        }

        #[test]
        fn the_account_bar_says_the_status_chosen_and_online_once_it_ends() {
            let clock = Clock(Instant::now());
            let (mut window, owls) = chatting(&clock);
            assert_eq!(window.account_line(owls), Some(StatusWord::Online), "until a choice is told");
            for (choice, word) in [
                (Choice::Online, StatusWord::Online),
                (Choice::Away, StatusWord::Away),
                (Choice::DoNotDisturb, StatusWord::DoNotDisturb),
                (Choice::Invisible, StatusWord::Invisible),
            ] {
                chosen(&mut window, &clock, owls, Chosen { choice, until: None });
                assert_eq!(window.account_line(owls), Some(word));
            }
            chosen(&mut window, &clock, owls, Chosen { choice: Choice::Away, until: Some(WALL) });
            assert_eq!(window.account_line(owls), Some(StatusWord::Online), "an end that passed is Online");
        }

        #[test]
        fn do_not_disturb_stops_the_windows_chimes_and_settings_point_to_the_status() {
            let clock = Clock(Instant::now());
            let (mut window, owls) = own_popover(&clock);
            let chimes = |todo: &[Ask]| todo.iter().filter(|done| matches!(done, Ask::Chime(_))).count();
            assert_eq!(chimes(&window.update(Message::NextMute, clock.at())), 1, "a press sounds");
            let note = |window: &Window| words::Words::english().said(&window.do_not_disturb_note());
            assert_eq!(note(&window), "Your status: choose Do not disturb in it, and none of your devices alerts.");
            chosen(&mut window, &clock, owls, Chosen { choice: Choice::DoNotDisturb, until: None });
            assert_eq!(chimes(&window.update(Message::NextMute, clock.at())), 0, "not while not to be disturbed");
            assert_eq!(note(&window), "On: no popups or sounds on any of your devices.");
            window.update(Message::OpenDevice(DevicePage::Notifications), clock.at());
            window.update(Message::Status(StatusMessage::Choose), clock.at());
            assert_eq!(window.device(), None, "settings close");
            let card = window.person().expect("the account's popover opens");
            assert_eq!(card.open.from, PersonFrom::AccountBar);
            assert!(card.status.is_some());
        }
    }
}
