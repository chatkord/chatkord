//! Server settings › Roles: the server's roles in their order, the one chosen, and its name, colour and words, each
//! changed only where the asker's place and words allow, as the server rules it; a role made and one deleted, asked in
//! a dialog. A change is shown at once, and a refused one put back.

use crate::Asked;
use crate::colour::{self, Picker};
use crate::loaded::Loaded;
use crate::name_field::{self, NameField};
use crate::order::Moved;
use crate::place;
use crate::place::ops;
use crate::window::{Confirming, DialogMessage, Next};
use proto::chatkord_v1::{ErrorCode, RoleEntry};
use proto::rules::{Colour, Grant, Grants, MAX_NAME_CHARS, Place, TextRefused, text_shape};
use terms::{ErrorKind, LinkError, RoleId};
use words::{Phrase, Sentence, Words};

/// What a role is in the order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoleKind {
    /// The owner role, first: it holds every word and only moves with the server's ownership.
    Owner,
    /// A role made on the server, given and taken.
    Made,
    /// The default role, last: everyone holds it.
    Default,
}

/// A role as the page lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListedRole {
    /// Its id at the server.
    pub id: RoleId,
    /// What it is shown as; two roles may share a name.
    pub name: String,
    /// The colour it is shown in; none for none.
    pub colour: Option<Colour>,
    /// Where it stands in the order.
    pub place: Place,
    /// The words it gives its holders.
    pub grants: Grants,
    /// The owner role, the default one, or one made.
    pub kind: RoleKind,
}

impl ListedRole {
    /// The role `entry` lists; none when it names no id, a server not keeping the protocol.
    pub(crate) fn read(entry: &RoleEntry) -> Option<ListedRole> {
        let kind = match (entry.is_owner, entry.is_default) {
            (true, _) => RoleKind::Owner,
            (false, true) => RoleKind::Default,
            (false, false) => RoleKind::Made,
        };
        Some(ListedRole {
            id: RoleId::parse(&entry.role_id)?,
            name: entry.name.clone(),
            colour: place::wire_colour(entry.colour.as_deref()),
            place: Place::at(entry.position),
            grants: Grants::from_words(&entry.grants),
            kind,
        })
    }

    /// Whether someone at `asker` may change it: never the owner role, and only a role below their place.
    pub fn changeable(&self, asker: Place) -> bool {
        self.kind != RoleKind::Owner && asker.outranks(self.place)
    }

    /// Whether someone at `asker` may move it: a role they may change, never the default one.
    pub fn movable(&self, asker: Place) -> bool {
        self.kind == RoleKind::Made && self.changeable(asker)
    }
}

/// The grant words as a role's page groups them, under each group's heading, in the order shown.
pub const WORD_GROUPS: [(Phrase, &[Grant]); 3] = [
    (
        Phrase::WordsChannels,
        &[
            Grant::ChannelRead,
            Grant::SendMessage,
            Grant::AddReactions,
            Grant::AttachFiles,
            Grant::ManageMessages,
            Grant::PinMessages,
            Grant::MentionEveryone,
        ],
    ),
    (Phrase::WordsCalls, &[Grant::StartVoice, Grant::ShareScreen, Grant::ShareCamera, Grant::ModerateVoice]),
    (
        Phrase::WordsServer,
        &[
            Grant::InviteMember,
            Grant::ManageChannel,
            Grant::ManageMembers,
            Grant::ManageRoles,
            Grant::ManageEmoji,
            Grant::ManageWebhooks,
            Grant::ManageServer,
        ],
    ),
];

/// A grant word's name on a role's page, and the line under it that says what it lets its holders do.
pub const fn grant_words(grant: Grant) -> (Phrase, Phrase) {
    match grant {
        Grant::ChannelRead => (Phrase::GrantChannelRead, Phrase::GrantChannelReadLine),
        Grant::SendMessage => (Phrase::GrantSendMessage, Phrase::GrantSendMessageLine),
        Grant::AddReactions => (Phrase::GrantAddReactions, Phrase::GrantAddReactionsLine),
        Grant::AttachFiles => (Phrase::GrantAttachFiles, Phrase::GrantAttachFilesLine),
        Grant::ManageMessages => (Phrase::GrantManageMessages, Phrase::GrantManageMessagesLine),
        Grant::PinMessages => (Phrase::GrantPinMessages, Phrase::GrantPinMessagesLine),
        Grant::MentionEveryone => (Phrase::GrantMentionEveryone, Phrase::GrantMentionEveryoneLine),
        Grant::StartVoice => (Phrase::GrantStartVoice, Phrase::GrantStartVoiceLine),
        Grant::ShareScreen => (Phrase::GrantShareScreen, Phrase::GrantShareScreenLine),
        Grant::ShareCamera => (Phrase::GrantShareCamera, Phrase::GrantShareCameraLine),
        Grant::ModerateVoice => (Phrase::GrantModerateVoice, Phrase::GrantModerateVoiceLine),
        Grant::InviteMember => (Phrase::GrantInviteMember, Phrase::GrantInviteMemberLine),
        Grant::ManageChannel => (Phrase::GrantManageChannel, Phrase::GrantManageChannelLine),
        Grant::ManageMembers => (Phrase::GrantManageMembers, Phrase::GrantManageMembersLine),
        Grant::ManageRoles => (Phrase::GrantManageRoles, Phrase::GrantManageRolesLine),
        Grant::ManageEmoji => (Phrase::GrantManageEmoji, Phrase::GrantManageEmojiLine),
        Grant::ManageWebhooks => (Phrase::GrantManageWebhooks, Phrase::GrantManageWebhooksLine),
        Grant::ManageServer => (Phrase::GrantManageServer, Phrase::GrantManageServerLine),
    }
}

