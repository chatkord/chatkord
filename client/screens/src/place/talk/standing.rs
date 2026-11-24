//! What this account has read in a timeline, and what owns it.

use super::*;

/// What this account has read of a conversation or a thread and how much is unread, as the server last said and as
/// told messages since count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TimelineStanding {
    /// The revision of its messages read through: one placed after it is unread.
    pub read: u64,
    /// The messages after `read` not this account's own, up to [`proto::rules::COUNT_CAP`].
    pub unread: u32,
    /// Of those, the ones that mention this account: every one of a conversation's.
    pub mentions: u32,
    /// This account follows the thread; never so for a conversation.
    pub followed: bool,
}

impl TimelineStanding {
    /// The standing the wire states, its counts within their cap.
    pub(in crate::place) fn read(told: &wire::TimelineStanding) -> TimelineStanding {
        TimelineStanding {
            read: told.read,
            unread: told.unread.min(proto::rules::COUNT_CAP),
            mentions: told.mentions.min(proto::rules::COUNT_CAP),
            followed: told.followed,
        }
    }

    /// The standing `told` says, over this one: the higher read mark stands, and the counts go with the newer mark.
    pub(super) fn over(self, told: TimelineStanding) -> TimelineStanding {
        let newer = if told.read >= self.read { told } else { self };
        TimelineStanding { read: self.read.max(told.read), followed: told.followed, ..newer }
    }

    /// One message more after the mark, mentioning this account or not; neither count passes its cap.
    pub(crate) fn count(&mut self, mentioned: bool) {
        let cap = proto::rules::COUNT_CAP;
        self.unread = (self.unread + 1).min(cap);
        if mentioned {
            self.mentions = (self.mentions + 1).min(cap);
        }
    }
}

impl Talk {
    /// This account's standing in a timeline of a conversation or a thread; nothing read where none was told.
    pub(crate) fn standing(&self, timeline: &TimelineId) -> TimelineStanding {
        self.standings.get(timeline).copied().unwrap_or_default()
    }
}

impl Place {
    /// What this account has read of the thread `timeline` and whether it follows it; nothing read where none was told.
    pub fn thread_standing(&self, timeline: &TimelineId) -> TimelineStanding {
        match &self.status {
            Status::Signed(signed) => signed.talk.standing(timeline),
            Status::Reconnecting(_) | Status::Door(_) => TimelineStanding::default(),
        }
    }

    /// A timeline's standing told: the higher read mark stands, a thread followed joins the list and one let go
    /// leaves it, and a popup for what is now read is taken away.
    pub(super) fn timeline_standing(&mut self, timeline: &TimelineId, told: TimelineStanding) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        let kept = signed.talk.standings.entry(timeline.clone()).or_default();
        let was = kept.followed;
        *kept = kept.over(told);
        let read = kept.read;
        let mut todo = Vec::new();
        if told.followed != was {
            match &mut signed.talk.followed {
                Loaded::Ready(listed) if !told.followed => listed.retain(|thread| thread.timeline != *timeline),
                Loaded::Ready(listed) if !listed.iter().any(|thread| thread.timeline == *timeline) => {
                    todo.push(ops::list_followed());
                }
                Loaded::Ready(_) | Loaded::Loading | Loaded::Failed(_) => {}
            }
        }
        todo.extend(self.withdraw_if_read(timeline, read));
        todo
    }

    /// What owns the timeline `timeline`, as far as this place knows: a listed channel of either kind, a listed
    /// conversation, or a thread this account follows or has open.
    pub(crate) fn owner_of(&self, timeline: &TimelineId) -> Option<Owner> {
        if let Some(Lobby::Ready(listed)) = self.lobby() {
            let text = listed.text.iter().find(|channel| channel.timeline == *timeline).map(|channel| &channel.id);
            let voice = listed.voice.iter().find(|channel| channel.timeline == *timeline).map(|channel| &channel.id);
            if let Some(id) = text.or(voice) {
                return Some(Owner::Channel(id.clone()));
            }
        }
        let Status::Signed(signed) = &self.status else { return None };
        if let Some(conversation) = signed.talk.conversation_of(timeline) {
            return Some(Owner::Conversation(conversation.id.clone()));
        }
        let open = signed.open().find(|open| open.shows(timeline) && matches!(open.owner, Owner::Thread { .. }));
        open.map(|open| open.owner.clone()).or_else(|| {
            let thread = signed.talk.thread(timeline)?;
            Some(Owner::Thread { channel: thread.channel.clone(), root: thread.root.clone() })
        })
    }
}

impl SignedIn {
    /// What this account has read in the timeline `timeline` of `owner`, if its standing is held.
    pub(crate) fn tally(&self, owner: &Owner, timeline: &TimelineId) -> Option<Readings> {
        match owner {
            Owner::Channel(id) => match &self.lobby {
                Lobby::Ready(listed) => listed.standings.get(id).map(|standing| Readings::from(*standing)),
                Lobby::Loading | Lobby::Failed(_) => None,
            },
            Owner::Conversation(_) | Owner::Thread { .. } => {
                self.talk.standings.get(timeline).map(|standing| Readings::from(*standing))
            }
        }
    }

    /// `owner`'s timeline read through `revision`: nothing is unread in it.
    pub(crate) fn read_through(&mut self, owner: &Owner, timeline: &TimelineId, revision: u64) {
        match owner {
            Owner::Channel(id) => {
                if let Lobby::Ready(listed) = &mut self.lobby {
                    let standing = listed.standings.entry(id.clone()).or_default();
                    *standing = ChannelStanding { read: revision, unread: 0, mentions: 0, ..*standing };
                }
            }
            Owner::Conversation(_) | Owner::Thread { .. } => {
                let standing = self.talk.standings.entry(timeline.clone()).or_default();
                *standing = TimelineStanding { read: revision, unread: 0, mentions: 0, ..*standing };
            }
        }
    }
}
