//! The About of an app's post: the name an app posted under and the webhook that carries it, opened from the name
//! beside a message and put away as a person's popover is.

use super::*;
use crate::place::Pane;

/// An app's About, open on one post.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AppOpen {
    pub(super) place: PlaceId,
    pub(super) message: MessageId,
}

/// An app's About, as drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppAbout {
    /// The server the post is in.
    pub place: PlaceId,
    /// The name the post was made under.
    pub name: String,
    /// The webhook's own name when it posted.
    pub webhook_name: String,
    /// The channel the post is in; none where its name is unknown, such as a conversation.
    pub channel: Option<String>,
}

impl Window {
    /// The app's About open, as drawn; none where it is not, or its post is no longer shown.
    pub fn app_about(&self) -> Option<AppAbout> {
        let Some(menu::Opened::App(open)) = self.desk.menu.as_ref() else { return None };
        let place = self.place(open.place)?;
        [Pane::Content, Pane::CallChat, Pane::Thread].into_iter().find_map(|pane| {
            let shown = place.in_pane(pane)?;
            let post = shown.timeline()?.get(&open.message)?;
            let chat::Author::App { name, webhook_name } = &post.author else { return None };
            let channel = match &shown.owner {
                place::Owner::Channel(id) | place::Owner::Thread { channel: id, .. } => place.channel_name(id),
                place::Owner::Conversation(_) => None,
            };
            Some(AppAbout {
                place: open.place,
                name: name.clone(),
                webhook_name: webhook_name.clone(),
                channel: channel.map(str::to_owned),
            })
        })
    }

    /// An app's name pressed where it shows: its About opens, or closes when it is open on that post already.
    pub(super) fn open_app_about(&mut self, id: PlaceId, message: MessageId) -> Vec<Ask> {
        let same =
            matches!(&self.desk.menu, Some(menu::Opened::App(open)) if open.place == id && open.message == message);
        self.desk.menu = (!same).then_some(menu::Opened::App(AppOpen { place: id, message }));
        self.desk.toast = None;
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{Clock, chatting, message_id, said, tell, told};

    #[test]
    fn an_apps_about_opens_on_its_post_and_goes_with_a_press_outside() {
        let clock = Clock(Instant::now());
        let (mut window, owls) = chatting(&clock);
        let mut post = said(2, 1, "ada", "Game night starts in one hour.");
        post.author = chat::Author::App { name: "Home Assistant".into(), webhook_name: "Reminders".into() };
        tell(&mut window, &clock, owls, told(post));
        assert_eq!(window.app_about(), None);

        window.update(Message::AppAbout(owls, message_id(2)), clock.at());
        let about = window.app_about();
        assert_eq!(
            about.as_ref().map(|about| (about.name.as_str(), about.webhook_name.as_str())),
            Some(("Home Assistant", "Reminders"))
        );

        window.update(Message::CloseMenu, clock.at());
        assert_eq!(window.app_about(), None);

        window.update(Message::AppAbout(owls, message_id(2)), clock.at());
        window.update(Message::AppAbout(owls, message_id(2)), clock.at());
        assert_eq!(window.app_about(), None, "pressed again, it closes");

        window.update(Message::AppAbout(owls, message_id(1)), clock.at());
        assert_eq!(window.app_about(), None, "a member's post is no app's");
    }
}
