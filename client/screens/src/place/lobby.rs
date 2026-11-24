//! The channels, text and voice, and the way back into a call.

use super::*;

/// A voice channel as the list shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Channel {
    /// The channel's id at the server.
    pub id: ChannelId,
    /// Its name as the server last said it.
    pub name: String,
    /// Who is in its voice room, as the server last said.
    pub people: Vec<Occupant>,
    /// The timeline of the messages written beside its call.
    pub timeline: TimelineId,
}

impl Channel {
    /// How many are in its voice room now.
    pub fn in_voice(&self) -> u32 {
        u32::try_from(self.people.len()).unwrap_or(u32::MAX)
    }
}

/// Someone in a voice room, as the server's occupancy names them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Occupant {
    /// Their id at the server.
    pub user_id: MemberId,
    /// The name others see them by.
    pub name: String,
    /// The colour they picked, if any.
    pub colour: Option<proto::rules::Colour>,
    /// Their own mute and deafen.
    pub state: State,
    /// They share a screen.
    pub sharing: bool,
    /// Their camera is on.
    pub camera: bool,
    /// An administrator muted them for everyone.
    pub held: bool,
    /// Where they stand in the roles' order: the bottom for a server that does not say.
    pub place: proto::rules::Place,
}

/// A server's channels as the side pane lists them, each kind in the list's order: each category's channels in the
/// categories' order, each by its position there.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChannelList {
    /// The text channels; none from a server that has no messages (`chat`).
    pub text: Vec<TextChannel>,
    /// The voice channels, with who is in each.
    pub voice: Vec<Channel>,
    /// The categories, in their order.
    pub categories: Vec<Category>,
    /// Where each channel stands, by the channel's id; one whose category the list does not hold is not shown.
    pub placed: std::collections::HashMap<ChannelId, Placement>,
    /// The list's version as the server told it: a change told with a higher one applies to it, one told with this
    /// or a lower one is already in it. Versions may skip: a change of a channel this account may not see is not told.
    pub version: u64,
    /// This account's standing in each channel, by the channel's id.
    pub standings: std::collections::HashMap<ChannelId, ChannelStanding>,
    /// The channels hidden from the default role (the server's `private`), by id; every other one everyone sees.
    pub private: std::collections::HashSet<ChannelId>,
}

/// Who sees a channel, as the list says it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Sight {
    /// Everyone its roles let see it: the default role sees it.
    #[default]
    Everyone,
    /// Hidden from the default role: only the roles and members let in, and the owner, see it. Marked by a lock.
    Private,
}

/// A named group of channels in the list, one level deep.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Category {
    /// The category's id at the server.
    pub id: CategoryId,
    /// Its name as the server last said it.
    pub name: String,
    /// Where it stands among the categories, lowest first.
    pub position: i64,
}

/// Where a channel stands in the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    /// The category it is in.
    pub category: CategoryId,
    /// Where it stands among the channels of its category, lowest first.
    pub position: i64,
}

impl ChannelList {
    /// Applies `change`, told with `version`, when it is newer than the list, and says whether it did; one at or
    /// below the list's version is already in it. A voice channel made is seated from `rooms`.
    pub(super) fn apply(
        &mut self,
        change: ChannelChange,
        version: u64,
        rooms: &std::collections::HashMap<ChannelId, Vec<Occupant>>,
    ) -> bool {
        if version <= self.version {
            return false;
        }
        self.version = version;
        match change {
            ChannelChange::AddedText { channel, placed, sight } => {
                self.see(&channel.id, sight);
                self.placed.insert(channel.id.clone(), placed);
                match self.text.iter_mut().find(|listed| listed.id == channel.id) {
                    Some(listed) => *listed = channel,
                    None => self.text.push(channel),
                }
            }
            ChannelChange::AddedVoice { mut channel, placed, sight } => {
                self.see(&channel.id, sight);
                channel.people = rooms.get(&channel.id).cloned().unwrap_or_default();
                self.placed.insert(channel.id.clone(), placed);
                match self.voice.iter_mut().find(|listed| listed.id == channel.id) {
                    Some(listed) => *listed = channel,
                    None => self.voice.push(channel),
                }
            }
            ChannelChange::Renamed { id, name } => {
                let text = self.text.iter_mut().filter(|channel| channel.id == id).map(|c| &mut c.name);
                let voice = self.voice.iter_mut().filter(|channel| channel.id == id).map(|c| &mut c.name);
                for named in text.chain(voice) {
                    named.clone_from(&name);
                }
            }
            ChannelChange::Removed(id) => self.forget(&id),
            ChannelChange::Access { id, sight } => {
                if self.find(&id).is_some() {
                    self.see(&id, sight);
                }
            }
            ChannelChange::Moved { id, placed } => {
                if self.text.iter().any(|channel| channel.id == id) || self.voice.iter().any(|c| c.id == id) {
                    self.placed.insert(id, placed);
                }
            }
            ChannelChange::CategoryAdded(category) => {
                match self.categories.iter_mut().find(|listed| listed.id == category.id) {
                    Some(listed) => *listed = category,
                    None => self.categories.push(category),
                }
            }
            ChannelChange::CategoryRenamed { id, name } => {
                if let Some(listed) = self.categories.iter_mut().find(|listed| listed.id == id) {
                    listed.name = name;
                }
            }
            ChannelChange::CategoryRemoved(id) => {
                let inside: Vec<ChannelId> = self
                    .placed
                    .iter()
                    .filter(|(_, placed)| placed.category == id)
                    .map(|(channel, _)| channel.clone())
                    .collect();
                for channel in &inside {
                    self.forget(channel);
                }
                self.categories.retain(|listed| listed.id != id);
            }
            ChannelChange::CategoryMoved { id, position } => {
                if let Some(listed) = self.categories.iter_mut().find(|listed| listed.id == id) {
                    listed.position = position;
                }
            }
        }
        self.arrange();
        true
    }

    /// Drops the channel `id` and all the list keeps of it.
    fn forget(&mut self, id: &ChannelId) {
        self.text.retain(|channel| channel.id != *id);
        self.voice.retain(|channel| channel.id != *id);
        self.standings.remove(id);
        self.placed.remove(id);
        self.private.remove(id);
    }

    /// Keeps who sees the channel `id`.
    fn see(&mut self, id: &ChannelId, sight: Sight) {
        match sight {
            Sight::Everyone => self.private.remove(id),
            Sight::Private => self.private.insert(id.clone()),
        };
    }

    /// Who sees the channel `id`, as listed.
    pub fn sight(&self, id: &ChannelId) -> Sight {
        if self.private.contains(id) { Sight::Private } else { Sight::Everyone }
    }

