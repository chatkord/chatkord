//! What a screen decides, apart from how it looks and how it reaches the world. Each model is plain data: an input
//! goes in, the model changes, and the effects the host must carry out come back.
#![forbid(unsafe_code)]

mod about;
mod account;
mod asked;
mod call;
pub mod channel_settings;
mod channels;
mod chime;
mod chosen;
mod client_report;
mod closed;
pub mod colour;
mod discussion;
mod door;
/// The channel model the front end draws from: its messages, rows and their days.
pub use chat;
pub mod frame;
#[cfg(any(test, fuzzing))]
pub mod fuzz;
#[cfg(any(test, fuzzing, feature = "testing"))]
pub mod harness;
pub mod hosting;
mod invite;
mod loaded;
mod look;
mod members;
mod name_field;
pub mod notice;
mod order;
mod overview;
mod paged;
mod people;
pub mod picture_row;
mod place;
pub mod popover_place;
mod proof;
mod reconnect;
mod roles;
mod server_about;
mod server_report;
mod settings;
mod setup;
mod shortened;
mod sign_in;
mod startup;
mod status;
mod tell;
mod update;
pub mod webhooks;
mod window;

pub use about::{About, Line, LineKind, Note, encoder_row, protocol_said};
pub use account::{
    Account, ChangingPassword, Field, Message as AccountMessage, Problem as PasswordProblem, same_as_now,
};
pub use asked::Asked;
pub use call::camera::{CameraSent, CanCamera, ListedCamera, OwnCamera};
pub use call::fit::{Placed, fit, whole_pixels};
pub use call::floating::{FloatAt, FloatMessage, Floating};
pub use call::health::{
    CallFigures, CallLooks, Flow, Graph, Health, KEPT, KEPT_CALLS, KEPT_LOOKS, LOOK_EVERY, MeasuredOn, Minute,
    Outgoing, OwnVideo, OwnWatch, Point, Reading, Reduced, Reduction, RoundTrip, RoundTrips, Spread, Tally,
};
pub use call::lens::{COVERED_AT_LEAST, Lens, Seen};
pub use call::quality::{Counts, Quality};
pub use call::room::{End, Event as RoomEvent, Lane, Person, Room, Take, Track, Why};
pub use call::share::{Can, Route, ShareCard, ShareState, Shared, SoundApp, SoundSaid, Watched};
pub use call::sound::{DeviceOption, DeviceState, Keep, Listed, Sound};
pub use call::sources::{
    Card as SourceCard, Chooser, Painting, Source, SourceKey, SourceKind, Sources, Tab as SourcesTab,
};
pub use call::voice::{CameraSeen, Card, Connection, Grid, Pinned, Preview, Stage, Tile, grid};
pub use call::voice_settings::{CameraTest, ProbeAsk, ProbeHeard, VoiceProbe};
pub use channels::{
    ChannelAction, ChannelProblem, Deleting, DeletingCategory, Making, MakingMessage, Named as NameFor, Naming,
};
pub use chime::Chime;
pub use chosen::{LengthNote, MatchNote};
pub use client_report::{
    Download, LISTED_LOG_LINES, Listing as ReportListing, Logs as ReportLogs, Machine, ReportMessage, Saved,
    Shown as ReportShown,
};
pub use door::{
    Admitted, Arrival, Ask as DoorAsk, Door, Errand, Found, Heard, Invite, Message, NameNote, Opening, Problem, Server,
    Shown, Step, Unvouched, Work, route_warning,
};
pub use invite::{
    Days, DeleteInviteRefused, DeletingInvite, InviteMessage, InviteProblem, Inviting, ListedInvite, Made, RoleOption,
    Uses, UsesChoice, UsesMessage, UsesRefused,
};
pub use loaded::Loaded;
pub use look::{PREVIEW, PreviewRun, theme_phrase};
pub use members::{
    BanAsk, Banning, Finding, Kicking, ListedMember, MemberAction, MemberMenu, MemberTab, Message as MembersMessage,
    Offered, Resetting, Transferring, counts as member_counts,
};
pub use name_field::{FieldNote, Message as NameFieldMessage, NameField};
pub use net::Jitter;
pub use notice::{Notice, Subject};
pub use order::Moved;
pub use overview::{Message as OverviewMessage, Overview, Toggle};
pub use paged::{Cursor, Paged};
pub use people::{colour_of, initials, presence_phrase};
pub use place::{
    Access, Answer as PlaceAnswer, Ask as PlaceAsk, Asked as PlaceAsked, Call, CallMessage, CallReturn, CardTool,
    Category, Caught, Channel, ChannelChange, ChannelList, ChannelStanding, Composing, Control, Directory, Discussion,
    DiscussionMessage, Early, EditStage, Editing, EndCard, Fold, FullScreen, History, ListEntry, ListKey, ListMove,
    Lobby, LobbyMessage, Marked, Message as PlaceMessage, Moderation, Moment, Named, OVERSCAN, Occupant, Offers, Out,
    OutSound, Over, PageMessage, PaneRow, Panel, PeopleMessage, PeoplePage, PinnedScreen, PinnedShare, Place, PlaceId,
    Placement, RailState, Received, Reconnecting, Roster, RosterGroup, RosterLine, RosterMessage, RosterRow, Row,
    Sending, ServerMessage, SessionMessage, Sight, SignedIn, Status, Strip, TextChannel, Unsent, WayBack, Whom, listed,
    person_of, read_message, wire_colour,
};
pub use place::{
    AttachState, Attaching, CHOICES_SHOWN, ChannelLevel, Choice, Choosing, Compose, EmojiList, EmojiMessage, EmojiPage,
    MARK_EVERY, MuteFor, NoPicker, NotifyDialog, NotifyMessage, Notifying, Pane, Pins, Quoted, RateMessage, Replying,
    SaveAnswer, ServerEmoji, Typers, Upload, Uploaded, Viewing, level_phrase,
};
pub use place::{
    Brief, ComposerGone, Conversation, ConversationRow, DeviceUse, FollowedThread, Idleness, Owner, PlaceUnread,
    PreviewLine, Readings, Roll, Root, Rows, Served, Talk, TalkMessage, ThreadRow, TimelineStanding, Title, Unread,
    With,
};
pub use place::{Fetched, PickFor, PickerMessage, PickerSection, Picking};
pub use place::{
    FoundFile, FoundIn, LinkPreviewShown, PREVIEW_PAUSE, PreviewMade, PreviewPicture, SEARCH_PAUSE, Search,
    SearchMessage, SearchStage,
};
pub use place::{
    ROSTER_BOTTOM, ROSTER_HEADING, ROSTER_HEADING_ABOVE, ROSTER_HEADING_BELOW, ROSTER_ROW, ROSTER_SIDE, ROSTER_SPACING,
    ROSTER_TOP,
};
pub use proof::{Proof, Refused};
pub use proto::rules::Place as RolePlace;
pub use report::Side as ReportSide;
pub use roles::{
    Creating as CreatingRole, DeletingRole, Editor as RoleEditor, ListedRole, Message as RolesMessage, RoleKind,
    RoleList, RoleProblem, Roles, RolesView, WORD_GROUPS, grant_words,
};
pub use server_about::{Message as ServerAboutMessage, ServerAbout};
pub use server_report::{ReportLine, ServerReport};
pub use settings::{Page, ServerSettings};
pub use setup::{Message as SetUpMessage, SetUp};
pub use sign_in::{Effect, Input, Refusal, Report, SignIn, Standing, Target};
pub use startup::{TrayItem, TrayMenu, TrayPick};
pub use status::{StatusDot, StatusFor, StatusMenu, StatusMessage, StatusRow, StatusWord, TOMORROW_AT};
pub use tell::{
    Action, Callout, CentrePanel, Device, Faults, FieldSaid, FieldTone, Mark, Offer, StateLine, Telling, Toast,
};
pub use update::{
    CHECK_EVERY, CLOCK_EVERY, FIRST_CHECK, InVoice, Percent, RestartOffer, SendsVideo, Unfinished, UpdateAsk,
    UpdateButton, UpdateCard, UpdateFound, UpdateMessage, UpdateNotice, UpdateStage, Updates, clock_in, is_due,
};
pub use window::{
    Adding, AdminPart, AppAbout, ArrivalNote, Ask, Asking, Build, Carried, Clicked, DevicePage, Dialog, DialogMessage,
    DropTarget, Entry, FullView, Gear, LanguageSaid, ListItem, ListMenu, ListMenuView, ListPick, Menu, MenuFrom,
    Message as WindowMessage, NotCopied, Origin, PersonCard, PersonFrom, PersonOpen, Pick, PoppedOut, ReportDialog,
    Revision, RoleBadge, RoleShown, Stated, Window, whole_seconds,
};
pub use window::{
    Awake, Badge, BadgeShown, IDLE_AFTER, IdleRun, IdleWatch, POPUP_BODY_MAX, POPUP_ID_MAX, POPUP_TITLE_MAX, PopupId,
};
mod emoji_settings;
pub use emoji_settings::{
    Doing as EmojiDoing, EmojiSettings, Message as EmojiPageMessage, Named as EmojiNamed, Naming as EmojiNaming,
};
pub use window::{DeletingEmoji, DeletingMessage};
pub use window::{ListSub, SubItem, SubMenuView};
