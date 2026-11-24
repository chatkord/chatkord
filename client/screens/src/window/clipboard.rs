//! What the front end is to put on the clipboard, and what is said once it went.

use super::*;

/// What the front end is to put on the clipboard, and for what.
#[derive(Debug)]
pub(super) struct Copying {
    pub(super) place: PlaceId,
    pub(super) text: Secret,
    pub(super) what: Copied,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Copied {
    /// A server's address: a toast says it went.
    Address,
    /// An invite's link: its Copy button says it went.
    Invite,
    /// A temporary password: its Copy button says it went.
    Temporary,
    /// A webhook's URL: its Copy button says it went.
    Webhook,
    /// A message's text: nothing says it went, as a selection's copy does not.
    Message,
}

impl Window {
    /// The text the front end is to put on the clipboard, reporting [`Message::Copied`].
    pub fn copying(&self) -> Option<&str> {
        self.desk.copying.as_ref().map(|copying| copying.text.expose())
    }

    /// The front end put the text on the clipboard, or could not: said where the copy was asked.
    pub(super) fn copied(&mut self, written: Result<(), NotCopied>) -> Vec<Ask> {
        let done = written.is_ok();
        match self.desk.copying.take() {
            Some(Copying { place, what: Copied::Address, .. }) if done => match self.place(place) {
                Some(place) => self.done(place.id(), Sentence::Copied(place.server().shown().to_string())),
                None => Vec::new(),
            },
            Some(Copying { place, what: Copied::Invite, .. }) => {
                if let Some(Dialog { asking: Asking::Invite(inviting), .. }) =
                    self.desk.dialog.as_mut().filter(|dialog| dialog.place == place)
                {
                    inviting.copied(done);
                }
                Vec::new()
            }
            Some(Copying { place, what: Copied::Temporary, .. }) => {
                if let Some(Dialog { asking: Asking::Reset(resetting), .. }) =
                    self.desk.dialog.as_mut().filter(|dialog| dialog.place == place)
                {
                    resetting.copied = done;
                }
                Vec::new()
            }
            Some(Copying { place, what: Copied::Webhook, .. }) => {
                if let Some(place) = self.places.iter_mut().find(|held| held.id() == place) {
                    place.webhook_copied(done);
                }
                Vec::new()
            }
            Some(Copying { what: Copied::Address | Copied::Message, .. }) | None => Vec::new(),
        }
    }

    /// Copy on a made webhook's dialog: its URL goes on the clipboard.
    pub(super) fn copy_webhook(&mut self, id: PlaceId) -> Vec<Ask> {
        let url = self.place(id).and_then(Place::webhook_url).cloned();
        if let Some(text) = url {
            self.desk.copying = Some(Copying { place: id, text, what: Copied::Webhook });
        }
        Vec::new()
    }

    /// Copy on a message's menu: its text goes on the clipboard and the menu closes.
    pub(super) fn copy_message(&mut self, id: PlaceId, message: &chat::MessageId, at: Moment) -> Vec<Ask> {
        self.desk.menu = None;
        let text = self
            .place(id)
            .and_then(Place::discussion)
            .and_then(place::Discussion::timeline)
            .and_then(|timeline| timeline.get(message))
            .and_then(|message| message.body.as_ref().map(|body| body.text().to_owned()));
        if let Some(text) = text {
            self.desk.copying = Some(Copying { place: id, text: Secret::new(text), what: Copied::Message });
        }
        self.at(id, place::Message::Discussion(place::DiscussionMessage::CloseMenu), at)
    }
}
