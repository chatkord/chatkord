//! A server's settings in the content area: which pages a person may open, by the grant words their roles hold and what
//! the server offers, and what the open one shows: the server's name and colour, the roles, the channels (the list the
//! pane shows), the server's emoji, the invites that work, or the members.

use crate::channel_settings::ChannelSettings;
use crate::door::Server;
use crate::emoji_settings::EmojiSettings;
use crate::invite::ListedInvite;
use crate::loaded::Loaded;
use crate::members::{Finding, ListedMember};
use crate::notice::{self, Notice};
use crate::overview::Overview;
use crate::place::Served;
use crate::roles::Roles;
use crate::server_about::ServerAbout;
use crate::webhooks::Webhooks;
use proto::rules::{Grant, Grants};

/// A page of a server's settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    /// The server's name, colour and picture.
    Overview,
    /// The roles in their order, to make, change, move and delete.
    Roles,
    /// The channels and categories, to make, rename, move and delete, and who sees each channel or writes in it.
    Channels,
    /// The server's own emoji, to upload, rename and delete.
    Emoji,
    /// The webhooks, to make, give a new secret and delete.
    Webhooks,
    /// The invites that still work.
    Invites,
    /// The members, to manage.
    Members,
    /// The server's release and protocol, and its debug report.
    About,
}

impl Page {
    /// Every page, in the order the list shows them.
    pub const ALL: [Page; 8] = [
        Page::Overview,
        Page::Roles,
        Page::Channels,
        Page::Emoji,
        Page::Webhooks,
        Page::Invites,
        Page::Members,
        Page::About,
    ];

    /// The pages someone holding `grants` may open, in the order the list shows them.
    pub(crate) fn offered(grants: Grants) -> Vec<Page> {
        Page::ALL.into_iter().filter(|page| grants.holds(page.needs())).collect()
    }

    const fn needs(self) -> Grant {
        match self {
            Page::Overview => Grant::ManageServer,
            Page::Roles => Grant::ManageRoles,
            Page::Channels => Grant::ManageChannel,
            Page::Emoji => Grant::ManageEmoji,
            Page::Invites => Grant::InviteMember,
            Page::Webhooks => Grant::ManageWebhooks,
            Page::Members => Grant::ManageMembers,
            Page::About => Grant::ManageServer,
        }
    }
}

/// A server's settings, open.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerSettings {
    pub(crate) page: Page,
    /// The server's name and colour, while that page is open.
    pub(crate) overview: Option<Overview>,
    /// The server's release, protocol and debug report, while that page is open.
    pub(crate) about: Option<ServerAbout>,
    /// The roles, while that page is open.
    pub(crate) roles: Option<Roles>,
    /// The channels, while that page is open.
    pub(crate) channels: Option<ChannelSettings>,
    /// The emoji page's own state, while that page is open; the emoji themselves are the place's.
    pub(crate) emoji: Option<EmojiSettings>,
    pub(crate) invites: Loaded<Vec<ListedInvite>>,
    /// The webhooks and the dialog the page is in, while that page is open.
    pub(crate) webhooks: Option<Webhooks>,
    pub(crate) members: Loaded<Vec<ListedMember>>,
    /// Which members the members page shows, and the menu open on one.
    pub(crate) finding: Finding,
    /// What the page says about an ask made on it that did not go through; a page opened anew has none.
    pub(crate) notices: Vec<Notice>,
}

impl ServerSettings {
    pub(crate) fn new(page: Page, server: &Server, served: &Served) -> ServerSettings {
        let overview = (page == Page::Overview).then(|| Overview::new(server, served));
        let roles = (page == Page::Roles).then(Roles::default);
        ServerSettings {
            page,
            overview,
            about: (page == Page::About).then(ServerAbout::default),
            roles,
            channels: None,
            emoji: (page == Page::Emoji).then(EmojiSettings::default),
            invites: Loaded::Loading,
            webhooks: (page == Page::Webhooks).then(Webhooks::default),
            members: Loaded::Loading,
            finding: Finding::default(),
            notices: Vec::new(),
        }
    }

    /// The roles page's own state; the roles themselves are the place's.
    pub(crate) fn roles(&self) -> Option<&Roles> {
        self.roles.as_ref()
    }

