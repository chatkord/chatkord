//! Server settings › Channels: every category and channel in the list's order, the one chosen, and its name, its
//! category and who sees it or writes in it, read from the channel's overwrites and changed as the server rules them.
//! Access comes first (a Private switch with who can see it, and for a text channel a Read-only switch with who can
//! write); the words one by one fold away under Advanced permissions. A change shows at once and is put back when the
//! server refuses it; a choice that would leave the asker unable to see the channel is never offered, as the server
//! would refuse it.

use crate::loaded::Loaded;
use crate::name_field::{self, NameField};
use crate::order::Moved;
use crate::people::colour_of;
use crate::place::ops;
use crate::place::{self, Access, ChannelList, Directory, ListEntry, ListKey, Whom};
use crate::roles::{RoleKind, RoleList, WORD_GROUPS};
use proto::rules::{Colour, Grant, Grants, Kind, Leaning, Overwrite, Place, resolve};
use std::collections::HashMap;
use terms::{CategoryId, ChannelId, LinkError, MemberId, RoleId};
use words::{Phrase, Sentence};

/// One role's or member's choice for one word in one channel; a word inherited is no mark.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Mark {
    /// Whose choice it is.
    pub whom: Whom,
    /// The word.
    pub word: Grant,
    /// Allowed or denied.
    pub state: Overwrite,
}

impl Mark {
    /// The mark `told` lists; none for one naming no one, a word this build does not know or no state, a server not
    /// keeping the protocol.
    pub(crate) fn read(told: &proto::chatkord_v1::ChannelOverwrite) -> Option<Mark> {
        use proto::chatkord_v1::channel_overwrite::Whom as Told;
        let whom = match told.whom.as_ref()? {
            Told::RoleId(role) => Whom::Role(RoleId::parse(role)?),
            Told::UserId(user) => Whom::Member(MemberId::parse(user)?),
        };
        let word = Grant::of_word(&told.word)?;
        let state = Overwrite::asked(told.state).ok()??;
        Some(Mark { whom, word, state })
    }
}

/// What `whom` chose for `word` in `marks`; none where they inherit.
fn state(marks: &[Mark], whom: &Whom, word: Grant) -> Option<Overwrite> {
    marks.iter().find(|mark| mark.whom == *whom && mark.word == word).map(|mark| mark.state)
}

/// `marks` with `whom`'s choice for `word` set to `state`, none to inherit.
fn with(marks: &[Mark], whom: &Whom, word: Grant, state: Option<Overwrite>) -> Vec<Mark> {
    let kept = marks.iter().filter(|mark| !(mark.whom == *whom && mark.word == word)).cloned();
    let set = state.map(|state| Mark { whom: whom.clone(), word, state });
    kept.chain(set).collect()
}

/// The words a channel of `kind` takes, in the order its page shows them.
pub const fn channel_words(kind: Kind) -> &'static [Grant] {
    match kind {
        Kind::Text => &[Grant::ChannelRead, Grant::SendMessage, Grant::ManageMessages],
        Kind::Voice => &[Grant::ChannelRead, Grant::StartVoice, Grant::ShareScreen, Grant::ShareCamera],
    }
}

/// Who asks: what the lockout rule and the place rule read of this account.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Asker<'a> {
    /// This account's id; none until it signs in.
    pub(crate) me: Option<&'a MemberId>,
    /// The roles it holds, the default one among them, as the members list says.
    pub(crate) roles: &'a [RoleId],
    /// Where it stands in the roles' order.
    pub(crate) place: Place,
    /// The words its roles give it, before any channel's overwrites.
    pub(crate) base: Grants,
}

impl Asker<'_> {
    /// Whether it owns the server, which every word in every channel follows.
    fn owner(&self) -> bool {
        self.place.is_owner()
    }
}

/// What the page and the create dialog read of the place: its roles, its members and who asks.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Context<'a> {
    pub(crate) roles: &'a RoleList,
    pub(crate) directory: &'a Directory,
    pub(crate) asker: Asker<'a>,
}

impl Context<'_> {
    /// The channel's overwrites `marks` as the asker meets them, one layer each.
    fn leaning(&self, marks: &[Mark]) -> Leaning {
        let default = self.roles.default_role().map(|role| Whom::Role(role.id.clone()));
        let me = self.asker.me.map(|me| Whom::Member(me.clone()));
        let mut leaning = Leaning::default();
        for mark in marks {
            let layer = if Some(&mark.whom) == default.as_ref() {
                &mut leaning.default
            } else if Some(&mark.whom) == me.as_ref() {
                &mut leaning.member
            } else if matches!(&mark.whom, Whom::Role(role) if self.asker.roles.contains(role)) {
                &mut leaning.roles
            } else {
                continue;
            };
            *layer = layer.with(mark.word, mark.state);
        }
        leaning
    }

    /// Whether the asker sees the channel under `marks`.
    fn sees(&self, marks: &[Mark]) -> bool {
        resolve(self.asker.owner(), self.asker.base, self.leaning(marks)).holds(Grant::ChannelRead)
    }

    /// Whether changing `now` to `after` would leave the asker, not the owner, unable to see the channel: the server
    /// refuses such a change.
    fn locks_out(&self, now: &[Mark], after: &[Mark]) -> bool {
        !self.asker.owner() && self.sees(now) && !self.sees(after)
    }

    /// Where a member stands in the roles' order, by the roles the members list says they hold.
    fn place_of(&self, user_id: &MemberId) -> Place {
        self.directory.person(user_id).map_or(Place::BOTTOM, |person| self.roles.place_of(&person.roles))
    }

    /// Whether the asker may not change `whom`'s choices: a role at or above its place, or a member there, other than
    /// itself; the default role never is.
    fn locked(&self, whom: &Whom) -> bool {
        match whom {
            Whom::Role(role) => self.roles.role(role).is_some_and(|role| match role.kind {
                RoleKind::Default => false,
                RoleKind::Owner | RoleKind::Made => !self.asker.place.outranks(role.place),
            }),
            Whom::Member(member) => Some(member) != self.asker.me && !self.asker.place.outranks(self.place_of(member)),
        }
    }

    /// How `whom` is named and shown: "You" for the asker.
    pub(crate) fn shown(&self, whom: &Whom) -> (Shown, Look) {
        match whom {
            Whom::Role(role) => {
                let role = self.roles.role(role);
                let name = role.map_or_else(String::new, |role| role.name.clone());
                (Shown::Name(name), Look::Role(role.and_then(|role| role.colour)))
            }
            Whom::Member(member) => {
                let person = self.directory.person(member);
                let look = Look::Person(
                    colour_of(member, person.and_then(|person| person.colour)),
                    person.and_then(|person| person.picture),
                );
                if Some(member) == self.asker.me {
                    (Shown::You, look)
                } else {
                    (Shown::Name(person.map_or_else(|| member.as_str().to_owned(), |person| person.name.clone())), look)
                }
            }
        }
    }

    /// The roles and members the asker may add to a list that does not hold them yet (`listed` says which it holds):
    /// the roles made below its place, in their order, then the members below it whose name holds `query`, by name.
    pub(crate) fn add_menu(&self, listed: impl Fn(&Whom) -> bool, query: &str) -> AddMenu {
        let place = self.asker.place;
        let roles: Vec<Offer> = self
            .roles
            .roles()
            .iter()
            .filter(|role| role.kind == RoleKind::Made && place.outranks(role.place))
            .map(|role| Whom::Role(role.id.clone()))
            .filter(|whom| !listed(whom))
            .map(|whom| self.offer(whom))
            .collect();
        let query = query.trim().to_lowercase();
        let mut members: Vec<Offer> = self
            .directory
            .people()
            .filter(|(user_id, person)| {
                Some(*user_id) != self.asker.me
                    && place.outranks(self.roles.place_of(&person.roles))
                    && (query.is_empty() || person.name.to_lowercase().contains(&query))
            })
            .map(|(user_id, _)| Whom::Member(user_id.clone()))
            .filter(|whom| !listed(whom))
            .map(|whom| self.offer(whom))
            .collect();
        members.sort_by_cached_key(|offer| match &offer.name {
            Shown::Name(name) => name.to_lowercase(),
            Shown::You => String::new(),
        });
        let said =
            if query.is_empty() { Sentence::Phrase(Phrase::EveryMemberListed) } else { Sentence::NoOneMatches(query) };
        let none = members.is_empty().then_some(said);
        AddMenu { roles, members, none }
    }

    fn offer(&self, whom: Whom) -> Offer {
        let (name, look) = self.shown(&whom);
        Offer { whom, name, look }
    }
}

