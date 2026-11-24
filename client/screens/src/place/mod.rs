//! One server in the window: getting in to it, its voice channels, a call on it, and its connection made again by
//! itself when it drops. A [`crate::Window`] holds one per server; values in, effects out.

use crate::account::{self, Account};
use crate::call::room::Event as RoomEvent;
use crate::call::sources::{Chooser, SourceKey};
use crate::call::voice::{Connection, Tile};
use crate::chime::Chime;
use crate::closed::Then;
use crate::colour::Picker;
use crate::door::{self, Arrival, Door, Opening, Server, Showing, Step, Work};
use crate::invite::ListedInvite;
use crate::loaded::Loaded;
use crate::members;
use crate::notice::{self, Notice, Subject};
use crate::overview;
use crate::people::{colour_of, unpicked};
use crate::picture_row;
use crate::reconnect::{BACKOFF, retryable};
use crate::roles::{self, RoleList};
use crate::server_about;
use crate::settings::{Page, ServerSettings};
use crate::sign_in::Standing;
use crate::tell::{Device, Telling};
use crate::{
    channel_settings::ChannelSettings, emoji_settings::EmojiSettings, members::ListedMember, overview::Overview,
    roles::Roles, server_about::ServerAbout, webhooks::Webhooks,
};
use choices::CallView;
use choices::CameraAsk;
use choices::ShareQuality;
use choices::State;
use choices::{Audio, Volume, Volumes};
use net::Jitter;
use proto::chatkord_v1::{
    CategorySummary, ChannelKind, ChannelSummary, ChannelsSummary, Event, InviteSummary, MemberEntry, MemberSummary,
    RoleEntry, StandingChanged, event,
};
use proto::files::FileId;
use proto::rules::{Grant, Grants, TYPING_LAPSE};
use proto::video::Video;
use std::time::{Duration, Instant};
use terms::{
    CategoryId, ChannelId, ConversationId, Ended, InviteId, Invited, LinkError, MemberId, MessageId, RoleId, Secret,
    ServerIdentity, ServerUrl, TimelineId,
};
use words::Phrase;
use words::Sentence;
use words::failure;

mod arrange;
pub(crate) mod ops;
pub use ops::read_message;
mod content;
mod directory;
mod effects;
mod emoji;
mod lobby;
mod notify;
mod pages;
mod painted;
mod roster;
mod server;
mod session;
mod shown;
mod talk;
mod wire;

pub use crate::call::{
    Call, CallMessage, CallReturn, CardTool, EndCard, FullScreen, Out, OutSound, Over, PinnedScreen, PinnedShare,
    RateMessage, Received, Sending, Strip, WayBack,
};
pub use crate::discussion::{
    AttachState, Attaching, CHOICES_SHOWN, Caught, Choice, Choosing, Compose, ComposerGone, Composing, Discussion,
    DiscussionMessage, Early, EditStage, Editing, FoundFile, FoundIn, History, LinkPreviewShown, MARK_EVERY, NoPicker,
    OVERSCAN, PREVIEW_PAUSE, Pane, PickFor, PickerMessage, PickerSection, Picking, Pins, PlaceUnread, PreviewMade,
    PreviewPicture, Quoted, Readings, Replying, SEARCH_PAUSE, SaveAnswer, Search, SearchMessage, SearchStage,
    TextChannel, Title, Typers, Unread, Unsent, Upload, Uploaded, Viewing,
};
use crate::discussion::{Held, leave};
pub use arrange::*;
pub(crate) use content::Content;
pub use content::Marked;
pub use directory::*;
pub use effects::*;
pub use emoji::*;
pub use lobby::*;
pub use notify::*;
pub use pages::*;
pub use roster::*;
pub use server::*;
pub use session::*;
pub use talk::*;
pub use wire::*;

/// A server's place in the window, issued by the window and never reused, so a late report for a server that was
/// taken away reaches nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlaceId(pub(crate) u64);