    /// The channels page: the channel chosen and what is being changed of it; none unless Channels is open.
    pub fn channels(&self) -> Option<&ChannelSettings> {
        self.channels.as_ref()
    }

    /// The emoji page: what is being uploaded or named, and the last refusal; none unless Emoji is open.
    pub fn emoji(&self) -> Option<&EmojiSettings> {
        self.emoji.as_ref()
    }

    /// Which members the members page shows, what its search holds, and the menu open on a row.
    pub fn finding(&self) -> &Finding {
        &self.finding
    }

    /// The server's name, colour and picture as set here; none unless Overview is open.
    pub fn overview(&self) -> Option<&Overview> {
        self.overview.as_ref()
    }

    /// The webhooks and the dialog the page is in; none unless Webhooks is open.
    pub fn webhooks(&self) -> Option<&Webhooks> {
        self.webhooks.as_ref()
    }

    /// The server's debug report, as About holds it; none unless About is open.
    pub fn about(&self) -> Option<&ServerAbout> {
        self.about.as_ref()
    }

    /// What the page says, as it shows it.
    pub fn notices(&self) -> Vec<Notice> {
        notice::shown(&self.notices)
    }

    /// The page open.
    pub fn page(&self) -> Page {
        self.page
    }

    /// The invites that still work, for the Invites page.
    pub fn invites(&self) -> &Loaded<Vec<ListedInvite>> {
        &self.invites
    }

    /// The members, for the Members page.
    pub fn members(&self) -> &Loaded<Vec<ListedMember>> {
        &self.members
    }
}

/// What one page of the settings holds while it is open, reached the same way for every page; the settings whole are
/// one too.
pub(crate) trait Part {
    /// The part in `settings`, while its page holds it.
    fn of(settings: &mut ServerSettings) -> Option<&mut Self>;
}

impl Part for ServerSettings {
    fn of(settings: &mut ServerSettings) -> Option<&mut Self> {
        Some(settings)
    }
}

impl Part for Overview {
    fn of(settings: &mut ServerSettings) -> Option<&mut Self> {
        settings.overview.as_mut()
    }
}

impl Part for ServerAbout {
    fn of(settings: &mut ServerSettings) -> Option<&mut Self> {
        settings.about.as_mut()
    }
}

impl Part for Roles {
    fn of(settings: &mut ServerSettings) -> Option<&mut Self> {
        settings.roles.as_mut()
    }
}

impl Part for ChannelSettings {
    fn of(settings: &mut ServerSettings) -> Option<&mut Self> {
        settings.channels.as_mut()
    }
}

impl Part for EmojiSettings {
    fn of(settings: &mut ServerSettings) -> Option<&mut Self> {
        settings.emoji.as_mut()
    }
}

impl Part for Webhooks {
    fn of(settings: &mut ServerSettings) -> Option<&mut Self> {
        settings.webhooks.as_mut()
    }
}

impl Part for Loaded<Vec<ListedInvite>> {
    fn of(settings: &mut ServerSettings) -> Option<&mut Self> {
        Some(&mut settings.invites)
    }
}

impl Part for Loaded<Vec<ListedMember>> {
    fn of(settings: &mut ServerSettings) -> Option<&mut Self> {
        Some(&mut settings.members)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_page_is_offered_to_whoever_holds_its_word_in_the_order_the_nav_shows() {
        let offered = |grants: &[Grant]| Page::offered(grants.iter().copied().collect());
        assert_eq!(offered(&[]), []);
        assert_eq!(offered(&[Grant::ManageChannel]), [Page::Channels]);
        assert_eq!(offered(&[Grant::InviteMember]), [Page::Invites]);
        assert_eq!(offered(&[Grant::ManageServer]), [Page::Overview, Page::About]);
        assert_eq!(offered(&[Grant::ManageWebhooks]), [Page::Webhooks]);
        assert_eq!(offered(&[Grant::ManageMembers]), [Page::Members]);
        assert_eq!(offered(&[Grant::ManageRoles]), [Page::Roles]);
        assert_eq!(offered(&[Grant::ManageEmoji]), [Page::Emoji]);
        let every: Grants = Grant::ALL.into_iter().collect();
        assert_eq!(Page::offered(every), Page::ALL);
    }
}
