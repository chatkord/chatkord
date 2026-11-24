//! Managing a server's channels and categories in dialogs: a channel made with its kind, category and who sees it or
//! writes in it, a channel or a category renamed, a category made, and a channel or a category deleted after asking,
//! each with why the server refused it said in the dialog.

use crate::Asked;
use crate::channel_settings::{AccessView, Context, Listed, Removal, Shown, Switch};
use crate::place::ops;
use crate::place::{self, Access, Whom};
use crate::window::{Confirming, DialogMessage, Next};
use proto::chatkord_v1::ErrorCode;
use proto::rules::{Kind, MAX_NAME_CHARS, Overwrite, TextRefused, text_shape};
use terms::{CategoryId, ChannelId, ErrorKind, LinkError, MemberId, RoleId};
use words::{Phrase, Sentence};

/// What a button on a channel's row does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelAction {
    /// Opens the dialog that renames it.
    Rename,
    /// Asks whether to delete it.
    Delete,
}

/// Why a channel or a category was not made, renamed or deleted, or its name cannot be sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelProblem {
    /// The name is over its bound in characters.
    TooLong,
    /// The name holds a line break or another control character.
    Characters,
    /// The server holds as many channels as it may.
    Full,
    /// The server holds as many categories as it may.
    CategoriesFull,
    /// The channel was deleted meanwhile.
    Gone,
    /// This account may no longer manage channels.
    NotAllowed,
    /// The server refused: too many asks too soon.
    TooManyTries,
    /// The server could not be reached, or did not answer.
    NoAnswer,
}

impl ChannelProblem {
    /// What the dialog says of it: `server` names the server that did not answer.
    pub fn said(self, server: &str) -> Sentence {
        match self {
            ChannelProblem::TooLong => Sentence::TextTooLong,
            ChannelProblem::Characters => Sentence::Phrase(Phrase::NameCharacters),
            ChannelProblem::Full => Sentence::TooManyChannels,
            ChannelProblem::CategoriesFull => Sentence::TooManyCategories,
            ChannelProblem::Gone => Sentence::Phrase(Phrase::ChannelGone),
            ChannelProblem::NotAllowed => Sentence::Phrase(Phrase::CantManageChannels),
            ChannelProblem::TooManyTries => Sentence::Phrase(Phrase::TooManyTries),
            ChannelProblem::NoAnswer => Sentence::NoAnswer(server.to_string()),
        }
    }
}

/// What a name is typed for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Named {
    /// A channel, renamed: its id, kind and the name it has.
    Channel {
        /// The channel's id at the server.
        id: ChannelId,
        /// Its kind, which its title says it by.
        kind: Kind,
        /// The name it has.
        name: String,
    },
    /// A category, made.
    NewCategory,
    /// A category, renamed: its id and the name it has.
    Category {
        /// The category's id at the server.
        id: CategoryId,
        /// The name it has.
        name: String,
    },
}

/// A name being typed: a channel's new name, a new category's, or a category's new name.
#[derive(Debug)]
pub struct Naming {
    target: Named,
    name: String,
    asked: Asked<ChannelProblem>,
}

impl Naming {
    /// Renaming starts from the name it has.
    pub(crate) fn new(target: Named) -> Naming {
        let name = match &target {
            Named::Channel { name, .. } | Named::Category { name, .. } => name.clone(),
            Named::NewCategory => String::new(),
        };
        Naming { target, name, asked: Asked::Idle }
    }

    /// What the name is for.
    pub fn target(&self) -> &Named {
        &self.target
    }

    /// The dialog's title.
    pub fn title(&self) -> Sentence {
        match &self.target {
            Named::Channel { kind: Kind::Text, name, .. } => Sentence::RenameTextTitle(name.clone()),
            Named::Channel { kind: Kind::Voice, name, .. } | Named::Category { name, .. } => {
                Sentence::RenameTitle(name.clone())
            }
            Named::NewCategory => Sentence::Phrase(Phrase::CreateCategoryTitle),
        }
    }

    /// The main button's words.
    pub fn action(&self) -> Phrase {
        match self.target {
            Named::NewCategory => Phrase::CreateCategory,
            Named::Channel { .. } | Named::Category { .. } => Phrase::Save,
        }
    }

    /// What the empty field shows: the name it had, else an example.
    pub fn placeholder(&self) -> Sentence {
        match &self.target {
            Named::Channel { name, .. } | Named::Category { name, .. } => Sentence::Said(name.clone()),
            Named::NewCategory => Sentence::Phrase(Phrase::CategoryExample),
        }
    }

    /// The name the channel or category renamed has now; none when making one.
    pub fn renaming(&self) -> Option<&str> {
        match &self.target {
            Named::Channel { name, .. } | Named::Category { name, .. } => Some(name),
            Named::NewCategory => None,
        }
    }

    /// The name as it is typed, untrimmed.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the server is being asked to make or rename it.
    pub fn asking(&self) -> bool {
        self.asked.asking()
    }

    /// What is wrong, said under the field: the name as it is typed, else the server's answer.
    pub fn problem(&self) -> Option<ChannelProblem> {
        shape_problem(&self.name).or_else(|| self.asked.refused())
    }

    /// Whether the main button does something now: a name in its bounds, and no answer awaited.
    pub fn ready(&self) -> bool {
        !self.asking() && text_shape(&self.name, MAX_NAME_CHARS).is_ok()
    }

    /// Something typed or asked, which a press outside the dialog must not throw away.
    pub(crate) fn kept(&self) -> bool {
        self.asking() || self.name != self.renaming().unwrap_or_default()
    }