/// Signed in: the channels, a call when one is open, and the notes the content area opens with.
#[derive(Debug)]
pub struct SignedIn {
    /// The channels.
    pub lobby: Lobby,
    /// Who is in each voice room, as the server last said, by the channel's id: told apart from the list, so a list
    /// read again keeps it.
    pub rooms: std::collections::HashMap<ChannelId, Vec<Occupant>>,
    /// The list's changes told lately, applied again over a list read while they were told.
    recent: Recent,
    /// What the content area shows under the page over it; a call goes on in the pane meanwhile.
    pub(crate) content: Content,
    /// The text channel the content area showed last, which it goes back to once the call it shows is put away.
    last_text: Option<ChannelId>,
    /// Conversations and followed threads, and what this account has read in them.
    pub talk: Talk,
    /// The call's voice channel's messages, in the column beside the call while it is open.
    pub call_chat: Option<Discussion>,
    /// The server's Notification settings dialog, while open.
    pub notify_dialog: Option<NotifyDialog>,
    /// The call open here; one that ended by itself stays, to say so, until a person puts it away.
    pub call: Option<Call>,
    /// The note a first arrival opens with.
    pub note: Option<Arrival>,
    /// The call a dropped connection left, offered to rejoin.
    pub back: Option<Channel>,
    /// The page open over the channels, in place of the side pane and the content (a call goes on under it).
    page: Option<OpenPage>,
    /// A move of the list shown at once, while the server's answer is awaited.
    moving: Option<Moving>,
    /// Each channel's overwrites, as last listed for the channels page or a private channel's start.
    overwrites: std::collections::HashMap<ChannelId, Loaded<Vec<crate::channel_settings::Mark>>>,
    /// The figures of the calls that ended here since the window last took them.
    pub(crate) finished: Vec<crate::call::health::CallFigures>,
    /// The token the session signed in with, offered first when its connection drops; it goes with the session.
    token: Option<Secret>,
}

impl SignedIn {
    /// The call put away, its figures kept for the window to take; the content area that showed it goes back to the
    /// text channel it showed last, its days read in `zone`, else to the channels' panel. A page open over it stays.
    pub(crate) fn end_call(
        &mut self,
        held: &mut std::collections::HashMap<Compose, Held>,
        zone: chat::TimeZone,
    ) -> Vec<Ask> {
        if self.call.is_none() {
            return Vec::new();
        }
        self.hold_call(None);
        let Some(channel) = self.last_text.clone().filter(|_| self.content.is_call()) else { return Vec::new() };
        let Some((open, first)) = crate::discussion::reading(self, held, &channel, zone) else { return Vec::new() };
        self.put(Content::seated(open), held);
        vec![first]
    }

    /// `call` held here in place of the call held before, whose figures are kept for the window to take.
    fn hold_call(&mut self, call: Option<Call>) {
        if let Some(before) = std::mem::replace(&mut self.call, call) {
            self.finished.push(before.figures());
        }
    }

    /// `content` shown in the content area in place of what was there, which leaves with where it was scrolled; the
    /// page over it closes, asking what it asks as it is left.
    pub(crate) fn show(
        &mut self,
        content: Content,
        held: &mut std::collections::HashMap<Compose, Held>,
        roles: &RoleList,
    ) -> Vec<Ask> {
        let todo = pages::close_page(self, roles);
        self.put(content, held);
        todo
    }

    /// `content` shown in the content area in place of what was there, under any page that is open.
    fn put(&mut self, content: Content, held: &mut std::collections::HashMap<Compose, Held>) {
        if let Some(Owner::Channel(channel)) = content.discussion().map(|open| &open.owner) {
            self.last_text = Some(channel.clone());
        }
        for left in std::mem::replace(&mut self.content, content).into_discussions() {
            leave(held, left);
        }
    }

