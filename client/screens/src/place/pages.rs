//! The pages: the server's settings and the person's own account.

use super::*;

/// The page open over a server's channels, one at a time, in place of the side pane and the content.
#[derive(Debug)]
pub(super) enum OpenPage {
    /// The server's settings.
    Settings(Box<ServerSettings>),
    /// The person's own account.
    Account(Box<Account>),
}

/// `T` of the server's settings, while they are the page open: one way to every page's part, borrowing only the page.
pub(super) fn opened<T: crate::settings::Part>(page: &mut Option<OpenPage>) -> Option<&mut T> {
    match page.as_mut()? {
        OpenPage::Settings(open) => T::of(open),
        OpenPage::Account(_) => None,
    }
}

impl OpenPage {
    /// The server's settings, while they are the page.
    pub(super) fn settings(&self) -> Option<&ServerSettings> {
        match self {
            OpenPage::Settings(open) => Some(open),
            OpenPage::Account(_) => None,
        }
    }

    /// The account page, while it is the page.
    fn account(&self) -> Option<&Account> {
        match self {
            OpenPage::Account(open) => Some(open),
            OpenPage::Settings(_) => None,
        }
    }

    /// The same, to change.
    pub(super) fn account_mut(&mut self) -> Option<&mut Account> {
        match self {
            OpenPage::Account(open) => Some(open),
            OpenPage::Settings(_) => None,
        }
    }
}

/// The pages: the server's settings and the account.
#[derive(Clone, Debug)]
pub enum PageMessage {
    /// What the open settings page says of this subject, put away.
    DismissPageNotice(Subject),
    /// A page of the server's settings opened.
    Settings(Page),
    /// The server's settings closed.
    CloseSettings,
    /// Something done in the account page's name field.
    Name(crate::name_field::Message),
    /// The server's answer to setting the name others see.
    NameSet(Result<(), LinkError>),
    /// Something done on Server settings › Overview.
    Overview(overview::Message),
    /// Something done on Server settings › About.
    About(server_about::Message),
    /// Something done on Server settings › Webhooks.
    Webhooks(crate::webhooks::Message),
    /// The server's answer to turning a switch on or off.
    SwitchSet {
        /// The switch.
        switch: proto::rules::Switch,
        /// What it was asked to be.
        on: bool,
        /// The server's answer.
        answer: Result<(), LinkError>,
    },
    /// How the server is hosted and which switches are off, or why there is no answer.
    ServerHostingListed(Result<proto::chatkord_v1::ServerHosting, LinkError>),
    /// The server's answer to setting how it is hosted.
    ServerHostingSet(Result<(), LinkError>),
    /// The webhooks, or why there is no list.
    WebhooksListed(Result<Vec<proto::chatkord_v1::Webhook>, LinkError>),
    /// The server's answer to making a webhook or giving one a new secret: it and its secret, shown once.
    WebhookMade(Result<(proto::chatkord_v1::Webhook, Secret), LinkError>),
    /// The server's answer to deleting a webhook.
    WebhookDeleted(Result<(), LinkError>),
    /// The server's answer to naming it.
    ServerNameSet(Result<(), LinkError>),
    /// The server's answer to colouring its picture.
    ServerColourSet(Result<(), LinkError>),
    /// The server's answer to its debug report: its text as sent, already read as a report.
    ServerReported(Result<terms::ArrivedReport, terms::ReportError>),
    /// The picture row's picker or upload answered (Overview's, the account page's, or the set-up dialog's): the row
    /// of the page open takes it.
    Picture(picture_row::Message),
    /// The server's answer to setting its picture.
    ServerPictureSet(Result<(), LinkError>),
    /// The server's answer to setting this account's own picture.
    ProfilePictureSet(Result<(), LinkError>),
    /// Something done with the account page's colour picker.
    ProfileColour(crate::colour::Message),
    /// The server's answer to setting this account's colour.
    ProfileColourSet(Result<(), LinkError>),
    /// The account page opened (the gear on the account strip).
    OpenAccount,
    /// The account page closed.
    CloseAccount,
    /// Something done in the password dialog.
    Account(account::Message),
    /// The server's answer to a change of password.
    PasswordChanged(Result<(), LinkError>),
    /// The invites that still work, or why there is no list.
    Invites(Result<Vec<InviteSummary>, LinkError>),
    /// The server's answer to making an invite; the window's to read.
    Invited(Result<Invited, LinkError>),
    /// The server's answer to deleting one; the window's to read.
    InviteDeleted(Result<(), LinkError>),
    /// Something done on the members page apart from acting on a member: its lists, its search, a row's menu.
    Members(members::Message),
    /// Every member, page after page, or why there is no list.
    MembersListed(Result<Vec<MemberEntry>, LinkError>),
    /// The server's answer to giving the member named `name` a role or taking it; the window's to read.
    RoleSet {
        /// The name others see the member by, for what the window says of the answer.
        name: String,
        /// The server's answer.
        answer: Result<(), LinkError>,
    },
    /// The server's answer to a kick; the window's to read.
    Kicked(Result<(), LinkError>),
    /// The server's answer to a ban or an unban; the window's to read.
    BanSet(Result<(), LinkError>),
    /// Something done on the roles page.
    Roles(roles::Message),
    /// The server's answer to making a role: every role it now holds; the window's to read.
    RoleCreated(Result<Vec<RoleEntry>, LinkError>),
    /// The server's answer to renaming a role.
    RoleNamed {
        /// The role renamed.
        role: RoleId,
        /// The server's answer.
        answer: Result<(), LinkError>,
    },
    /// The server's answer to a change of a role shown at once: its colour, a word, its place; the window's to read.
    RoleChanged {
        /// The role's name, for what the page says of the answer.
        name: String,
        /// The server's answer.
        answer: Result<(), LinkError>,
    },
    /// The server's answer to deleting a role; the window's to read.
    RoleDeleted(Result<(), LinkError>),
    /// The server's answer to a reset: the temporary password to hand over; the window's to read.
    PasswordReset(Result<Secret, LinkError>),
    /// The server's answer to handing ownership on; the window's to read.
    Transferred(Result<(), LinkError>),
    /// The server's answer to making a channel: every channel it now holds; the window's to read.
    ChannelCreated(Result<ChannelsSummary, LinkError>),
    /// The server's answer to renaming a channel; the window's to read.
    ChannelRenamed(Result<(), LinkError>),
    /// The server's answer to deleting a channel; the window's to read.
    ChannelDeleted(Result<(), LinkError>),
    /// Something done on the channels page.
    Channels(crate::channel_settings::Message),
    /// The server's answer to a change of a channel's overwrite.
    OverwriteSet {
        /// The channel.
        channel: ChannelId,
        /// The server's answer.
        answer: Result<(), LinkError>,
    },
    /// The server's answer to making a category: every channel and category it now holds; the window's to read.
    CategoryCreated(Result<ChannelsSummary, LinkError>),
    /// The server's answer to renaming a category; the window's to read.
    CategoryRenamed(Result<(), LinkError>),
    /// The server's answer to deleting a category; the window's to read.
    CategoryDeleted(Result<(), LinkError>),
    /// Something done on the emoji page, or the answer to it; a card's bin is the window's, which asks first.
    Emoji(crate::emoji_settings::Message),
}

