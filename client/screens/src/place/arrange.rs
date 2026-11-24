//! The channel list's order as a person changes it: the rows the side pane draws (a folded category keeps only what is
//! open), a row dragged or moved by a key read as the move the server takes, shown at once and put back when refused,
//! and a move refused because the list changed asked once more over the list read again.

use super::*;
use crate::order::Moved;
use std::collections::BTreeSet;

/// A row of the channel list, as a drag and a menu name it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ListKey {
    /// A category's header.
    Category(CategoryId),
    /// A channel, of either kind.
    Channel(ChannelId),
}

/// A move of the list as the server takes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListMove {
    /// A channel into a category, just before a channel there or last.
    Channel {
        /// The channel moved.
        id: ChannelId,
        /// The category it goes in.
        category: CategoryId,
        /// The channel it goes just before; none for last.
        before: Option<ChannelId>,
    },
    /// A category just before another, or last.
    Category {
        /// The category moved.
        id: CategoryId,
        /// The category it goes just before; none for last.
        before: Option<CategoryId>,
    },
}

/// Whether a category shows its channels in the side pane, as this device keeps it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Fold {
    /// Its channels show under it.
    #[default]
    Open,
    /// Only the channel open and the call this account is in show under it.
    Folded,
}

/// A row of the side pane's channel list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneRow<'a> {
    /// A category's header, folded or open; one holding no channel shows only to whoever manages the channels.
    Category(&'a Category, Fold),
    /// A text channel, and who sees it.
    Text(&'a TextChannel, Sight),
    /// A voice channel, with who is in its call, and who sees it.
    Voice(&'a Channel, Sight),
}

impl PaneRow<'_> {
    /// The row's key, as a drag and a menu name it.
    pub fn key(&self) -> ListKey {
        match self {
            PaneRow::Category(category, _) => ListKey::Category(category.id.clone()),
            PaneRow::Text(channel, _) => ListKey::Channel(channel.id.clone()),
            PaneRow::Voice(channel, _) => ListKey::Channel(channel.id.clone()),
        }
    }
}

impl ListEntry<'_> {
    /// The row's key, as a drag and a menu name it.
    pub fn key(&self) -> ListKey {
        match self {
            ListEntry::Category(category) => ListKey::Category(category.id.clone()),
            ListEntry::Text(channel) => ListKey::Channel(channel.id.clone()),
            ListEntry::Voice(channel) => ListKey::Channel(channel.id.clone()),
        }
    }
}

impl ChannelList {
    /// The category the channel `id` is in as listed; none for a channel not listed, or one whose category the list does
    /// not hold.
    fn category_of(&self, id: &ChannelId) -> Option<CategoryId> {
        self.find(id)?;
        let category = &self.placed.get(id)?.category;
        self.categories.iter().any(|listed| listed.id == *category).then(|| category.clone())
    }

    /// The channels of one category, in their order.
    fn group(&self, category: &CategoryId) -> Vec<ChannelId> {
        self.in_order()
            .into_iter()
            .filter_map(|entry| match entry {
                ListEntry::Text(channel) => Some(channel.id.clone()),
                ListEntry::Voice(channel) => Some(channel.id.clone()),
                ListEntry::Category(_) => None,
            })
            .filter(|id| self.category_of(id).as_ref() == Some(category))
            .collect()
    }