/// The colour a role's picker starts from while it has none: the edge the list draws such a role's dot in, as the dark
/// scheme draws it.
fn uncoloured() -> Colour {
    let edge = design::Palette::new(design::Scheme::Dark, design::Accent::DEFAULT)
        .color(design::Role::Border(design::Border::Strong));
    Colour::new(edge.r, edge.g, edge.b)
}

/// Which of the page shows where the list and the chosen role do not fit side by side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Step {
    /// The list.
    #[default]
    List,
    /// The chosen role, opened from the list.
    Role,
}

/// The chosen role's name and colour as they are being changed.
#[derive(Clone, Debug, PartialEq)]
pub struct Editor {
    role: RoleId,
    name: NameField,
    colour: Picker,
}

impl Editor {
    fn of(role: &ListedRole) -> Editor {
        Editor {
            role: role.id.clone(),
            name: NameField::required(role.name.clone()),
            colour: Picker::new(role.colour, uncoloured()),
        }
    }

    /// The name's field.
    pub fn name(&self) -> &NameField {
        &self.name
    }

    /// The colour's picker.
    pub fn colour(&self) -> &Picker {
        &self.colour
    }
}

/// What a person does on the page.
#[derive(Clone, Debug)]
pub enum Message {
    /// A role in the list chosen: a name typed for the one chosen before is saved, and a narrow page opens it.
    Choose(RoleId),
    /// Back to the list from a role opened alone: a name typed is saved.
    Back,
    /// Something done in the name's field.
    Name(name_field::Message),
    /// The focus left the name's field: what is typed is saved.
    NameLeft,
    /// Something done in the colour's picker.
    Colour(colour::Message),
    /// A word's switch, to give the chosen role the word or take it.
    Word(Grant, bool),
    /// A role dragged, or moved by Alt and an arrow key.
    Move(Moved<RoleId>),
}

/// What the page asks of the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Ask {
    Name { role: RoleId, name: String },
    Colour { role: RoleId, name: String, colour: Option<Colour> },
    Word { role: RoleId, name: String, grant: Grant, held: bool },
    Move { role: RoleId, name: String, before: Option<RoleId> },
}

impl From<Ask> for place::Ask {
    fn from(ask: Ask) -> place::Ask {
        match ask {
            Ask::Name { role, name } => ops::set_role_name(role, name),
            Ask::Colour { role, name, colour } => ops::set_role_colour(&role, name, colour),
            Ask::Word { role, name, grant, held } => ops::set_role_word(&role, name, grant, held),
            Ask::Move { role, name, before } => ops::move_role(&role, name, before.as_ref()),
        }
    }
}

/// The server's roles in their order, the owner's first and the default one last, as this place last read them: one
/// list the roles page edits and every other place that names a role reads. A change the roles page shows at once is
/// made here, and put back here when the server refuses it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoleList {
    listed: Loaded<Vec<ListedRole>>,
    /// The order before a move shown at once, put back if the server refuses it.
    unmoved: Option<Vec<ListedRole>>,
}

impl RoleList {
    /// The roles as last read: loading until a first answer, failed only when no list was ever had.
    pub fn listed(&self) -> &Loaded<Vec<ListedRole>> {
        &self.listed
    }

    /// The roles in their order; none until they are read.
    pub fn roles(&self) -> &[ListedRole] {
        match &self.listed {
            Loaded::Ready(roles) => roles,
            Loaded::Loading | Loaded::Failed(_) => &[],
        }
    }

    /// The role with `id`, as listed.
    pub fn role(&self, id: &RoleId) -> Option<&ListedRole> {
        self.roles().iter().find(|role| role.id == *id)
    }

    /// The default role, last in the order.
    pub fn default_role(&self) -> Option<&ListedRole> {
        self.roles().iter().find(|role| role.kind == RoleKind::Default)
    }

    /// The roles someone at `asker` holding `grants` may give, in the order: under `MANAGE_ROLES`, every role made
    /// below the asker that holds no word they lack; none without it. What the server allows a member's roles and an
    /// invite's role alike.
    pub fn givable(&self, asker: Place, grants: Grants) -> impl Iterator<Item = &ListedRole> {
        let manages = grants.holds(Grant::ManageRoles);
        self.roles().iter().filter(move |role| {
            manages && role.kind == RoleKind::Made && asker.outranks(role.place) && grants.holds_all(role.grants)
        })
    }

    /// The name of the role an invite that names `role` gives beside the default one, the default role's for an invite
    /// naming none; Member where the roles are not read yet.
    pub fn granted_name(&self, role: Option<&RoleId>, words: &Words) -> String {
        let granted = match role {
            Some(id) => self.role(id),
            None => self.default_role(),
        };
        granted.map_or_else(|| words.phrase(Phrase::MemberRole).into_owned(), |role| role.name.clone())
    }