impl Place {
    /// The pages of the server's settings this account may open here.
    pub fn pages(&self) -> Vec<Page> {
        Page::offered(self.grants())
    }

    /// Whether this account may make, rename and delete the channels, as listed.
    pub fn manages_channels(&self) -> bool {
        self.grants().holds(Grant::ManageChannel) && matches!(self.lobby(), Some(Lobby::Ready(_)))
    }

    /// Says `notice` on the open settings page, if one is open.
    pub(crate) fn raise_on_page(&mut self, notice: Notice) {
        if let Status::Signed(signed) = &mut self.status
            && let Some(settings) = opened::<ServerSettings>(&mut signed.page)
        {
            notice::raise(&mut settings.notices, notice);
        }
    }

    /// Whether the settings are open on `page`.
    pub(crate) fn showing(&self, page: Page) -> bool {
        self.settings().is_some_and(|open| open.page == page)
    }

    /// The server's settings, while they are open.
    pub fn settings(&self) -> Option<&ServerSettings> {
        match &self.status {
            Status::Signed(signed) => signed.page.as_ref().and_then(OpenPage::settings),
            Status::Door(_) | Status::Reconnecting(_) => None,
        }
    }

    /// The URL a made or regenerated webhook shows, while it is shown: what Copy puts on the clipboard.
    pub(crate) fn webhook_url(&self) -> Option<&Secret> {
        self.settings()?.webhooks()?.url()
    }