    /// The channel listed with `id`, of either kind.
    pub fn find(&self, id: &ChannelId) -> Option<ListEntry<'_>> {
        let text = self.text.iter().find(|channel| channel.id == *id).map(ListEntry::Text);
        text.or_else(|| self.voice.iter().find(|channel| channel.id == *id).map(ListEntry::Voice))
    }

    /// Puts the categories and each kind's channels in the list's order. The sorts are stable, so a tie, which a server
    /// keeping the protocol never lists, keeps the order it came in.
    pub(super) fn arrange(&mut self) {
        self.categories.sort_by_key(|category| category.position);
        let groups = groups(&self.categories);
        let key = |id: &ChannelId| sort_key(&groups, &self.placed, id);
        self.text.sort_by_key(|channel| key(&channel.id));
        self.voice.sort_by_key(|channel| key(&channel.id));
    }

    /// The list in its order, both kinds together, as a pane draws it: each category followed by its own channels; a
    /// channel whose category the list does not hold is left out. Each kind's channels keep their order in
    /// [`ChannelList::text`] and [`ChannelList::voice`].
    pub fn in_order(&self) -> Vec<ListEntry<'_>> {
        let groups = groups(&self.categories);
        let key = |id: &ChannelId| sort_key(&groups, &self.placed, id);
        let mut channels: Vec<((Option<usize>, i64), ListEntry<'_>)> = self
            .text
            .iter()
            .map(|channel| (key(&channel.id), ListEntry::Text(channel)))
            .chain(self.voice.iter().map(|channel| (key(&channel.id), ListEntry::Voice(channel))))
            .collect();
        channels.sort_by_key(|(sorts, _)| *sorts);
        let mut channels = channels.into_iter().skip_while(|((group, _), _)| group.is_none()).peekable();
        let mut entries = Vec::with_capacity(self.categories.len() + self.text.len() + self.voice.len());
        for (at, category) in self.categories.iter().enumerate() {
            entries.push(ListEntry::Category(category));
            while let Some((_, entry)) = channels.next_if(|((group, _), _)| *group == Some(at)) {
                entries.push(entry);
            }
        }
        entries
    }
}

/// Each category's place among the categories as they are held.
fn groups(categories: &[Category]) -> std::collections::HashMap<&CategoryId, usize> {
    categories.iter().enumerate().map(|(at, category)| (&category.id, at)).collect()
}

/// Where a channel sorts: the place of its category among the categories (none, for a category the list does not hold,
/// first and unshown), then its position there.
fn sort_key(
    groups: &std::collections::HashMap<&CategoryId, usize>,
    placed: &std::collections::HashMap<ChannelId, Placement>,
    id: &ChannelId,
) -> (Option<usize>, i64) {
    let placement = placed.get(id);
    let group = placement.and_then(|placed| groups.get(&placed.category).copied());
    (group, placement.map_or(0, |placed| placed.position))
}

