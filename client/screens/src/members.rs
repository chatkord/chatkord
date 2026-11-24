//! A server's members as an administrator manages them: where each stands in the roles' order, what may be done to
//! them (only what the server allows, by the rule it applies), the banned apart, a search over both, and a reset
//! password, whose temporary one is shown once; a kick asked first, and a ban or an unban proved by the password.

use crate::Asked;
use crate::place::ops;
use crate::proof::{Proof, Refused, refusal};
use crate::roles::RoleList;
use crate::window::{Confirming, Copied, DialogMessage, Next};
use proto::chatkord_v1::MemberEntry;
use proto::rules::{Colour, Grant, Grants, Place};
use terms::LinkError;
use terms::Secret;
use terms::{MemberId, RoleId};
use words::{Phrase, Sentence};

/// A member as the page lists them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListedMember {
    /// Their account's id.
    pub user_id: MemberId,
    /// The name others see.
    pub name: String,
    /// Their username, which the search finds them by too.
    pub handle: String,
    /// The colour they picked, if any.
    pub colour: Option<Colour>,
    /// Where they stand in the roles' order, as the server places them.
    pub place: Place,
    /// The account signed in here.
    pub you: bool,
    /// Banned: they cannot sign in until they are unbanned.
    pub banned: bool,
    /// The roles they hold, by id; named from the place's roles where they are drawn.
    holds: Vec<RoleId>,
}

/// A role a member's Roles menu offers: ticked while they hold it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offered {
    /// The role's id at the server.
    pub role: RoleId,
    /// Its name.
    pub name: String,
    /// Its colour; none for none.
    pub colour: Option<Colour>,
    /// The member holds it.
    pub held: bool,
}

impl Offered {
    /// What its tick does: give the role, or take it.
    pub fn action(&self) -> MemberAction {
        if self.held {
            MemberAction::TakeRole(self.role.clone(), self.name.clone())
        } else {
            MemberAction::GiveRole(self.role.clone(), self.name.clone())
        }
    }
}

/// What may be done to a member from the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemberAction {
    /// Give them the role, by its id and its name.
    GiveRole(RoleId, String),
    /// Take the role from them, by its id and its name.
    TakeRole(RoleId, String),
    /// Give them a temporary password in place of theirs.
    ResetPassword,
    /// Make them the server owner in place of the asker; offered only to the owner.
    MakeOwner,
    /// Sign them out on every device; asked first.
    Kick,
    /// Keep them out until they are unbanned; proved by the password.
    Ban,
    /// Let a banned member sign in again; proved by the password.
    Unban,
}

impl MemberAction {
    /// What its menu entry or button says, of the member named `name`.
    pub fn said(&self, name: &str) -> Sentence {
        match self {
            MemberAction::GiveRole(_, role) | MemberAction::TakeRole(_, role) => Sentence::Said(role.clone()),
            MemberAction::ResetPassword => Sentence::Phrase(Phrase::ResetPasswordAction),
            MemberAction::MakeOwner => Sentence::Phrase(Phrase::MakeOwner),
            MemberAction::Kick => Sentence::KickMember(name.to_string()),
            MemberAction::Ban => Sentence::BanMember(name.to_string()),
            MemberAction::Unban => Sentence::Phrase(Phrase::Unban),
        }
    }

    /// It ends the member's sessions, or keeps them out: drawn in the danger colour, set off by a rule.
    pub fn danger(&self) -> bool {
        matches!(self, MemberAction::Kick | MemberAction::Ban)
    }
}

impl ListedMember {
    /// The member `entry` lists; none when it names no id, a server not keeping the protocol.
    fn read(entry: MemberEntry, me: Option<&MemberId>) -> Option<Self> {
        let user_id = MemberId::parse(&entry.user_id)?;
        Some(ListedMember {
            you: me == Some(&user_id),
            place: Place::of_wire(entry.place),
            banned: entry.banned,
            holds: entry.role_ids.iter().filter_map(|id| RoleId::parse(id)).collect(),
            user_id,
            name: entry.display_name,
            handle: entry.handle,
            colour: crate::place::wire_colour(entry.colour.as_deref()),
        })
    }

    /// Whether someone at `asker` acts on this member at all: someone else, placed below them.
    fn below(&self, asker: Place) -> bool {
        !self.you && asker.outranks(self.place)
    }

    /// The roles of `list` someone at `asker` holding `grants` may give this member or take from them, in the order,
    /// each ticked while held: under `MANAGE_ROLES`, every role made below the asker that holds no word they lack;
    /// none for a banned member or one not placed below the asker.
    pub fn roles(&self, list: &RoleList, asker: Place, grants: Grants) -> Vec<Offered> {
        if self.banned || !self.below(asker) {
            return Vec::new();
        }
        list.givable(asker, grants)
            .map(|role| Offered {
                role: role.id.clone(),
                name: role.name.clone(),
                colour: role.colour,
                held: self.holds.contains(&role.id),
            })
            .collect()
    }

    /// What else someone at `asker` holding `grants` may do to this member, in the order their More menu shows it: a
    /// reset, a kick and a ban under `MANAGE_MEMBERS`, ownership as the owner. Nothing for a banned member or one not
    /// placed below the asker.
    pub fn more(&self, asker: Place, grants: Grants) -> Vec<MemberAction> {
        if self.banned || !self.below(asker) {
            return Vec::new();
        }
        let manages = grants.holds(Grant::ManageMembers);
        let reset = manages.then_some(MemberAction::ResetPassword);
        let owner = asker.is_owner().then_some(MemberAction::MakeOwner);
        let out = manages.then_some([MemberAction::Kick, MemberAction::Ban]).into_iter().flatten();
        reset.into_iter().chain(owner).chain(out).collect()
    }

    /// Unban, for a banned member placed below someone at `asker` holding `MANAGE_MEMBERS`.
    pub fn unban(&self, asker: Place, grants: Grants) -> Option<MemberAction> {
        (self.banned && self.below(asker) && grants.holds(Grant::ManageMembers)).then_some(MemberAction::Unban)
    }

    /// Everything someone at `asker` holding `grants` may do to this member, the roles offered from `list`.
    pub fn actions(&self, list: &RoleList, asker: Place, grants: Grants) -> Vec<MemberAction> {
        let roles = self.roles(list, asker, grants).into_iter().map(|offered| offered.action());
        roles.chain(self.more(asker, grants)).chain(self.unban(asker, grants)).collect()
    }

    /// The names of every role in `list` they hold, highest first, the default one only where they hold no other.
    fn held_names(&self, list: &RoleList) -> Vec<String> {
        list.shown(&self.holds).into_iter().map(|role| role.name.clone()).collect()
    }