    /// The channels a webhook may be made in: those listed, which this account sees, of a kind that holds webhooks;
    /// none until they are listed.
    pub fn webhook_channels(&self) -> Option<Vec<crate::webhooks::Postable<'_>>> {
        let Lobby::Ready(listed) = self.lobby()? else { return None };
        let holds = |kind: proto::rules::Kind| kind.holds(proto::rules::Holds::Webhooks);
        let postable = |id, name| crate::webhooks::Postable { id, name };
        let text = listed.text.iter().filter(|_| holds(proto::rules::Kind::Text));
        let voice = listed.voice.iter().filter(|_| holds(proto::rules::Kind::Voice));
        let text = text.map(|channel| postable(&channel.id, channel.name.as_str()));
        Some(text.chain(voice.map(|channel| postable(&channel.id, channel.name.as_str()))).collect())
    }

    /// What turns "May mention everyone" over in the form `making`; none where this account may not let a webhook
    /// mention everyone, so the switch cannot be turned.
    pub fn webhook_everyone_turn(&self, making: &crate::webhooks::Making) -> Option<crate::webhooks::Message> {
        self.may_mention_everyone().then(|| crate::webhooks::Message::MentionEveryone(!making.mention_everyone()))
    }

    fn may_mention_everyone(&self) -> bool {
        self.grants().holds(Grant::MentionEveryone)
    }

    /// The Webhooks page, over the text channels listed and what this account may let a webhook do.
    fn on_webhooks(&mut self, message: crate::webhooks::Message) -> Vec<Ask> {
        let Some(listed) = self.webhook_channels() else { return Vec::new() };
        let channels: Vec<ChannelId> = listed.iter().map(|channel| channel.id.clone()).collect();
        let context =
            crate::webhooks::Context { channels: &channels, may_mention_everyone: self.may_mention_everyone() };
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        let open = opened::<Webhooks>(&mut signed.page);
        open.and_then(|open| open.update(message, &context)).map(webhook_ask).into_iter().collect()
    }

    /// The webhook's URL went onto the clipboard, or could not.
    pub(crate) fn webhook_copied(&mut self, done: bool) {
        let Status::Signed(signed) = &mut self.status else { return };
        if let Some(open) = opened::<Webhooks>(&mut signed.page) {
            open.copied(done);
        }
    }

    /// The server's debug report as About holds it, while About is open.
    pub fn server_report(&self) -> Option<&crate::ServerReport> {
        self.settings().and_then(ServerSettings::about).map(crate::ServerAbout::report)
    }

    /// The name others see of this account, as the server last said it, else its username.
    pub fn shown(&self) -> Option<&str> {
        self.standing.as_ref().and_then(|standing| standing.shown.as_deref()).or(self.identity.as_deref())
    }

    /// The account page's picture while its colour is being picked: the pick, or the id's swatch where it stands on none.
    pub fn profile_preview(&self) -> Option<design::PersonColour> {
        let picker = self.account()?.colour()?;
        Some(colour_of(self.me()?, (!picker.is_fallback()).then(|| picker.picked())))
    }

    /// The colour others see this account in: the one it picked, else its id's swatch; none until it signs in.
    pub fn own_colour(&self) -> Option<design::PersonColour> {
        self.standing.as_ref().map(|standing| colour_of(&standing.user_id, standing.colour))
    }

    /// The person's own account page, while it is open.
    pub fn account(&self) -> Option<&Account> {
        match &self.status {
            Status::Signed(signed) => signed.page.as_ref().and_then(OpenPage::account),
            Status::Door(_) | Status::Reconnecting(_) => None,
        }
    }

    /// The pages: only a signed-in server opens them.
    pub(super) fn on_page(&mut self, message: PageMessage, told: &mut Option<Told>, at: Moment) -> Vec<Ask> {
        let grants = self.grants();
        let place = self.role_place();
        let message = match message {
            PageMessage::Channels(message) => return self.on_channels(message),
            PageMessage::Webhooks(message) => return self.on_webhooks(message),
            PageMessage::Emoji(message) => return self.on_emoji_page(message),
            message => message,
        };
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        match message {
            PageMessage::DismissPageNotice(subject) => {
                if let Some(settings) = opened::<ServerSettings>(&mut signed.page) {
                    notice::put_away(&mut settings.notices, subject);
                }
                Vec::new()
            }
            PageMessage::Settings(page) if Page::offered(grants).contains(&page) => {
                let mut todo = close_page(signed, &self.roles);
                let mut open = ServerSettings::new(page, &self.server, &self.served);
                if let Some(roles) = open.roles.as_mut() {
                    roles.follow(&self.roles, place);
                }
                if page == Page::Channels
                    && let Lobby::Ready(listed) = &signed.lobby
                {
                    let channels = crate::channel_settings::ChannelSettings::new(listed);
                    let lists = grants.holds(Grant::ManageRoles);
                    if let Some(chosen) = channels.chosen().filter(|_| lists) {
                        signed.overwrites.entry(chosen.clone()).or_default();
                        todo.push(ops::list_overwrites(chosen.clone()));
                    }
                    open.channels = Some(channels);
                }
                signed.page = Some(OpenPage::Settings(Box::new(open)));
                todo.extend(match page {
                    // The server's name and colour came with its greeting; its hosting facts are asked for.
                    Page::Overview => Some(ops::get_server_hosting()),
                    // The channels are the list the pane shows, which the server's events keep current.
                    Page::Channels | Page::About => None,
                    Page::Webhooks => Some(ops::list_webhooks()),
                    // The emoji are the place's list, which the server's events keep current.
                    Page::Emoji => None,
                    // The roles are the place's own, read again for the page.
                    Page::Roles => Some(ops::list_roles()),
                    Page::Invites => Some(ops::list_invites()),
                    Page::Members => Some(Ask::ListMembers),
                });
                todo
            }
            PageMessage::CloseSettings => match signed.page {
                Some(OpenPage::Settings(_)) => close_page(signed, &self.roles),
                Some(OpenPage::Account(_)) | None => Vec::new(),
            },
            PageMessage::Picture(message) => {
                if let Some(open) = signed.page.as_mut().and_then(OpenPage::account_mut) {
                    return match open.update_picture(message) {
                        Some(picture_row::Ask::Pick) => vec![Ask::PickPicture],
                        Some(picture_row::Ask::Upload(bytes)) => {
                            vec![Ask::UploadPicture { bytes, kind: proto::files::FileKind::ProfilePicture }]
                        }
                        Some(picture_row::Ask::Hold(picture)) => vec![ops::set_profile_picture(picture)],
                        None => Vec::new(),
                    };
                }
                let asked = opened::<Overview>(&mut signed.page)
                    .and_then(|open| open.update(overview::Message::Picture(message)));
                asked.map(asked_of).into_iter().collect()
            }
            PageMessage::ProfilePictureSet(Err(failed)) => {
                let me = self.standing.as_ref().map(|standing| &standing.user_id);
                let held = me.and_then(|me| self.directory.person(me)).and_then(|person| person.picture);
                if let Some(open) = signed.page.as_mut().and_then(OpenPage::account_mut) {
                    open.picture_not_set(held, failed);
                }
                Vec::new()
            }
            PageMessage::ProfilePictureSet(Ok(())) => Vec::new(),
            PageMessage::ServerPictureSet(Err(failed)) => {
                if let Some(open) = opened::<Overview>(&mut signed.page) {
                    open.picture.not_set(self.server.picture, failed);
                }
                Vec::new()
            }
            PageMessage::ServerPictureSet(Ok(())) => Vec::new(),
            PageMessage::Overview(message) => {
                let asked = opened::<Overview>(&mut signed.page).and_then(|open| open.update(message));
                asked.map(asked_of).into_iter().collect()
            }
            PageMessage::WebhooksListed(answer) => {
                if let Some(open) = opened::<Webhooks>(&mut signed.page) {
                    open.listed(answer);
                }
                Vec::new()
            }
            PageMessage::WebhookMade(answer) => {
                let host = self.server.host().to_owned();
                let open = opened::<Webhooks>(&mut signed.page);
                open.and_then(|open| open.made(answer, &host)).map(webhook_ask).into_iter().collect()
            }
            PageMessage::WebhookDeleted(answer) => {
                let open = opened::<Webhooks>(&mut signed.page);
                open.and_then(|open| open.deleted(answer)).map(webhook_ask).into_iter().collect()
            }
            PageMessage::SwitchSet { switch, on, answer } => {
                if let Some(open) = opened::<Overview>(&mut signed.page) {
                    open.switched(switch, on, &answer);
                }
                Vec::new()
            }
            PageMessage::ServerHostingListed(answer) => {
                if let Some(open) = opened::<Overview>(&mut signed.page) {
                    open.listed(&answer);
                }
                Vec::new()
            }
            PageMessage::ServerHostingSet(answer) => {
                let open = opened::<Overview>(&mut signed.page);
                if let Some(open) = open {
                    open.hosting.answered(&answer);
                }
                Vec::new()
            }
            PageMessage::About(message) => {
                let asked = opened::<ServerAbout>(&mut signed.page).and_then(|open| open.update(message));
                asked
                    .map(|asked| match asked {
                        server_about::Ask::ServerReport => Ask::GetServerReport,
                        server_about::Ask::SaveReport(download) => Ask::SaveReport(download),
                    })
                    .into_iter()
                    .collect()
            }
            PageMessage::ServerNameSet(answer) => {
                if let Some(open) = opened::<Overview>(&mut signed.page) {
                    open.name.answered(&answer);
                }
                Vec::new()
            }
            PageMessage::ServerColourSet(answer) => {
                if let Some(open) = opened::<Overview>(&mut signed.page) {
                    open.colour.answered(&answer);
                }
                *told = colour_refused(&answer);
                Vec::new()
            }
            PageMessage::ServerReported(answer) => {
                if let Some(open) = opened::<ServerAbout>(&mut signed.page) {
                    open.report.replied(answer, at.wall);
                }
                Vec::new()
            }
            PageMessage::Name(message) => {
                let asked =
                    signed.page.as_mut().and_then(OpenPage::account_mut).and_then(|open| open.update_name(message));
                asked.map(ops::set_display_name).into_iter().collect()
            }
            PageMessage::ProfileColour(message) => {
                let asked =
                    signed.page.as_mut().and_then(OpenPage::account_mut).and_then(|open| open.update_colour(message));
                asked.map(ops::set_profile_colour).into_iter().collect()
            }
            PageMessage::ProfileColourSet(answer) => {
                if let Some(open) = signed.page.as_mut().and_then(OpenPage::account_mut) {
                    open.colour_answered(&answer);
                }
                *told = colour_refused(&answer);
                Vec::new()
            }
            PageMessage::NameSet(answer) => {
                if let Some(open) = signed.page.as_mut().and_then(OpenPage::account_mut) {
                    open.name_answered(&answer);
                }
                Vec::new()
            }
            PageMessage::OpenAccount => {
                let todo = close_page(signed, &self.roles);
                signed.page = Some(OpenPage::Account(Box::new(match self.standing.as_ref() {
                    Some(standing) => {
                        let colour = Picker::new(standing.colour, unpicked(&standing.user_id));
                        let held = self.directory.person(&standing.user_id).and_then(|person| person.picture);
                        Account::new(&standing.username, standing.shown.as_deref(), Some(colour)).holding(held)
                    }
                    None => Account::unknown(),
                })));
                todo
            }
            PageMessage::CloseAccount => match signed.page {
                Some(OpenPage::Account(_)) => close_page(signed, &self.roles),
                Some(OpenPage::Settings(_)) | None => Vec::new(),
            },
            PageMessage::Account(message) => {
                let asked = signed.page.as_mut().and_then(OpenPage::account_mut).and_then(|open| open.update(message));
                asked.map(|(current, new)| ops::change_password(&current, &new)).into_iter().collect()
            }
            PageMessage::PasswordChanged(answer) => {
                if signed.page.as_mut().and_then(OpenPage::account_mut).is_some_and(|open| open.answered(&answer)) {
                    *told = Some(Told::PasswordChanged);
                }
                Vec::new()
            }
            PageMessage::Invites(listed) => {
                if let Some(open) = opened::<ServerSettings>(&mut signed.page) {
                    open.invites = match listed {
                        Ok(invites) => Loaded::Ready(invites.into_iter().filter_map(ListedInvite::read).collect()),
                        Err(failed) => Loaded::Failed(failed),
                    };
                }
                Vec::new()
            }
            PageMessage::Members(message) => {
                if let Some(open) = opened::<ServerSettings>(&mut signed.page) {
                    open.finding.update(message);
                }
                Vec::new()
            }
            PageMessage::Roles(message) => {
                let asked = opened::<Roles>(&mut signed.page)
                    .map(|open| open.update(message, &mut self.roles, place, grants))
                    .unwrap_or_default();
                asked.into_iter().map(Ask::from).collect()
            }
            PageMessage::OverwriteSet { channel, answer } => {
                let said = opened::<ChannelSettings>(&mut signed.page)
                    .and_then(|open| open.answered(&channel, &answer, &mut signed.overwrites));
                match said {
                    Some(said) => {
                        if let Some(settings) = opened::<ServerSettings>(&mut signed.page) {
                            notice::raise(
                                &mut settings.notices,
                                Notice::new(Subject::Channels, design::Status::Warning, said),
                            );
                        }
                        vec![ops::list_overwrites(channel)]
                    }
                    // A change asked from the create dialog that did not go through: said where the dialog was.
                    None if answer.is_err()
                        && signed
                            .page
                            .as_ref()
                            .and_then(OpenPage::settings)
                            .is_none_or(|open| open.channels.is_none()) =>
                    {
                        let refused = answer.as_ref().err().map(failure::said);
                        *told = refused.map(|said| Told::Toast(Telling::of(design::Status::Danger, said)));
                        Vec::new()
                    }
                    None => Vec::new(),
                }
            }
            PageMessage::RoleNamed { role, answer } => {
                if let Some(open) = opened::<Roles>(&mut signed.page) {
                    open.named(&role, &answer);
                }
                Vec::new()
            }
            PageMessage::MembersListed(listed) => {
                let me = self.standing.as_ref().map(|standing| &standing.user_id);
                if let Some(open) = opened::<ServerSettings>(&mut signed.page) {
                    open.members = match listed {
                        Ok(entries) => Loaded::Ready(members::listed(entries, me)),
                        Err(failed) => Loaded::Failed(failed),
                    };
                }
                Vec::new()
            }
            // A page the account may not open stays shut.
            PageMessage::Settings(_) => Vec::new(),
            // The answers the window reads before handing them on.
            PageMessage::Invited(_)
            | PageMessage::InviteDeleted(_)
            | PageMessage::RoleSet { .. }
            | PageMessage::Kicked(_)
            | PageMessage::BanSet(_)
            | PageMessage::RoleCreated(_)
            | PageMessage::RoleChanged { .. }
            | PageMessage::RoleDeleted(_)
            | PageMessage::PasswordReset(_)
            | PageMessage::Transferred(_)
            | PageMessage::ChannelCreated(_)
            | PageMessage::ChannelRenamed(_)
            | PageMessage::ChannelDeleted(_)
            | PageMessage::CategoryCreated(_)
            | PageMessage::CategoryRenamed(_)
            | PageMessage::CategoryDeleted(_) => Vec::new(),
            // Read before the place was borrowed.
            PageMessage::Channels(_) | PageMessage::Webhooks(_) | PageMessage::Emoji(_) => Vec::new(),
        }
    }

    /// The channels page.
    fn on_channels(&mut self, message: crate::channel_settings::Message) -> Vec<Ask> {
        let held = self.grants();
        let roles = held.holds(Grant::ManageRoles) && held.holds(Grant::ManageChannel);
        let chosen = self.channel_settings().and_then(|open| open.chosen().cloned());
        let held_there = chosen.as_ref().map_or(Grants::NONE, |chosen| self.grants_in(chosen));
        let me = self.standing.as_ref().map(|standing| standing.user_id.clone());
        let mine: Vec<RoleId> =
            me.as_ref().and_then(|me| self.directory.person(me)).map(|me| me.roles.clone()).unwrap_or_default();
        let asker =
            crate::channel_settings::Asker { me: me.as_ref(), roles: &mine, place: self.role_place(), base: held };
        let context = crate::channel_settings::Context { roles: &self.roles, directory: &self.directory, asker };
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        let Lobby::Ready(listed) = &signed.lobby else { return Vec::new() };
        let Some(open) = opened::<ChannelSettings>(&mut signed.page) else {
            return Vec::new();
        };
        let asked = open.update(message, &context, listed, &mut signed.overwrites, held_there, roles);
        let manages = held.holds(Grant::ManageChannel);
        let mut todo = Vec::new();
        for asked in asked {
            match asked {
                crate::channel_settings::Asked::Server(asked) => todo.push(asked),
                crate::channel_settings::Asked::Move(moved) => {
                    todo.extend(super::arrange::moved(signed, manages, moved))
                }
            }
        }
        todo
    }

    /// The channels page, while it is open.
    pub fn channel_settings(&self) -> Option<&crate::channel_settings::ChannelSettings> {
        self.settings().and_then(ServerSettings::channels)
    }

    /// Who asks, as the channels page and the create dialog read it.
    pub(crate) fn asker(&self) -> (Option<&MemberId>, &[RoleId]) {
        let me = self.me();
        let roles = me.and_then(|me| self.directory.person(me)).map_or(&[][..], |me| me.roles.as_slice());
        (me, roles)
    }

    /// The chosen channel's editor on the channels page, as drawn.
    pub fn channel_editor(&self) -> Option<crate::channel_settings::EditorView<'_>> {
        let open = self.channel_settings()?;
        let Status::Signed(signed) = &self.status else { return None };
        let Lobby::Ready(listed) = &signed.lobby else { return None };
        let held = self.grants();
        let roles = held.holds(Grant::ManageRoles) && held.holds(Grant::ManageChannel);
        let held_there = open.chosen().map_or(Grants::NONE, |chosen| self.grants_in(chosen));
        open.editor(&self.context(), listed, &signed.overwrites, held_there, roles)
    }

    /// What who asks reads for the channels page and the create dialog.
    fn context(&self) -> crate::channel_settings::Context<'_> {
        let (me, mine) = self.asker();
        let asker = crate::channel_settings::Asker { me, roles: mine, place: self.role_place(), base: self.grants() };
        crate::channel_settings::Context { roles: &self.roles, directory: &self.directory, asker }
    }

    /// An access of the create dialog `making`, as drawn here: none where it offers none.
    pub fn making_access(
        &self,
        making: &crate::channels::Making,
        access: Access,
    ) -> Option<crate::channel_settings::AccessView> {
        making.access_view(access, &self.context())
    }

    /// What the start of the text channel `id` says under its welcome while it is private; none for one everyone
    /// sees, whose start says only what it is.
    pub fn private_line(&self, id: &ChannelId) -> Option<Sentence> {
        let Status::Signed(signed) = &self.status else { return None };
        let Lobby::Ready(listed) = &signed.lobby else { return None };
        let channel = listed.text.iter().find(|channel| channel.id == *id)?;
        (listed.sight(id) == Sight::Private).then(|| Sentence::PrivateChannelStart(channel.name.clone()))
    }

    /// Whether this account may list a channel's overwrites: it manages the channels and the roles.
    pub(super) fn lists_overwrites(&self) -> bool {
        let held = self.grants();
        held.holds(Grant::ManageChannel) && held.holds(Grant::ManageRoles)
    }

    /// The server's answer to a channel's name saved from the channels page.
    pub(crate) fn channel_named(&mut self, answer: &Result<(), LinkError>) {
        let Status::Signed(signed) = &mut self.status else { return };
        if let Some(open) = opened::<ChannelSettings>(&mut signed.page) {
            open.named(answer);
        }
    }

    /// The place's roles in their order, as last read: one list every page and popover reads.
    pub fn role_list(&self) -> &RoleList {
        &self.roles
    }

    /// The roles `user_id` holds that this place knows, highest first, the default one only where they hold no other:
    /// their popover's.
    pub fn roles_of(&self, user_id: &MemberId) -> Vec<&crate::roles::ListedRole> {
        self.directory.person(user_id).map_or_else(Vec::new, |person| self.roles.shown(&person.roles))
    }

    /// The roles a member holds changed: their row on the members page follows at once.
    pub(super) fn held_roles(&mut self, user_id: &MemberId, roles: &[RoleId]) {
        let Status::Signed(signed) = &mut self.status else { return };
        if let Some(Loaded::Ready(members)) = opened::<Loaded<Vec<ListedMember>>>(&mut signed.page) {
            for member in members.iter_mut().filter(|member| &member.user_id == user_id) {
                member.held_now(roles);
            }
        }
    }
}