    /// The roles of `held` this list knows, the default one left out, highest first.
    pub fn held<'a>(&'a self, held: &[RoleId]) -> Vec<&'a ListedRole> {
        self.roles().iter().filter(|role| role.kind != RoleKind::Default && held.contains(&role.id)).collect()
    }

    /// What a person's popover and their row in Members name: [`RoleList::held`], or the default role where it is
    /// empty, so nobody reads as having no role.
    pub fn shown<'a>(&'a self, held: &[RoleId]) -> Vec<&'a ListedRole> {
        let named = self.held(held);
        if named.is_empty() { self.default_role().into_iter().collect() } else { named }
    }

    /// Where someone holding `held` stands in the order: their highest known role's place, else the bottom.
    pub fn place_of(&self, held: &[RoleId]) -> Place {
        self.held(held).first().map_or(Place::BOTTOM, |role| role.place)
    }

    /// The words someone holding `held` has from their roles, the default one's among them.
    pub fn grants_of(&self, held: &[RoleId]) -> Grants {
        self.roles()
            .iter()
            .filter(|role| role.kind == RoleKind::Default || held.contains(&role.id))
            .fold(Grants::NONE, |grants, role| grants.union(role.grants))
    }

    /// Whether someone at `asker` may make the move `moved`: a role they may move, to sit just above another role
    /// below their place or just above the default role, and not where it already is.
    pub fn may_move(&self, moved: &Moved<RoleId>, asker: Place) -> bool {
        self.moved_before(moved, asker).is_some()
    }

    /// What the server is asked for `moved`: the role it goes just above, none for just above the default role; none
    /// at all for a move that may not be made.
    fn moved_before(&self, moved: &Moved<RoleId>, asker: Place) -> Option<Option<RoleId>> {
        let roles = self.roles();
        let at = roles.iter().position(|role| role.id == moved.key)?;
        if !roles[at].movable(asker) {
            return None;
        }
        let before = roles.iter().position(|role| Some(&role.id) == moved.before.as_ref())?;
        let target = &roles[before];
        if before == at || before == at + 1 || !target.changeable(asker) {
            return None;
        }
        Some(match target.kind {
            RoleKind::Default => None,
            RoleKind::Made => Some(target.id.clone()),
            RoleKind::Owner => return None,
        })
    }

    /// The server's list of roles. A list that could not be read leaves one already had.
    pub(crate) fn read(&mut self, listed: Result<Vec<RoleEntry>, LinkError>) {
        match listed {
            Ok(entries) => {
                self.listed = Loaded::Ready(entries.iter().filter_map(ListedRole::read).collect());
                self.unmoved = None;
            }
            Err(_) if matches!(self.listed, Loaded::Ready(_)) => {}
            Err(failed) => self.listed = Loaded::Failed(failed),
        }
    }

    /// A role deleted: it leaves the list at once.
    pub(crate) fn deleted(&mut self, role: &RoleId) {
        if let Loaded::Ready(roles) = &mut self.listed {
            roles.retain(|listed| &listed.id != role);
        }
    }

    /// The server refused a change shown at once: a move is put back; the list is read again either way.
    pub(crate) fn refused(&mut self) {
        if let (Some(unmoved), Loaded::Ready(roles)) = (self.unmoved.take(), &mut self.listed) {
            *roles = unmoved;
        }
    }

    /// The server took a move: the order shown is the order kept.
    pub(crate) fn kept(&mut self) {
        self.unmoved = None;
    }
}

/// The page, open: the role chosen and what is typed for it; the roles themselves are the place's [`RoleList`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Roles {
    /// The role chosen; none until the list is read.
    chosen: Option<RoleId>,
    step: Step,
    editor: Option<Editor>,
}

/// The roles page as a view draws it: the page and the place's roles together.
#[derive(Clone, Copy, Debug)]
pub struct RolesView<'a> {
    page: &'a Roles,
    list: &'a RoleList,
}

impl<'a> RolesView<'a> {
    pub(crate) fn new(page: &'a Roles, list: &'a RoleList) -> RolesView<'a> {
        RolesView { page, list }
    }

    /// The roles in their order, the owner's first and the default one last.
    pub fn listed(&self) -> &'a Loaded<Vec<ListedRole>> {
        self.list.listed()
    }

    /// The role chosen, as listed.
    pub fn chosen(&self) -> Option<&'a ListedRole> {
        self.page.chosen(self.list)
    }

    /// The chosen role's name and colour, while it is one that may be changed by someone.
    pub fn editor(&self) -> Option<&'a Editor> {
        self.page.editor.as_ref().filter(|editor| Some(&editor.role) == self.page.chosen.as_ref())
    }

    /// The default role, last in the order.
    pub fn default_role(&self) -> Option<&'a ListedRole> {
        self.list.default_role()
    }

    /// Whether someone at `asker` may make the move `moved`.
    pub fn may_move(&self, moved: &Moved<RoleId>, asker: Place) -> bool {
        self.list.may_move(moved, asker)
    }

    /// Where a narrow page is: the list, or the chosen role opened from it.
    pub(crate) fn step(&self) -> Step {
        self.page.step
    }
}