    /// The discussions open, in every place one can be: the content area's, the thread's beside it, the call's.
    pub(super) fn open(&self) -> impl Iterator<Item = &Discussion> {
        self.content.discussions().chain(self.call_chat.iter())
    }

    /// The same, to change.
    pub(super) fn open_mut(&mut self) -> impl Iterator<Item = &mut Discussion> {
        self.content.discussions_mut().chain(self.call_chat.iter_mut())
    }

    fn new(note: Option<Arrival>, back: Option<Channel>, token: Option<Secret>) -> SignedIn {
        SignedIn {
            token,
            lobby: Lobby::Loading,
            rooms: std::collections::HashMap::new(),
            recent: Recent::default(),
            content: Content::default(),
            last_text: None,
            talk: Talk::default(),
            call_chat: None,
            notify_dialog: None,
            call: None,
            note,
            back,
            page: None,
            moving: None,
            overwrites: std::collections::HashMap::new(),
            finished: Vec::new(),
        }
    }
}

/// Where a server's connection and session are.
#[derive(Debug)]
pub enum Status {
    /// Signing in by hand: the account's session is over, or it never was, or its password is temporary.
    Door(Box<Door>),
    /// Making the connection again by itself, at launch or after a drop.
    Reconnecting(Box<Reconnecting>),
    /// Signed in: the channels, a call when one is open, and the pages open over them.
    Signed(Box<SignedIn>),
}

/// What the content area shows for a server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    /// Making the connection, with nothing yet to show.
    Connecting,
    /// The channels as they were, under the notice that the connection is being made again.
    Lost,
    /// A try came back unanswered; the next waits.
    NotAnswering {
        /// When the next try starts.
        due: Instant,
    },
    /// Signed out: the sign-in asks for the password, saying why.
    SignedOut,
    /// The account is gone: only taking the server off this device is left.
    Deleted,
    /// The server presented another key than the one this device trusts for it: trusting the new one, or taking the
    /// server off this device, is the person's to choose; the session stays.
    KeyChanged,
    /// Signed in on a temporary password that must be replaced first.
    NewPassword,
    /// The channels, none open.
    Channels,
    /// A text channel, a conversation or a thread.
    Discussion,
    /// The list of the threads this account follows.
    Threads,
    /// The list of this account's conversations.
    Conversations,
    /// The call open here.
    Call,
    /// The server's settings, in place of the side pane and the content.
    Settings,
    /// The person's own account page, in the same place.
    Account,
}

/// How the rail marks a server.
#[derive(Clone, Copy, Debug, PartialEq, Eq, report::Public)]
pub enum RailState {
    /// Signed in, or showing the channels while the connection is made again.
    Online,
    /// Making the connection, with nothing yet to show.
    Connecting,
    /// A try came back unanswered; the next waits.
    NotAnswering,
    /// Signed out, or the account is gone.
    SignedOut,
    /// The server presented another key than the one this device trusts for it.
    KeyChanged,
    /// Signed in on a temporary password that must be replaced first.
    NewPassword,
}

/// What a person does at a server, and what the host reports about it.
#[derive(Clone, Debug)]
pub enum Message {
    /// To the connection and the session.
    Session(SessionMessage),
    /// About what the server and its members are.
    Server(ServerMessage),
    /// About the channels.
    Lobby(LobbyMessage),
    /// In the text channel open here.
    Discussion(DiscussionMessage),
    /// In the call's own channel, beside the call.
    CallChat(DiscussionMessage),
    /// In the thread beside the channel.
    Thread(DiscussionMessage),
    /// About conversations and threads.
    Talk(TalkMessage),
    /// About the server's own emoji.
    Emoji(EmojiMessage),
    /// About how much the server and its channels alert this account.
    Notify(NotifyMessage),
    /// About the members every member sees.
    People(PeopleMessage),
    /// From the members column.
    Roster(RosterMessage),
    /// To the call open here.
    Call(CallMessage),
    /// On one of the pages.
    Page(PageMessage),
    /// In the search of the whole server.
    Search(SearchMessage),
}