    /// What `moved` asks of the server, read against this list; none for a move that changes nothing or may not be
    /// made. A channel dropped before a channel goes into that one's category, just before it; before a category's
    /// header or at the end, last in the group above, so a drop just under a folded or empty header lands in it; above
    /// the first header, first in the first category. A category moves only before another header, or last.
    pub fn move_of(&self, moved: &Moved<ListKey>) -> Option<ListMove> {
        match &moved.key {
            ListKey::Channel(id) => {
                self.category_of(id)?;
                let (category, before) = match &moved.before {
                    Some(ListKey::Channel(before)) if before == id => return None,
                    Some(ListKey::Channel(before)) => (self.category_of(before)?, Some(before.clone())),
                    Some(ListKey::Category(header)) => {
                        let at = self.categories.iter().position(|category| category.id == *header)?;
                        match at.checked_sub(1) {
                            Some(above) => (self.categories[above].id.clone(), None),
                            None => {
                                let first = self.categories[0].id.clone();
                                let top = self.group(&first).into_iter().find(|one| one != id);
                                (first, top)
                            }
                        }
                    }
                    None => (self.categories.last()?.id.clone(), None),
                };
                let group = self.group(&category);
                if let Some(at) = group.iter().position(|one| one == id)
                    && group.get(at + 1) == before.as_ref()
                {
                    return None;
                }
                Some(ListMove::Channel { id: id.clone(), category, before })
            }
            ListKey::Category(id) => {
                let at = self.categories.iter().position(|category| category.id == *id)?;
                let before = match &moved.before {
                    Some(ListKey::Category(before)) if before == id => return None,
                    Some(ListKey::Category(before)) => {
                        self.categories.iter().any(|category| category.id == *before).then(|| before.clone())?;
                        Some(before.clone())
                    }
                    Some(ListKey::Channel(_)) => return None,
                    None => None,
                };
                if self.categories.get(at + 1).map(|next| &next.id) == before.as_ref() {
                    return None;
                }
                Some(ListMove::Category { id: id.clone(), before })
            }
        }
    }

    /// Shows `moved` at once, where the server will place it: the moved row alone takes a position between its new
    /// neighbours' own, so the server's `ChannelMoved` or `CategoryMoved` for it, which names its position alone,
    /// lands among positions the server wrote.
    pub(super) fn show(&mut self, moved: &ListMove) {
        match moved {
            ListMove::Channel { id, category, before } => {
                let group: Vec<i64> = self
                    .group(category)
                    .into_iter()
                    .filter(|one| one != id)
                    .map(|one| self.placed.get(&one).map_or(0, |placed| placed.position))
                    .collect();
                let at = before.as_ref().and_then(|before| self.placed.get(before)).map(|placed| placed.position);
                let position = shown_between(&group, at);
                self.placed.insert(id.clone(), Placement { category: category.clone(), position });
            }
            ListMove::Category { id, before } => {
                let others: Vec<i64> =
                    self.categories.iter().filter(|category| category.id != *id).map(|c| c.position).collect();
                let at = before
                    .as_ref()
                    .and_then(|before| self.categories.iter().find(|category| category.id == *before))
                    .map(|category| category.position);
                let position = shown_between(&others, at);
                if let Some(category) = self.categories.iter_mut().find(|category| category.id == *id) {
                    category.position = position;
                }
            }
        }
        self.arrange();
    }
}

/// A position just before `before` among `order` (ascending), or after the last: the midpoint of its neighbours. Only
/// shown until the server's own number for it arrives, so where no number is left between them it may tie.
fn shown_between(order: &[i64], before: Option<i64>) -> i64 {
    match before {
        Some(upper) => match order.iter().copied().filter(|&one| one < upper).max() {
            Some(lower) => lower.midpoint(upper),
            None => upper.saturating_sub(1),
        },
        None => order.iter().copied().max().map_or(0, |last| last.saturating_add(1)),
    }
}

/// A move shown at once, waiting for the server's answer.
#[derive(Debug)]
pub(super) struct Moving {
    moved: Moved<ListKey>,
    /// The list as it was before the move was shown, put back when the server refuses it.
    unmoved: ChannelList,
    tries: Tries,
}

/// Where a move stands with the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tries {
    /// Asked once.
    First,
    /// Refused as the list changed: the list is read again, and the move tried once more over it.
    Listing,
    /// Asked again over the list read again: a second refusal is said.
    Last,
}

/// What the server is asked for `moved`, made against the list at `version`.
fn ask(moved: ListMove, version: u64) -> Ask {
    match moved {
        ListMove::Channel { id, category, before } => ops::move_channel(&id, &category, before.as_ref(), version),
        ListMove::Category { id, before } => ops::move_category(&id, before.as_ref(), version),
    }
}