    pub(crate) fn typed(&mut self, name: String) {
        if !self.asking() {
            self.name = name;
            self.asked.forget();
        }
    }

    /// The server is asked, or nothing when it cannot be now.
    pub(crate) fn ask(&mut self) -> Option<place::Ask> {
        if !self.ready() {
            return None;
        }
        let name = text_shape(&self.name, MAX_NAME_CHARS).ok()?.to_string();
        self.asked.ask();
        Some(match &self.target {
            Named::Channel { id, .. } => ops::rename_channel(id, name),
            Named::NewCategory => ops::create_category(name),
            Named::Category { id, .. } => ops::rename_category(id, name),
        })
    }

    pub(crate) fn refused(&mut self, refused: &LinkError) {
        let made = match self.target {
            Named::NewCategory => Made::Category,
            Named::Channel { .. } | Named::Category { .. } => Made::Renamed,
        };
        self.asked.answered(Some(refusal(refused, made)));
    }

    /// The server's answer to the name asked: the dialog closes, or says why not. A category renamed that is gone
    /// closes quietly: the list shows it gone.
    pub(crate) fn answered(&mut self, answer: Result<(), &LinkError>) -> Next {
        match (answer, &self.target) {
            (Ok(()), _) | (Err(LinkError::Refused(ErrorCode::NotFound)), Named::Category { .. }) => Next::Closes,
            (Err(refused), _) => {
                self.refused(refused);
                Next::Stays
            }
        }
    }
}

/// Why a name as typed cannot be sent; none for one in its bounds or empty.
fn shape_problem(name: &str) -> Option<ChannelProblem> {
    match text_shape(name, MAX_NAME_CHARS) {
        Err(TextRefused::TooLong) => Some(ChannelProblem::TooLong),
        Err(TextRefused::ControlCharacter) => Some(ChannelProblem::Characters),
        Ok(_) | Err(TextRefused::Empty) => None,
    }
}

/// A name, typed and asked.
impl Confirming for Naming {
    fn holds_outside(&self) -> bool {
        self.kept()
    }

    fn said(&mut self, said: DialogMessage) -> Next {
        match said {
            DialogMessage::Named(typed) => {
                self.typed(typed);
                Next::Stays
            }
            DialogMessage::Confirm => self.ask().map_or(Next::Stays, Next::Asks),
            DialogMessage::Password(_)
            | DialogMessage::Reveal
            | DialogMessage::Cancel
            | DialogMessage::Outside
            | DialogMessage::Invite(_)
            | DialogMessage::CopyTemporary
            | DialogMessage::SetUp(_)
            | DialogMessage::Making(_) => Next::Stays,
        }
    }
}

/// Who an access of the create dialog lets in besides this account, which the server lets in with the channel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Lets {
    on: Switch,
    /// The roles and members listed, in the order they were added.
    listed: Vec<Whom>,
}

/// What a person does in the create dialog, apart from its name.
#[derive(Clone, Debug)]
pub enum MakingMessage {
    /// Text or Voice.
    Kind(Kind),
    /// The Category field pressed: its menu opens, or closes while it is open.
    CategoryMenu,
    /// The category it goes in; the field's menu closes.
    Category(CategoryId),
    /// An access's switch.
    Switch(Access),
    /// An access's add menu opened, or closed when it is open.
    Adding(Access),
    /// The add menu's search, as typed.
    Query(String),
    /// A role or member let in by an access.
    Allow(Access, Whom),
    /// A role or member taken off an access's list.
    Remove(Access, Whom),
}

/// Which of the dialog's menus is open: one at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Opened {
    /// The Category field's.
    Category,
    /// An access's add menu.
    Adding(Access),
}

/// A channel being made: its kind, name and category, and who sees it or writes in it from the start.
#[derive(Debug)]
pub struct Making {
    kind: Kind,
    name: String,
    category: CategoryId,
    private: Lets,
    read_only: Lets,
    menu: Option<Opened>,
    query: String,
    /// Whether this account may set who sees the channel: it holds `MANAGE_ROLES`.
    access: bool,
    asked: Asked<ChannelProblem>,
}

impl Making {
    /// A channel of `kind` to make in `category`; `access` says whether this account may set who sees it.
    pub(crate) fn new(kind: Kind, category: CategoryId, access: bool) -> Making {
        Making {
            kind,
            name: String::new(),
            category,
            private: Lets::default(),
            read_only: Lets::default(),
            menu: None,
            query: String::new(),
            access,
            asked: Asked::Idle,
        }
    }

    /// Text or Voice.
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// The name as it is typed, untrimmed.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What the empty name shows: an example of the kind's names.
    pub fn placeholder(&self) -> Phrase {
        match self.kind {
            Kind::Text => Phrase::TextChannelExample,
            Kind::Voice => Phrase::ChannelExample,
        }
    }

    /// The category it goes in.
    pub fn category(&self) -> &CategoryId {
        &self.category
    }

    /// Whether the dialog offers who sees the channel: this account holds `MANAGE_ROLES`.
    pub fn offers_access(&self) -> bool {
        self.access
    }

    /// Whether the Category field's menu is open.
    pub fn category_menu_open(&self) -> bool {
        self.menu == Some(Opened::Category)
    }

    /// The add menu's search, as typed.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Whether the server is being asked to make it.
    pub fn asking(&self) -> bool {
        self.asked.asking()
    }

    /// What is wrong, said under the name: the name as it is typed, else the server's answer.
    pub fn problem(&self) -> Option<ChannelProblem> {
        shape_problem(&self.name).or_else(|| self.asked.refused())
    }