    /// What their row says under their name: their username and every role of `list` they hold, highest first (the
    /// default one where they hold no other), or that they are banned. A view cuts it to one line;
    /// [`ListedMember::roles_said`] is the whole.
    pub fn caption(&self, list: &RoleList) -> Sentence {
        if self.banned {
            Sentence::BannedHandle(self.handle.clone())
        } else {
            Sentence::Placed { handle: self.handle.clone(), roles: self.held_names(list), you: self.you }
        }
    }

    /// Every role [`ListedMember::caption`] names, as one list: the caption's tooltip; none where `list` names none.
    pub fn roles_said(&self, list: &RoleList) -> Option<Sentence> {
        let names = self.held_names(list);
        (!names.is_empty()).then_some(Sentence::Listed(names))
    }

    /// The role given or taken, shown at once until the list is read again.
    pub(crate) fn hold(&mut self, role: &RoleId, held: bool) {
        self.holds.retain(|one| one != role);
        if held {
            self.holds.push(role.clone());
        }
    }

    /// Whether `query`, trimmed and lowered, is part of their name or username; an empty one finds everyone.
    fn found(&self, query: &str) -> bool {
        query.is_empty() || self.name.to_lowercase().contains(query) || self.handle.to_lowercase().contains(query)
    }

    /// The roles they hold changed: shown at once, as the server told it.
    pub(crate) fn held_now(&mut self, roles: &[RoleId]) {
        self.holds = roles.to_vec();
    }
}

/// The members as the server listed them, in the order drawn: the server owner, you, then the rest by name; one that
/// names no id is passed over. Their roles are named from the place's roles where they are drawn.
pub(crate) fn listed(entries: Vec<MemberEntry>, me: Option<&MemberId>) -> Vec<ListedMember> {
    let mut members: Vec<ListedMember> =
        entries.into_iter().filter_map(|entry| ListedMember::read(entry, me)).collect();
    members.sort_by_cached_key(|member| (!member.place.is_owner(), !member.you, member.name.to_lowercase()));
    members
}

/// Which of the members the page lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MemberTab {
    /// Everyone not banned.
    #[default]
    Members,
    /// The banned.
    Banned,
}

/// A menu on a member's row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberMenu {
    /// The roles to give or take.
    Roles,
    /// Everything else: a reset, ownership, a kick, a ban.
    More,
}

/// What a person does on the members page apart from acting on a member.
#[derive(Clone, Debug)]
pub enum Message {
    /// One of its two lists picked; a menu open closes.
    Tab(MemberTab),
    /// The search's text after an edit; a menu open closes.
    Search(String),
    /// Clear search, or Escape in the search while it holds something.
    ClearSearch,
    /// A row's menu button: it opens, or closes while it is open.
    Menu(MemberId, MemberMenu),
    /// A press outside the menu open, or Escape.
    CloseMenu,
}

/// How the members page lists them: which list, what the search holds, and the menu open on a row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Finding {
    tab: MemberTab,
    query: String,
    menu: Option<(MemberId, MemberMenu)>,
}

impl Finding {
    /// The list shown.
    pub fn tab(&self) -> MemberTab {
        self.tab
    }

    /// What the search holds, as typed.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The menu open, on whose row.
    pub fn menu(&self) -> Option<(&MemberId, MemberMenu)> {
        self.menu.as_ref().map(|(who, menu)| (who, *menu))
    }

    pub(crate) fn update(&mut self, message: Message) {
        match message {
            Message::Tab(tab) => {
                self.tab = tab;
                self.menu = None;
            }
            Message::Search(query) => {
                self.query = query;
                self.menu = None;
            }
            Message::ClearSearch => self.query.clear(),
            Message::Menu(who, menu) => {
                let open = self.menu.as_ref().is_some_and(|(at, kind)| *at == who && *kind == menu);
                self.menu = (!open).then_some((who, menu));
            }
            Message::CloseMenu => self.menu = None,
        }
    }

    /// A menu entry chosen that opens a dialog: the menu closes.
    pub(crate) fn close_menu(&mut self) {
        self.menu = None;
    }

    /// The members of `all` the page shows now: the list picked, those the search finds.
    pub fn shown<'a>(&self, all: &'a [ListedMember]) -> Vec<&'a ListedMember> {
        let query = self.query.trim().to_lowercase();
        let banned = self.tab == MemberTab::Banned;
        all.iter().filter(|member| member.banned == banned && member.found(&query)).collect()
    }

    /// What the page says where it shows no one: that the search found no one, or that no one is banned.
    pub fn none_said(&self) -> Sentence {
        match self.query.trim() {
            "" => Sentence::Phrase(Phrase::NoOneBanned),
            query => Sentence::NoOneMatches(query.to_string()),
        }
    }
}

/// How many of `all` each list holds: the members, and the banned.
pub fn counts(all: &[ListedMember]) -> (u32, u32) {
    let banned = all.iter().filter(|member| member.banned).count();
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    (count(all.len() - banned), count(banned))
}

/// Resetting a member's password: asked first, then the temporary one to hand over, shown once.
#[derive(Debug)]
pub struct Resetting {
    pub(crate) user_id: MemberId,
    pub(crate) name: String,
    asked: Asked<Refused>,
    pub(crate) temporary: Option<Secret>,
    pub(crate) copied: bool,
}
impl Resetting {
    pub(crate) fn new(user_id: MemberId, name: String) -> Resetting {
        Resetting { user_id, name, asked: Asked::Idle, temporary: None, copied: false }
    }

    /// The member's name as others see it.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the server is being asked for the reset.
    pub fn asking(&self) -> bool {
        self.asked.asking()
    }

    /// Why the last ask was refused; none while asking and before a refusal.
    pub fn refused(&self) -> Option<Refused> {
        self.asked.refused()
    }

    /// The temporary password the server made; none until it answered with one.
    pub fn temporary(&self) -> Option<&Secret> {
        self.temporary.as_ref()
    }

    /// Whether the temporary password went onto the clipboard.
    pub fn copied(&self) -> bool {
        self.copied
    }
}

/// Making a member the server owner, proved by the current password.
#[derive(Debug)]
pub struct Transferring {
    pub(crate) user_id: MemberId,
    pub(crate) name: String,
    pub(crate) proof: Proof,
}

impl Transferring {
    /// The member's name as others see it.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The asker's current password as typed, and how the ask stands.
    pub fn proof(&self) -> &Proof {
        &self.proof
    }
}

/// A reset is asked first, and its temporary password shown once.
impl Confirming for Resetting {
    fn holds_outside(&self) -> bool {
        self.asking() || self.temporary.is_some()
    }

    // A reset under way would lose the temporary password it answers with.
    fn holds_cancel(&self) -> bool {
        self.asking()
    }