/// One line of the channel list in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListEntry<'a> {
    /// A category, its channels on the lines after it.
    Category(&'a Category),
    /// A text channel.
    Text(&'a TextChannel),
    /// A voice channel.
    Voice(&'a Channel),
}

/// The changes of the list told since the list held was read, kept so a list read while they were told is not left
/// without them: each with its version, oldest first.
#[derive(Debug, Default)]
pub(super) struct Recent {
    changes: std::collections::VecDeque<(u64, ChannelChange)>,
    /// The highest version of a change let go to keep the bound.
    let_go: u64,
}

impl Recent {
    /// The changes kept: past these, the oldest is let go.
    const BOUND: usize = 128;

    /// Keeps a change told with `version`, letting the oldest go past the bound.
    fn keep(&mut self, version: u64, change: ChannelChange) {
        if self.changes.len() == Self::BOUND
            && let Some((dropped, _)) = self.changes.pop_front()
        {
            self.let_go = self.let_go.max(dropped);
        }
        self.changes.push_back((version, change));
    }

    /// Applies over a list just read the changes kept that are newer than it, and lets go of the rest, which it holds.
    /// Answers whether a change it let go earlier is newer than the list, so the list misses it and is read again.
    pub(super) fn over(
        &mut self,
        listed: &mut ChannelList,
        rooms: &std::collections::HashMap<ChannelId, Vec<Occupant>>,
    ) -> bool {
        let read = listed.version;
        self.changes.retain(|(version, _)| *version > read);
        for (version, change) in &self.changes {
            listed.apply(change.clone(), *version, rooms);
        }
        self.let_go > read
    }
}

/// What this account may do and has read in one channel, as the server last said and as told messages since count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChannelStanding {
    /// The words the account holds in it.
    pub grants: Grants,
    /// The revision of its messages read through: one placed after it is unread.
    pub read: u64,
    /// The messages after `read` not this account's own, up to [`proto::rules::COUNT_CAP`].
    pub unread: u32,
    /// Of those, the ones that mention this account, up to [`proto::rules::COUNT_CAP`].
    pub mentions: u32,
}

impl ChannelStanding {
    /// The standing `told` says, over this one: the higher read mark stands, and the counts go with the newer mark.
    pub(super) fn over(self, told: ChannelStanding) -> ChannelStanding {
        let newer = if told.read >= self.read { told } else { self };
        ChannelStanding { grants: told.grants, read: self.read.max(told.read), ..newer }
    }

    /// One message more after the mark, mentioning this account or not; neither count passes its cap.
    pub(crate) fn count(&mut self, mentioned: bool) {
        let cap = proto::rules::COUNT_CAP;
        self.unread = (self.unread + 1).min(cap);
        if mentioned {
            self.mentions = (self.mentions + 1).min(cap);
        }
    }

    /// Whether a count reached its cap, so it reads as "at least".
    pub fn capped(count: u32) -> bool {
        count >= proto::rules::COUNT_CAP
    }
}

/// One change of the server's channel list, told by itself. Applying one again changes nothing: a channel or category
/// made replaces one listed with its id, and one deleted that is not listed is nothing to delete.
#[derive(Clone, Debug)]
pub enum ChannelChange {
    /// A text channel was made, last in its category, or came into sight.
    AddedText {
        /// The channel.
        channel: TextChannel,
        /// Where it stands.
        placed: Placement,
        /// Who sees it.
        sight: Sight,
    },
    /// A voice channel was made, last in its category, or came into sight.
    AddedVoice {
        /// The channel.
        channel: Channel,
        /// Where it stands.
        placed: Placement,
        /// Who sees it.
        sight: Sight,
    },
    /// A channel became private, or stopped being.
    Access {
        /// The channel, by its id.
        id: ChannelId,
        /// Who sees it now.
        sight: Sight,
    },
    /// A channel was renamed.
    Renamed {
        /// The channel, by its id.
        id: ChannelId,
        /// Its name now.
        name: String,
    },
    /// A channel was deleted, by its id.
    Removed(ChannelId),
    /// A channel took another place: another category, or another position in its own.
    Moved {
        /// The channel, by its id.
        id: ChannelId,
        /// Where it stands now.
        placed: Placement,
    },
    /// A category was made, last in the order.
    CategoryAdded(Category),
    /// A category was renamed.
    CategoryRenamed {
        /// The category, by its id.
        id: CategoryId,
        /// Its name now.
        name: String,
    },
    /// A category was deleted, by its id, after its channels were told moved out of it.
    CategoryRemoved(CategoryId),
    /// A category took another place among the categories.
    CategoryMoved {
        /// The category, by its id.
        id: CategoryId,
        /// Its position now.
        position: i64,
    },
}

/// The channel list, as the side pane shows it.
pub type Lobby = Loaded<ChannelList>;

/// The channels and the way back into a call.
#[derive(Clone, Debug)]
pub enum LobbyMessage {
    /// The channels of the server, or why there is no list.
    Channels(Result<ChannelList, LinkError>),
    /// Who is in a voice room now.
    Occupancy {
        /// The channel whose room it is, by its id.
        channel_id: ChannelId,
        /// Who they are.
        people: Vec<Occupant>,
    },
    /// This account's standing in a channel.
    Standing {
        /// The channel, by its id.
        channel_id: ChannelId,
        /// The standing now.
        standing: ChannelStanding,
    },
    /// One change of the list, and the list's version with it.
    Changed {
        /// What changed.
        change: ChannelChange,
        /// The list's version with it.
        version: u64,
    },
    /// The list is asked for again.
    Retry,
    /// The first arrival's note is put away.
    Dismiss,
    /// A channel picked in the list, by its id: its call opens unless one is live here.
    Join(ChannelId),
    /// "Rejoin" the call a drop left.
    Rejoin,
    /// A category's header pressed: it folds, or opens again.
    Fold(CategoryId),
    /// A row of the list dragged, or moved by Alt and an arrow key.
    Move(crate::order::Moved<ListKey>),
    /// The server's answer to a move.
    Moved(Result<(), LinkError>),
}

impl Place {
    /// Who is listed under `channel` in the side pane: the room itself where this window's call is live in it, else
    /// who the server last said is there, this account first. An account that is there while no call here is live in
    /// the channel is in it on another device.
    pub fn seats(&self, channel: &Channel) -> Vec<Tile> {
        let call = self.call().filter(|call| call.channel.id == channel.id && call.live());
        if let Some(call) = call.filter(|call| call.joined()) {
            return call.tiles().to_vec();
        }
        let me = self.me();
        let mut seats: Vec<Tile> = channel
            .people
            .iter()
            .map(|person| {
                let own = me == Some(&person.user_id);
                Tile {
                    user_id: Some(person.user_id.clone()),
                    name: person.name.clone(),
                    colour: colour_of(&person.user_id, person.colour),
                    own,
                    connection: Connection::Connected,
                    speaking: false,
                    muted: person.state.mic_muted || person.state.deafened || person.held,
                    deafened: person.state.deafened,
                    held: person.held,
                    sharing: person.sharing,
                    camera: if person.camera { crate::CameraSeen::Unshown } else { crate::CameraSeen::Off },
                    elsewhere: own && call.is_none(),
                }
            })
            .collect();
        seats.sort_by_key(|seat| !seat.own());
        seats
    }

    /// Whether this account is in `channel` on another device: the server lists it there and no call here is.
    pub fn elsewhere(&self, channel: &Channel) -> bool {
        self.seats(channel).iter().any(|seat| seat.elsewhere)
    }

    /// The voice channel listed here with this id.
    pub fn channel(&self, id: &ChannelId) -> Option<&Channel> {
        match self.lobby()? {
            Lobby::Ready(listed) => listed.voice.iter().find(|channel| channel.id == *id),
            Lobby::Loading | Lobby::Failed(_) => None,
        }
    }

    /// What this account may do in the channel `id`, as the list last said: its roles' words there, after the
    /// channel's overwrites. Nothing where the list holds no standing for it.
    pub fn grants_in(&self, id: &ChannelId) -> Grants {
        match self.lobby() {
            Some(Lobby::Ready(listed)) => listed.standings.get(id).map_or(Grants::NONE, |standing| standing.grants),
            Some(Lobby::Loading | Lobby::Failed(_)) | None => Grants::NONE,
        }
    }

    /// The channels shown, signed in or while the connection is made again.
    pub fn lobby(&self) -> Option<&Lobby> {
        match &self.status {
            Status::Signed(signed) => Some(&signed.lobby),
            Status::Reconnecting(again) => again.shown.as_ref(),
            Status::Door(_) => None,
        }
    }

    /// What the content says while no channel is open: where channels live when there is no category, else pick a
    /// channel where there is a text one, else a voice one.
    pub fn start_hint(&self) -> words::Phrase {
        match self.lobby() {
            Some(Lobby::Ready(listed)) if listed.categories.is_empty() => words::Phrase::NoCategoriesBody,
            Some(Lobby::Ready(listed)) if !listed.text.is_empty() => words::Phrase::PickChannel,
            Some(Lobby::Ready(_) | Lobby::Loading | Lobby::Failed(_)) | None => words::Phrase::PickVoiceChannel,
        }
    }

    /// The first text channel, while the content area shows nothing and the channels are listed: what opens on arrival.
    pub(crate) fn first_text(&self) -> Option<ChannelId> {
        let Some(Lobby::Ready(listed)) = self.lobby() else { return None };
        (self.panel() == Panel::Channels).then(|| listed.text.first().map(|held| held.id.clone())).flatten()
    }

    /// The note a first arrival opens with, while signed in.
    pub fn note(&self) -> Option<&Arrival> {
        match &self.status {
            Status::Signed(signed) => signed.note.as_ref(),
            Status::Door(_) | Status::Reconnecting(_) => None,
        }
    }

    /// The call a drop left, offered to rejoin.
    pub fn back(&self) -> Option<&Channel> {
        match &self.status {
            Status::Signed(signed) => signed.back.as_ref(),
            Status::Door(_) | Status::Reconnecting(_) => None,
        }
    }

    /// The channels: only a signed-in server lists them.
    pub(super) fn on_lobby(&mut self, message: LobbyMessage, told: &mut Option<Told>) -> Vec<Ask> {
        let manages = self.manages_channels();
        let lists = self.lists_overwrites();
        if let LobbyMessage::Fold(category) = message {
            return self.fold(category);
        }
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        match message {
            LobbyMessage::Fold(_) => Vec::new(),
            LobbyMessage::Move(moved) => super::arrange::moved(signed, manages, moved),
            LobbyMessage::Moved(answer) => super::arrange::answered(signed, answer, told),
            LobbyMessage::Channels(listed) => {
                let mut todo = Vec::new();
                signed.lobby = match listed {
                    Ok(mut listed) => {
                        for channel in &mut listed.voice {
                            channel.people = signed.rooms.get(&channel.id).cloned().unwrap_or_default();
                        }
                        if signed.recent.over(&mut listed, &signed.rooms) {
                            todo.push(ops::list_channels());
                        }
                        follow(signed, &listed);
                        Lobby::Ready(listed)
                    }
                    Err(failed) => Lobby::Failed(failed),
                };
                todo.extend(super::arrange::retried(signed, told));
                todo
            }
            LobbyMessage::Occupancy { channel_id, people } => {
                if let Lobby::Ready(listed) = &mut signed.lobby
                    && let Some(channel) = listed.voice.iter_mut().find(|channel| channel.id == channel_id)
                {
                    channel.people.clone_from(&people);
                }
                signed.rooms.insert(channel_id, people);
                Vec::new()
            }
            LobbyMessage::Standing { channel_id, standing } => {
                if let Lobby::Ready(listed) = &mut signed.lobby {
                    let kept = listed.standings.entry(channel_id.clone()).or_default();
                    // A mark read on another device may be told after a newer one: the higher stands.
                    *kept = kept.over(standing);
                    share_allowed(signed.call.as_mut(), &listed.standings);
                }
                let mut todo = relist(signed, lists, &channel_id);
                todo.extend(self.withdraw_channel(&channel_id, standing.read));
                todo
            }
            LobbyMessage::Changed { change, version } => {
                if let ChannelChange::Removed(id) = &change {
                    signed.rooms.remove(id);
                    signed.overwrites.remove(id);
                }
                let access = match &change {
                    ChannelChange::Access { id, .. } => relist(signed, lists, id),
                    _ => Vec::new(),
                };
                signed.recent.keep(version, change.clone());
                let Lobby::Ready(listed) = &mut signed.lobby else { return access };
                if listed.apply(change, version, &signed.rooms) {
                    let listed = listed.clone();
                    follow(signed, &listed);
                }
                access
            }
            LobbyMessage::Retry if matches!(signed.lobby, Lobby::Failed(_)) => {
                signed.lobby = Lobby::Loading;
                vec![ops::list_channels()]
            }
            LobbyMessage::Dismiss => {
                signed.note = None;
                Vec::new()
            }
            LobbyMessage::Rejoin => match signed.back.as_ref().map(|channel| channel.id.clone()) {
                Some(id) => join(
                    signed,
                    &mut self.held,
                    &self.roles,
                    id,
                    self.standing.as_ref().map(|standing| standing.user_id.clone()),
                    &self.volumes,
                    self.call_view,
                ),
                None => Vec::new(),
            },
            LobbyMessage::Join(id) => join(
                signed,
                &mut self.held,
                &self.roles,
                id,
                self.standing.as_ref().map(|standing| standing.user_id.clone()),
                &self.volumes,
                self.call_view,
            ),
            // A list that did not fail is not asked for again.
            LobbyMessage::Retry => Vec::new(),
        }
    }
}

/// What follows the list: a call's channel renamed under it keeps the call under the new name, a share denied there is
/// no longer said denied once the channel allows it, and an open text channel follows its name and closes once it is
/// gone. A call whose channel left the list goes on until the server closes its room.
fn follow(signed: &mut SignedIn, listed: &ChannelList) {
    if let Some(open) = opened::<ChannelSettings>(&mut signed.page) {
        open.follow(listed);
    }
    if let Some(call) = signed.call.as_mut()
        && let Some(named) = listed.voice.iter().find(|channel| channel.id == call.channel.id)
    {
        call.channel.name.clone_from(&named.name);
    }
    share_allowed(signed.call.as_mut(), &listed.standings);
    // An open channel, or the thread of one, closes once its channel is gone; a rename needs nothing here, the name is
    // read from the list.
    let gone = |open: &Discussion| match &open.owner {
        Owner::Channel(id) | Owner::Thread { channel: id, .. } => listed.find(id).is_none(),
        Owner::Conversation(_) => false,
    };
    if signed.content.discussion().is_some_and(gone) {
        signed.content = Content::default();
    }
    if signed.last_text.as_ref().is_some_and(|channel| listed.find(channel).is_none()) {
        signed.last_text = None;
    }
    if let Some(beside) = signed.content.beside()
        && beside.as_ref().is_some_and(gone)
    {
        *beside = None;
    }
}

/// A channel's overwrites read again where they were read, once its standing or who sees it changed.
fn relist(signed: &mut SignedIn, lists: bool, channel: &ChannelId) -> Vec<Ask> {
    if lists && signed.overwrites.contains_key(channel) {
        vec![ops::list_overwrites(channel.clone())]
    } else {
        Vec::new()
    }
}

/// A share the server denied is no longer marked failed once the call's channel allows sharing again.
fn share_allowed(call: Option<&mut Call>, standings: &std::collections::HashMap<ChannelId, ChannelStanding>) {
    if let Some(call) = call
        && standings.get(&call.channel.id).is_some_and(|standing| standing.grants.holds(Grant::ShareScreen))
        && call.faults.of(Device::Share) == Some(&Sentence::Phrase(Phrase::ShareDenied))
    {
        call.faults.clear(Device::Share);
    }
}

/// A voice channel picked: its call shows in the content area in place of what was there, and starts unless one is
/// running here (the window leaves one on another channel first).
pub(super) fn join(
    signed: &mut SignedIn,
    held: &mut std::collections::HashMap<Compose, Held>,
    roles: &RoleList,
    id: ChannelId,
    me: Option<MemberId>,
    volumes: &Volumes,
    view: CallView,
) -> Vec<Ask> {
    if signed.call.as_ref().is_some_and(Call::running) {
        return signed.show(Content::Call, held, roles);
    }
    let Lobby::Ready(listed) = &signed.lobby else { return Vec::new() };
    let Some(channel) = listed.voice.iter().find(|channel| channel.id == id).cloned() else { return Vec::new() };
    signed.back = None;
    signed.hold_call(Some(Call::new(channel, me, view)));
    let mut todo = signed.show(Content::Call, held, roles);
    todo.push(Ask::Join { channel_id: id, volumes: volumes.clone() });
    todo
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chime::Chime;
    use crate::door::{self, Errand, Opening};
    use crate::harness::{
        ADA, Clock, GENERAL, accepted, accepted_as, ada_in_call, channels, chatting, every_switch_on, flow, holding,
        id, in_call, member, owls, tell, token, two_signed_in,
    };
    use crate::harness::{sends, wire};
    use crate::place;
    use crate::window::Window;
    use choices::State;
    use proto::rules::{Grant, Grants};
    use std::time::Instant;
    use terms::{Ended, LinkError};

    /// Ada chatting in #general at a server that offers conversations and threads, with `roll` open.
    fn rolled(clock: &Clock, roll: place::Roll) -> (Window, place::PlaceId) {
        let (mut window, owls) = chatting(clock);
        every_switch_on(&mut window, clock, owls);
        tell(&mut window, clock, owls, place::Message::Talk(place::TalkMessage::Show(roll)));
        (window, owls)
    }

    #[test]
    fn a_voice_channel_picked_over_a_list_shows_its_call() {
        for roll in [place::Roll::Conversations, place::Roll::Threads] {
            let clock = Clock(Instant::now());
            let (mut window, owls) = rolled(&clock, roll);
            in_call(&mut window, &clock, owls);
            let place = &window.places()[0];
            assert_eq!((place.panel(), place.roll()), (Panel::Call, None), "the list gave way, from {roll:?}");
            assert_eq!(place.marked(), Some(Marked::Channel(crate::harness::channel_id("lounge"))), "one row marked");
        }
    }

    #[test]
    fn the_live_calls_channel_picked_over_a_list_shows_the_call_again() {
        for roll in [place::Roll::Conversations, place::Roll::Threads] {
            let clock = Clock(Instant::now());
            let (mut window, owls) = chatting(&clock);
            every_switch_on(&mut window, &clock, owls);
            in_call(&mut window, &clock, owls);
            tell(&mut window, &clock, owls, place::Message::Talk(place::TalkMessage::Show(roll)));
            assert_ne!(window.places()[0].panel(), Panel::Call, "the list shows over the call");
            let todo = tell(
                &mut window,
                &clock,
                owls,
                place::Message::Lobby(place::LobbyMessage::Join(crate::harness::channel_id("lounge"))),
            );
            assert!(todo.is_empty(), "the call goes on: {todo:?}");
            let place = &window.places()[0];
            assert_eq!((place.panel(), place.roll()), (Panel::Call, None), "back at once, from {roll:?}");
            assert_eq!(place.marked(), Some(Marked::Channel(crate::harness::channel_id("lounge"))), "one row marked");
        }
    }

    #[test]
    fn the_pane_marks_one_row_for_what_the_content_shows() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = rolled(&clock, place::Roll::Threads);
        let marked = |window: &Window| window.places()[0].marked();
        assert_eq!(marked(&window), Some(Marked::Roll(place::Roll::Threads)));
        in_call(&mut window, &clock, owls);
        let lounge = Marked::Channel(crate::harness::channel_id("lounge"));
        assert_eq!(marked(&window), Some(lounge.clone()));
        let general = crate::harness::channel_id(GENERAL);
        tell(&mut window, &clock, owls, place::Message::Discussion(place::DiscussionMessage::Open(general.clone())));
        assert_eq!(marked(&window), Some(Marked::Channel(general)), "the call goes on, its row unmarked");
        tell(&mut window, &clock, owls, place::Message::Talk(place::TalkMessage::Show(place::Roll::Conversations)));
        assert_eq!(marked(&window), Some(Marked::Roll(place::Roll::Conversations)));
        tell(
            &mut window,
            &clock,
            owls,
            place::Message::Lobby(place::LobbyMessage::Join(crate::harness::channel_id("lounge"))),
        );
        assert_eq!(marked(&window), Some(lounge));
        tell(&mut window, &clock, owls, place::Message::Call(place::CallMessage::Leave));
        tell(&mut window, &clock, owls, place::Message::Call(place::CallMessage::CallOver(None)));
        let general = Marked::Channel(crate::harness::channel_id(GENERAL));
        assert_eq!(
            (window.places()[0].panel(), marked(&window)),
            (Panel::Discussion, Some(general)),
            "the call put away: the channel last open"
        );
    }

    #[test]
    fn leaving_a_call_returns_the_content_to_the_text_channel_last_open_else_to_the_panel() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        in_call(&mut window, &clock, owls);
        assert_eq!(window.places()[0].panel(), Panel::Call);
        tell(&mut window, &clock, owls, place::Message::Call(place::CallMessage::Leave));
        let todo = tell(&mut window, &clock, owls, place::Message::Call(place::CallMessage::CallOver(None)));
        let place = &window.places()[0];
        assert_eq!(place.panel(), Panel::Discussion, "back on the channel the call replaced");
        assert_eq!(
            place.discussion().map(|open| open.owner.clone()),
            Some(place::Owner::Channel(crate::harness::channel_id(GENERAL)))
        );
        assert!(
            todo.iter().any(
                |done| matches!(done, crate::Ask::At(_, ask1) if crate::harness::is(ask1, crate::harness::Op::History))
            ),
            "its first page is asked for again: {todo:?}"
        );
        assert!(place.own_call().is_none(), "and no call is shown");

        let mut window = two_signed_in(&clock);
        let owls = id(&window, 0);
        in_call(&mut window, &clock, owls);
        tell(&mut window, &clock, owls, place::Message::Call(place::CallMessage::Leave));
        tell(&mut window, &clock, owls, place::Message::Call(place::CallMessage::CallOver(None)));
        assert_eq!(window.places()[0].panel(), Panel::Discussion, "the first text channel opened on arrival");
    }

    #[test]
    fn a_call_is_the_accounts_own_only_while_it_is_joined_live_and_shown() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        assert!(window.places()[0].own_call().is_none(), "no call");
        tell(
            &mut window,
            &clock,
            owls,
            place::Message::Lobby(place::LobbyMessage::Join(crate::harness::channel_id("lounge"))),
        );
        assert_eq!(window.places()[0].panel(), Panel::Call);
        assert!(window.places()[0].own_call().is_none(), "still being joined");
        let (mut window, owls) = chatting(&clock);
        in_call(&mut window, &clock, owls);
        assert!(window.places()[0].own_call().is_some(), "joined, live and shown");
        tell(
            &mut window,
            &clock,
            owls,
            place::Message::Discussion(place::DiscussionMessage::Open(crate::harness::channel_id(GENERAL))),
        );
        assert!(window.places()[0].own_call().is_none(), "the call goes on in the pane while a channel shows");
    }

    #[test]
    fn a_page_closed_by_what_opens_under_it_saves_what_it_holds() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = rolled(&clock, place::Roll::Conversations);
        tell(&mut window, &clock, owls, place::Message::Page(place::PageMessage::OpenAccount));
        crate::harness::account_name(&mut window, &clock, owls, crate::name_field::Message::Typed("Ada L".into()));
        let todo =
            tell(&mut window, &clock, owls, place::Message::Talk(place::TalkMessage::Show(place::Roll::Threads)));
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, ask)] if sends(ask, wire::SetDisplayName { display_name: Some("Ada L".into()) })),
            "{todo:?}"
        );
        assert_eq!(window.places()[0].panel(), Panel::Threads);
    }

    #[test]
    fn a_dropped_call_comes_back_to_the_channels_with_a_rejoin() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let owls_id = id(&window, 0);
        in_call(&mut window, &clock, owls_id);
        let todo = tell(
            &mut window,
            &clock,
            owls_id,
            place::Message::Session(place::SessionMessage::Disconnected(Ended { silent: true, ..Ended::default() })),
        );
        assert!(
            matches!(
                &todo[..],
                [crate::Ask::At(_, place::Ask::Wait { attempt: 1, .. }), crate::Ask::Chime(Chime::Disconnected)]
            ),
            "{todo:?}"
        );
        assert_eq!(window.places()[0].panel(), Panel::Lost, "the list stays, under the lost notice");
        let line = crate::StateLine::of(&window.places()[0]).expect("a lost connection holds");
        assert_eq!((line.kind, line.said.in_english().starts_with("Connection to")), (design::Status::Warning, true));
        assert_eq!(window.places()[0].rail(), RailState::Online, "the rail does not change while it is lost");
        let todo = tell(&mut window, &clock, owls_id, place::Message::Session(place::SessionMessage::Reconnect(1)));
        assert!(
            matches!(
                &todo[..],
                [crate::Ask::At(_, place::Ask::Door(door::Ask::Go(Errand::TryToken(..)))), crate::Ask::WakeAt(_)]
            ),
            "the token held is offered at once: {todo:?}"
        );
        tell(&mut window, &clock, owls_id, flow(accepted(Grants::default())));
        let place = &window.places()[0];
        assert_eq!(place.panel(), Panel::Channels, "never back into the call by itself");
        assert_eq!(place.back().map(|channel| channel.name.as_str()), Some("Lounge"));
        let toast = window.toast().expect("the way back is told in a toast");
        assert_eq!(toast.action(), Some(&crate::Action::Rejoin("Lounge".into())));
        assert_eq!(toast.kind(), design::Status::Success);
        tell(&mut window, &clock, owls_id, place::Message::Lobby(place::LobbyMessage::Channels(Ok(channels()))));
        let todo = tell(&mut window, &clock, owls_id, place::Message::Lobby(place::LobbyMessage::Rejoin));
        assert!(matches!(&todo[..], [crate::Ask::At(_, place::Ask::Join { .. })]), "{todo:?}");
        assert_eq!(window.places()[0].back(), None);
    }

    #[test]
    fn the_list_fails_and_is_asked_again_and_a_channel_not_listed_is_not_joined() {
        let clock = Clock(Instant::now());
        let mut window = two_signed_in(&clock);
        let owls_id = id(&window, 0);
        assert!(
            tell(
                &mut window,
                &clock,
                owls_id,
                place::Message::Lobby(place::LobbyMessage::Join(crate::harness::channel_id("nope")))
            )
            .is_empty()
        );
        tell(
            &mut window,
            &clock,
            owls_id,
            place::Message::Lobby(place::LobbyMessage::Channels(Err(LinkError::Connect))),
        );
        assert_eq!(window.places()[0].lobby(), Some(&Lobby::Failed(LinkError::Connect)));
        let todo = tell(&mut window, &clock, owls_id, place::Message::Lobby(place::LobbyMessage::Retry));
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, ask1)] if crate::harness::is(ask1, crate::harness::Op::ChannelsList)),
            "{todo:?}"
        );
        assert!(
            tell(&mut window, &clock, owls_id, place::Message::Lobby(place::LobbyMessage::Retry)).is_empty(),
            "nothing to retry while loading"
        );
    }

    #[test]
    fn a_voice_channel_lists_who_is_in_it_and_this_account_there_is_in_it_on_another_device() {
        let clock = Clock(Instant::now());
        let (mut window, _) = Window::launch(vec![Opening::Account(owls(), ADA.into())], None, clock.0);
        let place = id(&window, 0);
        tell(&mut window, &clock, place, flow(token()));
        tell(&mut window, &clock, place, flow(accepted_as("ada")));
        tell(&mut window, &clock, place, place::Message::Lobby(place::LobbyMessage::Channels(Ok(channels()))));
        let person = |user_id: &str, name: &str, muted: bool| place::Occupant {
            user_id: member(user_id),
            colour: None,
            name: name.into(),
            state: choices::State { mic_muted: muted, deafened: false },
            sharing: false,
            camera: false,
            held: false,
            place: proto::rules::Place::BOTTOM,
        };
        tell(
            &mut window,
            &clock,
            place,
            place::Message::Lobby(place::LobbyMessage::Occupancy {
                channel_id: crate::harness::channel_id("lounge"),
                people: vec![person("grace", "Grace", true), person("ada", ADA, false)],
            }),
        );
        let channel = match window.places()[0].lobby() {
            Some(Lobby::Ready(channels)) => channels.voice[0].clone(),
            other => panic!("{other:?}"),
        };
        let seats = window.places()[0].seats(&channel);
        assert_eq!(
            seats.iter().map(|seat| seat.name.as_str()).collect::<Vec<_>>(),
            [ADA, "Grace"],
            "this account first"
        );
        assert!(seats[0].own() && seats[0].elsewhere, "listed, and no call here: the seat is on another device");
        assert!(seats[1].muted && !seats[1].elsewhere);
        assert!(window.places()[0].elsewhere(&channel));
        // Taking the seat back from the other device is said, and the room is then the channel's list.
        tell(
            &mut window,
            &clock,
            place,
            place::Message::Lobby(place::LobbyMessage::Join(crate::harness::channel_id("lounge"))),
        );
        assert_eq!(
            window.toast().map(|toast| toast.said().in_english()).as_deref(),
            None,
            "the call here is the answer"
        );
        assert!(!window.places()[0].elsewhere(&channel));
    }

    #[test]
    fn the_servers_channel_changes_keep_the_list_and_a_calls_channel_name_current() {
        use proto::chatkord_v1::{
            ChannelAdded, ChannelKind, ChannelRemoved, ChannelRenamed, ChannelSummary, Event, MemberLeft,
            MemberSummary, VoiceOccupancy, event,
        };
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = holding(&clock, Grants::default());
        in_call(&mut window, &clock, owls_id);
        let heard = |kind| place::Message::heard(&Event { cursor: Vec::new(), kind: Some(kind) });
        let told = |window: &mut Window, kind| tell(window, &clock, owls_id, heard(kind).expect("read"));
        let shown = |window: &Window| match window.open().unwrap().lobby() {
            Some(Lobby::Ready(channels)) => {
                channels.voice.iter().map(|c| (c.name.clone(), c.in_voice())).collect::<Vec<_>>()
            }
            other => panic!("no list: {other:?}"),
        };
        let renamed = |version| ChannelRenamed { channel_id: "lounge".into(), name: "Big room".into(), version };
        told(&mut window, event::Kind::ChannelRenamed(renamed(1)));
        assert_eq!(window.open().unwrap().call().unwrap().channel.name, "Big room", "the call goes on, renamed");
        let people = ["ada", "bo", "cy"].map(|user_id| MemberSummary { user_id: user_id.into(), ..Default::default() });
        let occupied = VoiceOccupancy { channel_id: "movie".into(), people: people.to_vec() };
        told(&mut window, event::Kind::VoiceOccupancy(occupied));
        let movie = ChannelSummary {
            channel_id: "movie".into(),
            name: "Movie night".into(),
            kind: ChannelKind::Voice.into(),
            timeline_id: "t-movie".into(),
            category_id: "chat".into(),
            position: 2,
            ..Default::default()
        };
        told(&mut window, event::Kind::ChannelAdded(ChannelAdded { channel: Some(movie), version: 2 }));
        assert_eq!(
            shown(&window),
            [("Big room".into(), 0), ("Movie night".into(), 3)],
            "made last, with who the room was told to hold before the list knew it"
        );
        let todo = told(&mut window, event::Kind::ChannelRenamed(ChannelRenamed { name: "Old".into(), ..renamed(1) }));
        assert!(todo.is_empty(), "an event the list already holds is passed over: {todo:?}");
        assert_eq!(shown(&window)[0].0, "Big room");
        let todo =
            told(&mut window, event::Kind::ChannelRemoved(ChannelRemoved { channel_id: "movie".into(), version: 3 }));
        assert!(todo.is_empty(), "{todo:?}");
        assert_eq!(shown(&window), [("Big room".into(), 0)]);
        let left = MemberLeft { user_id: "bo".into(), channel_id: "lounge".into() };
        assert!(heard(event::Kind::MemberLeft(left)).is_none(), "a room event is the call's to read");
    }

    /// What the server told, read as the place reads it.
    fn heard(kind: proto::chatkord_v1::event::Kind) -> place::Message {
        place::Message::heard(&proto::chatkord_v1::Event { cursor: Vec::new(), kind: Some(kind) }).expect("read")
    }

    /// The list shown: the text channels' names, then the voice channels', each in the list's order.
    fn names(window: &Window) -> (Vec<String>, Vec<String>) {
        match window.open().unwrap().lobby() {
            Some(Lobby::Ready(listed)) => (
                listed.text.iter().map(|channel| channel.name.clone()).collect(),
                listed.voice.iter().map(|channel| channel.name.clone()).collect(),
            ),
            other => panic!("no list: {other:?}"),
        }
    }

    /// The list held, whole.
    fn held(window: &Window) -> ChannelList {
        match window.open().unwrap().lobby() {
            Some(Lobby::Ready(listed)) => listed.clone(),
            other => panic!("no list: {other:?}"),
        }
    }

    #[test]
    fn a_change_past_a_gap_is_applied_and_lists_nothing_again() {
        use proto::chatkord_v1::{ChannelRenamed, event};
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = holding(&clock, Grants::default());
        // The list is at version 0; versions 1 to 4 changed a channel this account does not see.
        let renamed = ChannelRenamed { channel_id: GENERAL.into(), name: "town-square".into(), version: 5 };
        let todo = tell(&mut window, &clock, owls_id, heard(event::Kind::ChannelRenamed(renamed)));
        assert!(todo.is_empty(), "a version skipped is no change missed: {todo:?}");
        assert_eq!(names(&window).0, ["town-square"]);
        assert_eq!(held(&window).version, 5);
        let older = ChannelRenamed { channel_id: GENERAL.into(), name: "general".into(), version: 3 };
        let todo = tell(&mut window, &clock, owls_id, heard(event::Kind::ChannelRenamed(older)));
        assert!(todo.is_empty(), "{todo:?}");
        assert_eq!(names(&window).0, ["town-square"], "one at or below the list's version is already in it");
    }

    #[test]
    fn a_change_told_while_the_list_is_read_survives_its_reply() {
        use proto::chatkord_v1::{ChannelAdded, ChannelKind, ChannelRenamed, ChannelSummary, event};
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = holding(&clock, Grants::default());
        let todo = tell(
            &mut window,
            &clock,
            owls_id,
            place::Message::Session(place::SessionMessage::Standing {
                place: proto::rules::Place::BOTTOM,
                grants: Grants::default(),
            }),
        );
        assert!(todo.iter().any(|done| matches!(done, crate::Ask::At(_, ask1) if crate::harness::is(ask1, crate::harness::Op::ChannelsList))), "{todo:?}");
        // Renamed after the server read the list it answers with, and told before that answer.
        let renamed = ChannelRenamed { channel_id: GENERAL.into(), name: "town-square".into(), version: 1 };
        tell(&mut window, &clock, owls_id, heard(event::Kind::ChannelRenamed(renamed)));
        let todo =
            tell(&mut window, &clock, owls_id, place::Message::Lobby(place::LobbyMessage::Channels(Ok(channels()))));
        assert!(todo.is_empty(), "{todo:?}");
        assert_eq!(names(&window).0, ["town-square"], "the rename outlives the older list");
        assert_eq!(held(&window).version, 1);

        // So does a channel made while the first list is read, before there is a list to apply it to.
        let (mut window, _) = Window::launch(vec![Opening::Account(owls(), ADA.into())], None, clock.0);
        let owls_id = id(&window, 0);
        tell(&mut window, &clock, owls_id, flow(token()));
        tell(&mut window, &clock, owls_id, flow(accepted(Grants::default())));
        let movie = ChannelSummary {
            channel_id: "movie".into(),
            name: "Movie night".into(),
            kind: ChannelKind::Voice.into(),
            timeline_id: "t-movie".into(),
            category_id: "chat".into(),
            position: 9,
            ..Default::default()
        };
        tell(
            &mut window,
            &clock,
            owls_id,
            heard(event::Kind::ChannelAdded(ChannelAdded { channel: Some(movie), version: 1 })),
        );
        tell(&mut window, &clock, owls_id, place::Message::Lobby(place::LobbyMessage::Channels(Ok(channels()))));
        assert_eq!(names(&window).1, ["Lounge", "Movie night"]);
        // A list read after it holds it already: it is not made twice.
        let mut later = channels();
        later.version = 1;
        later.voice.push(Channel {
            id: crate::harness::channel_id("movie"),
            name: "Movie night".into(),
            people: Vec::new(),
            timeline: TimelineId::parse("t-movie").unwrap(),
        });
        tell(&mut window, &clock, owls_id, place::Message::Lobby(place::LobbyMessage::Channels(Ok(later))));
        assert_eq!(names(&window).1, ["Lounge", "Movie night"]);
    }

    #[test]
    fn a_change_applied_again_changes_nothing() {
        use proto::chatkord_v1::{ChannelAdded, ChannelKind, ChannelRemoved, ChannelSummary, event};
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = holding(&clock, Grants::default());
        let lounge = ChannelSummary {
            channel_id: "lounge".into(),
            name: "Lounge".into(),
            kind: ChannelKind::Voice.into(),
            timeline_id: "t-lounge".into(),
            category_id: "chat".into(),
            ..Default::default()
        };
        tell(
            &mut window,
            &clock,
            owls_id,
            heard(event::Kind::ChannelAdded(ChannelAdded { channel: Some(lounge), version: 2 })),
        );
        assert_eq!(names(&window).1, ["Lounge"], "a channel made again replaces the one with its id");
        for version in [3, 4] {
            let removed = ChannelRemoved { channel_id: "nowhere".into(), version };
            let todo = tell(&mut window, &clock, owls_id, heard(event::Kind::ChannelRemoved(removed)));
            assert!(todo.is_empty(), "{todo:?}");
        }
        assert_eq!(names(&window), (vec!["general".to_string()], vec!["Lounge".to_string()]), "nothing to delete");
    }

    #[test]
    fn a_channel_moved_into_a_category_is_listed_there_in_its_place() {
        use proto::chatkord_v1::{
            CategoryMoved, CategoryRemoved, CategoryRenamed, CategorySummary, ChannelKind, ChannelMoved,
            ChannelSummary, ChannelsSummary, event,
        };
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = holding(&clock, Grants::default());
        let text = |id: &str, category: &str, position| ChannelSummary {
            channel_id: id.into(),
            name: id.into(),
            kind: ChannelKind::Text.into(),
            position,
            timeline_id: format!("t-{id}"),
            category_id: category.to_string(),
            ..Default::default()
        };
        let category =
            |id: &str, name: &str, position| CategorySummary { category_id: id.into(), name: name.into(), position };
        let summary = ChannelsSummary {
            channels: vec![
                text("chess", "games", 3),
                text("plans", "talk", 5),
                text("general", "talk", 1),
                text("clips", "games", 1),
            ],
            version: 7,
            standings: Vec::new(),
            categories: vec![category("games", "Games", 2), category("talk", "Talk", 1)],
        };
        let listed = place::listed(&summary);
        tell(&mut window, &clock, owls_id, place::Message::Lobby(place::LobbyMessage::Channels(Ok(listed))));
        assert_eq!(names(&window).0, ["general", "plans", "clips", "chess"], "Talk first, then Games");

        let moved = ChannelMoved { channel_id: "general".into(), category_id: "games".into(), position: 2, version: 8 };
        let todo = tell(&mut window, &clock, owls_id, heard(event::Kind::ChannelMoved(moved)));
        assert!(todo.is_empty(), "{todo:?}");
        assert_eq!(names(&window).0, ["plans", "clips", "general", "chess"], "between its new neighbours");
        let general = held(&window).placed.get(&crate::harness::channel_id(GENERAL)).cloned();
        let games = terms::CategoryId::parse("games").expect("an id");
        assert_eq!(general, Some(Placement { category: games, position: 2 }));

        let first = CategoryMoved { category_id: "games".into(), position: 0, version: 9 };
        tell(&mut window, &clock, owls_id, heard(event::Kind::CategoryMoved(first)));
        assert_eq!(names(&window).0, ["clips", "general", "chess", "plans"], "Games now comes before Talk");
        let renamed = CategoryRenamed { category_id: "talk".into(), name: "Chat".into(), version: 10 };
        tell(&mut window, &clock, owls_id, heard(event::Kind::CategoryRenamed(renamed)));
        let shown: Vec<String> = held(&window).categories.iter().map(|category| category.name.clone()).collect();
        assert_eq!(shown, ["Games", "Chat"]);
        tell(
            &mut window,
            &clock,
            owls_id,
            heard(event::Kind::CategoryRemoved(CategoryRemoved { category_id: "talk".into(), version: 12 })),
        );
        let listed = held(&window);
        let shown: Vec<&str> = listed
            .in_order()
            .into_iter()
            .map(|entry| match entry {
                ListEntry::Category(category) => category.name.as_str(),
                ListEntry::Text(channel) => channel.name.as_str(),
                ListEntry::Voice(channel) => channel.name.as_str(),
            })
            .collect();
        assert_eq!(shown, ["Games", "clips", "general", "chess"], "a deleted category's channels go with it");
        assert_eq!(listed.categories.len(), 1);
        assert!(listed.text.iter().all(|channel| channel.name != "plans"), "no channel is left in a deleted category");
    }

    #[test]
    fn the_composer_and_the_share_follow_the_channel_s_words_not_the_account_s() {
        let clock = Clock(Instant::now());
        let words = |grants: &[Grant]| grants.iter().copied().collect::<Grants>();
        let told = |channel: &str, grants: Grants| {
            let standing = ChannelStanding { grants, ..ChannelStanding::default() };
            let channel_id = crate::harness::channel_id(channel);
            place::Message::Lobby(place::LobbyMessage::Standing { channel_id, standing })
        };

        // The account may send; #general's overwrite denies it there.
        let (mut window, owls_id) = crate::harness::chatting(&clock);
        assert!(window.places()[0].may_send(crate::place::Pane::Content));
        tell(&mut window, &clock, owls_id, told(GENERAL, words(&[Grant::ChannelRead])));
        assert!(window.places()[0].grants().holds(Grant::SendMessage), "the account's words are unchanged");
        assert!(!window.places()[0].may_send(crate::place::Pane::Content));
        let discussion = |message| place::Message::Discussion(message);
        tell(&mut window, &clock, owls_id, discussion(place::DiscussionMessage::Draft("hi".into())));
        let todo = tell(&mut window, &clock, owls_id, discussion(place::DiscussionMessage::Send));
        assert!(todo.is_empty(), "nothing is sent: {todo:?}");
        // The account may not send; #general's overwrite allows it there.
        tell(
            &mut window,
            &clock,
            owls_id,
            place::Message::Session(place::SessionMessage::Standing {
                place: proto::rules::Place::BOTTOM,
                grants: words(&[Grant::ChannelRead]),
            }),
        );
        tell(&mut window, &clock, owls_id, told(GENERAL, words(&[Grant::ChannelRead, Grant::SendMessage])));
        assert!(window.places()[0].may_send(crate::place::Pane::Content));
        let todo = tell(&mut window, &clock, owls_id, discussion(place::DiscussionMessage::Send));
        assert!(matches!(&todo[..], [crate::Ask::At(_, place::Ask::SendMessage { .. })]), "{todo:?}");

        // The account may share and send its camera; Lounge's overwrites deny both there, then allow them again.
        let (mut window, owls_id) = crate::harness::moderator_in_call(
            &clock,
            proto::rules::Place::BOTTOM,
            words(&[Grant::StartVoice, Grant::ShareScreen, Grant::ShareCamera]),
        );
        assert_eq!(window.places()[0].can_share(), crate::Can::Yes);
        tell(&mut window, &clock, owls_id, told("lounge", words(&[Grant::StartVoice])));
        assert_eq!(window.places()[0].can_share(), crate::Can::NotAllowed);
        assert_eq!(window.places()[0].can_camera(), crate::CanCamera::NotAllowed);
        let todo = tell(&mut window, &clock, owls_id, place::Message::Call(place::CallMessage::ToggleCamera));
        assert!(todo.is_empty(), "{todo:?}");
        tell(&mut window, &clock, owls_id, told("lounge", words(&[Grant::ShareScreen, Grant::ShareCamera])));
        assert_eq!(window.places()[0].can_share(), crate::Can::Yes);
        assert_eq!(window.places()[0].can_camera(), crate::CanCamera::Yes);
    }

    #[test]
    fn a_channels_standing_keeps_the_highest_read_and_a_change_of_roles_lists_again() {
        use proto::chatkord_v1::{ChannelStanding, Event, event};
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = holding(&clock, Grants::default());
        let heard = |kind| place::Message::heard(&Event { cursor: Vec::new(), kind: Some(kind) }).expect("read");
        let told = |read| ChannelStanding {
            channel_id: GENERAL.into(),
            grants: vec!["CHANNEL_READ".into()],
            read,
            ..ChannelStanding::default()
        };
        let kept = |window: &Window| match window.open().unwrap().lobby() {
            Some(Lobby::Ready(channels)) => channels.standings.get(&crate::harness::channel_id(GENERAL)).copied(),
            other => panic!("no list: {other:?}"),
        };
        tell(&mut window, &clock, owls_id, heard(event::Kind::ChannelStanding(told(7))));
        tell(&mut window, &clock, owls_id, heard(event::Kind::ChannelStanding(told(5))));
        let standing = kept(&window).expect("a standing for the channel");
        assert_eq!(standing.read, 7, "a mark told late never takes back one read further");
        assert!(standing.grants.holds(Grant::ChannelRead));
        let todo = tell(
            &mut window,
            &clock,
            owls_id,
            place::Message::Session(place::SessionMessage::Standing {
                place: proto::rules::Place::BOTTOM,
                grants: Grants::default(),
            }),
        );
        assert!(todo.iter().any(|done| matches!(done, crate::Ask::At(_, ask1) if crate::harness::is(ask1, crate::harness::Op::ChannelsList))), "{todo:?}");
    }

    #[test]
    fn someone_elses_rename_reaches_the_channels_list() {
        let clock = Clock(Instant::now());
        let (mut window, owls_id) = ada_in_call(&clock);
        let grace = place::Occupant {
            user_id: member("grace"),
            colour: None,
            name: "Grace".into(),
            state: State::default(),
            sharing: false,
            camera: false,
            held: false,
            place: proto::rules::Place::BOTTOM,
        };
        let occupancy = place::Message::Lobby(place::LobbyMessage::Occupancy {
            channel_id: crate::harness::channel_id("lounge"),
            people: vec![grace],
        });
        tell(&mut window, &clock, owls_id, occupancy);
        let renamed = place::Message::Server(place::ServerMessage::Renamed {
            user_id: member("grace"),
            name: "Grace Hopper".into(),
        });
        tell(&mut window, &clock, owls_id, renamed);
        let place = window.open().unwrap();
        let Some(Lobby::Ready(listed)) = place.lobby() else { panic!("listed") };
        assert_eq!(listed.voice[0].people[0].name, "Grace Hopper");
        assert_eq!(place.shown(), Some(ADA), "not this account");
    }
}