impl Roles {
    /// The role chosen, as `list` lists it.
    fn chosen<'a>(&self, list: &'a RoleList) -> Option<&'a ListedRole> {
        list.roles().iter().find(|role| Some(&role.id) == self.chosen.as_ref())
    }

    /// The page follows the list as read again: the choice and what is typed for it stay while it is listed, and
    /// follow a change made elsewhere that nobody is making here.
    pub(crate) fn follow(&mut self, list: &RoleList, asker: Place) {
        let roles = list.roles();
        let kept = self.chosen.as_ref().is_some_and(|chosen| roles.iter().any(|role| &role.id == chosen));
        if !kept {
            self.chosen = roles
                .iter()
                .find(|role| role.changeable(asker))
                .or_else(|| list.default_role())
                .map(|role| role.id.clone());
        }
        self.follow_editor(list);
    }

    /// The editor follows the chosen role as listed, or is made for one chosen anew.
    fn follow_editor(&mut self, list: &RoleList) {
        let Some(role) = self.chosen(list).cloned() else {
            self.editor = None;
            return;
        };
        match self.editor.as_mut().filter(|editor| editor.role == role.id) {
            Some(editor) => {
                editor.name.follow(role.name.clone());
                editor.colour.follow(role.colour);
            }
            None => self.editor = (role.kind != RoleKind::Owner).then(|| Editor::of(&role)),
        }
    }

    /// The list the server answered a role made with: the new role is chosen, and what was typed for the one chosen
    /// before is saved.
    pub(crate) fn created(&mut self, list: &mut RoleList, entries: Vec<RoleEntry>, asker: Place) -> Vec<Ask> {
        let made = entries.iter().filter_map(ListedRole::read).find(|role| list.role(&role.id).is_none());
        let saved =
            if self.chosen(list).is_some_and(|role| role.changeable(asker)) { self.left(list) } else { Vec::new() };
        if let Some(made) = made {
            self.chosen = Some(made.id);
            self.step = Step::Role;
        }
        list.read(Ok(entries));
        self.follow(list, asker);
        saved
    }

    /// A role deleted: the default role is chosen in its place.
    pub(crate) fn deleted(&mut self, list: &RoleList, role: &RoleId) {
        if self.chosen.as_ref() == Some(role) {
            self.chosen = list.default_role().map(|default| default.id.clone());
            self.step = Step::List;
            self.follow_editor(list);
        }
    }

    /// What leaving the page, or the chosen role, saves: a name typed and not yet sent, and a colour the open picker
    /// holds.
    pub(crate) fn left(&mut self, list: &RoleList) -> Vec<Ask> {
        let Some(role) = self.chosen(list).cloned() else { return Vec::new() };
        let Some(editor) = self.editor.as_mut().filter(|editor| editor.role == role.id) else { return Vec::new() };
        let name = editor.name.ask().flatten().map(|name| Ask::Name { role: role.id.clone(), name });
        let colour = if editor.colour.is_open() { editor.colour.update(colour::Message::Done) } else { None };
        let colour = colour.map(|colour| Ask::Colour { role: role.id.clone(), name: role.name.clone(), colour });
        name.into_iter().chain(colour).collect()
    }

    /// What `message` does to the page and the place's `list`, by someone at `asker` holding `grants`, and what it asks
    /// of the server.
    pub(crate) fn update(&mut self, message: Message, list: &mut RoleList, asker: Place, grants: Grants) -> Vec<Ask> {
        let editable = self.chosen(list).is_some_and(|role| role.changeable(asker));
        match message {
            Message::Choose(role) => {
                if list.role(&role).is_none() {
                    return Vec::new();
                }
                let saved = if editable { self.left(list) } else { Vec::new() };
                self.chosen = Some(role);
                self.step = Step::Role;
                self.follow_editor(list);
                saved
            }
            Message::Back => {
                self.step = Step::List;
                if editable { self.left(list) } else { Vec::new() }
            }
            Message::Name(_) | Message::NameLeft | Message::Colour(_) | Message::Word(..) if !editable => Vec::new(),
            Message::Name(typed) => {
                let Some(role) = self.chosen.clone() else { return Vec::new() };
                let Some(editor) = self.editor.as_mut() else { return Vec::new() };
                editor.name.update(typed).flatten().map(|name| Ask::Name { role, name }).into_iter().collect()
            }
            Message::NameLeft => {
                let Some(role) = self.chosen.clone() else { return Vec::new() };
                let Some(editor) = self.editor.as_mut() else { return Vec::new() };
                editor.name.ask().flatten().map(|name| Ask::Name { role, name }).into_iter().collect()
            }
            Message::Colour(picked) => {
                let Some(role) = self.chosen(list).cloned() else { return Vec::new() };
                let Some(editor) = self.editor.as_mut() else { return Vec::new() };
                let kept = editor.colour.update(picked);
                kept.map(|colour| Ask::Colour { role: role.id, name: role.name, colour }).into_iter().collect()
            }
            Message::Word(grant, held) => {
                if !grants.holds(grant) {
                    return Vec::new();
                }
                let Loaded::Ready(roles) = &mut list.listed else { return Vec::new() };
                let Some(role) = roles.iter_mut().find(|role| Some(&role.id) == self.chosen.as_ref()) else {
                    return Vec::new();
                };
                if role.grants.holds(grant) == held {
                    return Vec::new();
                }
                role.grants = if held { role.grants.with(grant) } else { role.grants.without(Grants::of(grant)) };
                vec![Ask::Word { role: role.id.clone(), name: role.name.clone(), grant, held }]
            }
            Message::Move(moved) => {
                let Some(before) = list.moved_before(&moved, asker) else { return Vec::new() };
                let Loaded::Ready(roles) = &mut list.listed else { return Vec::new() };
                let Some(at) = roles.iter().position(|role| role.id == moved.key) else { return Vec::new() };
                list.unmoved.get_or_insert_with(|| roles.clone());
                let role = roles.remove(at);
                let to = roles.iter().position(|one| Some(&one.id) == moved.before.as_ref()).unwrap_or(roles.len());
                let name = role.name.clone();
                roles.insert(to, role);
                vec![Ask::Move { role: moved.key, name, before }]
            }
        }
    }

    /// The server's answer to a name saved.
    pub(crate) fn named(&mut self, role: &RoleId, answer: &Result<(), LinkError>) {
        if let Some(editor) = self.editor.as_mut().filter(|editor| &editor.role == role) {
            editor.name.answered(answer);
        }
    }
}