    /// Whether Create channel does something now: a name in its bounds, and no answer awaited.
    pub fn ready(&self) -> bool {
        !self.asking() && text_shape(&self.name, MAX_NAME_CHARS).is_ok()
    }

    fn lets(&self, access: Access) -> &Lets {
        match access {
            Access::Private => &self.private,
            Access::ReadOnly => &self.read_only,
        }
    }

    fn lets_mut(&mut self, access: Access) -> &mut Lets {
        match access {
            Access::Private => &mut self.private,
            Access::ReadOnly => &mut self.read_only,
        }
    }

    /// The accesses on, in the order the server is asked them: Private first, Read-only a text channel's alone.
    fn on(&self) -> Vec<Access> {
        let read_only = (self.kind == Kind::Text).then_some(Access::ReadOnly);
        [Some(Access::Private), read_only]
            .into_iter()
            .flatten()
            .filter(|access| self.access && self.lets(*access).on == Switch::On)
            .collect()
    }

    /// An access's switch and list as drawn, for someone in `context`: this account first, fixed, then the roles in
    /// their order, then the members by name; none for Read-only on a voice channel, or where the dialog offers no
    /// access.
    pub(crate) fn access_view(&self, access: Access, context: &Context<'_>) -> Option<AccessView> {
        if !self.access || (access == Access::ReadOnly && self.kind != Kind::Text) {
            return None;
        }
        let lets = self.lets(access);
        let you = context.asker.me.map(|me| {
            let (_, look) = context.shown(&Whom::Member(me.clone()));
            Listed { whom: Whom::Member(me.clone()), name: Shown::You, look, removal: Removal::Fixed }
        });
        let roles = context.roles.roles().iter().map(|role| Whom::Role(role.id.clone()));
        let roles = roles.filter(|whom| lets.listed.contains(whom));
        let mut members: Vec<Whom> =
            lets.listed.iter().filter(|whom| matches!(whom, Whom::Member(_))).cloned().collect();
        members.sort_by_cached_key(|whom| match context.shown(whom).0 {
            Shown::Name(name) => name.to_lowercase(),
            Shown::You => String::new(),
        });
        let others = roles.chain(members).map(|whom| {
            let (name, look) = context.shown(&whom);
            Listed { whom, name, look, removal: Removal::Offered }
        });
        let listed = you.into_iter().chain(others).collect();
        let adding = (self.menu == Some(Opened::Adding(access)))
            .then(|| context.add_menu(|whom| lets.listed.contains(whom), &self.query));
        Some(AccessView { access, on: lets.on, listed, adding })
    }

    /// What `message` does to the dialog.
    pub(crate) fn update(&mut self, message: MakingMessage) {
        if self.asking() {
            return;
        }
        match message {
            MakingMessage::Kind(kind) => self.kind = kind,
            MakingMessage::CategoryMenu => {
                self.menu = if self.menu == Some(Opened::Category) { None } else { Some(Opened::Category) };
            }
            MakingMessage::Category(category) => {
                self.category = category;
                self.menu = None;
            }
            MakingMessage::Switch(access) => {
                let lets = self.lets_mut(access);
                lets.on = lets.on.toggled();
                self.menu = None;
            }
            MakingMessage::Adding(access) => {
                let adding = Opened::Adding(access);
                self.menu = if self.menu == Some(adding) { None } else { Some(adding) };
                self.query.clear();
            }
            MakingMessage::Query(query) => self.query = query,
            MakingMessage::Allow(access, whom) => {
                let lets = self.lets_mut(access);
                if !lets.listed.contains(&whom) {
                    lets.listed.push(whom);
                }
                self.menu = None;
                self.query.clear();
            }
            MakingMessage::Remove(access, whom) => self.lets_mut(access).listed.retain(|listed| *listed != whom),
        }
        self.asked.forget();
    }

    /// The server is asked to make the channel with its first access and that access's first role, or nothing when it
    /// cannot be now.
    fn ask(&mut self) -> Option<place::Ask> {
        if !self.ready() {
            return None;
        }
        let name = text_shape(&self.name, MAX_NAME_CHARS).ok()?.to_string();
        let access = self.on().first().copied();
        let role = access.and_then(|access| self.first_role(access));
        self.asked.ask();
        Some(ops::create_channel(self.kind.wire(), name, &self.category, access, role.as_ref()))
    }

    /// The first role an access lists, which the server lets in with the channel.
    fn first_role(&self, access: Access) -> Option<RoleId> {
        self.lets(access).listed.iter().find_map(|whom| match whom {
            Whom::Role(role) => Some(role.clone()),
            Whom::Member(_) => None,
        })
    }

    /// The overwrites set once the server made the channel `made`: the rest of the first access's list, then the
    /// second access whole (this account let in first unless it is the owner `me`, `owner` says, then the default role
    /// `everyone` denied, then its list). Until they land the channel is private to fewer, never to more.
    pub(crate) fn after_made(
        &self,
        made: &ChannelId,
        everyone: Option<&RoleId>,
        me: Option<&MemberId>,
        owner: bool,
    ) -> Vec<place::Ask> {
        let set = |whom: Whom, access: Access, state: Option<Overwrite>| {
            ops::set_overwrite(made.clone(), &whom, access.word(), state)
        };
        let mut asks = Vec::new();
        let mut on = self.on().into_iter();
        if let Some(first) = on.next() {
            let sent = self.first_role(first).map(Whom::Role);
            let rest = self.lets(first).listed.iter().filter(|whom| Some(*whom) != sent.as_ref());
            asks.extend(rest.map(|whom| set(whom.clone(), first, Some(Overwrite::Allow))));
        }
        for second in on {
            let you = me.filter(|_| !owner).map(|me| set(Whom::Member(me.clone()), second, Some(Overwrite::Allow)));
            let deny = everyone.map(|role| set(Whom::Role(role.clone()), second, Some(Overwrite::Deny)));
            let listed = self.lets(second).listed.iter().map(|whom| set(whom.clone(), second, Some(Overwrite::Allow)));
            asks.extend(you.into_iter().chain(deny).chain(listed));
        }
        asks
    }