/// How a role or member is named in a list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shown {
    /// This account: "You".
    You,
    /// By the name the server gave the role, or the member's name others see.
    Name(String),
}

/// How a role or member is marked beside its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// A role's colour dot; none for a role with no colour.
    Role(Option<Colour>),
    /// A member's avatar: their picture where they chose one, else their initials in their colour.
    Person(design::PersonColour, Option<proto::files::FileId>),
}

/// A role or member the add menu offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    /// Who it adds.
    pub whom: Whom,
    /// How it is named.
    pub name: Shown,
    /// How it is marked.
    pub look: Look,
}

/// The menu that adds a role or a member to a list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddMenu {
    /// The roles it offers, in their order.
    pub roles: Vec<Offer>,
    /// The members it offers, by name, as the search finds them.
    pub members: Vec<Offer>,
    /// What it says in place of members when it offers none.
    pub none: Option<Sentence>,
}

/// Whether a row of a who-list may be taken off it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Removal {
    /// Its × takes it off.
    Offered,
    /// It stays: this account in the create dialog, which the server lets in with the channel.
    Fixed,
    /// It stays: taking it off would leave this account unable to see the channel (said by
    /// [`Phrase::WouldNotSee`]).
    LocksOut,
}

/// A role or member let in by an access.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    /// Who it is.
    pub whom: Whom,
    /// How it is named.
    pub name: Shown,
    /// How it is marked.
    pub look: Look,
    /// Whether it may be taken off.
    pub removal: Removal,
}

/// A switch of who sees a channel or writes in it, and who it lets in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessView {
    /// Private or Read-only.
    pub access: Access,
    /// The switch.
    pub on: Switch,
    /// Who it lets in, in order: this account first where listed, then the roles in their order, then the members.
    pub listed: Vec<Listed>,
    /// The add menu, while it is open.
    pub adding: Option<AddMenu>,
}

/// A switch's state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Switch {
    /// Off.
    #[default]
    Off,
    /// On.
    On,
}

impl Switch {
    /// The other one.
    #[must_use]
    pub const fn toggled(self) -> Switch {
        match self {
            Switch::Off => Switch::On,
            Switch::On => Switch::Off,
        }
    }
}

/// Which add menu is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Adding {
    /// The one under an access's list.
    Access(Access),
    /// The one beside the targets of Advanced permissions.
    Advanced,
}

/// Which of the page's menus is open: one at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Opened {
    /// The Category field's.
    Category,
    /// An add menu.
    Adding(Adding),
}

/// Whether Advanced permissions is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fold {
    /// Folded away, as the page opens.
    #[default]
    Folded,
    /// Open.
    Open,
}

/// What a target chip is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetKind {
    /// The default role, first: "Everyone" beside its name.
    Everyone,
    /// Another role.
    Role,
    /// A member.
    Member,
}

/// One whose choices Advanced permissions shows: a chip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// Who it is.
    pub whom: Whom,
    /// How it is named.
    pub name: Shown,
    /// How it is marked.
    pub look: Look,
    /// What it is.
    pub kind: TargetKind,
    /// Whether it is the one whose words show.
    pub chosen: bool,
}

/// Why a choice of a word is not offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Off {
    /// The asker does not hold the word in this channel.
    Lacks,
    /// The target is at or above the asker's place.
    Locked,
    /// It would leave the asker unable to see the channel.
    LocksOut,
}

/// One choice of a word: deny, inherit or allow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    /// What it sets: allowed or denied, none to inherit.
    pub state: Option<Overwrite>,
    /// Why it is not offered; none where it is.
    pub off: Option<Off>,
}

/// A word's row under Advanced permissions, for the chosen target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WordRow {
    /// The word.
    pub word: Grant,
    /// The chosen target's choice now; none for inherit.
    pub now: Option<Overwrite>,
    /// Deny, Inherit and Allow, in that order.
    pub choices: [Choice; 3],
    /// A choice is held back because it would leave the asker unable to see the channel: the row says so.
    pub locks_out: bool,
}

/// Advanced permissions, as drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdvancedView {
    /// Folded or open.
    pub fold: Fold,
    /// The targets: the default role first, then the roles with a choice here in their order, then the members.
    pub targets: Vec<Target>,
    /// The add menu, while it is open.
    pub adding: Option<AddMenu>,
    /// Why the chosen target may not be changed: at or above the asker's place.
    pub locked: Option<Sentence>,
    /// Whether Remove takes every choice of the chosen target off the channel.
    pub removable: bool,
    /// Some word shown is one the asker lacks: "You can change only the words you hold."
    pub lacks: bool,
    /// The words, under their groups' headings, those the channel's kind takes.
    pub groups: Vec<(Phrase, Vec<WordRow>)>,
}

/// The chosen channel's editor, as drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorView<'a> {
    /// The channel, by id.
    pub channel: ChannelId,
    /// Its kind.
    pub kind: Kind,
    /// Its name's field.
    pub name: &'a NameField,
    /// The category it is in.
    pub category: CategoryId,
    /// Who sees it and who writes in it, with the advanced choices: none where the asker may not manage roles, or
    /// while its overwrites are read.
    pub access: Option<AccessViews>,
}

/// The access part of the editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessViews {
    /// Private, with who can see it.
    pub private: AccessView,
    /// Read-only, with who can write; a text channel's alone.
    pub read_only: Option<AccessView>,
    /// "You stay in it as long as you’re listed.": private, and the asker not the owner.
    pub stay: bool,
    /// The words one by one.
    pub advanced: AdvancedView,
}

/// Which of the page shows where the list and the chosen channel do not fit side by side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Step {
    /// The list.
    #[default]
    List,
    /// The chosen channel, opened from the list.
    Channel,
}

/// What the page shows, decided by its room and, where the list and the chosen channel do not fit side by side, by
/// [`Step`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shows {
    /// The list beside the chosen channel.
    Beside,
    /// The list alone.
    List,
    /// The chosen channel alone, with Back to the list in place of the page's heading.
    Channel,
}