/// The wrapper each family of messages has, so an answer names only its own.
macro_rules! family {
    ($($family:ident => $variant:ident),+ $(,)?) => {$(
        impl From<$family> for Message {
            fn from(message: $family) -> Self {
                Self::$variant(message)
            }
        }
    )+};
}

family!(
    SessionMessage => Session,
    ServerMessage => Server,
    LobbyMessage => Lobby,
    TalkMessage => Talk,
    EmojiMessage => Emoji,
    NotifyMessage => Notify,
    PeopleMessage => People,
    RosterMessage => Roster,
    PageMessage => Page,
    SearchMessage => Search,
);

/// The time and the randomness an update may use, from the host.
#[derive(Clone, Copy, Debug)]
pub struct Moment {
    /// The host's clock at this update.
    pub now: Instant,
    /// The spread for the next try's wait.
    pub jitter: Jitter,
    /// Drawn for a message this update may send.
    pub nonce: terms::Nonce,
    /// The wall clock at this update, in seconds since the epoch: what a mute lasts until.
    pub wall: i64,
}

/// One server in the window: its sign-in, its connection, its channels and a call, and what it asks of the host.
#[derive(Debug)]
pub struct Place {
    id: PlaceId,
    pub(crate) server: Server,
    identity: Option<String>,
    /// Where the account stands here, as the server last said; none until it signs in.
    pub(crate) standing: Option<Standing>,
    pub(crate) status: Status,
    /// Whether this device has an encoder for a screen, as the window last found.
    pub(crate) encodes: bool,
    /// The camera the camera button turns on, as the person chose it.
    pub(crate) camera_asked: CameraAsk,
    pub(crate) mirror: choices::Mirror,
    pub(crate) chooser: Chooser,
    /// How a share starts: the last share's choice, kept, on this device's encoders.
    pub(crate) share_starts: ShareQuality,
    /// What a call's grid leaves out, as the window keeps it.
    pub(crate) call_view: CallView,
    /// The picture the host last said it has ready to paint.
    painted: Option<FileId>,
    /// How loud this device plays the members here: this server's alone, kept with its address.
    pub(crate) volumes: Volumes,
    /// What each timeline keeps while it is open or not: its draft, its unsent sends and its scroll.
    pub(crate) held: std::collections::HashMap<Compose, Held>,
    /// The members, as every member sees them.
    pub(crate) directory: Directory,
    /// Where the members column is scrolled.
    roster_scroll: RosterScroll,
    /// The server's roles in their order, as last read: one list every page and popover reads, kept across
    /// reconnects.
    pub(crate) roles: RoleList,
    /// The categories folded in the side pane, as this device keeps them for this server.
    folded: std::collections::BTreeSet<CategoryId>,
    /// The server's own emoji, as last read: kept across reconnects.
    pub(crate) emoji: EmojiList,
    /// The pictures this run asked the host for (emoji, thumbnails), and where each is.
    files: std::collections::HashMap<FileId, emoji::Fetched>,
    /// The kind each file this run fetched is, so one the host let go is fetched again as it was once drawn again.
    kinds: std::collections::HashMap<FileId, proto::files::FileKind>,
    /// The account's notification settings, once read.
    pub(crate) notify: Option<Notifying>,
    /// Who is writing in each timeline, as last told, each with when.
    pub(crate) typing: std::collections::HashMap<TimelineId, Vec<(MemberId, Instant)>>,
    /// The window shows this server and has the focus.
    pub(crate) in_view: bool,
    /// The time zone days and times are said in, as the window last said.
    pub(crate) zone: chat::TimeZone,
    /// What the server offers, as its greeting said and a later `ServerChanged` changed it.
    pub(crate) served: Served,
    /// The search of the whole server, which every page of it shows.
    pub(super) search: Search,
    /// Whether the first link of a draft gets a preview, as this device's choice last said.
    pub(crate) previews: choices::Previews,
    /// Whether this device alerts for the account, as the server last said on this connection; none until it does.
    pub(crate) device: Option<DeviceUse>,
    /// Whether this device shows a popup for what alerts.
    pub(crate) popups: choices::Popups,
    /// The account's own status choice here, as the server last told it this session: Online until it does.
    chosen: proto::status::Chosen,
    /// The popup shown for a timeline, by the place of the message it shows: taken away once that is read.
    pub(crate) popped: std::collections::HashMap<TimelineId, u64>,
    /// What this device last told the server of its being idle on this connection.
    activity_told: Option<Idleness>,
    /// What the server takes in files, as its greeting said.
    pub(crate) limits: terms::FileLimits,
    /// The release the server named in its greeting.
    build: Option<terms::ServerBuild>,
    /// The protocol version the server spoke in its greeting.
    protocol: Option<proto::chatkord_v1::ProtoVersion>,
    /// The uploads issued, so the next has a new id.
    pub(crate) uploads: u64,
}