/// What the page says when the server would not change the role named `role` at the server shown as `server`; none
/// when it went through or the role is gone.
pub(crate) fn change_refused(role: String, server: String, answer: &Result<(), LinkError>) -> Option<Sentence> {
    match answer.as_ref().map_err(crate::proof::refusal) {
        Ok(()) | Err(None) => None,
        Err(Some(crate::proof::Refused::NotAllowed)) => Some(Sentence::CantChangeRole(role)),
        Err(Some(crate::proof::Refused::TooManyTries)) => Some(Sentence::Phrase(Phrase::TooManyTries)),
        Err(Some(crate::proof::Refused::WrongPassword | crate::proof::Refused::NoAnswer)) => {
            Some(Sentence::NoAnswer(server))
        }
    }
}

/// Why a role was not made or deleted, or its name cannot be sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoleProblem {
    /// The name is over its bound in characters.
    TooLong,
    /// The name holds a line break or another control character.
    Characters,
    /// The server holds as many roles as it may.
    Full,
    /// This account may not change that role (any more).
    NotAllowed,
    /// The server refused: too many asks too soon.
    TooManyTries,
    /// The server could not be reached, or did not answer.
    NoAnswer,
}

fn problem(refused: &LinkError, creating: bool) -> RoleProblem {
    match refused.kind() {
        // The name was shaped as the server shapes it, so a refused make is the cap on roles.
        ErrorKind::Refused(ErrorCode::InvalidRequest) if creating => RoleProblem::Full,
        ErrorKind::Refused(ErrorCode::NotAuthorized) => RoleProblem::NotAllowed,
        ErrorKind::Refused(ErrorCode::RateLimited) => RoleProblem::TooManyTries,
        ErrorKind::Refused(_) | ErrorKind::Lasting | ErrorKind::Unreached => RoleProblem::NoAnswer,
    }
}

/// A new role's name being typed.
#[derive(Debug)]
pub struct Creating {
    name: String,
    /// The default role's name, which the new role starts just above.
    above: String,
    asked: Asked<RoleProblem>,
}

impl Creating {
    pub(crate) fn new(above: String) -> Creating {
        Creating { name: String::new(), above, asked: Asked::Idle }
    }

    /// The name as it is typed, untrimmed.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The default role's name, which the new role starts just above.
    pub fn above(&self) -> &str {
        &self.above
    }

    /// Whether the server is being asked to make it.
    pub fn asking(&self) -> bool {
        self.asked.asking()
    }

    /// What is wrong, said under the field: the name as it is typed, else the server's answer.
    pub fn problem(&self) -> Option<RoleProblem> {
        match text_shape(&self.name, MAX_NAME_CHARS) {
            Err(TextRefused::TooLong) => Some(RoleProblem::TooLong),
            Err(TextRefused::ControlCharacter) => Some(RoleProblem::Characters),
            Ok(_) | Err(TextRefused::Empty) => self.asked.refused(),
        }
    }

    /// Whether the main button does something now: a name in its bounds, and no answer awaited.
    pub fn ready(&self) -> bool {
        !self.asking() && text_shape(&self.name, MAX_NAME_CHARS).is_ok()
    }

    /// The server's answer: the dialog closes, or says why not.
    pub(crate) fn answered(&mut self, answer: Result<(), &LinkError>) -> Next {
        match answer {
            Ok(()) => Next::Closes,
            Err(refused) => {
                self.asked.answered(Some(problem(refused, true)));
                Next::Stays
            }
        }
    }
}

/// A new role's name, typed and asked.
impl Confirming for Creating {
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
            DialogMessage::Confirm if self.ready() => {
                let Ok(name) = text_shape(&self.name, MAX_NAME_CHARS) else { return Next::Stays };
                let name = name.to_string();
                self.asked.ask();
                Next::Asks(ops::create_role(name))
            }
            DialogMessage::Named(_)
            | DialogMessage::Confirm
            | DialogMessage::Password(_)
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

/// Deleting one role: asked first.
#[derive(Debug)]
pub struct DeletingRole {
    pub(crate) id: RoleId,
    name: String,
    asked: Asked<RoleProblem>,
}

impl DeletingRole {
    pub(crate) fn new(id: RoleId, name: String) -> DeletingRole {
        DeletingRole { id, name, asked: Asked::Idle }
    }

    /// The role's name as the list shows it.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the server is being asked to delete it.
    pub fn asking(&self) -> bool {
        self.asked.asking()
    }

    /// Why the last delete was refused; none before one was.
    pub fn problem(&self) -> Option<RoleProblem> {
        self.asked.refused()
    }

    /// The server's answer: gone (now, or already), the dialog closing, or why not.
    pub(crate) fn answered(&mut self, answer: &Result<(), LinkError>) -> Next {
        match answer {
            Ok(()) | Err(LinkError::Refused(ErrorCode::NotFound)) => Next::Closes,
            Err(refused) => {
                self.asked.answered(Some(problem(refused, false)));
                Next::Stays
            }
        }
    }
}

/// Deleting a role is asked once at a time.
impl Confirming for DeletingRole {
    fn holds_outside(&self) -> bool {
        self.asking()
    }