impl Place {
    /// The roles page, while it is open, over the place's roles.
    pub fn roles(&self) -> Option<crate::roles::RolesView<'_>> {
        let page = self.settings().and_then(ServerSettings::roles)?;
        Some(crate::roles::RolesView::new(page, &self.roles))
    }

    /// The roles a made role was answered with: listed, the new one chosen on the roles page; what that saves of the
    /// one chosen before is asked.
    pub(crate) fn roles_created(&mut self, listed: Vec<RoleEntry>) -> Vec<Ask> {
        let place = self.role_place();
        let Status::Signed(signed) = &mut self.status else {
            self.roles.read(Ok(listed));
            return Vec::new();
        };
        match opened::<Roles>(&mut signed.page) {
            Some(open) => open.created(&mut self.roles, listed, place).into_iter().map(Ask::from).collect(),
            None => {
                self.roles.read(Ok(listed));
                Vec::new()
            }
        }
    }

    /// A role deleted leaves the list at once.
    pub(crate) fn role_deleted(&mut self, role: &RoleId) {
        self.roles.deleted(role);
        let Status::Signed(signed) = &mut self.status else { return };
        if let Some(open) = opened::<Roles>(&mut signed.page) {
            open.deleted(&self.roles, role);
        }
    }

    /// The server took a change shown at once.
    pub(crate) fn role_kept(&mut self) {
        self.roles.kept();
    }

    /// The server refused a change shown at once: a move is put back.
    pub(crate) fn role_refused(&mut self) {
        self.roles.refused();
    }

    /// The roles as the server listed them: the place's list, which the roles page follows.
    pub(super) fn roles_read(&mut self, listed: Result<Vec<RoleEntry>, LinkError>) {
        self.roles.read(listed);
        let place = self.role_place();
        let Status::Signed(signed) = &mut self.status else { return };
        if let Some(open) = opened::<Roles>(&mut signed.page) {
            open.follow(&self.roles, place);
        }
    }

    /// A role given or taken from a member's menu, shown at once until the list is read again.
    pub(crate) fn hold(&mut self, user_id: &MemberId, role: &RoleId, held: bool) {
        let Status::Signed(signed) = &mut self.status else { return };
        if let Some(Loaded::Ready(members)) = opened::<Loaded<Vec<ListedMember>>>(&mut signed.page) {
            for member in members.iter_mut().filter(|member| &member.user_id == user_id) {
                member.hold(role, held);
            }
        }
    }

    /// A member's menu closed, as a dialog opens from it.
    pub(crate) fn close_member_menu(&mut self) {
        if let Status::Signed(signed) = &mut self.status
            && let Some(open) = opened::<ServerSettings>(&mut signed.page)
        {
            open.finding.close_menu();
        }
    }

    /// Every page open over this server's channels or call closed, asking what each asks as it closes.
    pub(crate) fn close_pages(&mut self) -> Vec<Ask> {
        let Status::Signed(signed) = &mut self.status else { return Vec::new() };
        close_page(signed, &self.roles)
    }
}