    /// The server's answer: the dialog closes, or says why not.
    pub(crate) fn answered(&mut self, answer: Result<(), &LinkError>) -> Next {
        match answer {
            Ok(()) => Next::Closes,
            Err(refused) => {
                self.asked.answered(Some(refusal(refused, Made::Channel)));
                Next::Stays
            }
        }
    }
}

/// A channel being made, its name typed and asked.
impl Confirming for Making {
    fn holds_outside(&self) -> bool {
        self.asking() || !self.name.is_empty()
    }

    fn said(&mut self, said: DialogMessage) -> Next {
        match said {
            DialogMessage::Named(typed) if !self.asking() => {
                self.name = typed;
                self.asked.forget();
                Next::Stays
            }
            DialogMessage::Making(message) => {
                self.update(message);
                Next::Stays
            }
            DialogMessage::Confirm => self.ask().map_or(Next::Stays, Next::Asks),
            DialogMessage::Named(_)
            | DialogMessage::Password(_)
            | DialogMessage::Reveal
            | DialogMessage::Cancel
            | DialogMessage::Outside
            | DialogMessage::Invite(_)
            | DialogMessage::CopyTemporary
            | DialogMessage::SetUp(_) => Next::Stays,
        }
    }
}

/// Deleting one channel: asked first.
#[derive(Debug)]
pub struct Deleting {
    pub(crate) id: ChannelId,
    kind: Kind,
    name: String,
    asked: Asked<ChannelProblem>,
}

impl Deleting {
    pub(crate) fn new(id: ChannelId, kind: Kind, name: String) -> Deleting {
        Deleting { id, kind, name, asked: Asked::Idle }
    }

    /// The channel's name as the list shows it.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The dialog's title.
    pub fn title(&self) -> Sentence {
        match self.kind {
            Kind::Text => Sentence::DeleteTextTitle(self.name.clone()),
            Kind::Voice => Sentence::DeleteChannelTitle(self.name.clone()),
        }
    }

    /// What deleting it takes with it: a voice channel's call, a text channel's messages.
    pub fn body(&self) -> Phrase {
        match self.kind {
            Kind::Text => Phrase::DeleteTextBody,
            Kind::Voice => Phrase::DeleteVoiceBody,
        }
    }

    /// The main button's words.
    pub fn action(&self) -> Sentence {
        match self.kind {
            Kind::Text => Sentence::DeleteTextAction(self.name.clone()),
            Kind::Voice => Sentence::DeleteChannelAction(self.name.clone()),
        }
    }

    /// Whether the server is being asked to delete it.
    pub fn asking(&self) -> bool {
        self.asked.asking()
    }

    /// Why the last delete was refused; none before one was.
    pub fn problem(&self) -> Option<ChannelProblem> {
        self.asked.refused()
    }

    pub(crate) fn ask(&mut self) {
        self.asked.ask();
    }

    pub(crate) fn refused(&mut self, refused: &LinkError) {
        self.asked.answered(Some(refusal(refused, Made::Renamed)));
    }
}

/// What the server's answer to a delete means: gone (deleted now, or already), or why not.
fn deleted(answer: &Result<(), LinkError>) -> bool {
    matches!(answer, Ok(()) | Err(LinkError::Refused(ErrorCode::NotFound)))
}

/// What an ask refused was for, which reads its `INVALID_REQUEST`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Made {
    /// A channel made: the cap on channels.
    Channel,
    /// A category made: the cap on categories.
    Category,
    /// Something renamed or deleted.
    Renamed,
}

fn refusal(refused: &LinkError, made: Made) -> ChannelProblem {
    match (refused.kind(), made) {
        // The name was shaped as the server shapes it, so a refused make is the cap.
        (ErrorKind::Refused(ErrorCode::InvalidRequest), Made::Channel) => ChannelProblem::Full,
        (ErrorKind::Refused(ErrorCode::InvalidRequest), Made::Category) => ChannelProblem::CategoriesFull,
        (ErrorKind::Refused(ErrorCode::NotFound), _) => ChannelProblem::Gone,
        (ErrorKind::Refused(ErrorCode::NotAuthorized), _) => ChannelProblem::NotAllowed,
        (ErrorKind::Refused(ErrorCode::RateLimited), _) => ChannelProblem::TooManyTries,
        (ErrorKind::Refused(_) | ErrorKind::Lasting | ErrorKind::Unreached, _) => ChannelProblem::NoAnswer,
    }
}

/// Deleting a channel is asked once at a time.
impl Confirming for Deleting {
    fn holds_outside(&self) -> bool {
        self.asking()
    }