    fn said(&mut self, said: DialogMessage) -> Next {
        match said {
            DialogMessage::Confirm if !self.asking() => {
                self.asked.ask();
                Next::Asks(ops::delete_role(&self.id))
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
    use crate::WindowMessage;
    use crate::frame::{Drawer, Frame, RolesShown, Room};
    use crate::harness::{ADMIN_PLACE, Clock, default_role_entry, placed_holding, refused, role_entry, role_id, tell};
    use crate::harness::{sends, wire};
    use crate::window::Ask as Asked;
    use crate::window::{Asking, Dialog, Window};
    use choices::PaneWidth;
    use std::time::Instant;

    const OWNER: &str = "11";
    const ADMIN: &str = "12";
    const MODS: &str = "13";
    const REGULARS: &str = "14";
    const MEMBER: &str = "15";
    const ADMIN_AT: u32 = ADMIN_PLACE.position();

    /// The canvas's order: Owner, Admin, Moderators, Regulars, and Member last, which everyone holds.
    fn entries() -> Vec<RoleEntry> {
        vec![
            role_entry(OWNER, "Owner", Place::OWNER, &Grant::ALL),
            role_entry(ADMIN, "Admin", Place::at(ADMIN_AT), &Grant::ALL),
            role_entry(MODS, "Moderators", Place::at(ADMIN_AT + 1), &[Grant::ManageMessages, Grant::ManageMembers]),
            role_entry(REGULARS, "Regulars", Place::at(ADMIN_AT + 2), &[Grant::InviteMember]),
            default_role_entry(MEMBER, "Member", Place::at(ADMIN_AT + 3), &[Grant::ChannelRead, Grant::StartVoice]),
        ]
    }

    /// The page open on the roles as read, by the owner.
    fn listed() -> (Roles, RoleList) {
        let mut list = RoleList::default();
        list.read(Ok(entries()));
        let mut roles = Roles::default();
        roles.follow(&list, Place::OWNER);
        (roles, list)
    }

    fn moved(key: &str, before: Option<&str>) -> Moved<RoleId> {
        Moved { key: role_id(key), before: before.map(role_id) }
    }

    fn order(roles: &Loaded<Vec<ListedRole>>) -> Vec<String> {
        match roles {
            Loaded::Ready(roles) => roles.iter().map(|role| role.name.clone()).collect(),
            Loaded::Loading | Loaded::Failed(_) => Vec::new(),
        }
    }

    #[test]
    fn a_person_holding_no_other_role_is_shown_with_the_default_one() {
        let (_, list) = listed();
        let names = |held: &[RoleId]| list.shown(held).iter().map(|role| role.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&[]), ["Member"], "no role held shows the default one");
        assert_eq!(names(&[role_id(MODS), role_id(ADMIN)]), ["Admin", "Moderators"], "others hide it");
        assert!(list.held(&[]).is_empty(), "the default one is still not a held role");
    }

    #[test]
    fn a_role_moves_only_among_the_roles_below_the_asker_and_never_past_the_default_one() {
        let (_, roles) = listed();
        let may = |key, before, asker| roles.may_move(&moved(key, before), asker);
        assert!(may(MODS, Some(ADMIN), Place::OWNER), "above another role below the owner");
        assert!(may(ADMIN, Some(MEMBER), Place::OWNER), "just above the default role");
        assert!(!may(REGULARS, Some(MEMBER), Place::OWNER), "where it already is");
        assert!(!may(MODS, Some(MODS), Place::OWNER) && !may(MODS, Some(REGULARS), Place::OWNER), "nowhere new");
        assert!(!may(MODS, None, Place::OWNER), "never past the default role");
        assert!(!may(MODS, Some(OWNER), Place::OWNER), "never above the owner role");
        assert!(!may(OWNER, Some(MEMBER), Place::OWNER) && !may(MEMBER, Some(ADMIN), Place::OWNER), "neither moves");
        assert!(!may(MODS, Some(ADMIN), ADMIN_PLACE), "never above the asker's own place");
        assert!(!may(ADMIN, Some(MEMBER), ADMIN_PLACE), "nor the asker's own role");
        assert!(may(REGULARS, Some(MODS), ADMIN_PLACE));
        assert!(!may(MODS, Some(ADMIN), Place::BOTTOM), "a member acts on no role");
    }

    #[test]
    fn a_move_is_shown_at_once_and_one_just_above_the_default_role_is_asked_with_none_before() {
        let (mut roles, mut list) = listed();
        let every = Grants::every();
        let asked = roles.update(Message::Move(moved(ADMIN, Some(MEMBER))), &mut list, Place::OWNER, every);
        assert_eq!(asked, [super::Ask::Move { role: role_id(ADMIN), name: "Admin".into(), before: None }]);
        assert_eq!(order(list.listed()), ["Owner", "Moderators", "Regulars", "Admin", "Member"]);
        let asked = roles.update(Message::Move(moved(REGULARS, Some(MODS))), &mut list, Place::OWNER, every);
        assert_eq!(
            asked,
            [super::Ask::Move { role: role_id(REGULARS), name: "Regulars".into(), before: Some(role_id(MODS)) }]
        );
        assert_eq!(order(list.listed()), ["Owner", "Regulars", "Moderators", "Admin", "Member"]);
        list.refused();
        assert_eq!(order(list.listed()), ["Owner", "Admin", "Moderators", "Regulars", "Member"], "both put back");
        assert!(roles.update(Message::Move(moved(MODS, None)), &mut list, Place::OWNER, every).is_empty());
    }

    #[test]
    fn a_typed_name_is_kept_on_leaving_the_field_or_choosing_another_role_and_a_lacked_word_is_not_given() {
        let (mut roles, mut list) = listed();
        let every = Grants::every();
        assert_eq!(
            roles.chosen(&list).map(|role| role.name.as_str()),
            Some("Admin"),
            "the highest the owner may change"
        );
        roles.update(Message::Choose(role_id(MODS)), &mut list, Place::OWNER, every);
        roles.update(Message::Name(name_field::Message::Typed("Mods ".into())), &mut list, Place::OWNER, every);
        let asked = roles.update(Message::NameLeft, &mut list, Place::OWNER, every);
        assert_eq!(asked, [super::Ask::Name { role: role_id(MODS), name: "Mods".into() }], "kept on leaving the field");
        roles.named(&role_id(MODS), &Ok(()));
        assert_eq!(
            RolesView::new(&roles, &list).editor().map(|editor| editor.name().note()),
            Some(crate::FieldNote::Saved)
        );
        roles.update(Message::Name(name_field::Message::Typed("Keepers".into())), &mut list, Place::OWNER, every);
        let asked = roles.update(Message::Choose(role_id(REGULARS)), &mut list, Place::OWNER, every);
        assert_eq!(
            asked,
            [super::Ask::Name { role: role_id(MODS), name: "Keepers".into() }],
            "kept on choosing another"
        );
        assert_eq!(RolesView::new(&roles, &list).editor().map(|editor| editor.name().typed()), Some("Regulars"));
        let lacking: Grants = [Grant::InviteMember].into_iter().collect();
        let asked = roles.update(Message::Word(Grant::ManageServer, true), &mut list, Place::OWNER, lacking);
        assert!(asked.is_empty(), "a word the asker lacks is never given");
        let asked = roles.update(Message::Word(Grant::InviteMember, false), &mut list, Place::OWNER, lacking);
        assert_eq!(
            asked,
            [super::Ask::Word {
                role: role_id(REGULARS),
                name: "Regulars".into(),
                grant: Grant::InviteMember,
                held: false
            }]
        );
        assert!(!roles.chosen(&list).is_some_and(|role| role.grants.holds(Grant::InviteMember)), "shown at once");
        roles.update(Message::Choose(role_id(ADMIN)), &mut list, Place::OWNER, every);
        roles.update(Message::Choose(role_id(OWNER)), &mut list, Place::OWNER, every);
        assert!(RolesView::new(&roles, &list).editor().is_none(), "the owner role is never changed");
        assert!(roles.update(Message::Word(Grant::ManageServer, false), &mut list, Place::OWNER, every).is_empty());
        roles.update(Message::Choose(role_id(ADMIN)), &mut list, ADMIN_PLACE, every);
        roles.update(Message::Name(name_field::Message::Typed("Stewards".into())), &mut list, ADMIN_PLACE, every);
        assert_eq!(
            RolesView::new(&roles, &list).editor().map(|editor| editor.name().typed()),
            Some("Admin"),
            "nothing typed"
        );
        assert!(
            roles.update(Message::NameLeft, &mut list, ADMIN_PLACE, every).is_empty(),
            "a role at the asker's place is locked"
        );
    }

    #[test]
    fn a_list_read_again_keeps_the_choice_and_what_is_typed_and_follows_what_nobody_changes_here() {
        let (mut roles, mut list) = listed();
        let every = Grants::every();
        roles.update(Message::Choose(role_id(MODS)), &mut list, Place::OWNER, every);
        roles.update(Message::Name(name_field::Message::Typed("Mods".into())), &mut list, Place::OWNER, every);
        let mut changed = entries();
        changed[2].colour = Some("#5FD39B".into());
        list.read(Ok(changed));
        roles.follow(&list, Place::OWNER);
        let editor = RolesView::new(&roles, &list).editor().expect("chosen");
        assert_eq!(editor.name().typed(), "Mods", "what is typed stays");
        assert_eq!(editor.colour().picked(), Colour::parse("#5FD39B").expect("green"), "the colour follows");
        list.read(Ok(entries().into_iter().filter(|role| role.role_id != MODS).collect()));
        roles.follow(&list, Place::OWNER);
        assert_eq!(roles.chosen(&list).map(|role| role.name.as_str()), Some("Admin"), "a role gone leaves the choice");
    }

    /// Ada, who owns Night Owls, on its roles page as the server listed them.
    fn on_roles(clock: &Clock) -> (Window, crate::PlaceId) {
        let (mut window, owls) = placed_holding(clock, Place::OWNER, Grants::every());
        let todo =
            tell(&mut window, clock, owls, place::Message::Page(place::PageMessage::Settings(crate::Page::Roles)));
        assert!(
            matches!(&todo[..], [Asked::At(_, ask1)] if crate::harness::is(ask1, crate::harness::Op::ListRoles)),
            "{todo:?}"
        );
        tell(&mut window, clock, owls, place::Message::Server(place::ServerMessage::RolesListed(Ok(entries()))));
        (window, owls)
    }

    fn page(window: &Window) -> RolesView<'_> {
        window.open().and_then(place::Place::roles).expect("the roles page")
    }

    fn roles(message: Message) -> place::Message {
        place::Message::Page(place::PageMessage::Roles(message))
    }

    #[test]
    fn a_refused_move_is_put_back_said_on_the_page_and_the_roles_read_again() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = on_roles(&clock);
        let todo = tell(&mut window, &clock, owls, roles(Message::Move(moved(REGULARS, Some(ADMIN)))));
        assert!(
            matches!(&todo[..], [Asked::At(_, ask)]
                if sends(ask, wire::MoveRole { role_id: REGULARS.into(), before: Some(ADMIN.into()) })),
            "{todo:?}"
        );
        assert_eq!(order(page(&window).listed()), ["Owner", "Regulars", "Admin", "Moderators", "Member"]);
        let answer = Err(refused(ErrorCode::NotAuthorized));
        let changed = place::PageMessage::RoleChanged { name: "Regulars".into(), answer };
        let todo = tell(&mut window, &clock, owls, place::Message::Page(changed));
        assert_eq!(order(page(&window).listed()), ["Owner", "Admin", "Moderators", "Regulars", "Member"], "put back");
        assert!(
            matches!(&todo[..], [Asked::At(_, ask1)] if crate::harness::is(ask1, crate::harness::Op::ListRoles)),
            "{todo:?}"
        );
        let said: Vec<String> = window
            .open()
            .and_then(place::Place::settings)
            .map(|open| open.notices().into_iter().map(|notice| notice.said.in_english()).collect())
            .unwrap_or_default();
        assert_eq!(said, ["You can’t change Regulars."]);
    }