/// The page open over the channels closed. One that saves what it holds on leaving asks it now: the account's name
/// and the colour its open picker holds, the server's name and colour on Overview, and the chosen role's on Roles,
/// typed or picked and not yet sent.
pub(crate) fn close_page(signed: &mut SignedIn, roles: &RoleList) -> Vec<Ask> {
    match signed.page.take() {
        Some(OpenPage::Settings(mut open)) => {
            let mut todo: Vec<Ask> = Vec::new();
            if let Some(open) = open.overview.as_mut() {
                todo.extend(open.left().into_iter().map(asked_of));
            }
            if let Some(open) = open.roles.as_mut() {
                todo.extend(open.left(roles).into_iter().map(Ask::from));
            }
            if let Some(open) = open.channels.as_mut() {
                todo.extend(open.left().into_iter().filter_map(|asked| match asked {
                    crate::channel_settings::Asked::Server(asked) => Some(asked),
                    crate::channel_settings::Asked::Move(_) => None,
                }));
            }
            todo
        }
        Some(OpenPage::Account(mut open)) => {
            let name = open.ask_name().map(ops::set_display_name);
            let colour = open.left_colour().map(ops::set_profile_colour);
            name.into_iter().chain(colour).collect()
        }
        None => Vec::new(),
    }
}