impl Place {
    /// A place at `server` with nothing yet read or chosen, in `status`.
    fn bare(
        id: PlaceId,
        server: Server,
        identity: Option<String>,
        standing: Option<Standing>,
        status: Status,
    ) -> Place {
        Place {
            id,
            server,
            identity,
            standing,
            status,
            encodes: true,
            camera_asked: CameraAsk::default(),
            mirror: choices::Mirror::default(),
            chooser: Chooser::default(),
            share_starts: ShareQuality::default(),
            call_view: CallView::default(),
            painted: None,
            volumes: Volumes::default(),
            held: std::collections::HashMap::new(),
            directory: Directory::default(),
            roster_scroll: RosterScroll::default(),
            roles: RoleList::default(),
            folded: std::collections::BTreeSet::new(),
            emoji: EmojiList::default(),
            files: std::collections::HashMap::new(),
            kinds: std::collections::HashMap::new(),
            notify: None,
            typing: std::collections::HashMap::new(),
            in_view: false,
            zone: chat::TimeZone::UTC,
            served: Served::nothing(),
            search: Search::default(),
            previews: choices::Previews::default(),
            device: None,
            popups: choices::Popups::default(),
            chosen: proto::status::Chosen::ONLINE,
            popped: std::collections::HashMap::new(),
            activity_told: None,
            limits: terms::FileLimits::default(),
            build: None,
            protocol: None,
            uploads: 0,
        }
    }

    /// A server a run before this one knew: an account opens by itself, else the sign-in panel. What it asks first is
    /// returned with it.
    pub(crate) fn remembered(id: PlaceId, server: Server, identity: Option<String>) -> (Place, Vec<Ask>) {
        let shown: Vec<Ask> = server.picture.map(|picture| Ask::LookUpPicture { picture }).into_iter().collect();
        let Some(identity) = identity else {
            let door = Door::opening(Opening::Server(server.clone()));
            return (Place::bare(id, server, None, None, Status::Door(Box::new(door))), shown);
        };
        let (again, begun) = Reconnecting::opening(&server, identity.clone());
        let todo = shown.into_iter().chain(begun).collect();
        let status = Status::Reconnecting(Box::new(again));
        (Place::bare(id, server, Some(identity), None, status), todo)
    }

    /// A server put back after its forget left its token: signed out, saying why.
    pub(crate) fn token_left(id: PlaceId, server: Server, identity: String) -> (Place, Vec<Ask>) {
        let (mut place, shown) = Place::remembered(id, server.clone(), None);
        let why = Sentence::Text(words::Text::TokenLeft);
        place.status = Status::Door(Box::new(Door::signed_out(server, identity, why)));
        (place, shown)
    }