    fn said(&mut self, said: DialogMessage) -> Next {
        match said {
            DialogMessage::Confirm if !self.asking() => {
                self.ask();
                Next::Asks(ops::delete_channel(&self.id))
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

impl Deleting {
    /// The server's answer: gone (now, or already), the dialog closing, or why not.
    pub(crate) fn answered(&mut self, answer: &Result<(), LinkError>) -> Next {
        if deleted(answer) {
            return Next::Closes;
        }
        if let Err(refused) = answer {
            self.refused(refused);
        }
        Next::Stays
    }
}

/// Deleting one category, which deletes its channels and their messages: asked first.
#[derive(Debug)]
pub struct DeletingCategory {
    pub(crate) id: CategoryId,
    name: String,
    holds: u32,
    asked: Asked<ChannelProblem>,
}

impl DeletingCategory {
    pub(crate) fn new(id: CategoryId, name: String, holds: u32) -> DeletingCategory {
        DeletingCategory { id, name, holds, asked: Asked::Idle }
    }

    /// The dialog's title.
    pub fn title(&self) -> Sentence {
        Sentence::DeleteChannelTitle(self.name.clone())
    }

    /// What it takes with it: its channels, by how many it holds.
    pub fn body(&self) -> Sentence {
        Sentence::DeleteCategoryBody(self.holds)
    }

    /// Whether the server is being asked to delete it.
    pub fn asking(&self) -> bool {
        self.asked.asking()
    }

    /// Why the last delete was refused; none before one was.
    pub fn problem(&self) -> Option<ChannelProblem> {
        self.asked.refused()
    }

    /// The server's answer: gone (now, or already), the dialog closing, or why not.
    pub(crate) fn answered(&mut self, answer: &Result<(), LinkError>) -> Next {
        match answer {
            Err(refused) if !deleted(answer) => {
                self.asked.answered(Some(refusal(refused, Made::Renamed)));
                Next::Stays
            }
            Ok(()) | Err(_) => Next::Closes,
        }
    }
}

/// Deleting a category is asked once at a time.
impl Confirming for DeletingCategory {
    fn holds_outside(&self) -> bool {
        self.asking()
    }

    fn said(&mut self, said: DialogMessage) -> Next {
        match said {
            DialogMessage::Confirm if !self.asking() => {
                self.asked.ask();
                Next::Asks(ops::delete_category(&self.id))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{Clock, channel_id, holding, member, refused, role_id, tell};
    use crate::harness::{sends, wire};
    use crate::place::{self, Lobby, PlaceId};
    use crate::settings::Page;
    use crate::window::{Asking, Dialog, DialogMessage, Pick, Window};
    use crate::{Ask, WindowMessage};
    use proto::chatkord_v1::{CategorySummary, ChannelKind, ChannelSummary, ChannelsSummary, ErrorCode};
    use proto::rules::{Grant, Grants};
    use std::time::Instant;
    use words::Sentence;

    /// An ask as a test compares it: its words, since an ask may carry a secret and is not compared.
    fn shown(ask: &impl std::fmt::Debug) -> String {
        format!("{ask:?}")
    }

    fn renaming(id: &str, name: &str) -> Naming {
        Naming::new(Named::Channel { id: channel_id(id), kind: Kind::Voice, name: name.into() })
    }

    #[test]
    fn a_name_is_trimmed_and_one_out_of_its_bounds_is_refused_before_any_ask() {
        let mut naming = Naming::new(Named::NewCategory);
        assert!(!naming.ready() && naming.problem().is_none(), "nothing typed: not ready, nothing said");
        naming.typed("n".repeat(MAX_NAME_CHARS + 1));
        assert_eq!(naming.problem(), Some(ChannelProblem::TooLong));
        assert!(naming.ask().is_none(), "never cut, never sent");
        naming.typed("a\nb".into());
        assert_eq!(naming.problem(), Some(ChannelProblem::Characters));
        assert!(naming.ask().is_none());
        naming.typed("  Projects ".into());
        assert_eq!(shown(&naming.ask()), shown(&Some(crate::place::ops::create_category("Projects".into()))));
        assert!(naming.asking() && !naming.ready(), "one ask at a time");
        naming.refused(&refused(ErrorCode::InvalidRequest));
        assert_eq!(naming.problem(), Some(ChannelProblem::CategoriesFull), "a refused category is the cap on them");
        assert_eq!(naming.title().in_english(), "Create a category");
    }

    #[test]
    fn a_refused_rename_says_why_and_a_text_channel_is_named_with_its_hash() {
        let mut naming = renaming("9", "Lounge");
        assert!(!naming.kept(), "the name it had is nothing typed");
        naming.typed("Big room".into());
        assert!(naming.kept());
        let asked = Some(crate::place::ops::rename_channel(&channel_id("9"), "Big room".into()));
        assert_eq!(shown(&naming.ask()), shown(&asked));
        naming.refused(&refused(ErrorCode::NotFound));
        assert_eq!(naming.problem(), Some(ChannelProblem::Gone));
        naming.refused(&refused(ErrorCode::NotAuthorized));
        assert_eq!(naming.problem(), Some(ChannelProblem::NotAllowed));
        naming.typed("Bigger room".into());
        assert_eq!(naming.problem(), None, "typing again clears the answer");
        let text = Naming::new(Named::Channel { id: channel_id("1"), kind: Kind::Text, name: "general".into() });
        assert_eq!(text.title().in_english(), "Rename #general");
        let deleting = Deleting::new(channel_id("1"), Kind::Text, "ideas".into());
        assert_eq!(deleting.title().in_english(), "Delete #ideas?");
        assert_eq!(Sentence::Phrase(deleting.body()).in_english(), "Its messages go with it. This can’t be undone.");
    }

    #[test]
    fn a_channel_already_gone_counts_as_deleted() {
        assert!(deleted(&Ok(())));
        assert!(deleted(&Err(refused(ErrorCode::NotFound))));
        assert!(!deleted(&Err(refused(ErrorCode::NotAuthorized))));
        let mut deleting = Deleting::new(channel_id("9"), Kind::Voice, "Lounge".into());
        deleting.refused(&refused(ErrorCode::NotAuthorized));
        assert_eq!(deleting.problem(), Some(ChannelProblem::NotAllowed));
    }

    #[test]
    fn the_category_menu_toggles_closes_on_a_pick_and_gives_way_to_an_add_menu() {
        let projects = projects();
        let mut making = Making::new(Kind::Text, chat(), true);
        making.update(MakingMessage::CategoryMenu);
        assert!(making.category_menu_open());
        making.update(MakingMessage::CategoryMenu);
        assert!(!making.category_menu_open(), "a second press closes it");
        making.update(MakingMessage::CategoryMenu);
        making.update(MakingMessage::Category(projects.clone()));
        assert!(!making.category_menu_open(), "a pick closes it");
        assert_eq!(*making.category(), projects);
        making.update(MakingMessage::CategoryMenu);
        making.update(MakingMessage::Adding(Access::Private));
        assert!(!making.category_menu_open(), "one menu at a time");
    }

    #[test]
    fn a_private_text_channel_is_made_with_its_first_role_and_the_rest_are_let_in_after_it() {
        let mut making = Making::new(Kind::Text, projects(), true);
        making.name = "staff".into();
        let (mods, hosts, grace) =
            (Whom::Role(role_id("mods")), Whom::Role(role_id("hosts")), Whom::Member(member("grace")));
        making.update(MakingMessage::Switch(Access::Private));
        for whom in [grace.clone(), mods.clone(), hosts.clone()] {
            making.update(MakingMessage::Allow(Access::Private, whom));
        }
        making.update(MakingMessage::Switch(Access::ReadOnly));
        making.update(MakingMessage::Allow(Access::ReadOnly, mods.clone()));
        let asked = making.ask();
        assert_eq!(
            shown(&asked),
            shown(&Some(crate::place::ops::create_channel(
                ChannelKind::Text,
                "staff".into(),
                &projects(),
                Some(Access::Private),
                Some(&role_id("mods")),
            ))),
            "the first access, with the first role it lists"
        );
        let made = channel_id("staff");
        let everyone = role_id("member");
        let set = |whom: &Whom, word, state| crate::place::ops::set_overwrite(made.clone(), whom, word, state);
        let (allow, deny) = (Some(Overwrite::Allow), Some(Overwrite::Deny));
        assert_eq!(
            shown(&making.after_made(&made, Some(&everyone), Some(&member("ada")), false)),
            shown(&[
                set(&grace, Grant::ChannelRead, allow),
                set(&hosts, Grant::ChannelRead, allow),
                set(&Whom::Member(member("ada")), Grant::SendMessage, allow),
                set(&Whom::Role(everyone.clone()), Grant::SendMessage, deny),
                set(&mods, Grant::SendMessage, allow),
            ]),
            "the rest of who sees it, then read-only: the maker first, then everyone denied, then who writes"
        );
        assert_eq!(
            shown(&making.after_made(&made, Some(&everyone), Some(&member("ada")), true)[2]),
            shown(&set(&Whom::Role(everyone), Grant::SendMessage, deny)),
            "the owner writes everywhere: not listed"
        );
        let mut voice = Making::new(Kind::Voice, chat(), true);
        voice.name = "Lounge".into();
        voice.update(MakingMessage::Switch(Access::ReadOnly));
        assert!(
            matches!(voice.ask(), Some(ask) if crate::harness::is(&ask, crate::harness::Op::ChannelCreate) && matches!(ask.op(), Some(crate::harness::Body::ChannelCreate(made)) if made.access.is_none() && made.role_id.is_none())),
            "read-only is a text channel's alone"
        );
        let mut plain = Making::new(Kind::Text, chat(), false);
        plain.name = "general".into();
        plain.update(MakingMessage::Switch(Access::Private));
        assert!(
            matches!(plain.ask(), Some(ask) if matches!(ask.op(), Some(crate::harness::Body::ChannelCreate(made)) if made.access.is_none())),
            "without Manage roles, who sees it is not offered"
        );
    }

    fn chat() -> terms::CategoryId {
        terms::CategoryId::parse("chat").unwrap()
    }

    fn projects() -> terms::CategoryId {
        terms::CategoryId::parse("projects").unwrap()
    }

    fn channel_manager(clock: &Clock) -> (Window, PlaceId) {
        holding(clock, [Grant::ManageChannel].into_iter().collect())
    }

    fn naming(window: &Window) -> &Naming {
        match window.dialog().map(Dialog::asking) {
            Some(Asking::Name(naming)) => naming,
            other => panic!("not naming: {other:?}"),
        }
    }

    fn making(window: &Window) -> &Making {
        match window.dialog().map(Dialog::asking) {
            Some(Asking::MakeChannel(making)) => making,
            other => panic!("not making a channel: {other:?}"),
        }
    }

    #[test]
    fn channels_are_offered_only_to_whoever_manages_them() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = holding(&clock, Grants::default());
        assert!(window.update(WindowMessage::CreateChannel(owls_id), clock.at()).is_empty());
        window.update(WindowMessage::Channel(owls_id, ChannelAction::Delete, channel_id("lounge")), clock.at());
        assert!(window.dialog().is_none(), "no page, no \"+\", no row buttons");
        let (mut window, owls_id) = channel_manager(&clock);
        let todo = window.update(WindowMessage::Pick(owls_id, Pick::Settings), clock.at());
        assert_eq!(window.open().unwrap().settings().map(crate::ServerSettings::page), Some(Page::Channels));
        assert!(todo.is_empty(), "the page shows the list the pane has: {todo:?}");
        window.update(WindowMessage::Channel(owls_id, ChannelAction::Rename, channel_id("nope")), clock.at());
        assert!(window.dialog().is_none(), "a channel no longer listed does nothing");
    }

    #[test]
    fn a_channel_is_made_from_its_kind_and_a_typed_name_and_the_new_list_shows_it() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = channel_manager(&clock);
        window.update(WindowMessage::CreateChannel(owls_id), clock.at());
        assert!(!making(&window).offers_access(), "who sees it is Manage roles'");
        assert!(window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at()).is_empty(), "no name, no ask");
        window.update(WindowMessage::Dialog(DialogMessage::Making(MakingMessage::Kind(Kind::Voice))), clock.at());
        window.update(WindowMessage::Dialog(DialogMessage::Named("  Movie night ".into())), clock.at());
        window.update(WindowMessage::Dialog(DialogMessage::Outside), clock.at());
        assert_eq!(making(&window).name(), "  Movie night ", "a press outside keeps a typed name");
        let todo = window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert!(
            matches!(&todo[..], [Ask::At(_, ask)]
                if matches!(ask.op(), Some(crate::harness::Body::ChannelCreate(made))
                    if made.kind == i32::from(wire::ChannelKind::Voice) && made.name == "Movie night" && made.category_id == "chat")),
            "{todo:?}"
        );
        let full = place::Message::Page(place::PageMessage::ChannelCreated(Err(refused(ErrorCode::InvalidRequest))));
        tell(&mut window, &clock, owls_id, full);
        assert_eq!(making(&window).problem(), Some(ChannelProblem::Full));
        window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        let listed = |id: &str, name: &str| ChannelSummary {
            channel_id: id.into(),
            name: name.into(),
            kind: ChannelKind::Voice.into(),
            timeline_id: format!("t-{id}"),
            category_id: "chat".into(),
            ..Default::default()
        };
        let channels = vec![listed("lounge", "Lounge"), listed("movie", "Movie night")];
        let made = place::Message::Page(place::PageMessage::ChannelCreated(Ok(ChannelsSummary {
            channels,
            version: 1,
            standings: Vec::new(),
            categories: vec![CategorySummary { category_id: "chat".into(), name: "Chat".into(), position: 0 }],
        })));
        let todo = tell(&mut window, &clock, owls_id, made);
        assert!(todo.is_empty(), "nothing more to set: {todo:?}");
        assert!(window.dialog().is_none());
        assert_eq!(window.toast().map(|toast| toast.said().in_english()).as_deref(), None, "the list shows it");
        assert!(matches!(window.open().unwrap().lobby(), Some(Lobby::Ready(channels)) if channels.voice.len() == 2));
    }

    #[test]
    fn a_private_channel_made_from_the_dialog_asks_the_rest_of_who_sees_it_once_it_is_made() {
        let clock = Clock(Instant::now());
        let grants = [Grant::ManageChannel, Grant::ManageRoles, Grant::ChannelRead].into_iter().collect();
        let (mut window, owls_id) = crate::harness::placed_holding(&clock, crate::harness::ADMIN_PLACE, grants);
        window.update(WindowMessage::CreateChannel(owls_id), clock.at());
        assert!(making(&window).offers_access());
        let say = |window: &mut Window, message| {
            window.update(WindowMessage::Dialog(DialogMessage::Making(message)), clock.at())
        };
        say(&mut window, MakingMessage::Switch(Access::Private));
        say(&mut window, MakingMessage::Allow(Access::Private, Whom::Role(role_id("mods"))));
        say(&mut window, MakingMessage::Allow(Access::Private, Whom::Member(member("grace"))));
        window.update(WindowMessage::Dialog(DialogMessage::Named("staff".into())), clock.at());
        let todo = window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert!(
            matches!(&todo[..], [Ask::At(_, ask)]
                if matches!(ask.op(), Some(crate::harness::Body::ChannelCreate(made))
                    if made.access == Some(i32::from(wire::ChannelAccess::Private)) && made.role_id.as_deref() == Some("mods"))),
            "{todo:?}"
        );
        let text = |id: &str| ChannelSummary {
            channel_id: id.into(),
            name: id.into(),
            kind: ChannelKind::Text.into(),
            timeline_id: format!("t-{id}"),
            private: id == "staff",
            category_id: "chat".into(),
            ..Default::default()
        };
        let chat = CategorySummary { category_id: "chat".into(), name: "Chat".into(), position: 0 };
        let made = ChannelsSummary {
            channels: vec![text("general"), text("staff")],
            version: 1,
            categories: vec![chat],
            ..Default::default()
        };
        let todo =
            tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::ChannelCreated(Ok(made))));
        assert!(
            matches!(&todo[..], [Ask::At(_, ask)]
                if matches!(ask.op(), Some(crate::harness::Body::SetChannelOverwrite(set))
                    if set.channel_id == "staff"
                        && set.whom == Some(wire::set_channel_overwrite::Whom::UserId("grace".into()))
                        && set.word == Grant::ChannelRead.word()
                        && set.state == i32::from(wire::OverwriteState::Allow))),
            "the member let in after the channel is made: {todo:?}"
        );
        let Some(Lobby::Ready(listed)) = window.open().unwrap().lobby() else { panic!("listed") };
        assert_eq!(listed.sight(&channel_id("staff")), place::Sight::Private, "marked private from the list");
    }

    #[test]
    fn a_rename_starts_from_the_name_it_has_and_a_refusal_stays_in_the_dialog() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = channel_manager(&clock);
        window.update(WindowMessage::Channel(owls_id, ChannelAction::Rename, channel_id("lounge")), clock.at());
        assert_eq!((naming(&window).renaming(), naming(&window).name()), (Some("Lounge"), "Lounge"));
        window.update(WindowMessage::Dialog(DialogMessage::Outside), clock.at());
        assert!(window.dialog().is_none(), "nothing typed: a press outside closes it");
        window.update(WindowMessage::Channel(owls_id, ChannelAction::Rename, channel_id("general")), clock.at());
        assert_eq!(naming(&window).title().in_english(), "Rename #general", "a text channel is renamed too");
        window.update(WindowMessage::Dialog(DialogMessage::Cancel), clock.at());
        window.update(WindowMessage::Channel(owls_id, ChannelAction::Rename, channel_id("lounge")), clock.at());
        window.update(WindowMessage::Dialog(DialogMessage::Named("Big room".into())), clock.at());
        let todo = window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert!(
            matches!(&todo[..], [Ask::At(_, ask)]
                if sends(ask, wire::ChannelRename { channel_id: "lounge".into(), name: "Big room".into() })),
            "{todo:?}"
        );
        let gone = place::Message::Page(place::PageMessage::ChannelRenamed(Err(refused(ErrorCode::NotFound))));
        tell(&mut window, &clock, owls_id, gone);
        assert_eq!(naming(&window).problem(), Some(ChannelProblem::Gone));
        window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::ChannelRenamed(Ok(()))));
        assert!(window.dialog().is_none());
        assert_eq!(window.toast().map(|toast| toast.said().in_english()).as_deref(), None, "the list shows it");
    }

    #[test]
    fn a_delete_is_asked_first_and_one_already_gone_closes_quietly() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = channel_manager(&clock);
        let delete = |window: &mut Window| {
            window.update(WindowMessage::Channel(owls_id, ChannelAction::Delete, channel_id("lounge")), clock.at());
            window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at())
        };
        let todo = delete(&mut window);
        assert!(
            matches!(&todo[..], [Ask::At(_, ask)] if sends(ask, wire::ChannelDelete { channel_id: "lounge".into() })),
            "{todo:?}"
        );
        window.update(WindowMessage::Dialog(DialogMessage::Outside), clock.at());
        assert!(
            window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at()).is_empty(),
            "one ask at a time"
        );
        let not_allowed =
            place::Message::Page(place::PageMessage::ChannelDeleted(Err(refused(ErrorCode::NotAuthorized))));
        tell(&mut window, &clock, owls_id, not_allowed);
        assert!(matches!(
            window.dialog().map(Dialog::asking),
            Some(Asking::DeleteChannel(deleting)) if deleting.problem() == Some(ChannelProblem::NotAllowed)
        ));
        window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert!(
            matches!(
                window.dialog().map(Dialog::asking),
                Some(Asking::DeleteChannel(deleting)) if deleting.asking() && deleting.problem().is_none()
            ),
            "asked again, the old refusal is gone"
        );
        tell(&mut window, &clock, owls_id, place::Message::Page(place::PageMessage::ChannelDeleted(Ok(()))));
        assert!(window.dialog().is_none());
        assert_eq!(window.toast().map(|toast| toast.said().in_english()).as_deref(), None, "the list shows it gone");
        delete(&mut window);
        let gone = place::Message::Page(place::PageMessage::ChannelDeleted(Err(refused(ErrorCode::NotFound))));
        tell(&mut window, &clock, owls_id, gone);
        assert!(
            window.dialog().is_none() && window.toast().map(|toast| toast.said().in_english()).as_deref().is_none(),
            "one already gone leaves quietly"
        );
    }

    #[test]
    fn a_channel_is_made_in_the_first_category_and_not_at_all_where_there_is_none() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = channel_manager(&clock);
        window.update(WindowMessage::CreateChannel(owls_id), clock.at());
        assert_eq!(*making(&window).category(), chat(), "the first category is chosen");
        window.update(WindowMessage::Dialog(DialogMessage::Cancel), clock.at());
        let bare = place::listed(&ChannelsSummary { version: 1, ..Default::default() });
        tell(&mut window, &clock, owls_id, place::Message::Lobby(place::LobbyMessage::Channels(Ok(bare))));
        window.update(WindowMessage::CreateChannel(owls_id), clock.at());
        assert!(window.dialog().is_none(), "no category, no channel");
        window.update(WindowMessage::CreateChannelIn(owls_id, chat()), clock.at());
        assert!(window.dialog().is_none(), "a category no longer listed is not chosen");
    }

    #[test]
    fn a_category_s_plus_makes_a_channel_in_it_for_whoever_manages_the_channels() {
        let clock = Clock(Instant::now());
        let hangouts = chat();
        let (mut window, owls_id) = holding(&clock, Grants::default());
        window.update(WindowMessage::CreateChannelIn(owls_id, hangouts.clone()), clock.at());
        assert!(window.dialog().is_none(), "no \"+\" without the word");
        let (mut window, owls_id) = channel_manager(&clock);
        window.update(WindowMessage::CreateChannelIn(owls_id, hangouts.clone()), clock.at());
        assert_eq!(*making(&window).category(), hangouts);
    }
}