/// What a refused colour says: it was not saved, and the picker is back on the one that was.
fn colour_refused(answer: &Result<(), LinkError>) -> Option<Told> {
    answer.is_err().then(|| Told::Toast(Telling::of(design::Status::Danger, Sentence::Phrase(Phrase::ColourRefused))))
}

/// What Overview asks, as the host carries it out.
pub(super) fn asked_of(ask: overview::Ask) -> Ask {
    match ask {
        overview::Ask::PickPicture => Ask::PickPicture,
        overview::Ask::UploadPicture(bytes) => {
            Ask::UploadPicture { bytes, kind: proto::files::FileKind::ServerPicture }
        }
        overview::Ask::Picture(picture) => ops::set_server_picture(picture),
        overview::Ask::Name(name) => ops::set_server_name(name),
        overview::Ask::Colour(colour) => ops::set_server_colour(colour),
        overview::Ask::Switch(switch, on) => ops::set_server_switch(switch, on),
        overview::Ask::Hosting(crate::hosting::Ask::Get) => ops::get_server_hosting(),
        overview::Ask::Hosting(crate::hosting::Ask::Set(hosting)) => ops::set_server_hosting(hosting),
    }
}

/// What Webhooks asks, as the host carries it out.
fn webhook_ask(ask: crate::webhooks::Ask) -> Ask {
    match ask {
        crate::webhooks::Ask::List => ops::list_webhooks(),
        crate::webhooks::Ask::Create { channel, name, mention_everyone } => {
            ops::create_webhook(&channel, name, mention_everyone)
        }
        crate::webhooks::Ask::Regenerate(id) => ops::regenerate_webhook(&id),
        crate::webhooks::Ask::Delete(id) => ops::delete_webhook(&id),
    }
}