    fn said(&mut self, said: DialogMessage) -> Next {
        match said {
            DialogMessage::Confirm if self.temporary.is_some() => Next::Closes,
            DialogMessage::Confirm => {
                self.asked.ask();
                Next::Asks(ops::reset_password(&self.user_id))
            }
            DialogMessage::CopyTemporary => match &self.temporary {
                Some(temporary) => Next::Copies(temporary.clone(), Copied::Temporary),
                None => Next::Stays,
            },
            DialogMessage::Password(_)
            | DialogMessage::Reveal
            | DialogMessage::Cancel
            | DialogMessage::Outside
            | DialogMessage::Invite(_)
            | DialogMessage::Named(_)
            | DialogMessage::SetUp(_)
            | DialogMessage::Making(_) => Next::Stays,
        }
    }
}

impl Resetting {
    /// The server's answer: the temporary password, or why not; a member no longer there closes the dialog.
    pub(crate) fn answered(&mut self, answer: Result<Secret, LinkError>) -> Next {
        match answer.map_err(|refused| refusal(&refused)) {
            Ok(temporary) => {
                self.asked.answered(None);
                self.temporary = Some(temporary);
            }
            Err(Some(refused)) => self.asked.answered(Some(refused)),
            Err(None) => return Next::Closes,
        }
        Next::Stays
    }
}

/// Handing the server on, proved by the password.
impl Confirming for Transferring {
    fn holds_outside(&self) -> bool {
        self.proof.kept()
    }

    fn said(&mut self, said: DialogMessage) -> Next {
        match self.proof.heard(said) {
            Some(password) => Next::Asks(ops::transfer_ownership(&self.user_id, &password)),
            None => Next::Stays,
        }
    }
}

impl Transferring {
    /// The server's answer: the dialog closes, or says why not; a member no longer there closes it too.
    pub(crate) fn answered(&mut self, answer: &Result<(), LinkError>) -> Next {
        match answer.as_ref().map_err(refusal) {
            Ok(()) | Err(None) => Next::Closes,
            Err(Some(refused)) => {
                self.proof.answered(refused);
                Next::Stays
            }
        }
    }
}

/// What the members page says when the server would not give `name` a role or take one, at the server shown as
/// `server`; none when it went through or the member is gone.
pub(crate) fn roles_refused(name: String, server: String, answer: &Result<(), LinkError>) -> Option<Sentence> {
    match answer.as_ref().map_err(refusal) {
        Ok(()) | Err(None) => None,
        Err(Some(Refused::NotAllowed)) => Some(Sentence::CantChangeRoles(name)),
        Err(Some(Refused::TooManyTries)) => Some(Sentence::Phrase(Phrase::TooManyTries)),
        Err(Some(Refused::WrongPassword | Refused::NoAnswer)) => Some(Sentence::NoAnswer(server)),
    }
}

/// Signing a member out everywhere: asked first.
#[derive(Debug)]
pub struct Kicking {
    pub(crate) user_id: MemberId,
    pub(crate) name: String,
    asked: Asked<Refused>,
}

impl Kicking {
    pub(crate) fn new(user_id: MemberId, name: String) -> Kicking {
        Kicking { user_id, name, asked: Asked::Idle }
    }

    /// The member's name as others see it.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the server is being asked.
    pub fn asking(&self) -> bool {
        self.asked.asking()
    }

    /// Why the last ask was refused; none while asking and before a refusal.
    pub fn refused(&self) -> Option<Refused> {
        self.asked.refused()
    }

    /// The server's answer: done, which the window says; gone already, which closes quietly; or why not.
    pub(crate) fn answered(&mut self, answer: &Result<(), LinkError>) -> Next {
        match answer.as_ref().map_err(refusal) {
            Ok(()) => Next::Done(Sentence::Kicked(self.name.clone())),
            Err(None) => Next::Closes,
            Err(Some(refused)) => {
                self.asked.answered(Some(refused));
                Next::Stays
            }
        }
    }
}

/// A kick is asked once at a time.
impl Confirming for Kicking {
    fn holds_outside(&self) -> bool {
        self.asking()
    }

    fn holds_cancel(&self) -> bool {
        self.asking()
    }

    fn said(&mut self, said: DialogMessage) -> Next {
        match said {
            DialogMessage::Confirm if !self.asking() => {
                self.asked.ask();
                Next::Asks(ops::kick(&self.user_id))
            }
            DialogMessage::Confirm
            | DialogMessage::Password(_)
            | DialogMessage::Reveal
            | DialogMessage::Cancel
            | DialogMessage::Outside
            | DialogMessage::Invite(_)
            | DialogMessage::CopyTemporary
            | DialogMessage::Named(_)
            | DialogMessage::SetUp(_)
            | DialogMessage::Making(_) => Next::Stays,
        }
    }
}

/// Whether a ban is put on or lifted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BanAsk {
    /// The member is banned.
    Ban,
    /// The member's ban is lifted.
    Unban,
}

/// Banning a member, or lifting their ban, proved by the current password.
#[derive(Debug)]
pub struct Banning {
    pub(crate) user_id: MemberId,
    pub(crate) name: String,
    pub(crate) ask: BanAsk,
    pub(crate) proof: Proof,
}

impl Banning {
    /// The member's name as others see it.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// A ban, or lifting one.
    pub fn ask(&self) -> BanAsk {
        self.ask
    }

    /// The asker's current password as typed, and how the ask stands.
    pub fn proof(&self) -> &Proof {
        &self.proof
    }

    /// The server's answer: the dialog closes, or says why not; a member no longer there closes it too.
    pub(crate) fn answered(&mut self, answer: &Result<(), LinkError>) -> Next {
        match answer.as_ref().map_err(refusal) {
            Ok(()) | Err(None) => Next::Closes,
            Err(Some(refused)) => {
                self.proof.answered(refused);
                Next::Stays
            }
        }
    }
}

/// A ban or an unban, proved by the password.
impl Confirming for Banning {
    fn holds_outside(&self) -> bool {
        self.proof.kept()
    }