impl Place {
    /// The side pane's rows, in the list's order: each category and its own channels. A
    /// folded category keeps only the text channel open and the voice channel this account is in; a category holding
    /// no channel this account sees shows only to whoever manages the channels.
    pub fn pane_rows(&self) -> Vec<PaneRow<'_>> {
        let Some(Lobby::Ready(listed)) = self.lobby() else { return Vec::new() };
        let manages = self.manages_channels();
        let open = self.discussion().and_then(|open| match &open.owner {
            Owner::Channel(id) => Some(id),
            Owner::Conversation(_) | Owner::Thread { .. } => None,
        });
        let call = self.live_call().map(|channel| &channel.id);
        let kept = |id: &ChannelId, voice: Option<&Channel>| {
            Some(id) == open || Some(id) == call || voice.is_some_and(|channel| self.elsewhere(channel))
        };
        let entries = listed.in_order();
        let mut rows = Vec::with_capacity(entries.len());
        let mut fold = Fold::Open;
        for (at, entry) in entries.iter().enumerate() {
            match *entry {
                ListEntry::Category(category) => {
                    fold = if self.folded.contains(&category.id) { Fold::Folded } else { Fold::Open };
                    let empty = !matches!(entries.get(at + 1), Some(ListEntry::Text(_) | ListEntry::Voice(_)));
                    if !empty || manages {
                        rows.push(PaneRow::Category(category, fold));
                    }
                }
                ListEntry::Text(channel) if fold == Fold::Open || kept(&channel.id, None) => {
                    rows.push(PaneRow::Text(channel, listed.sight(&channel.id)));
                }
                ListEntry::Voice(channel) if fold == Fold::Open || kept(&channel.id, Some(channel)) => {
                    rows.push(PaneRow::Voice(channel, listed.sight(&channel.id)));
                }
                ListEntry::Text(_) | ListEntry::Voice(_) => {}
            }
        }
        rows
    }

    /// The categories folded here, as this device keeps them for this server.
    pub fn folded(&self) -> &BTreeSet<CategoryId> {
        &self.folded
    }

    /// The categories this device kept folded for this server, as the host read them at the start.
    pub(crate) fn set_folded(&mut self, folded: BTreeSet<CategoryId>) {
        self.folded = folded;
    }

    /// Whether `moved` may be made here now: by whoever manages the channels, a move that changes something, while no
    /// other awaits the server.
    pub fn may_move(&self, moved: &Moved<ListKey>) -> bool {
        let Status::Signed(signed) = &self.status else { return false };
        let Lobby::Ready(listed) = &signed.lobby else { return false };
        self.manages_channels() && signed.moving.is_none() && listed.move_of(moved).is_some()
    }

    /// A category's header pressed: it folds, or opens, and this device keeps it.
    pub(super) fn fold(&mut self, category: CategoryId) -> Vec<Ask> {
        if !self.folded.remove(&category) {
            self.folded.insert(category);
        }
        vec![Ask::KeepFolded { server: self.server.url.clone(), folded: self.folded.clone() }]
    }
}

/// A row dragged, or moved by a key: shown at once and asked, one at a time.
pub(super) fn moved(signed: &mut SignedIn, manages: bool, moved: Moved<ListKey>) -> Vec<Ask> {
    if !manages || signed.moving.is_some() {
        return Vec::new();
    }
    let Lobby::Ready(listed) = &mut signed.lobby else { return Vec::new() };
    let Some(asked) = listed.move_of(&moved) else { return Vec::new() };
    let unmoved = listed.clone();
    listed.show(&asked);
    let version = unmoved.version;
    signed.moving = Some(Moving { moved, unmoved, tries: Tries::First });
    vec![ask(asked, version)]
}

/// The server's answer to a move: kept, or put back; a move refused because the list changed is asked once more over
/// the list read again, and a second refusal is said.
pub(crate) fn answered(signed: &mut SignedIn, answer: Result<(), LinkError>, told: &mut Option<Told>) -> Vec<Ask> {
    let Some(moving) = signed.moving.take() else { return Vec::new() };
    let Err(refused) = answer else { return Vec::new() };
    let again = matches!(refused, LinkError::Refused(proto::chatkord_v1::ErrorCode::ListChanged));
    let mut unmoved = moving.unmoved.clone();
    signed.recent.over(&mut unmoved, &signed.rooms);
    if let Lobby::Ready(listed) = &mut signed.lobby {
        *listed = unmoved;
    }
    if again && moving.tries == Tries::First {
        signed.moving = Some(Moving { tries: Tries::Listing, ..moving });
        return vec![ops::list_channels()];
    }
    let said = if again { Sentence::Phrase(Phrase::MoveRefused) } else { failure::said(&refused) };
    *told = Some(Told::Toast(Telling::of(design::Status::Danger, said)));
    Vec::new()
}