#[cfg(test)]
mod tests {
    use crate::harness::{Clock, every_switch_on, holding, tell};
    use crate::place::{self, PageMessage};
    use crate::settings::Page;
    use crate::webhooks::{Message as Said, Stage};
    use proto::rules::Grant;
    use std::time::Instant;

    #[test]
    fn the_webhook_form_offers_the_text_channels_listed_and_the_everyone_switch_to_whoever_may_turn_it() {
        let clock = Clock(Instant::now());
        for (held, turns) in
            [(&[Grant::ManageWebhooks][..], false), (&[Grant::ManageWebhooks, Grant::MentionEveryone], true)]
        {
            let (mut window, owls) = holding(&clock, held.iter().copied().collect());
            every_switch_on(&mut window, &clock, owls);
            tell(&mut window, &clock, owls, place::Message::Page(PageMessage::Settings(Page::Webhooks)));
            tell(&mut window, &clock, owls, place::Message::Page(PageMessage::Webhooks(Said::Make)));
            let place = &window.places()[0];
            let names: Vec<&str> =
                place.webhook_channels().unwrap_or_default().iter().map(|channel| channel.name).collect();
            assert_eq!(names, ["general"]);
            let Some(Stage::Making(making)) =
                place.settings().and_then(|open| open.webhooks()).map(|open| open.stage())
            else {
                panic!("the form is open")
            };
            let turn = place.webhook_everyone_turn(making);
            assert_eq!(turn.is_some(), turns, "{held:?}");
            assert!(turn.is_none_or(|said| matches!(said, Said::MentionEveryone(true))), "turned on from off");
        }
    }
}