    /// A server just added and signed in to by `door`.
    pub(crate) fn added(id: PlaceId, door: &Door) -> Option<Place> {
        let server = door.server()?.clone();
        door.target()?;
        let mut place = Place::bare(id, server, None, None, Status::Door(Box::default()));
        place.signed_by(door);
        Some(place)
    }

    /// Its place in the window.
    pub fn id(&self) -> PlaceId {
        self.id
    }

    /// The server as it greeted: its address, what it says of itself and what it offers.
    pub fn server(&self) -> &Server {
        &self.server
    }

    /// How loud this device plays each member here.
    pub fn volumes(&self) -> &Volumes {
        &self.volumes
    }

    /// The volumes this device kept for this server, as the host read them at the start.
    pub(crate) fn set_volumes(&mut self, volumes: Volumes) {
        self.volumes = volumes;
    }

    /// The account signed in, or last signed in, here.
    pub fn identity(&self) -> Option<&str> {
        self.identity.as_deref()
    }

    /// Where the account stands here, as the server last said; none until it signs in.
    pub fn standing(&self) -> Option<&Standing> {
        self.standing.as_ref()
    }

    /// This account's own id at the server, once it signed in.
    pub fn me(&self) -> Option<&MemberId> {
        self.standing.as_ref().map(|standing| &standing.user_id)
    }

    /// What the account's roles let it do here: nothing until it signs in.
    pub fn grants(&self) -> Grants {
        self.standing.as_ref().map(|standing| standing.grants).unwrap_or_default()
    }

    /// Where the account stands here in the roles' order: the bottom until it signs in.
    pub fn role_place(&self) -> proto::rules::Place {
        self.standing.as_ref().map_or(proto::rules::Place::BOTTOM, |standing| standing.place)
    }

    /// The connection and the session, each in the state it belongs to.
    pub fn status(&self) -> &Status {
        &self.status
    }

    /// Whether this device is signed in here, which offers the window's settings.
    pub fn signed_in(&self) -> bool {
        matches!(self.status, Status::Signed(_))
    }

    /// Whether a page of this server's own is open over its channels: its settings or the account.
    pub fn page_open(&self) -> bool {
        matches!(self.panel(), Panel::Settings | Panel::Account)
    }

    /// Whether the place shows its side pane: not while this device holds no account there (signed out, the account
    /// gone, a temporary password still to replace), when nothing in it works, a call elsewhere or not (the
    /// maintainer's call, 2026-11-12).
    pub fn shows_pane(&self) -> bool {
        !matches!(self.panel(), Panel::SignedOut | Panel::Deleted | Panel::NewPassword)
    }

    /// What the content area shows now: a page open over the channels before the call under it.
    pub fn panel(&self) -> Panel {
        match &self.status {
            Status::Signed(signed) => match (&signed.page, &signed.content) {
                (Some(OpenPage::Settings(_)), _) => Panel::Settings,
                (Some(OpenPage::Account(_)), _) => Panel::Account,
                (None, Content::Discussion(_)) => Panel::Discussion,
                (None, Content::Roll(Roll::Threads)) => Panel::Threads,
                (None, Content::Roll(Roll::Conversations)) => Panel::Conversations,
                (None, Content::Call) if signed.call.is_some() => Panel::Call,
                (None, Content::Call) => Panel::Channels,
            },
            Status::Reconnecting(again) => match (again.last, again.phase, &again.shown) {
                (session::Last::Unanswered, session::Phase::Waiting { due }, _) => Panel::NotAnswering { due },
                (session::Last::Unanswered, session::Phase::Quiet { began }, _) => Panel::NotAnswering { due: began },
                (session::Last::Untried, _, Some(_)) => Panel::Lost,
                (session::Last::Unanswered, _, _) | (session::Last::Untried, _, None) => Panel::Connecting,
            },
            Status::Door(door) => match (door.step(), door.work()) {
                (Step::NewPassword, _) | (Step::Working, Some(Work::SavingPassword)) => Panel::NewPassword,
                (Step::Opening | Step::Done, _) => Panel::Connecting,
                _ if matches!(door.problem(), Some(door::Problem::KeyChanged { .. })) => Panel::KeyChanged,
                _ if deleted(door) => Panel::Deleted,
                _ => Panel::SignedOut,
            },
        }
    }