/// What a person does on the page.
#[derive(Clone, Debug)]
pub enum Message {
    /// A channel in the list chosen: a name typed for the one chosen before is saved, and a narrow page opens it.
    Choose(ChannelId),
    /// A category in the list chosen in a channel's place: its name's field shows, and the way to delete it.
    ChooseCategory(CategoryId),
    /// Back to the list from a channel opened alone: a name typed is saved.
    Back,
    /// Something done in the name's field.
    Name(name_field::Message),
    /// The focus left the name's field: what is typed is saved.
    NameLeft,
    /// The Category field pressed: its menu opens, or closes while it is open.
    CategoryMenu,
    /// A category picked for the chosen channel: it goes last there; the field's menu closes.
    Category(CategoryId),
    /// An access's switch.
    Switch(Access),
    /// An add menu opened, or closed when it is open.
    Adding(Adding),
    /// The add menu's search, as typed.
    Query(String),
    /// A role or member added to an access's list: allowed its word.
    Allow(Access, Whom),
    /// A role or member taken off an access's list: its word inherited.
    Remove(Access, Whom),
    /// Advanced permissions opened, or folded.
    Advanced,
    /// A target chip chosen.
    Target(Whom),
    /// A role or member added to the targets, with no choice yet; it is chosen.
    AddTarget(Whom),
    /// A word's choice for the chosen target: allowed, denied, or none to inherit.
    Set(Grant, Option<Overwrite>),
    /// Remove under the chosen target: each of its choices here inherited.
    RemoveTarget,
}

/// What the page asks: of the server, or a move of the list.
#[derive(Debug)]
pub(crate) enum Asked {
    Server(place::Ask),
    Move(Moved<ListKey>),
}

/// The page, open.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChannelSettings {
    /// The channel chosen; none until there is one, and while a category is chosen in its place.
    chosen: Option<ChannelId>,
    /// The category chosen in a channel's place, and its name as it is being changed.
    category: Option<(CategoryId, NameField)>,
    step: Step,
    /// The chosen channel's name as it is being changed.
    name: Option<(ChannelId, NameField)>,
    fold: Fold,
    /// The target whose words show; none for the default role.
    target: Option<Whom>,
    /// Targets added with no choice yet.
    added: Vec<Whom>,
    menu: Option<Opened>,
    query: String,
    /// The chosen channel's marks before the changes shown at once, put back when the server refuses one.
    unchanged: Option<(ChannelId, Vec<Mark>)>,
    /// Changes asked and not yet answered.
    asking: usize,
}

/// The channel listed with `id`, its kind and its name.
fn listed_channel(list: &ChannelList, id: &ChannelId) -> Option<(Kind, String)> {
    match list.find(id)? {
        ListEntry::Text(channel) => Some((Kind::Text, channel.name.clone())),
        ListEntry::Voice(channel) => Some((Kind::Voice, channel.name.clone())),
        ListEntry::Category(_) => None,
    }
}

impl ChannelSettings {
    /// The page opened on `list`, its first channel chosen.
    pub(crate) fn new(list: &ChannelList) -> ChannelSettings {
        let mut page = ChannelSettings::default();
        page.follow(list);
        page
    }

    /// The channel chosen.
    pub fn chosen(&self) -> Option<&ChannelId> {
        self.chosen.as_ref()
    }

    /// The category chosen in a channel's place.
    pub fn chosen_category(&self) -> Option<&CategoryId> {
        self.category.as_ref().map(|(id, _)| id)
    }

    /// The chosen category's name's field.
    pub fn category_name(&self) -> Option<&NameField> {
        self.category.as_ref().map(|(_, field)| field)
    }

    /// Where a narrow page is: the list, or the chosen channel opened from it.
    pub fn step(&self) -> Step {
        self.step
    }

    /// Whether the Category field's menu is open.
    pub fn category_menu_open(&self) -> bool {
        self.menu == Some(Opened::Category)
    }

    /// The add menu's search, as typed.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The page follows the list: a choice no longer listed goes, the first channel taking its place, and the name's
    /// field follows a rename nobody is typing over.
    pub(crate) fn follow(&mut self, list: &ChannelList) {
        // A chosen category stays while it is listed, its field following a rename nobody is typing over; gone, a
        // channel takes its place.
        if let Some((category, field)) = self.category.as_mut() {
            if let Some(listed) = list.categories.iter().find(|listed| listed.id == *category) {
                field.follow(listed.name.clone());
                return;
            }
            self.category = None;
        }
        let kept = self.chosen.as_ref().is_some_and(|chosen| list.find(chosen).is_some());
        if !kept {
            let first = list.in_order().into_iter().find_map(|entry| match entry {
                ListEntry::Text(channel) => Some(channel.id.clone()),
                ListEntry::Voice(channel) => Some(channel.id.clone()),
                ListEntry::Category(_) => None,
            });
            self.choose(first);
        }
        let Some(chosen) = self.chosen.clone() else { return };
        let Some((_, name)) = listed_channel(list, &chosen) else { return };
        match self.name.as_mut().filter(|(id, _)| *id == chosen) {
            Some((_, field)) => field.follow(name),
            None => self.name = Some((chosen, NameField::required(name))),
        }
    }

    fn choose(&mut self, channel: Option<ChannelId>) {
        if channel == self.chosen {
            return;
        }
        *self = ChannelSettings { chosen: channel, step: self.step, ..ChannelSettings::default() };
    }

    /// What leaving the page, or what is chosen on it, saves: a name typed and not yet sent.
    pub(crate) fn left(&mut self) -> Vec<Asked> {
        let asked = match (self.category.as_mut(), self.name.as_mut()) {
            (Some((id, field)), _) => field.ask().flatten().map(|name| ops::rename_category(id, name)),
            (None, Some((id, field))) => field.ask().flatten().map(|name| ops::rename_channel(id, name)),
            (None, None) => None,
        };
        asked.map(Asked::Server).into_iter().collect()
    }

    /// The server's answer to a name saved from the page: the chosen category's, else the chosen channel's.
    pub(crate) fn named(&mut self, answer: &Result<(), LinkError>) {
        let field = match (self.category.as_mut(), self.name.as_mut()) {
            (Some((_, field)), _) | (None, Some((_, field))) => field,
            (None, None) => return,
        };
        field.answered(answer);
    }