    #[test]
    fn roles_changed_elsewhere_reads_the_place_s_roles_again_which_the_page_shows() {
        use proto::chatkord_v1::{Event, RolesChanged, event};
        let clock = Clock(Instant::now());
        let (mut window, owls) = on_roles(&clock);
        let event = Event { cursor: Vec::new(), kind: Some(event::Kind::RolesChanged(RolesChanged {})) };
        let heard = place::Message::heard(&event).expect("read");
        let todo = tell(&mut window, &clock, owls, heard.clone());
        assert!(
            matches!(&todo[..], [Asked::At(_, ask1)] if crate::harness::is(ask1, crate::harness::Op::ListRoles)),
            "{todo:?}"
        );
        tell(&mut window, &clock, owls, place::Message::Page(place::PageMessage::CloseSettings));
        let todo = tell(&mut window, &clock, owls, heard);
        assert!(
            matches!(&todo[..], [Asked::At(_, ask1)] if crate::harness::is(ask1, crate::harness::Op::ListRoles)),
            "the place's roles, page or not: {todo:?}"
        );
    }

    #[test]
    fn a_role_made_is_chosen_and_one_deleted_leaves_the_list() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = on_roles(&clock);
        window.update(WindowMessage::CreateRole(owls), clock.at());
        let Some(Asking::CreateRole(creating)) = window.dialog().map(Dialog::asking) else { panic!("not creating") };
        assert_eq!(creating.above(), "Member");
        window.update(WindowMessage::Dialog(DialogMessage::Named(" Film club ".into())), clock.at());
        let todo = window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert!(
            matches!(&todo[..], [Asked::At(_, ask)] if sends(ask, wire::CreateRole { name: "Film club".into(), colour: None })),
            "{todo:?}"
        );
        let full = place::PageMessage::RoleCreated(Err(refused(ErrorCode::InvalidRequest)));
        tell(&mut window, &clock, owls, place::Message::Page(full));
        let Some(Asking::CreateRole(creating)) = window.dialog().map(Dialog::asking) else { panic!("not creating") };
        assert_eq!(creating.problem(), Some(RoleProblem::Full));
        window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        let mut made = entries();
        made.insert(4, role_entry("16", "Film club", Place::at(ADMIN_AT + 3), &[]));
        made[5].position = ADMIN_AT + 4;
        tell(&mut window, &clock, owls, place::Message::Page(place::PageMessage::RoleCreated(Ok(made))));
        assert!(window.dialog().is_none());
        assert_eq!(page(&window).chosen().map(|role| role.name.as_str()), Some("Film club"));
        window.update(WindowMessage::DeleteRole(owls), clock.at());
        let todo = window.update(WindowMessage::Dialog(DialogMessage::Confirm), clock.at());
        assert!(
            matches!(&todo[..], [Asked::At(_, ask)] if sends(ask, wire::DeleteRole { role_id: "16".into() })),
            "{todo:?}"
        );
        tell(&mut window, &clock, owls, place::Message::Page(place::PageMessage::RoleDeleted(Ok(()))));
        assert!(window.dialog().is_none());
        assert!(!order(page(&window).listed()).contains(&"Film club".to_string()));
        assert_eq!(page(&window).chosen().map(|role| role.name.as_str()), Some("Member"), "the default chosen");
        window.update(WindowMessage::DeleteRole(owls), clock.at());
        assert!(window.dialog().is_none(), "the default role is never deleted");
    }