    /// The row the side pane marks for what the content area shows, if any: one at most.
    pub fn marked(&self) -> Option<Marked> {
        match &self.status {
            Status::Signed(signed) => signed.content.marked(signed.call.as_ref()),
            Status::Reconnecting(again) => again.discussion.as_ref().and_then(Marked::of),
            Status::Door(_) => None,
        }
    }

    /// How the rail marks the server now.
    pub fn rail(&self) -> RailState {
        match self.panel() {
            Panel::Channels
            | Panel::Discussion
            | Panel::Threads
            | Panel::Conversations
            | Panel::Call
            | Panel::Settings
            | Panel::Account
            | Panel::Lost => RailState::Online,
            Panel::Connecting => RailState::Connecting,
            Panel::NotAnswering { .. } => RailState::NotAnswering,
            Panel::SignedOut | Panel::Deleted => RailState::SignedOut,
            Panel::KeyChanged => RailState::KeyChanged,
            Panel::NewPassword => RailState::NewPassword,
        }
    }

    pub(crate) fn update(&mut self, message: Message, at: Moment) -> (Vec<Ask>, Option<Told>) {
        let mut told = None;
        let todo = match message {
            Message::Session(message) => self.on_session(message, &mut told, at),
            Message::Server(message) => self.on_server(message, at),
            Message::Lobby(message) => self.on_lobby(message, &mut told),
            Message::Discussion(message) => self.on_discussion(Pane::Content, message, &mut told, at),
            Message::CallChat(message) => self.on_discussion(Pane::CallChat, message, &mut told, at),
            Message::Thread(message) => self.on_discussion(Pane::Thread, message, &mut told, at),
            Message::Talk(message) => self.on_talk(message),
            Message::Emoji(message) => self.on_emoji(message),
            Message::Notify(message) => self.on_notify(message, at),
            Message::People(message) => self.on_people(message),
            Message::Roster(message) => self.on_roster(message),
            Message::Call(message) => self.on_call(message, &mut told, at),
            Message::Page(message) => self.on_page(message, &mut told, at),
            Message::Search(message) => self.on_search(message, at),
        };
        if let Status::Signed(signed) = &mut self.status
            && let Some(call) = signed.call.as_mut()
        {
            call.settle();
        }
        (todo, told)
    }

    /// The next time this server's view changes by itself: a read mark that waits, a writer lapsing, or a mute
    /// lapsing, which un-greys its channels; `now` and `wall` (seconds since the epoch) are the same moment.
    pub(crate) fn due(&self, now: Instant, wall: Option<i64>) -> Option<Instant> {
        let marks = match &self.status {
            Status::Signed(signed) => signed.open().filter_map(Discussion::mark_due).min(),
            Status::Reconnecting(again) => again.phase.edge(),
            Status::Door(_) => None,
        };
        let lapses = self.typing.values().flatten().map(|(_, told)| *told + TYPING_LAPSE).min();
        let unmutes = wall.zip(self.notify.as_ref()).and_then(|(wall, notify)| {
            let mutes = std::iter::once(notify.server.mute).chain(notify.channels.values().map(|channel| channel.mute));
            mutes
                .flatten()
                .filter_map(|mute| match mute {
                    proto::notify::Mute::Until(until) => u64::try_from(until.checked_sub(wall)?).ok(),
                    proto::notify::Mute::Forever => None,
                })
                .filter(|seconds| *seconds > 0)
                .min()
                .and_then(|seconds| now.checked_add(Duration::from_secs(seconds)))
        });
        marks.into_iter().chain(lapses).chain(unmutes).chain(self.search_due()).chain(self.preview_due()).min()
    }
}