    /// The marks of `channel` as `overwrites` holds them read.
    fn marks<'a>(overwrites: &'a HashMap<ChannelId, Loaded<Vec<Mark>>>, channel: &ChannelId) -> Option<&'a [Mark]> {
        match overwrites.get(channel)? {
            Loaded::Ready(marks) => Some(marks),
            Loaded::Loading | Loaded::Failed(_) => None,
        }
    }

    /// What `message` does, by someone in `context` holding `held` in the chosen channel; `roles` says whether they
    /// may manage roles, without which the access and advanced choices are not offered.
    pub(crate) fn update(
        &mut self,
        message: Message,
        context: &Context<'_>,
        list: &ChannelList,
        overwrites: &mut HashMap<ChannelId, Loaded<Vec<Mark>>>,
        held: Grants,
        roles: bool,
    ) -> Vec<Asked> {
        match message {
            Message::Choose(channel) => {
                if list.find(&channel).is_none() {
                    return Vec::new();
                }
                let mut asked = if self.chosen.as_ref() == Some(&channel) { Vec::new() } else { self.left() };
                self.choose(Some(channel.clone()));
                self.step = Step::Channel;
                self.follow(list);
                if roles {
                    overwrites.entry(channel.clone()).or_default();
                    asked.push(Asked::Server(ops::list_overwrites(channel)));
                }
                asked
            }
            Message::ChooseCategory(category) => {
                let Some(listed) = list.categories.iter().find(|listed| listed.id == category) else {
                    return Vec::new();
                };
                if self.chosen_category() == Some(&category) {
                    return Vec::new();
                }
                let asked = self.left();
                let chosen = (category, NameField::required(listed.name.clone()));
                *self = ChannelSettings { category: Some(chosen), step: Step::Channel, ..ChannelSettings::default() };
                asked
            }
            Message::Back => {
                self.step = Step::List;
                self.left()
            }
            Message::Name(typed) => {
                let asked = match (self.category.as_mut(), self.name.as_mut()) {
                    (Some((id, field)), _) => field.update(typed).flatten().map(|name| ops::rename_category(id, name)),
                    (None, Some((id, field))) => {
                        field.update(typed).flatten().map(|name| ops::rename_channel(id, name))
                    }
                    (None, None) => None,
                };
                asked.map(Asked::Server).into_iter().collect()
            }
            Message::NameLeft => self.left(),
            Message::CategoryMenu => {
                self.menu = if self.menu == Some(Opened::Category) { None } else { Some(Opened::Category) };
                Vec::new()
            }
            Message::Category(category) => {
                self.menu = None;
                let Some(chosen) = self.chosen.clone() else { return Vec::new() };
                let at = list.categories.iter().position(|one| one.id == category);
                let next = at.and_then(|at| list.categories.get(at + 1));
                let before = next.map(|next| ListKey::Category(next.id.clone()));
                vec![Asked::Move(Moved { key: ListKey::Channel(chosen), before })]
            }
            Message::Adding(adding) => {
                let adding = Opened::Adding(adding);
                self.menu = if self.menu == Some(adding) { None } else { Some(adding) };
                self.query.clear();
                Vec::new()
            }
            Message::Query(query) => {
                self.query = query;
                Vec::new()
            }
            Message::Advanced => {
                self.fold = match self.fold {
                    Fold::Folded => Fold::Open,
                    Fold::Open => Fold::Folded,
                };
                self.menu = self.menu.filter(|menu| *menu != Opened::Adding(Adding::Advanced));
                Vec::new()
            }
            Message::Target(whom) => {
                self.target = Some(whom);
                self.menu = None;
                Vec::new()
            }
            Message::AddTarget(whom) => {
                if !roles || context.locked(&whom) {
                    return Vec::new();
                }
                if !self.added.contains(&whom) {
                    self.added.push(whom.clone());
                }
                self.target = Some(whom);
                self.menu = None;
                self.query.clear();
                Vec::new()
            }
            Message::Switch(access) | Message::Allow(access, _) | Message::Remove(access, _)
                if !roles || !held.holds(access.word()) =>
            {
                Vec::new()
            }
            Message::Set(..) | Message::RemoveTarget if !roles => Vec::new(),
            Message::Switch(access) => {
                let Some((channel, now)) = self.chosen_marks(overwrites) else { return Vec::new() };
                let Some(default) = context.roles.default_role().map(|role| Whom::Role(role.id.clone())) else {
                    return Vec::new();
                };
                let word = access.word();
                let changes: Vec<(Whom, Option<Overwrite>)> = match state(&now, &default, word) {
                    Some(Overwrite::Deny) => vec![(default, None)],
                    Some(Overwrite::Allow) | None => {
                        let me = context.asker.me.filter(|_| !context.asker.place.is_owner());
                        let me = me.map(|me| Whom::Member(me.clone()));
                        let first = me.filter(|me| state(&now, me, word) != Some(Overwrite::Allow));
                        first
                            .map(|me| (me, Some(Overwrite::Allow)))
                            .into_iter()
                            .chain([(default, Some(Overwrite::Deny))])
                            .collect()
                    }
                };
                self.menu = None;
                self.change(&channel, &now, word, changes, context, overwrites)
            }
            Message::Allow(access, whom) => {
                let Some((channel, now)) = self.chosen_marks(overwrites) else { return Vec::new() };
                if context.locked(&whom) {
                    return Vec::new();
                }
                self.menu = None;
                self.query.clear();
                self.change(&channel, &now, access.word(), vec![(whom, Some(Overwrite::Allow))], context, overwrites)
            }
            Message::Remove(access, whom) => {
                let Some((channel, now)) = self.chosen_marks(overwrites) else { return Vec::new() };
                if context.locked(&whom) || state(&now, &whom, access.word()).is_none() {
                    return Vec::new();
                }
                self.change(&channel, &now, access.word(), vec![(whom, None)], context, overwrites)
            }
            Message::Set(word, set) => {
                let Some((channel, now)) = self.chosen_marks(overwrites) else { return Vec::new() };
                let Some(target) = self.target(context) else { return Vec::new() };
                let Some(kind) = listed_channel(list, &channel).map(|(kind, _)| kind) else { return Vec::new() };
                if !channel_words(kind).contains(&word) || !held.holds(word) || context.locked(&target) {
                    return Vec::new();
                }
                if state(&now, &target, word) == set {
                    return Vec::new();
                }
                self.change(&channel, &now, word, vec![(target, set)], context, overwrites)
            }
            Message::RemoveTarget => {
                let Some((channel, now)) = self.chosen_marks(overwrites) else { return Vec::new() };
                let Some(target) = self.target(context) else { return Vec::new() };
                let default = context.roles.default_role().map(|role| Whom::Role(role.id.clone()));
                if context.locked(&target) || Some(&target) == default.as_ref() {
                    return Vec::new();
                }
                let words: Vec<Grant> = now.iter().filter(|mark| mark.whom == target).map(|mark| mark.word).collect();
                if words.iter().any(|word| !held.holds(*word)) {
                    return Vec::new();
                }
                let after = words.iter().fold(now.clone(), |marks, word| with(&marks, &target, *word, None));
                if context.locks_out(&now, &after) {
                    return Vec::new();
                }
                self.added.retain(|added| *added != target);
                self.target = None;
                self.shown_at_once(&channel, &now, after, words.len(), overwrites);
                words
                    .into_iter()
                    .map(|word| {
                        let whom = target.clone();
                        Asked::Server(ops::set_overwrite(channel.clone(), &whom, word, None))
                    })
                    .collect()
            }
        }
    }

    /// The chosen channel and its marks, once read.
    fn chosen_marks(&self, overwrites: &HashMap<ChannelId, Loaded<Vec<Mark>>>) -> Option<(ChannelId, Vec<Mark>)> {
        let chosen = self.chosen.clone()?;
        let marks = Self::marks(overwrites, &chosen)?.to_vec();
        Some((chosen, marks))
    }

    /// `changes` of one `word`, applied in their order: refused whole where the last would leave the asker unable to
    /// see the channel; else shown at once and asked, each.
    fn change(
        &mut self,
        channel: &ChannelId,
        now: &[Mark],
        word: Grant,
        changes: Vec<(Whom, Option<Overwrite>)>,
        context: &Context<'_>,
        overwrites: &mut HashMap<ChannelId, Loaded<Vec<Mark>>>,
    ) -> Vec<Asked> {
        let after = changes.iter().fold(now.to_vec(), |marks, (whom, set)| with(&marks, whom, word, *set));
        if context.locks_out(now, &after) {
            return Vec::new();
        }
        self.shown_at_once(channel, now, after, changes.len(), overwrites);
        changes
            .into_iter()
            .map(|(whom, state)| Asked::Server(ops::set_overwrite(channel.clone(), &whom, word, state)))
            .collect()
    }

    /// `after` shown at once in place of `now`, which a refusal of any of the `asks` made for it puts back.
    fn shown_at_once(
        &mut self,
        channel: &ChannelId,
        now: &[Mark],
        after: Vec<Mark>,
        asks: usize,
        overwrites: &mut HashMap<ChannelId, Loaded<Vec<Mark>>>,
    ) {
        if self.unchanged.as_ref().is_none_or(|(kept, _)| kept != channel) {
            self.unchanged = Some((channel.clone(), now.to_vec()));
            self.asking = 0;
        }
        self.asking += asks;
        overwrites.insert(channel.clone(), Loaded::Ready(after));
    }

    /// The server's answer to a change of `channel` shown at once: kept once every change is answered, or every one put
    /// back on a refusal, which the page then says and the overwrites are read again for.
    pub(crate) fn answered(
        &mut self,
        channel: &ChannelId,
        answer: &Result<(), LinkError>,
        overwrites: &mut HashMap<ChannelId, Loaded<Vec<Mark>>>,
    ) -> Option<Sentence> {
        self.asking = self.asking.saturating_sub(1);
        let Err(refused) = answer else {
            if self.asking == 0 {
                self.unchanged = None;
            }
            return None;
        };
        if let Some((kept, marks)) = self.unchanged.take_if(|(kept, _)| kept == channel) {
            overwrites.insert(kept, Loaded::Ready(marks));
        }
        self.asking = 0;
        Some(match refused {
            LinkError::Refused(proto::chatkord_v1::ErrorCode::NotAuthorized) => {
                Sentence::Phrase(Phrase::OverwriteRefused)
            }
            other => words::failure::said(other),
        })
    }

    /// The target whose words show: the one chosen, else the default role; none until the roles are read.
    fn target(&self, context: &Context<'_>) -> Option<Whom> {
        self.target.clone().or_else(|| context.roles.default_role().map(|role| Whom::Role(role.id.clone())))
    }

    /// The chosen channel's editor as drawn, by someone in `context` holding `held` there; `roles` says whether they
    /// may manage roles.
    pub(crate) fn editor<'a>(
        &'a self,
        context: &Context<'_>,
        list: &ChannelList,
        overwrites: &HashMap<ChannelId, Loaded<Vec<Mark>>>,
        held: Grants,
        roles: bool,
    ) -> Option<EditorView<'a>> {
        let channel = self.chosen.clone()?;
        let (kind, _) = listed_channel(list, &channel)?;
        let (_, name) = self.name.as_ref().filter(|(id, _)| *id == channel)?;
        let category = list.placed.get(&channel)?.category.clone();
        let marks = Self::marks(overwrites, &channel).filter(|_| roles);
        let access = marks.map(|marks| self.access(context, kind, marks, held));
        Some(EditorView { channel, kind, name, category, access })
    }

    fn access(&self, context: &Context<'_>, kind: Kind, marks: &[Mark], held: Grants) -> AccessViews {
        let view = |access: Access| self.access_view(context, access, marks);
        let private = view(Access::Private);
        let stay = private.on == Switch::On && !context.asker.place.is_owner();
        AccessViews {
            read_only: (kind == Kind::Text).then(|| view(Access::ReadOnly)),
            private,
            stay,
            advanced: self.advanced(context, kind, marks, held),
        }
    }

    fn access_view(&self, context: &Context<'_>, access: Access, marks: &[Mark]) -> AccessView {
        let word = access.word();
        let default = context.roles.default_role().map(|role| Whom::Role(role.id.clone()));
        let on = match default.as_ref().and_then(|default| state(marks, default, word)) {
            Some(Overwrite::Deny) => Switch::On,
            Some(Overwrite::Allow) | None => Switch::Off,
        };
        let allowed = |whom: &Whom| state(marks, whom, word) == Some(Overwrite::Allow);
        let me = context.asker.me.map(|me| Whom::Member(me.clone()));
        let roles = context.roles.roles().iter().map(|role| Whom::Role(role.id.clone()));
        let roles = roles.filter(|whom| Some(whom) != default.as_ref() && allowed(whom));
        let mut members: Vec<Whom> = marks
            .iter()
            .filter(|mark| mark.word == word && mark.state == Overwrite::Allow)
            .filter_map(|mark| match &mark.whom {
                Whom::Member(member) if Some(&mark.whom) != me.as_ref() => Some(Whom::Member(member.clone())),
                Whom::Member(_) | Whom::Role(_) => None,
            })
            .collect();
        members.sort_by_cached_key(|whom| match context.shown(whom).0 {
            Shown::Name(name) => name.to_lowercase(),
            Shown::You => String::new(),
        });
        let you = me.filter(|me| allowed(me));
        let listed = you
            .into_iter()
            .chain(roles)
            .chain(members)
            .map(|whom| {
                let after = with(marks, &whom, word, None);
                let removal = if word == Grant::ChannelRead && context.locks_out(marks, &after) {
                    Removal::LocksOut
                } else {
                    Removal::Offered
                };
                let (name, look) = context.shown(&whom);
                Listed { whom, name, look, removal }
            })
            .collect();
        let adding =
            (self.menu == Some(Opened::Adding(Adding::Access(access)))).then(|| context.add_menu(allowed, &self.query));
        AccessView { access, on, listed, adding }
    }

    fn advanced(&self, context: &Context<'_>, kind: Kind, marks: &[Mark], held: Grants) -> AdvancedView {
        let default = context.roles.default_role().map(|role| Whom::Role(role.id.clone()));
        let marked = |whom: &Whom| marks.iter().any(|mark| mark.whom == *whom);
        let roles = context.roles.roles().iter().filter(|role| role.kind == RoleKind::Made);
        let roles = roles.map(|role| Whom::Role(role.id.clone())).filter(|whom| marked(whom));
        let mut members: Vec<Whom> = marks
            .iter()
            .filter_map(|mark| match &mark.whom {
                Whom::Member(_) => Some(mark.whom.clone()),
                Whom::Role(_) => None,
            })
            .collect();
        members.sort_by_cached_key(|whom| match context.shown(whom).0 {
            Shown::You => String::new(),
            Shown::Name(name) => name.to_lowercase(),
        });
        members.dedup();
        let mut whoms: Vec<Whom> = default.iter().cloned().chain(roles).chain(members).collect();
        for added in &self.added {
            if !whoms.contains(added) {
                whoms.push(added.clone());
            }
        }
        let chosen = self.target.clone().filter(|target| whoms.contains(target)).or(default.clone());
        let targets = whoms
            .iter()
            .map(|whom| {
                let (name, look) = context.shown(whom);
                let kind = match whom {
                    _ if Some(whom) == default.as_ref() => TargetKind::Everyone,
                    Whom::Role(_) => TargetKind::Role,
                    Whom::Member(_) => TargetKind::Member,
                };
                Target { whom: whom.clone(), name, look, kind, chosen: Some(whom) == chosen.as_ref() }
            })
            .collect();
        let adding = (self.menu == Some(Opened::Adding(Adding::Advanced)))
            .then(|| context.add_menu(|whom| whoms.contains(whom), &self.query));
        let Some(target) = chosen else {
            return AdvancedView {
                fold: self.fold,
                targets,
                adding,
                locked: None,
                removable: false,
                lacks: false,
                groups: Vec::new(),
            };
        };
        let locked_target = context.locked(&target);
        let locked = match context.shown(&target).0 {
            Shown::Name(name) if locked_target => Some(Sentence::RoleLocked(name)),
            Shown::Name(_) | Shown::You => None,
        };
        let words = channel_words(kind);
        let groups = WORD_GROUPS
            .iter()
            .map(|(heading, group)| {
                let rows = group
                    .iter()
                    .filter(|word| words.contains(word))
                    .map(|word| self.word_row(context, marks, &target, *word, held, locked_target))
                    .collect::<Vec<_>>();
                (*heading, rows)
            })
            .filter(|(_, rows)| !rows.is_empty())
            .collect();
        AdvancedView {
            fold: self.fold,
            targets,
            adding,
            locked,
            removable: !locked_target && Some(&target) != default.as_ref(),
            lacks: words.iter().any(|word| !held.holds(*word)),
            groups,
        }
    }

    fn word_row(
        &self,
        context: &Context<'_>,
        marks: &[Mark],
        target: &Whom,
        word: Grant,
        held: Grants,
        locked: bool,
    ) -> WordRow {
        let now = state(marks, target, word);
        let choice = |set: Option<Overwrite>| {
            let off = if !held.holds(word) {
                Some(Off::Lacks)
            } else if locked {
                Some(Off::Locked)
            } else if set != now && context.locks_out(marks, &with(marks, target, word, set)) {
                Some(Off::LocksOut)
            } else {
                None
            };
            Choice { state: set, off }
        };
        let choices = [choice(Some(Overwrite::Deny)), choice(None), choice(Some(Overwrite::Allow))];
        let locks_out = choices.iter().any(|choice| choice.off == Some(Off::LocksOut));
        WordRow { word, now, choices, locks_out }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{
        ADMIN_PLACE, Clock, GENERAL, channel_id, default_role_entry, member, placed_holding, refused, role_entry,
        role_id, tell,
    };
    use crate::harness::{sends, wire};
    use crate::place::{Named, PeopleMessage, PeoplePage};
    use crate::window::Window;
    use crate::{Ask as Asked, PlaceId, WindowMessage};
    use proto::chatkord_v1::{ChannelOverwrite, ErrorCode, OverwriteState, channel_overwrite};
    use std::time::Instant;

    const OWNER: &str = "1";
    const ADMIN: &str = "2";
    const MODS: &str = "3";
    const MEMBER: &str = "4";

    fn words() -> Grants {
        Grants::from_words(&["MANAGE_CHANNEL", "MANAGE_ROLES", "CHANNEL_READ", "SEND_MESSAGE"])
    }

    /// Ada, an admin (not the owner) managing the channels and roles, on Channels with #general chosen; grace holds
    /// Moderators, Bo nothing but the default role.
    fn on_channels(clock: &Clock, at: Place) -> (Window, PlaceId) {
        let (mut window, owls) = placed_holding(clock, at, words());
        let roles = vec![
            role_entry(OWNER, "Owner", Place::OWNER, &[]),
            role_entry(ADMIN, "Admin", ADMIN_PLACE, &[]),
            role_entry(MODS, "Moderators", Place::at(ADMIN_PLACE.position() + 1), &[]),
            default_role_entry(MEMBER, "Member", Place::at(ADMIN_PLACE.position() + 2), &[]),
        ];
        tell(&mut window, clock, owls, place::Message::Server(place::ServerMessage::RolesListed(Ok(roles))));
        let person = |id: &str, roles: &[&str]| {
            let named = Named {
                name: id.into(),
                colour: None,
                roles: roles.iter().map(|role| role_id(role)).collect(),
                picture: None,
            };
            (member(id), named, proto::status::Shown::Online)
        };
        let entries = vec![person("ada", &[ADMIN, MEMBER]), person("grace", &[MODS, MEMBER]), person("bo", &[MEMBER])];
        let page = PeoplePage { entries, next: None, presence: 1 };
        tell(&mut window, clock, owls, place::Message::People(PeopleMessage::Page(Ok(page))));
        let todo =
            tell(&mut window, clock, owls, place::Message::Page(place::PageMessage::Settings(crate::Page::Channels)));
        assert!(
            matches!(&todo[..], [Asked::At(_, ask)] if sends(ask, wire::ListChannelOverwrites { channel_id: GENERAL.into() })),
            "{todo:?}"
        );
        (window, owls)
    }

    fn told(whom: channel_overwrite::Whom, word: Grant, state: OverwriteState) -> ChannelOverwrite {
        ChannelOverwrite { whom: Some(whom), word: word.word().into(), state: state.into() }
    }

    fn role_mark(role: &str, word: Grant, state: OverwriteState) -> ChannelOverwrite {
        told(channel_overwrite::Whom::RoleId(role.into()), word, state)
    }

    fn member_mark(user: &str, word: Grant, state: OverwriteState) -> ChannelOverwrite {
        told(channel_overwrite::Whom::UserId(user.into()), word, state)
    }

    fn listed(window: &mut Window, clock: &Clock, owls: PlaceId, marks: Vec<ChannelOverwrite>) {
        let told = place::ServerMessage::Overwrites { channel: channel_id(GENERAL), listed: Ok(marks) };
        tell(window, clock, owls, place::Message::Server(told));
    }

    /// The canvas's #staff: hidden from everyone, let into Moderators and grace, and Ada, who made it.
    fn private() -> Vec<ChannelOverwrite> {
        vec![
            role_mark(MEMBER, Grant::ChannelRead, OverwriteState::Deny),
            role_mark(MODS, Grant::ChannelRead, OverwriteState::Allow),
            member_mark("ada", Grant::ChannelRead, OverwriteState::Allow),
            member_mark("grace", Grant::ChannelRead, OverwriteState::Allow),
        ]
    }

    fn page(message: Message) -> place::Message {
        place::Message::Page(place::PageMessage::Channels(message))
    }

    fn access(window: &Window) -> AccessViews {
        window.open().unwrap().channel_editor().and_then(|editor| editor.access).expect("the access part")
    }

    fn rows(view: &AccessView) -> Vec<(Shown, Removal)> {
        view.listed.iter().map(|row| (row.name.clone(), row.removal)).collect()
    }

    fn overwritten(set: &wire::SetChannelOverwrite) -> (Whom, Grant, Option<Overwrite>) {
        let whom = match set.whom.clone() {
            Some(wire::set_channel_overwrite::Whom::RoleId(role)) => Whom::Role(role_id(&role)),
            Some(wire::set_channel_overwrite::Whom::UserId(member)) => Whom::Member(crate::harness::member(&member)),
            None => panic!("an overwrite of no one"),
        };
        let word = Grant::of_word(&set.word).expect("a word");
        (whom, word, Overwrite::asked(set.state).expect("a state"))
    }

    fn asked(todo: &[Asked]) -> Vec<(Whom, Grant, Option<Overwrite>)> {
        todo.iter()
            .filter_map(|done| match done {
                Asked::At(_, ask) => match ask.op() {
                    Some(crate::harness::Body::SetChannelOverwrite(set)) => Some(overwritten(set)),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    /// A category is chosen on the page as a channel is, so it is renamed and deleted there, each asked first; once
    /// it is gone a channel takes its place.
    #[test]
    fn a_category_chosen_on_the_page_offers_its_rename_and_delete_and_gives_way_once_gone() {
        use proto::chatkord_v1::{CategorySummary, ChannelKind, ChannelSummary, ChannelsSummary};
        let clock = Clock(Instant::now());
        let (mut window, owls) = on_channels(&clock, ADMIN_PLACE);
        let general = |category: &str| ChannelSummary {
            channel_id: GENERAL.into(),
            name: "general".into(),
            kind: ChannelKind::Text.into(),
            position: 1,
            timeline_id: "t-general".into(),
            category_id: category.to_string(),
            ..Default::default()
        };
        let games = CategorySummary { category_id: "games".into(), name: "Games".into(), position: 1 };
        let list = |channels, categories, version| {
            let summary = ChannelsSummary { channels, version, standings: Vec::new(), categories };
            place::Message::Lobby(place::LobbyMessage::Channels(Ok(place::listed(&summary))))
        };
        tell(&mut window, &clock, owls, list(vec![general("games")], vec![games], 7));
        let id = CategoryId::parse("games").unwrap();
        let settings = |window: &Window| window.open().unwrap().channel_settings().unwrap().clone();
        tell(&mut window, &clock, owls, page(Message::ChooseCategory(id.clone())));
        assert_eq!((settings(&window).chosen_category(), settings(&window).chosen()), (Some(&id), None));
        assert!(window.open().unwrap().channel_editor().is_none(), "no channel's editor beside a category");
        // Its name is typed in a field as a channel's is, and saved by Enter.
        assert_eq!(settings(&window).category_name().map(NameField::typed), Some("Games"));
        let typed = Message::Name(name_field::Message::Typed("Board games".into()));
        assert!(tell(&mut window, &clock, owls, page(typed)).is_empty(), "nothing is sent while typing");
        let todo = tell(&mut window, &clock, owls, page(Message::Name(name_field::Message::Save)));
        assert!(
            matches!(&todo[..], [Asked::At(_, ask)]
                if sends(ask, wire::CategoryRename { category_id: id.as_str().into(), name: "Board games".into() })),
            "{todo:?}"
        );
        tell(&mut window, &clock, owls, place::Message::Page(place::PageMessage::CategoryRenamed(Ok(()))));
        // Delete is asked first.
        window.update(WindowMessage::Category(owls, crate::ChannelAction::Delete, id.clone()), clock.at());
        let asking = window.dialog().map(crate::Dialog::asking);
        assert!(matches!(asking, Some(crate::Asking::DeleteCategory(_))), "{asking:?}");
        window.update(WindowMessage::Dialog(crate::DialogMessage::Cancel), clock.at());
        let unknown = CategoryId::parse("nope").unwrap();
        tell(&mut window, &clock, owls, page(Message::ChooseCategory(unknown)));
        assert_eq!(settings(&window).chosen_category(), Some(&id), "one not listed is not chosen");
        let talk = CategorySummary { category_id: "talk".into(), name: "Talk".into(), position: 2 };
        tell(&mut window, &clock, owls, list(vec![general("talk")], vec![talk], 8));
        assert_eq!(
            (settings(&window).chosen_category(), settings(&window).chosen()),
            (None, Some(&channel_id(GENERAL))),
            "gone, a channel takes its place"
        );
    }

    #[test]
    fn the_category_menu_toggles_closes_on_a_pick_and_gives_way_to_an_add_menu() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = on_channels(&clock, ADMIN_PLACE);
        let open = |window: &Window| window.open().unwrap().channel_settings().unwrap().category_menu_open();
        tell(&mut window, &clock, owls, page(Message::CategoryMenu));
        assert!(open(&window));
        tell(&mut window, &clock, owls, page(Message::CategoryMenu));
        assert!(!open(&window), "a second press closes it");
        tell(&mut window, &clock, owls, page(Message::CategoryMenu));
        tell(&mut window, &clock, owls, page(Message::Category(CategoryId::parse("games").unwrap())));
        assert!(!open(&window), "a pick closes it");
        tell(&mut window, &clock, owls, page(Message::CategoryMenu));
        tell(&mut window, &clock, owls, page(Message::Adding(Adding::Advanced)));
        assert!(!open(&window), "one menu at a time");
    }

    #[test]
    fn who_sees_and_writes_is_read_from_the_overwrites_every_role_not_listed_inheriting() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = on_channels(&clock, ADMIN_PLACE);
        assert!(window.open().unwrap().channel_editor().is_some_and(|editor| editor.access.is_none()), "until read");
        listed(&mut window, &clock, owls, private());
        let access = access(&window);
        assert_eq!(access.private.on, Switch::On, "the default role is denied seeing it");
        assert_eq!(
            rows(&access.private),
            [
                (Shown::You, Removal::LocksOut),
                (Shown::Name("Moderators".into()), Removal::Offered),
                (Shown::Name("grace".into()), Removal::Offered),
            ]
        );
        assert!(access.stay, "an admin stays in it while listed");
        assert_eq!(access.read_only.as_ref().map(|view| view.on), Some(Switch::Off), "a text channel offers it");
        let targets: Vec<(Shown, TargetKind)> =
            access.advanced.targets.iter().map(|target| (target.name.clone(), target.kind)).collect();
        assert_eq!(
            targets,
            [
                (Shown::Name("Member".into()), TargetKind::Everyone),
                (Shown::Name("Moderators".into()), TargetKind::Role),
                (Shown::You, TargetKind::Member),
                (Shown::Name("grace".into()), TargetKind::Member),
            ],
            "the default role first, the roles with a choice here in order, then the members"
        );
        let words: Vec<(Grant, Option<Overwrite>)> =
            access.advanced.groups.iter().flat_map(|(_, rows)| rows.iter().map(|row| (row.word, row.now))).collect();
        assert_eq!(
            words,
            [(Grant::ChannelRead, Some(Overwrite::Deny)), (Grant::SendMessage, None), (Grant::ManageMessages, None)],
            "a text channel's words, each inherited unless chosen"
        );
        let manage = &access.advanced.groups[0].1[2];
        assert!(manage.choices.iter().all(|choice| choice.off == Some(Off::Lacks)), "a word Ada lacks");
        assert!(access.advanced.lacks);
        tell(&mut window, &clock, owls, page(Message::Target(Whom::Member(member("ada")))));
        let you = &self::access(&window).advanced.groups[0].1[0];
        assert_eq!(you.now, Some(Overwrite::Allow));
        let offs: Vec<Option<Off>> = you.choices.iter().map(|choice| choice.off).collect();
        assert_eq!(offs, [Some(Off::LocksOut), Some(Off::LocksOut), None], "Ada would no longer see it");
        assert!(you.locks_out);
    }

    #[test]
    fn private_turned_on_by_an_admin_lists_them_first_and_off_takes_only_everyone_s_deny() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = on_channels(&clock, ADMIN_PLACE);
        listed(&mut window, &clock, owls, Vec::new());
        let todo = tell(&mut window, &clock, owls, page(Message::Switch(Access::Private)));
        let everyone = Whom::Role(role_id(MEMBER));
        assert_eq!(
            asked(&todo),
            [
                (Whom::Member(member("ada")), Grant::ChannelRead, Some(Overwrite::Allow)),
                (everyone.clone(), Grant::ChannelRead, Some(Overwrite::Deny)),
            ],
            "listed first, so the server's guard never refuses the switch"
        );
        assert_eq!(rows(&access(&window).private), [(Shown::You, Removal::LocksOut)], "shown at once");
        let todo = tell(&mut window, &clock, owls, page(Message::Switch(Access::Private)));
        assert_eq!(asked(&todo), [(everyone.clone(), Grant::ChannelRead, None)], "the allows stay");
        let (mut window, owls) = on_channels(&clock, Place::OWNER);
        listed(&mut window, &clock, owls, Vec::new());
        let todo = tell(&mut window, &clock, owls, page(Message::Switch(Access::Private)));
        assert_eq!(
            asked(&todo),
            [(everyone, Grant::ChannelRead, Some(Overwrite::Deny))],
            "every channel is seen by the owner"
        );
    }

    #[test]
    fn a_change_that_would_hide_the_channel_from_its_asker_is_never_asked() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = on_channels(&clock, ADMIN_PLACE);
        let mut marks = private();
        marks.push(role_mark(ADMIN, Grant::SendMessage, OverwriteState::Allow));
        listed(&mut window, &clock, owls, marks);
        let me = Whom::Member(member("ada"));
        assert!(tell(&mut window, &clock, owls, page(Message::Remove(Access::Private, me.clone()))).is_empty());
        tell(&mut window, &clock, owls, page(Message::Target(me)));
        assert!(tell(&mut window, &clock, owls, page(Message::Set(Grant::ChannelRead, None))).is_empty());
        tell(&mut window, &clock, owls, page(Message::Target(Whom::Role(role_id(ADMIN)))));
        assert!(
            tell(&mut window, &clock, owls, page(Message::Set(Grant::SendMessage, Some(Overwrite::Deny)))).is_empty(),
            "a role at the asker's own place is locked"
        );
        assert_eq!(
            access(&window).advanced.locked.map(|said| said.in_english()).as_deref(),
            Some("Admin is at or above your own place, so only someone higher can change it.")
        );
    }

    #[test]
    fn a_refused_change_is_put_back_said_on_the_page_and_the_overwrites_read_again() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = on_channels(&clock, ADMIN_PLACE);
        listed(&mut window, &clock, owls, private());
        let mods = Whom::Role(role_id(MODS));
        let todo = tell(&mut window, &clock, owls, page(Message::Remove(Access::Private, mods.clone())));
        assert_eq!(asked(&todo), [(mods, Grant::ChannelRead, None)]);
        assert_eq!(rows(&access(&window).private).len(), 2, "shown at once");
        let refusal = place::PageMessage::OverwriteSet {
            channel: channel_id(GENERAL),
            answer: Err(refused(ErrorCode::NotAuthorized)),
        };
        let todo = tell(&mut window, &clock, owls, place::Message::Page(refusal));
        assert!(
            matches!(&todo[..], [Asked::At(_, ask)] if sends(ask, wire::ListChannelOverwrites { channel_id: GENERAL.into() })),
            "{todo:?}"
        );
        assert_eq!(rows(&access(&window).private).len(), 3, "put back");
        let said: Vec<String> = window
            .open()
            .and_then(place::Place::settings)
            .map(|open| open.notices().into_iter().map(|notice| notice.said.in_english()).collect())
            .unwrap_or_default();
        assert_eq!(said, ["That change wasn’t saved: you no longer hold Manage roles."]);
        let add = tell(&mut window, &clock, owls, page(Message::Allow(Access::ReadOnly, Whom::Member(member("bo")))));
        assert_eq!(asked(&add), [(Whom::Member(member("bo")), Grant::SendMessage, Some(Overwrite::Allow))]);
        let taken = place::PageMessage::OverwriteSet { channel: channel_id(GENERAL), answer: Ok(()) };
        assert!(tell(&mut window, &clock, owls, place::Message::Page(taken)).is_empty());
        let writers = access(&window).read_only.map(|view| rows(&view)).unwrap_or_default();
        assert_eq!(writers, [(Shown::Name("bo".into()), Removal::Offered)], "kept as shown");
    }

    #[test]
    fn a_private_channel_s_start_says_so_without_its_overwrites_and_a_change_of_its_access_reads_them_again() {
        use proto::chatkord_v1::{ChannelAccessChanged, Event, event};
        let clock = Clock(Instant::now());
        let (mut window, owls) = on_channels(&clock, ADMIN_PLACE);
        assert_eq!(window.open().unwrap().private_line(&channel_id(GENERAL)), None, "everyone sees it");
        let hidden = ChannelAccessChanged { channel_id: GENERAL.into(), private: true, version: 1 };
        let heard =
            place::Message::heard(&Event { cursor: Vec::new(), kind: Some(event::Kind::ChannelAccessChanged(hidden)) });
        let todo = tell(&mut window, &clock, owls, heard.expect("read"));
        assert!(
            matches!(&todo[..], [Asked::At(_, ask1)] if crate::harness::is(ask1, crate::harness::Op::ListChannelOverwrites)),
            "read again where they were read: {todo:?}"
        );
        tell(&mut window, &clock, owls, place::Message::Page(place::PageMessage::CloseSettings));
        let general = place::DiscussionMessage::Open(channel_id(GENERAL));
        let todo = window.update(WindowMessage::At(owls, place::Message::Discussion(general)), clock.at());
        assert!(
            !todo.iter().any(|done| matches!(done, Asked::At(_, ask1) if crate::harness::is(ask1, crate::harness::Op::ListChannelOverwrites))),
            "a private channel's start names no one: {todo:?}"
        );
        assert_eq!(
            window.open().unwrap().private_line(&channel_id(GENERAL)).map(|said| said.in_english()).as_deref(),
            Some("This is the start of the private #general channel.")
        );
    }

    #[test]
    fn the_create_dialog_lists_this_account_first_and_fixed() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = on_channels(&clock, ADMIN_PLACE);
        window.update(WindowMessage::CreateChannel(owls), clock.at());
        let making = |message| WindowMessage::Dialog(crate::DialogMessage::Making(message));
        window.update(making(crate::MakingMessage::Switch(Access::Private)), clock.at());
        window.update(making(crate::MakingMessage::Allow(Access::Private, Whom::Role(role_id(MODS)))), clock.at());
        window.update(making(crate::MakingMessage::Adding(Access::Private)), clock.at());
        let Some(crate::Asking::MakeChannel(making)) = window.dialog().map(crate::Dialog::asking) else {
            panic!("not making")
        };
        let view = window.open().unwrap().making_access(making, Access::Private).expect("offered");
        assert_eq!(rows(&view), [(Shown::You, Removal::Fixed), (Shown::Name("Moderators".into()), Removal::Offered)]);
        let menu = view.adding.expect("the add menu");
        let offered: Vec<Shown> = menu.roles.iter().chain(&menu.members).map(|offer| offer.name.clone()).collect();
        assert_eq!(
            offered,
            [Shown::Name("bo".into()), Shown::Name("grace".into())],
            "no role left below Ada that is not listed; the members below her, by name"
        );
        assert!(window.open().unwrap().making_access(making, Access::ReadOnly).is_some(), "a text channel's");
    }
}