    #[test]
    fn a_narrow_page_shows_the_list_then_a_role_opened_from_it_and_back() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = on_roles(&clock);
        let wide = Frame::of(Room { width: 1280.0, height: 800.0 }, Some(PaneWidth::LEAST), Drawer::Shut, None);
        let narrow = Frame::of(Room { width: 360.0, height: 800.0 }, None, Drawer::Shut, None);
        assert_eq!(window.roles_shown(&wide), Some(RolesShown::Beside));
        assert_eq!(window.roles_shown(&narrow), Some(RolesShown::List));
        tell(&mut window, &clock, owls, roles(Message::Choose(role_id(MODS))));
        assert_eq!(window.roles_shown(&narrow), Some(RolesShown::Role));
        assert_eq!(window.roles_shown(&wide), Some(RolesShown::Beside));
        tell(&mut window, &clock, owls, roles(Message::Back));
        assert_eq!(window.roles_shown(&narrow), Some(RolesShown::List));
        tell(&mut window, &clock, owls, place::Message::Page(place::PageMessage::Settings(crate::Page::Members)));
        assert_eq!(window.roles_shown(&narrow), None);
    }

    #[test]
    fn every_grant_word_is_in_one_group_once() {
        let grouped: Vec<Grant> = WORD_GROUPS.iter().flat_map(|(_, words)| words.iter().copied()).collect();
        assert_eq!(grouped.len(), Grant::ALL.len());
        assert!(Grant::ALL.iter().all(|grant| grouped.contains(grant)));
    }
}
