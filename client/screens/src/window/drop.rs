//! Files dragged over the window, as Discord takes them: while they hover, the channel they would go to is shaded
//! with where they go and how large each may be; dropped, they join that channel's pending uploads.

use super::*;
use crate::place::{DiscussionMessage, Pane};
use std::path::PathBuf;

/// Where files dragged over the window would go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DropTarget {
    /// The pane the channel is drawn in.
    pub pane: Pane,
    /// What its card says: the channel and the most a file may be.
    pub note: words::Sentence,
}

impl Window {
    /// Where files dragged over the window would go, while some are; none where the open channel takes no files.
    pub fn drop_target(&self) -> Option<DropTarget> {
        if !self.layout.dragging {
            return None;
        }
        let (pane, _) = self.drop_pane()?;
        let place = self.open()?;
        let open = place.in_pane(pane)?;
        let channel = place.channel_name(open.owner.channel()?)?.to_owned();
        let most = place.file_limits().attachment?;
        Some(DropTarget { pane, note: words::Sentence::DropNote { channel, most } })
    }

    /// The pane files dropped now go to, and its place: the channel shown, else a call's chat column beside it; none
    /// where neither takes files.
    fn drop_pane(&self) -> Option<(Pane, PlaceId)> {
        let place = self.open()?;
        let pane = match place.panel() {
            Panel::Discussion => Pane::Content,
            Panel::Call if place.call_chat().is_some() => Pane::CallChat,
            _ => return None,
        };
        let open = place.in_pane(pane)?;
        // Only a channel names where files go; a conversation's and a thread's are written to there.
        open.owner.channel()?;
        place.grants_of(&open.owner).holds(Grant::AttachFiles).then_some((pane, place.id()))
    }

    /// Files are over the window, or no longer.
    pub(super) fn files_hovered(&mut self, hovered: bool) -> Vec<Ask> {
        self.layout.dragging = hovered;
        Vec::new()
    }

    /// `paths` let go over the window: to the channel they would go to, if any.
    pub(super) fn files_dropped(&mut self, paths: Vec<PathBuf>, at: Moment) -> Vec<Ask> {
        self.layout.dragging = false;
        let Some((pane, id)) = self.drop_pane() else { return Vec::new() };
        self.at(id, pane.carrying(DiscussionMessage::Dropped(paths)), at)
    }
}