/// The list read again after a move was refused as the list changed: the move is asked once more over it, or said
/// refused where it no longer stands.
pub(super) fn retried(signed: &mut SignedIn, told: &mut Option<Told>) -> Vec<Ask> {
    let Some(moving) = signed.moving.take_if(|moving| moving.tries == Tries::Listing) else { return Vec::new() };
    let Lobby::Ready(listed) = &mut signed.lobby else { return Vec::new() };
    let Some(asked) = listed.move_of(&moving.moved) else {
        *told = Some(Told::Toast(Telling::of(design::Status::Danger, Sentence::Phrase(Phrase::MoveRefused))));
        return Vec::new();
    };
    let unmoved = listed.clone();
    listed.show(&asked);
    let version = unmoved.version;
    signed.moving = Some(Moving { moved: moving.moved, unmoved, tries: Tries::Last });
    vec![ask(asked, version)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{Clock, channel_id, holding, refused, tell};
    use crate::harness::{sends, wire};
    use crate::window::Window;
    use proto::chatkord_v1::{CategorySummary, ChannelKind, ChannelSummary, ChannelsSummary};
    use proto::rules::Grants;
    use std::time::Instant;

    fn category_id(id: &str) -> CategoryId {
        CategoryId::parse(id).unwrap()
    }

    /// The canvas's list: Chat with #general; Hangouts with #clips and Lounge; Projects with #game-night and #staff;
    /// and Archive, holding none.
    fn summary(version: u64) -> ChannelsSummary {
        let channel = |id: &str, kind: ChannelKind, category: &str, position| ChannelSummary {
            channel_id: id.into(),
            name: id.into(),
            kind: kind.into(),
            position,
            timeline_id: format!("t-{id}"),
            category_id: category.to_string(),
            ..Default::default()
        };
        let category =
            |id: &str, name: &str, position| CategorySummary { category_id: id.into(), name: name.into(), position };
        ChannelsSummary {
            channels: vec![
                channel("general", ChannelKind::Text, "chat", 0),
                channel("clips", ChannelKind::Text, "hangouts", 0),
                channel("lounge", ChannelKind::Voice, "hangouts", 1),
                channel("game-night", ChannelKind::Text, "projects", 0),
                channel("staff", ChannelKind::Text, "projects", 1),
            ],
            version,
            standings: Vec::new(),
            categories: vec![
                category("chat", "Chat", 0),
                category("hangouts", "Hangouts", 1),
                category("projects", "Projects", 2),
                category("archive", "Archive", 3),
            ],
        }
    }

    fn channel(key: &str) -> ListKey {
        ListKey::Channel(channel_id(key))
    }

    fn header(key: &str) -> ListKey {
        ListKey::Category(category_id(key))
    }

    fn moved(key: ListKey, before: Option<ListKey>) -> Moved<ListKey> {
        Moved { key, before }
    }

    fn into(id: &str, category: &str, before: Option<&str>) -> Option<ListMove> {
        Some(ListMove::Channel { id: channel_id(id), category: category_id(category), before: before.map(channel_id) })
    }

    #[test]
    fn a_channel_dropped_before_a_channel_joins_it_and_before_a_header_ends_the_group_above() {
        let list = listed(&summary(7));
        let of = |key, before| list.move_of(&moved(key, before));
        assert_eq!(of(channel("general"), Some(channel("lounge"))), into("general", "hangouts", Some("lounge")));
        assert_eq!(of(channel("staff"), Some(header("projects"))), into("staff", "hangouts", None));
        assert_eq!(of(channel("clips"), Some(header("hangouts"))), into("clips", "chat", None));
        assert_eq!(of(channel("clips"), Some(header("archive"))), into("clips", "projects", None));
        assert_eq!(of(channel("clips"), None), into("clips", "archive", None), "just under an empty header");
        assert_eq!(of(channel("clips"), Some(channel("lounge"))), None, "where it already is");
        assert_eq!(of(channel("staff"), Some(header("archive"))), None, "already last in its group");
        assert_eq!(of(channel("clips"), Some(channel("clips"))), None);
        assert_eq!(of(channel("nowhere"), None), None, "a channel not listed");
    }

    #[test]
    fn a_channel_dropped_above_the_first_header_lands_at_the_top_of_the_first_category() {
        let list = listed(&summary(7));
        let of = |key, before| list.move_of(&moved(key, before));
        assert_eq!(of(channel("clips"), Some(header("chat"))), into("clips", "chat", Some("general")));
        assert_eq!(of(channel("general"), Some(header("chat"))), None, "already first there");
        let mut bare = summary(7);
        bare.categories.clear();
        let bare = listed(&bare);
        assert_eq!(bare.move_of(&moved(channel("clips"), None)), None, "with no category there is no drop target");
    }

    #[test]
    fn a_category_moves_only_before_another_header_or_last() {
        let list = listed(&summary(7));
        let of = |key, before| list.move_of(&moved(key, before));
        let category = |id: &str, before: Option<&str>| {
            Some(ListMove::Category { id: category_id(id), before: before.map(category_id) })
        };
        assert_eq!(of(header("projects"), Some(header("hangouts"))), category("projects", Some("hangouts")));
        assert_eq!(of(header("hangouts"), None), category("hangouts", None));
        assert_eq!(of(header("hangouts"), Some(header("projects"))), None, "where it already is");
        assert_eq!(of(header("archive"), None), None, "already last");
        assert_eq!(of(header("hangouts"), Some(channel("game-night"))), None, "never among channels");
        let mut shown = list.clone();
        shown.show(&ListMove::Category { id: category_id("archive"), before: Some(category_id("hangouts")) });
        let names: Vec<&str> = shown.categories.iter().map(|category| category.name.as_str()).collect();
        assert_eq!(names, ["Chat", "Archive", "Hangouts", "Projects"]);
    }

    /// The server spaces positions and tells a moved row's own alone: the window that moved it keeps it where it was
    /// dropped once that arrives.
    #[test]
    fn a_move_shown_at_once_stays_where_it_was_dropped_once_the_server_tells_its_position() {
        let mut spaced = summary(7);
        for channel in &mut spaced.channels {
            channel.position = (channel.position + 1) * 1000;
        }
        for category in &mut spaced.categories {
            category.position *= 1000;
        }
        let mut list = listed(&spaced);
        let rooms = std::collections::HashMap::new();
        list.show(&ListMove::Channel {
            id: channel_id("general"),
            category: category_id("projects"),
            before: Some(channel_id("staff")),
        });
        let placed = Placement { category: category_id("projects"), position: 1500 };
        assert!(list.apply(ChannelChange::Moved { id: channel_id("general"), placed }, 8, &rooms));
        list.show(&ListMove::Category { id: category_id("archive"), before: Some(category_id("projects")) });
        assert!(list.apply(ChannelChange::CategoryMoved { id: category_id("archive"), position: 1500 }, 9, &rooms));
        let names: Vec<String> = list
            .in_order()
            .into_iter()
            .map(|entry| match entry {
                ListEntry::Category(category) => category.name.to_uppercase(),
                ListEntry::Text(channel) => channel.name.clone(),
                ListEntry::Voice(channel) => channel.name.clone(),
            })
            .collect();
        assert_eq!(
            names,
            ["CHAT", "HANGOUTS", "clips", "lounge", "ARCHIVE", "PROJECTS", "game-night", "general", "staff"]
        );
    }

    fn order(window: &Window) -> Vec<String> {
        let Some(Lobby::Ready(listed)) = window.open().unwrap().lobby() else { panic!("listed") };
        listed
            .in_order()
            .into_iter()
            .map(|entry| match entry {
                ListEntry::Category(category) => category.name.to_uppercase(),
                ListEntry::Text(channel) => channel.name.clone(),
                ListEntry::Voice(channel) => channel.name.clone(),
            })
            .collect()
    }

    fn manager(clock: &Clock) -> (Window, PlaceId) {
        let (mut window, owls) = holding(clock, Grants::from_words(&["MANAGE_CHANNEL"]));
        let listed = Message::Lobby(LobbyMessage::Channels(Ok(listed(&summary(7)))));
        tell(&mut window, clock, owls, listed);
        (window, owls)
    }

    const LISTED: [&str; 9] =
        ["CHAT", "general", "HANGOUTS", "clips", "lounge", "PROJECTS", "game-night", "staff", "ARCHIVE"];

    #[test]
    fn a_move_refused_as_the_list_changed_is_listed_again_and_tried_once_more_then_said() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = manager(&clock);
        let drag = Message::Lobby(LobbyMessage::Move(moved(channel("staff"), Some(channel("clips")))));
        let todo = tell(&mut window, &clock, owls, drag.clone());
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, ask)]
            if sends(
                ask,
                wire::ChannelMove {
                    channel_id: "staff".into(),
                    category_id: "hangouts".into(),
                    before: Some("clips".into()),
                    version: 7
                }
            )),
            "{todo:?}"
        );
        assert_eq!(
            order(&window),
            ["CHAT", "general", "HANGOUTS", "staff", "clips", "lounge", "PROJECTS", "game-night", "ARCHIVE"],
            "shown at once"
        );
        assert!(!window.open().unwrap().may_move(&moved(channel("general"), None)), "one move at a time");
        assert!(tell(&mut window, &clock, owls, drag).is_empty());
        let changed = Message::Lobby(LobbyMessage::Moved(Err(refused(proto::chatkord_v1::ErrorCode::ListChanged))));
        let todo = tell(&mut window, &clock, owls, changed.clone());
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, ask1)] if crate::harness::is(ask1, crate::harness::Op::ChannelsList)),
            "{todo:?}"
        );
        assert_eq!(order(&window), LISTED, "put back while the list is read again");
        let todo = tell(&mut window, &clock, owls, Message::Lobby(LobbyMessage::Channels(Ok(listed(&summary(9))))));
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, ask)] if matches!(ask.op(), Some(crate::harness::Body::ChannelMove(moved)) if moved.version == 9)),
            "tried once more over the list read again: {todo:?}"
        );
        assert_eq!(order(&window)[3], "staff");
        let todo = tell(&mut window, &clock, owls, changed);
        assert!(todo.iter().all(|done| matches!(done, crate::Ask::WakeAt(_))), "{todo:?}");
        assert_eq!(order(&window), LISTED, "put back");
        assert_eq!(
            window.toast().map(|toast| toast.said().in_english()).as_deref(),
            Some("The channel list changed. Try the move again.")
        );
        let todo = tell(
            &mut window,
            &clock,
            owls,
            Message::Lobby(LobbyMessage::Move(moved(header("archive"), Some(header("hangouts"))))),
        );
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, ask1)] if crate::harness::is(ask1, crate::harness::Op::CategoryMove)),
            "free to move again: {todo:?}"
        );
        tell(&mut window, &clock, owls, Message::Lobby(LobbyMessage::Moved(Ok(()))));
        assert_eq!(order(&window)[2], "ARCHIVE", "kept as shown");
    }

    #[test]
    fn a_folded_category_keeps_only_the_channel_open_and_a_category_with_none_shows_to_whoever_manages_them() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = manager(&clock);
        let todo = tell(&mut window, &clock, owls, Message::Lobby(LobbyMessage::Fold(category_id("projects"))));
        assert!(
            matches!(&todo[..], [crate::Ask::At(_, Ask::KeepFolded { folded, .. })] if folded.contains(&category_id("projects"))),
            "kept: {todo:?}"
        );
        let staff = DiscussionMessage::Open(channel_id("staff"));
        window.update(crate::WindowMessage::At(owls, Message::Discussion(staff)), clock.at());
        let rows = |window: &Window| -> Vec<String> {
            window
                .open()
                .unwrap()
                .pane_rows()
                .iter()
                .map(|row| match row {
                    PaneRow::Category(category, Fold::Folded) => format!("{} ▸", category.name),
                    PaneRow::Category(category, Fold::Open) => format!("{} ▾", category.name),
                    PaneRow::Text(channel, _) => channel.name.clone(),
                    PaneRow::Voice(channel, _) => channel.name.clone(),
                })
                .collect()
        };
        assert_eq!(
            rows(&window),
            ["Chat ▾", "general", "Hangouts ▾", "clips", "lounge", "Projects ▸", "staff", "Archive ▾"],
            "the channel open stays under its folded category"
        );
        let todo = tell(&mut window, &clock, owls, Message::Lobby(LobbyMessage::Fold(category_id("projects"))));
        assert!(matches!(&todo[..], [crate::Ask::At(_, Ask::KeepFolded { folded, .. })] if folded.is_empty()));
        let (mut member, owls) = holding(&clock, Grants::default());
        tell(&mut member, &clock, owls, Message::Lobby(LobbyMessage::Channels(Ok(listed(&summary(7))))));
        assert!(!rows(&member).contains(&"Archive ▾".to_string()), "an empty category is the managers'");
    }
}