    fn said(&mut self, said: DialogMessage) -> Next {
        let banned = self.ask == BanAsk::Ban;
        match self.proof.heard(said) {
            Some(password) => Next::Asks(ops::set_banned(&self.user_id, banned, &password)),
            None => Next::Stays,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::harness::{sends, wire};
    /// Every word: what the owner and admin roles hold.
    fn every() -> Grants {
        Grant::ALL.into_iter().collect()
    }

    use super::*;
    use crate::harness::{self, ADA, Clock, default_role_entry, holding, refused, role_entry, tell};
    use crate::loaded::Loaded;
    use crate::notice::Subject;
    use crate::place::{self, Place, PlaceId};
    use crate::proof::Refused;
    use crate::settings::Page;
    use crate::window::{Asking, Dialog, DialogMessage, Pick, Window};
    use crate::{Ask, WindowMessage};
    use proto::chatkord_v1::{ErrorCode, RoleEntry};
    use proto::rules::Grant;
    use std::time::Instant;
    use terms::Secret;
    use words::Sentence;

    use crate::RolePlace;
    use harness::ADMIN_PLACE;

    fn entry(id: &str, name: &str, at: RolePlace, roles: &[&str], grants: &[Grant]) -> MemberEntry {
        MemberEntry {
            user_id: id.into(),
            handle: name.into(),
            display_name: name.into(),
            role_ids: roles.iter().map(|role| role.to_string()).collect(),
            place: at.wire(),
            grants: grants.iter().map(|grant| grant.word().to_string()).collect(),
            colour: None,
            banned: false,
        }
    }

    const OWNER_ROLE: &str = "11";
    const ADMIN_ROLE: &str = "12";
    const MEMBER_ROLE: &str = "13";
    const MODERATORS: &str = "14";
    /// A role called "admin" that is not the one under the owner's.
    const NAMED_ADMIN: &str = "15";

    /// Where the role called "admin" stands: just below `stewards`.
    const NAMED_ADMIN_PLACE: RolePlace = RolePlace::at(ADMIN_PLACE.position() + 1);
    const MODERATORS_PLACE: RolePlace = RolePlace::at(ADMIN_PLACE.position() + 2);

    /// The server's roles in their order: the one under the owner's is called "stewards", another below it "admin", then
    /// "moderators", which holds `MANAGE_MEMBERS` and `SHARE_SCREEN`.
    fn roles() -> Vec<RoleEntry> {
        vec![
            role_entry(OWNER_ROLE, "owner", RolePlace::OWNER, &Grant::ALL),
            role_entry(ADMIN_ROLE, "stewards", ADMIN_PLACE, &Grant::ALL),
            role_entry(NAMED_ADMIN, "admin", NAMED_ADMIN_PLACE, &[Grant::ManageMembers]),
            role_entry(MODERATORS, "moderators", MODERATORS_PLACE, &[Grant::ManageMembers, Grant::ShareScreen]),
            default_role_entry(MEMBER_ROLE, "member", RolePlace::BOTTOM, &[Grant::StartVoice]),
        ]
    }

    /// Ada who owns the server, grace holding `stewards`, Cy holding "admin" and "moderators", and Bo; listed as Bo, the
    /// asker, would see it.
    fn server() -> Vec<MemberEntry> {
        let every = Grant::ALL;
        vec![
            entry("1", "Bo", RolePlace::BOTTOM, &[MEMBER_ROLE], &[Grant::StartVoice]),
            entry("2", "grace", ADMIN_PLACE, &[ADMIN_ROLE, MEMBER_ROLE], &every),
            entry("3", "Ada", RolePlace::OWNER, &[OWNER_ROLE, MEMBER_ROLE], &every),
            entry("4", "Cy", NAMED_ADMIN_PLACE, &[NAMED_ADMIN, MODERATORS, MEMBER_ROLE], &[Grant::ManageMembers]),
        ]
    }

    /// The list as the member with the username `who` sees it.
    fn listed(entries: Vec<MemberEntry>, who: &str) -> Vec<ListedMember> {
        let me = by_handle(&entries, who);
        super::listed(entries, me.as_ref())
    }

    /// The server's roles as the place reads them.
    fn role_list(entries: Vec<RoleEntry>) -> RoleList {
        let mut list = RoleList::default();
        list.read(Ok(entries));
        list
    }

    fn by_handle(entries: &[MemberEntry], who: &str) -> Option<MemberId> {
        entries
            .iter()
            .find(|entry| entry.handle.eq_ignore_ascii_case(who))
            .and_then(|entry| MemberId::parse(&entry.user_id))
    }

    fn give(id: &str, name: &str) -> MemberAction {
        MemberAction::GiveRole(harness::role_id(id), name.into())
    }

    fn take(id: &str, name: &str) -> MemberAction {
        MemberAction::TakeRole(harness::role_id(id), name.into())
    }

    /// The roles' Roles menu on `name`'s row, as someone at `asker` holding `grants` sees it: each role offered and
    /// whether it is ticked.
    fn menu(members: &[ListedMember], name: &str, asker: RolePlace, grants: Grants) -> Vec<(String, bool)> {
        let member = members.iter().find(|member| member.name == name).expect("listed");
        member
            .roles(&role_list(roles()), asker, grants)
            .into_iter()
            .map(|offered| (offered.name, offered.held))
            .collect()
    }

    #[test]
    fn every_role_below_the_asker_whose_words_they_hold_is_offered_ticked_where_held() {
        let members = listed(server(), "ada");
        let ticked = |name: &str, held| (name.to_string(), held);
        assert_eq!(
            menu(&members, "Cy", RolePlace::OWNER, every()),
            [ticked("stewards", false), ticked("admin", true), ticked("moderators", true)],
            "every role the owner may give, not only the highest; ticked where held"
        );
        assert_eq!(
            menu(&members, "Bo", ADMIN_PLACE, every()),
            [ticked("admin", false), ticked("moderators", false)],
            "only the roles below the asker"
        );
        let moderator = [Grant::ManageRoles, Grant::ManageMembers].into_iter().collect();
        assert_eq!(
            menu(&members, "Bo", ADMIN_PLACE, moderator),
            [ticked("admin", false)],
            "not a role holding a word the asker lacks"
        );
        let members_only = [Grant::ManageMembers].into_iter().collect();
        assert!(menu(&members, "Bo", RolePlace::OWNER, members_only).is_empty(), "giving a role is MANAGE_ROLES'");
        let unlisted = super::listed(server(), by_handle(&server(), "ada").as_ref());
        assert!(
            unlisted.iter().all(|member| member.roles(&RoleList::default(), RolePlace::OWNER, every()).is_empty()),
            "no role is offered where the server lists none"
        );
    }

    #[test]
    fn the_owner_comes_first_then_you_then_everyone_by_name() {
        let names: Vec<String> = listed(server(), "bo").into_iter().map(|member| member.name).collect();
        assert_eq!(names, ["Ada", "Bo", "Cy", "grace"]);
        let places: Vec<RolePlace> = listed(server(), "bo").into_iter().map(|member| member.place).collect();
        assert_eq!(places, [RolePlace::OWNER, RolePlace::BOTTOM, NAMED_ADMIN_PLACE, ADMIN_PLACE]);
        let list = role_list(roles());
        let captions: Vec<String> =
            listed(server(), "bo").iter().map(|member| member.caption(&list).in_english()).collect();
        assert_eq!(
            captions,
            ["@Ada · owner", "@Bo · member · you", "@Cy · admin, moderators", "@grace · stewards"],
            "a row names every role held, highest first, the default one only where none else is held"
        );
        let cy = listed(server(), "bo").into_iter().find(|member| member.name == "Cy").expect("listed");
        assert_eq!(cy.roles_said(&list), Some(Sentence::Listed(vec!["admin".into(), "moderators".into()])));
        let unknown = role_list(roles().into_iter().filter(|role| role.role_id != MODERATORS).collect());
        assert_eq!(cy.caption(&unknown).in_english(), "@Cy · admin", "a role the list does not hold is not named");
    }

    #[test]
    fn a_row_s_more_menu_offers_only_what_the_asker_s_place_and_words_allow() {
        use MemberAction::{Ban, Kick, MakeOwner, ResetPassword};
        let more = |asker: RolePlace, as_who: &str, grants: Grants| -> Vec<(String, Vec<MemberAction>)> {
            listed(server(), as_who)
                .into_iter()
                .map(|member| (member.name.clone(), member.more(asker, grants)))
                .collect()
        };
        assert_eq!(
            more(RolePlace::OWNER, "ada", every()),
            [
                ("Ada".to_string(), vec![]),
                ("Bo".into(), vec![ResetPassword, MakeOwner, Kick, Ban]),
                ("Cy".into(), vec![ResetPassword, MakeOwner, Kick, Ban]),
                ("grace".into(), vec![ResetPassword, MakeOwner, Kick, Ban]),
            ],
            "the owner acts on everyone else"
        );
        assert_eq!(
            more(ADMIN_PLACE, "grace", every()),
            [
                ("Ada".to_string(), vec![]),
                ("grace".into(), vec![]),
                ("Bo".into(), vec![ResetPassword, Kick, Ban]),
                ("Cy".into(), vec![ResetPassword, Kick, Ban]),
            ],
            "below the asker only, and never an owner made"
        );
        assert_eq!(
            more(NAMED_ADMIN_PLACE, "cy", [Grant::ManageRoles].into_iter().collect())[1..],
            [("Cy".to_string(), vec![]), ("Bo".into(), vec![]), ("grace".into(), vec![])],
            "kicking, banning and resetting are MANAGE_MEMBERS'"
        );
    }

    #[test]
    fn a_banned_member_is_offered_unban_alone_and_listed_apart_and_the_search_finds_names_and_usernames() {
        let mut entries = server();
        let mut cy = entry("5", "Cyril", RolePlace::BOTTOM, &[MEMBER_ROLE], &[]);
        cy.handle = "cy".into();
        cy.banned = true;
        entries.push(cy);
        let members = listed(entries, "ada");
        let cyril = members.iter().find(|member| member.name == "Cyril").expect("listed");
        assert_eq!(cyril.caption(&role_list(roles())), Sentence::BannedHandle("cy".into()));
        let offered = cyril.roles(&role_list(roles()), RolePlace::OWNER, every());
        assert!(offered.is_empty() && cyril.more(RolePlace::OWNER, every()).is_empty());
        assert_eq!(cyril.unban(RolePlace::OWNER, every()), Some(MemberAction::Unban));
        assert_eq!(cyril.unban(RolePlace::OWNER, [Grant::ManageRoles].into_iter().collect()), None);
        assert_eq!(counts(&members), (4, 1));
        let names = |finding: &Finding| -> Vec<String> {
            finding.shown(&members).into_iter().map(|member| member.name.clone()).collect()
        };
        let mut finding = Finding::default();
        assert_eq!(names(&finding), ["Ada", "Bo", "Cy", "grace"], "the banned are apart");
        finding.update(Message::Tab(MemberTab::Banned));
        assert_eq!(names(&finding), ["Cyril"]);
        finding.update(Message::Tab(MemberTab::Members));
        finding.update(Message::Search(" GR ".into()));
        assert_eq!(names(&finding), ["grace"], "by name, whatever its case, trimmed");
        finding.update(Message::Search("zz".into()));
        assert!(names(&finding).is_empty());
        assert_eq!(finding.none_said(), Sentence::NoOneMatches("zz".into()));
        finding.update(Message::Tab(MemberTab::Banned));
        finding.update(Message::Search("cy".into()));
        assert_eq!(names(&finding), ["Cyril"], "by username");
        finding.update(Message::ClearSearch);
        assert_eq!(finding.query(), "");
        assert_eq!(finding.none_said(), Sentence::Phrase(Phrase::NoOneBanned));
    }

    #[test]
    fn a_row_s_menu_opens_closes_and_goes_with_a_change_of_list_or_search() {
        let mut finding = Finding::default();
        let bo = harness::member("1");
        finding.update(Message::Menu(bo.clone(), MemberMenu::Roles));
        assert_eq!(finding.menu(), Some((&bo, MemberMenu::Roles)));
        finding.update(Message::Menu(bo.clone(), MemberMenu::More));
        assert_eq!(finding.menu(), Some((&bo, MemberMenu::More)), "the other menu takes its place");
        finding.update(Message::Menu(bo.clone(), MemberMenu::More));
        assert_eq!(finding.menu(), None, "pressed again, it closes");
        finding.update(Message::Menu(bo, MemberMenu::Roles));
        finding.update(Message::Search("b".into()));
        assert_eq!(finding.menu(), None);
    }

    /// Ada, the account signed in here, holding every word (and owning the server when `owner`, else holding
    /// `stewards`), on the members page with grace (holding `stewards` too) and Bo listed.
    fn on_members(clock: &Clock, owner: bool) -> (Window, PlaceId) {
        let at = if owner { RolePlace::OWNER } else { ADMIN_PLACE };
        let (mut window, owls_id) = crate::harness::placed_holding(clock, at, Grant::ALL.into_iter().collect());
        let todo = tell(&mut window, clock, owls_id, place::Message::Page(place::PageMessage::Settings(Page::Members)));
        assert!(matches!(&todo[..], [Ask::At(at, place::Ask::ListMembers)] if *at == owls_id), "{todo:?}");
        let every = Grant::ALL;
        let ada_role = if owner { OWNER_ROLE } else { ADMIN_ROLE };
        let listed = vec![
            entry("1", ADA, at, &[ada_role, MEMBER_ROLE], &every),
            entry("2", "grace", ADMIN_PLACE, &[ADMIN_ROLE, MEMBER_ROLE], &every),
            entry("3", "Bo", RolePlace::BOTTOM, &[MEMBER_ROLE], &[Grant::StartVoice]),
        ];
        let roles = vec![
            role_entry(OWNER_ROLE, "owner", RolePlace::OWNER, &Grant::ALL),
            role_entry(ADMIN_ROLE, "stewards", ADMIN_PLACE, &Grant::ALL),
            role_entry(MODERATORS, "moderators", MODERATORS_PLACE, &[Grant::ManageMembers]),
            default_role_entry(MEMBER_ROLE, "member", RolePlace::BOTTOM, &[Grant::StartVoice]),
        ];
        tell(&mut window, clock, owls_id, place::Message::Server(place::ServerMessage::RolesListed(Ok(roles))));
        tell(&mut window, clock, owls_id, place::Message::Page(place::PageMessage::MembersListed(Ok(listed))));
        (window, owls_id)
    }

    fn member(window: &mut Window, clock: &Clock, owls_id: PlaceId, action: MemberAction, user_id: &str) -> Vec<Ask> {
        window.update(WindowMessage::Member(owls_id, action, harness::member(user_id)), clock.at())
    }

    /// A server that passed this connection by says so before its state whole, which leaves out the people: they are
    /// read again, as for a connection that dropped events itself.
    #[test]
    fn a_connection_the_server_rewound_reads_the_people_again() {
        use proto::chatkord_v1::{Event, Rewound, event};
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = on_members(&clock, false);
        let rewound = Event { cursor: Vec::new(), kind: Some(event::Kind::Rewound(Rewound {})) };
        let heard = place::Message::heard(&rewound).expect("a rewind means something here");
        let todo = tell(&mut window, &clock, owls_id, heard);
        assert!(
            todo.iter().any(|done| matches!(done, Ask::At(_, ask) if sends(ask, wire::ListPeople { after: None }))),
            "{todo:?}"
        );
    }

    /// A connection that dropped events is sent the rest whole, but never the channels, which are told by change, nor
    /// the members: the list and the page that shows them ask again, and the people every member sees are read again.
    #[test]
    fn a_connection_behind_asks_the_list_and_the_members_again_only_where_they_show() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = on_members(&clock, false);
        let todo = tell(&mut window, &clock, owls_id, place::Message::Session(place::SessionMessage::Behind));
        assert!(
            matches!(
                &todo[..],
                [
                    Ask::At(_, ask1),
                    Ask::At(_, ask2),
                    Ask::At(at, place::Ask::ListMembers),
                    Ask::At(_, ask3),
                    Ask::At(_, ask4),
                    Ask::At(_, ask5),
                    Ask::At(_, ask6),
                ] if crate::harness::is(ask1, crate::harness::Op::ChannelsList) && crate::harness::is(ask2, crate::harness::Op::History) && crate::harness::is(ask3, crate::harness::Op::ListPeople) && crate::harness::is(ask4, crate::harness::Op::ListRoles) && crate::harness::is(ask5, crate::harness::Op::ListEmoji) && crate::harness::is(ask6, crate::harness::Op::GetNotify) && (*at == owls_id
            )),
            "{todo:?}"
        );
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::CloseSettings));
        let todo = tell(&mut window, &clock, owls_id, place::Message::Session(place::SessionMessage::Behind));
        assert!(
            matches!(
                &todo[..],
                [
                    Ask::At(_, ask1),
                    Ask::At(_, ask2),
                    Ask::At(_, ask3),
                    Ask::At(_, ask4),
                    Ask::At(_, ask5),
                    Ask::At(_, ask6)
                ]
             if crate::harness::is(ask1, crate::harness::Op::ChannelsList) && crate::harness::is(ask2, crate::harness::Op::History) && crate::harness::is(ask3, crate::harness::Op::ListPeople) && crate::harness::is(ask4, crate::harness::Op::ListRoles) && crate::harness::is(ask5, crate::harness::Op::ListEmoji) && crate::harness::is(ask6, crate::harness::Op::GetNotify)),
            "{todo:?}"
        );
    }

    #[test]
    fn the_members_page_lists_everyone_and_offers_each_row_what_the_server_allows() {
        let clock = Clock(Instant::now());
        let (window, _) = on_members(&clock, true);
        let Some(Loaded::Ready(members)) = window.open().unwrap().settings().map(crate::ServerSettings::members) else {
            panic!("not listed")
        };
        let (at, list) = (window.open().unwrap().role_place(), window.open().unwrap().role_list());
        let rows: Vec<(&str, usize)> =
            members.iter().map(|member| (member.name.as_str(), member.actions(list, at, every()).len())).collect();
        assert_eq!(
            rows,
            [(ADA, 0), ("Bo", 6), ("grace", 6)],
            "the server owner, then by name; nothing on your own row"
        );
        let (window, _) = on_members(&clock, false);
        let Some(Loaded::Ready(members)) = window.open().unwrap().settings().map(crate::ServerSettings::members) else {
            panic!("not listed")
        };
        let (at, list) = (window.open().unwrap().role_place(), window.open().unwrap().role_list());
        let rows: Vec<(&str, Vec<MemberAction>)> =
            members.iter().map(|member| (member.name.as_str(), member.actions(list, at, every()))).collect();
        assert_eq!(
            rows,
            [
                (ADA, vec![]),
                (
                    "Bo",
                    vec![
                        give(MODERATORS, "moderators"),
                        MemberAction::ResetPassword,
                        MemberAction::Kick,
                        MemberAction::Ban
                    ]
                ),
                ("grace", vec![])
            ],
            "an admin who is not the server's owner changes only members placed below them"
        );
    }

    #[test]
    fn a_given_role_shows_in_the_list_and_a_refusal_is_the_page_s_notice_never_a_toast() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = on_members(&clock, false);
        assert!(member(&mut window, &clock, owls_id, MemberAction::MakeOwner, "3").is_empty(), "not offered");
        assert!(member(&mut window, &clock, owls_id, MemberAction::ResetPassword, "2").is_empty(), "not offered");
        assert!(member(&mut window, &clock, owls_id, give(ADMIN_ROLE, "stewards"), "3").is_empty(), "not below Ada");
        let todo = member(&mut window, &clock, owls_id, give(MODERATORS, "moderators"), "3");
        assert!(
            matches!(&todo[..], [Ask::At(_, ask)]
                if sends(ask, wire::SetRole { user_id: "3".into(), role_id: MODERATORS.into(), held: true })),
            "{todo:?}"
        );
        let set = |answer| place::Message::Page(place::PageMessage::RoleSet { name: "Bo".into(), answer });
        let todo = tell(&mut window, &clock, owls_id, set(Ok(())));
        assert_eq!(window.toast().map(|toast| toast.said().in_english()).as_deref(), None, "the list shows the role");
        assert!(todo.iter().any(|done| matches!(done, Ask::At(_, place::Ask::ListMembers))), "{todo:?}");
        tell(&mut window, &clock, owls_id, set(Err(refused(ErrorCode::NotAuthorized))));
        assert_eq!(
            window.toast().map(|toast| toast.said().in_english()).as_deref(),
            None,
            "a refusal is not said in a toast"
        );
        let notices = |window: &Window| -> Vec<String> {
            window
                .open()
                .and_then(Place::settings)
                .map_or_else(Vec::new, |page| page.notices().into_iter().map(|one| one.said.in_english()).collect())
        };
        assert_eq!(notices(&window), ["You can’t change Bo’s roles."], "it is said on the page it was asked on");
        tell(&mut window, &clock, owls_id, set(Err(refused(ErrorCode::NotAuthorized))));
        assert_eq!(notices(&window).len(), 1, "said again, shown once");
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::DismissPageNotice(Subject::Roles)));
        assert!(notices(&window).is_empty(), "put away by a press");
        tell(&mut window, &clock, owls_id, set(Err(refused(ErrorCode::NotAuthorized))));
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::Settings(Page::Invites)));
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::Settings(Page::Members)));
        assert!(notices(&window).is_empty(), "a page opened anew says nothing it said before");
        let todo = tell(&mut window, &clock, owls_id, set(Err(refused(ErrorCode::NotFound))));
        assert!(
            window.toast().map(|toast| toast.said().in_english()).as_deref().is_none() && notices(&window).is_empty(),
            "someone gone leaves the list quietly"
        );
        assert!(todo.iter().any(|done| matches!(done, Ask::At(_, place::Ask::ListMembers))), "{todo:?}");
        let taken = place::Message::Page(place::PageMessage::RoleSet { name: "Bo".into(), answer: Ok(()) });
        tell(&mut window, &clock, owls_id, taken);
        assert_eq!(
            window.toast().map(|toast| toast.said().in_english()).as_deref(),
            None,
            "taken, the list shows it too"
        );
    }

    fn resetting(window: &Window) -> &Resetting {
        match window.dialog().map(Dialog::asking) {
            Some(Asking::Reset(resetting)) => resetting,
            other => panic!("not resetting: {other:?}"),
        }
    }

    #[test]
    fn a_reset_is_asked_first_and_its_temporary_password_is_shown_once_and_copied_on_its_button() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = on_members(&clock, true);
        member(&mut window, &clock, owls_id, MemberAction::ResetPassword, "2");
        assert_eq!(resetting(&window).name(), "grace");
        window.update(WindowMessage::Dialog(DialogMessage::Outside), clock.at());
        assert!(window.dialog().is_none(), "nothing asked yet: a press outside closes it");
        member(&mut window, &clock, owls_id, MemberAction::ResetPassword, "2");
        let todo = window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert!(
            matches!(&todo[..], [Ask::At(_, ask)] if sends(ask, wire::ResetPassword { user_id: "2".into() })),
            "{todo:?}"
        );
        for away in [DialogMessage::Cancel, DialogMessage::Outside] {
            window.update(WindowMessage::Dialog(away), clock.at());
            assert!(resetting(&window).asking(), "a reset under way keeps its dialog");
        }
        tell(
            &mut window,
            &clock,
            owls_id,
            place::Message::Page(place::PageMessage::PasswordReset(Err(refused(ErrorCode::NotAuthorized)))),
        );
        assert_eq!(resetting(&window).refused(), Some(Refused::NotAllowed));
        window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        let temporary = Secret::new("river-copper-lamp-42".into());
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::PasswordReset(Ok(temporary))));
        assert_eq!(resetting(&window).temporary().map(Secret::expose), Some("river-copper-lamp-42"));
        window.update(WindowMessage::Dialog(DialogMessage::Outside), clock.at());
        assert!(window.dialog().is_some(), "shown once: a press outside keeps it");
        window.update(WindowMessage::Dialog(DialogMessage::CopyTemporary), clock.at());
        assert_eq!(window.copying(), Some("river-copper-lamp-42"));
        window.update(WindowMessage::Copied(Ok(())), clock.at());
        assert!(resetting(&window).copied());
        assert_eq!((window.copying(), window.toast().map(|toast| toast.said().in_english()).as_deref()), (None, None));
        window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert!(window.dialog().is_none(), "Done closes it");
    }

    fn transferring(window: &Window) -> &Transferring {
        match window.dialog().map(Dialog::asking) {
            Some(Asking::Transfer(transferring)) => transferring,
            other => panic!("not transferring: {other:?}"),
        }
    }

    #[test]
    fn making_an_owner_is_proved_by_the_password_and_a_wrong_one_is_cleared() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = on_members(&clock, true);
        member(&mut window, &clock, owls_id, MemberAction::MakeOwner, "2");
        assert_eq!(transferring(&window).name(), "grace");
        assert!(window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at()).is_empty(), "no password yet");
        window.update(WindowMessage::Dialog(DialogMessage::Password(Secret::new("wrong one".into()))), clock.at());
        window.update(WindowMessage::Dialog(DialogMessage::Outside), clock.at());
        assert!(window.dialog().is_some(), "a typed password keeps it");
        let todo = window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert!(
            matches!(&todo[..], [Ask::At(_, ask)]
                if sends(ask, wire::TransferOwnership { user_id: "2".into(), current: "wrong one".into() })),
            "{todo:?}"
        );
        tell(
            &mut window,
            &clock,
            owls_id,
            place::Message::Page(place::PageMessage::Transferred(Err(refused(ErrorCode::Unauthenticated)))),
        );
        let proof = transferring(&window).proof();
        assert!(proof.password().is_empty());
        assert_eq!(proof.refused(), Some(Refused::WrongPassword));
        window.update(WindowMessage::Dialog(DialogMessage::Password(Secret::new("evening games".into()))), clock.at());
        window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        let todo = tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::Transferred(Ok(()))));
        assert!(window.dialog().is_none());
        assert_eq!(
            window.toast().map(|toast| toast.said().in_english()).as_deref(),
            None,
            "the list shows who owns it now"
        );
        assert!(todo.iter().any(|done| matches!(done, Ask::At(_, place::Ask::ListMembers))), "{todo:?}");
    }

    #[test]
    fn an_owner_leaving_chooses_a_new_owner_on_the_members_page() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = holding(&clock, Grant::ALL.into_iter().collect());
        let every = Grant::ALL.into_iter().collect();
        tell(
            &mut window,
            &clock,
            owls_id,
            place::Message::Session(place::SessionMessage::Standing {
                place: proto::rules::Place::OWNER,
                grants: every,
            }),
        );
        window.update(WindowMessage::Pick(owls_id, Pick::Leave), clock.at());
        assert!(matches!(window.dialog().map(Dialog::asking), Some(Asking::OwnerLeaves)));
        let todo = window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert!(window.dialog().is_none());
        assert_eq!(window.open().unwrap().settings().map(crate::ServerSettings::page), Some(Page::Members));
        assert!(matches!(&todo[..], [Ask::At(_, place::Ask::ListMembers)]), "{todo:?}");
    }

    #[test]
    fn someone_elses_rename_reaches_the_members_page() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = on_members(&clock, true);
        let renamed = place::Message::Server(place::ServerMessage::Renamed {
            user_id: harness::member("3"),
            name: "Bo Diddley".into(),
        });
        tell(&mut window, &clock, owls_id, renamed);
        let Loaded::Ready(listed) = window.open().unwrap().settings().unwrap().members() else { panic!("listed") };
        assert!(listed.iter().any(|member| member.name == "Bo Diddley"), "{listed:?}");
    }

    fn finding(window: &Window) -> &Finding {
        window.open().and_then(Place::settings).map(crate::ServerSettings::finding).expect("the members page")
    }

    fn listed_now(window: &Window) -> &[ListedMember] {
        match window.open().and_then(Place::settings).map(crate::ServerSettings::members) {
            Some(Loaded::Ready(members)) => members,
            other => panic!("not listed: {other:?}"),
        }
    }

    #[test]
    fn a_tick_in_a_row_s_roles_menu_gives_or_takes_at_once_and_leaves_the_menu_open() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = on_members(&clock, true);
        let page = |message| place::Message::Page(place::PageMessage::Members(message));
        tell(&mut window, &clock, owls_id, page(Message::Menu(harness::member("3"), MemberMenu::Roles)));
        let todo = member(&mut window, &clock, owls_id, give(MODERATORS, "moderators"), "3");
        assert!(
            matches!(&todo[..], [Ask::At(_, ask)]
                if matches!(ask.op(), Some(crate::harness::Body::SetRole(set)) if set.held && set.role_id == MODERATORS)),
            "{todo:?}"
        );
        assert!(finding(&window).menu().is_some(), "the menu stays open for the next tick");
        let bo = listed_now(&window).iter().find(|member| member.name == "Bo").expect("listed").clone();
        let offered = bo.roles(window.open().unwrap().role_list(), RolePlace::OWNER, every());
        assert!(offered.iter().any(|role| role.name == "moderators" && role.held), "ticked at once: {offered:?}");
        let todo = member(&mut window, &clock, owls_id, take(MODERATORS, "moderators"), "3");
        assert!(
            matches!(&todo[..], [Ask::At(_, ask)] if matches!(ask.op(), Some(crate::harness::Body::SetRole(set)) if !set.held)),
            "{todo:?}"
        );
        member(&mut window, &clock, owls_id, MemberAction::ResetPassword, "3");
        assert!(finding(&window).menu().is_none(), "an entry that asks first closes the menu");
    }

    fn kicking(window: &Window) -> &Kicking {
        match window.dialog().map(Dialog::asking) {
            Some(Asking::Kick(kicking)) => kicking,
            other => panic!("not kicking: {other:?}"),
        }
    }

    #[test]
    fn a_kick_is_asked_first_said_in_a_toast_once_done_and_a_refusal_is_said_in_its_dialog() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = on_members(&clock, false);
        assert!(member(&mut window, &clock, owls_id, MemberAction::Kick, "2").is_empty(), "grace is not below Ada");
        assert!(window.dialog().is_none());
        assert!(member(&mut window, &clock, owls_id, MemberAction::Kick, "3").is_empty(), "nothing asked yet");
        assert_eq!(kicking(&window).name(), "Bo");
        let todo = window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert!(matches!(&todo[..], [Ask::At(_, ask)] if sends(ask, wire::Kick { user_id: "3".into() })), "{todo:?}");
        assert!(window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at()).is_empty(), "asked once");
        window.update(WindowMessage::Dialog(DialogMessage::Cancel), clock.at());
        assert!(kicking(&window).asking(), "a kick under way keeps its dialog");
        let kicked = |answer| place::Message::Page(place::PageMessage::Kicked(answer));
        tell(&mut window, &clock, owls_id, kicked(Err(refused(ErrorCode::NotAuthorized))));
        assert_eq!(kicking(&window).refused(), Some(Refused::NotAllowed));
        window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        let todo = tell(&mut window, &clock, owls_id, kicked(Ok(())));
        assert!(window.dialog().is_none());
        assert_eq!(
            window.toast().map(|toast| toast.said().in_english()).as_deref(),
            Some("Bo was signed out everywhere.")
        );
        assert!(todo.iter().any(|done| matches!(done, Ask::At(_, place::Ask::ListMembers))), "{todo:?}");
    }

    fn banning(window: &Window) -> &Banning {
        match window.dialog().map(Dialog::asking) {
            Some(Asking::Ban(banning)) => banning,
            other => panic!("not banning: {other:?}"),
        }
    }

    #[test]
    fn a_ban_and_an_unban_are_proved_by_the_password_and_a_wrong_one_is_cleared() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = on_members(&clock, true);
        member(&mut window, &clock, owls_id, MemberAction::Ban, "3");
        assert_eq!((banning(&window).name(), banning(&window).ask()), ("Bo", BanAsk::Ban));
        assert!(window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at()).is_empty(), "no password yet");
        window.update(WindowMessage::Dialog(DialogMessage::Password(Secret::new("wrong one".into()))), clock.at());
        window.update(WindowMessage::Dialog(DialogMessage::Outside), clock.at());
        assert!(window.dialog().is_some(), "a typed password keeps it");
        let todo = window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert!(
            matches!(&todo[..], [Ask::At(_, ask)]
                if sends(ask, wire::SetBanned { user_id: "3".into(), banned: true, current: "wrong one".into() })),
            "{todo:?}"
        );
        assert!(!format!("{todo:?}").contains("wrong one"), "the password is never printed");
        let set = |answer| place::Message::Page(place::PageMessage::BanSet(answer));
        tell(&mut window, &clock, owls_id, set(Err(refused(ErrorCode::Unauthenticated))));
        let proof = banning(&window).proof();
        assert!(proof.password().is_empty());
        assert_eq!(proof.refused(), Some(Refused::WrongPassword));
        tell(&mut window, &clock, owls_id, set(Err(refused(ErrorCode::RateLimited))));
        window.update(WindowMessage::Dialog(DialogMessage::Password(Secret::new("evening games".into()))), clock.at());
        window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        let todo = tell(&mut window, &clock, owls_id, set(Ok(())));
        assert!(window.dialog().is_none());
        assert!(todo.iter().any(|done| matches!(done, Ask::At(_, place::Ask::ListMembers))), "{todo:?}");
        let mut listed = vec![
            entry("1", ADA, RolePlace::OWNER, &[OWNER_ROLE, MEMBER_ROLE], &Grant::ALL),
            entry("3", "Bo", RolePlace::BOTTOM, &[MEMBER_ROLE], &[]),
        ];
        listed[1].banned = true;
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::MembersListed(Ok(listed))));
        assert!(member(&mut window, &clock, owls_id, MemberAction::Ban, "3").is_empty() && window.dialog().is_none());
        member(&mut window, &clock, owls_id, MemberAction::Unban, "3");
        assert_eq!(banning(&window).ask(), BanAsk::Unban);
        window.update(WindowMessage::Dialog(DialogMessage::Password(Secret::new("evening games".into()))), clock.at());
        let todo = window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert!(
            matches!(&todo[..], [Ask::At(_, ask)] if matches!(ask.op(), Some(crate::harness::Body::SetBanned(set)) if !set.banned)),
            "{todo:?}"
        );
    }
}
