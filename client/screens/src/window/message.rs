//! What the window is told, and what it asks of the host.

use super::*;

/// The front end could not put text on the clipboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotCopied;

/// What a person did in the window, or what the host tells it.
#[derive(Clone, Debug)]
pub enum Message {
    /// At one server, or at the server being added.
    At(PlaceId, place::Message),
    /// The time zone this device keeps, read by the host: messages' days and times are said in it.
    Zone(chat::TimeZone),
    /// The day it is where the person is, told by the host at the start and as each day begins: a day line says
    /// Today and Yesterday by it.
    Today(chat::Date),
    /// The languages the system names for the person, most wanted first, read by the host at the start: the window
    /// is said in the first it carries while no language is chosen.
    SystemLanguages(Vec<LanguageIdentifier>),
    /// A language picked in Settings › Appearance, none to match the system's: the window is said in it at once,
    /// it is kept, and the Language field's menu closes.
    ChooseLanguage(Option<Language>),
    /// The Language field pressed: its menu opens, or closes while it is open.
    LanguageMenu,
    /// Share on the call controls at the pane's foot: where the call's page is not shown, its server and its voice
    /// page are opened first, then the share panel; while a share goes out, it is stopped.
    PaneShare(PlaceId),
    /// A server opened: picked in the rail while the pane is docked, or by the window itself.
    Open(PlaceId),
    /// A server's picture pressed in the rail: this device's page and any page open over the open server or this one
    /// close, so the open server's picture only leaves its pages; while the pane is in a drawer, the drawer opens on
    /// its channels, or shuts when it is the open server's.
    Rail(PlaceId),
    /// The Channels button, or Show channels: the drawer opens, or shuts when it is open.
    ToggleChannels,
    /// The scrim beside the drawer, or Escape over it.
    CloseChannels,
    /// The splitter between the pane and the content.
    Pane(PanePick),
    /// The rail's gear: the person's settings, on the open server's profile while it is signed in, else on this
    /// device's first page.
    OpenSettings,
    /// A page of this device's settings opened, over whichever server is open; any server's page closes.
    OpenDevice(DevicePage),
    /// This device's settings closed.
    CloseDevice,
    /// Back on a settings page shown alone: the list of pages shows.
    SettingsList,
    /// A time the window asked to be woken at has come.
    Tick,
    /// The rail scrolled: how far its first server now is above where it starts.
    RailScrolled(f32),
    /// A server's menu opened, from where it was asked for.
    Menu(PlaceId, MenuFrom),
    /// A press outside the open menu.
    CloseMenu,
    /// An item of a server's menu.
    Pick(PlaceId, Pick),
    /// The front end put the text asked for on the clipboard, or could not.
    Copied(Result<(), NotCopied>),
    /// Something done in the confirmation open over the window.
    Dialog(DialogMessage),
    /// Join again on a call that ended on a server other than the open one: that server opens, then the call is joined.
    JoinAgain(PlaceId),
    /// The toast's action pressed: it does what it offered, and the toast goes.
    ToastAct,
    /// The toast's ✕: it goes.
    DismissToast,
    /// The pointer came onto the toast, or left it: while it is on, the toast stays.
    HoldToast(bool),
    /// The rail's "+".
    Add,
    /// Cancel or ✕ on the add dialog: work under way is stopped.
    CloseAdd,
    /// A press outside the add dialog: it closes only before anything is typed.
    AddOutside,
    /// The deleted account's "Remove … from this device", or the changed key's "Remove from this device", which ask
    /// nothing more.
    Remove(PlaceId),
    /// Delete on a row of a server’s invites: asked first.
    DeleteInvite(PlaceId, InviteId),
    /// An entry of a member's menu, or Unban on their row, by the id others see them by.
    Member(PlaceId, MemberAction, MemberId),
    /// Create channel on the channels page, or the pane's "+".
    CreateChannel(PlaceId),
    /// A category's "+" in the side pane: a channel is made in it.
    CreateChannelIn(PlaceId, terms::CategoryId),
    /// Create category on the channels page.
    CreateCategory(PlaceId),
    /// A channel's or a category's menu asked for: a right press, its gear or the menu key.
    ListMenu(PlaceId, place::ListKey),
    /// An item of a channel's or a category's menu.
    ListPick(PlaceId, place::ListKey, ListPick),
    /// A person pressed where they show: their popover opens, or closes.
    Person(PlaceId, MemberId, PersonFrom),
    /// An app's name pressed beside its post: its About opens, or closes.
    AppAbout(PlaceId, MessageId),
    /// An administrator's entry of the open person's popover.
    Moderate(place::Moderation),
    /// Something done in the account's status menu, or Settings › Notifications' pointer to it.
    Status(crate::status::StatusMessage),
    /// The members toggle in a channel's or a call's header.
    ToggleMembers,
    /// A press outside the members column over the content, or Escape there.
    CloseMembers,
    /// The splitter on the members column's edge.
    MembersWidth(PanePick),
    /// A button on a channel's row, by the channel's id.
    Channel(PlaceId, ChannelAction, ChannelId),
    /// A button of a category chosen on the channels page, by the category's id.
    Category(PlaceId, ChannelAction, terms::CategoryId),
    /// Create role on the roles page.
    CreateRole(PlaceId),
    /// Delete role, under the chosen role: asked first.
    DeleteRole(PlaceId),
    /// A choice on the Appearance page.
    Look(LookPick),
    /// The emoji picker's skin tone button: hands are shown and picked in this tone from now on, and it is kept.
    EmojiTone(chat::emoji::SkinTone),
    /// The account bar's mute outside a call: how the next call starts.
    NextMute,
    /// The account bar's deafen outside a call.
    NextDeafen,
    /// The voice settings' switch of the window's own sounds.
    ToggleEventSounds,
    /// Settings › Notifications' switch of the sound a message that would alert makes.
    ToggleMessageSounds,
    /// Settings › Notifications' switch of desktop popups.
    TogglePopups,
    /// A desktop popup was clicked: the window shows and the place it is for opens.
    PopupClicked(PopupId),
    /// The run before this one did not end cleanly: a popup offers the debug report with its log.
    EarlierRunEnded,
    /// The host's watch found the person idle, or back at the machine.
    IdleChanged(IdleRun, place::Idleness),
    /// The host's watch ended or found no source: the person's input to this window stands in, [`IDLE_AFTER`] with
    /// none being idle.
    IdleSourceLost(IdleRun),
    /// A key, a click, a wheel turn, a pointer move or a touch in this window; the host tells it about once a second.
    Input,
    /// The window's close was asked for, by its own control or the system's.
    CloseAsked,
    /// Settings › Startup and window's first switch: open at sign-in.
    ToggleAtSignIn,
    /// Its second: a start at sign-in opens in the tray.
    ToggleStartsIn,
    /// Its third: closing the window keeps Chatkord running in the tray.
    ToggleCloseTo,
    /// Whether the desktop shows the tray icon, as the host found when it put it there.
    TrayShown(Tray),
    /// How the system starts Chatkord now, read again for the Startup and window page.
    StartupRead(Startup),
    /// The package's notices, read when About Chatkord first opened; none in a build run from its tree, or when the
    /// file was refused.
    NoticesRead(Option<notices::Notices>),
    /// A group of About Chatkord's notices pressed, by its place among them.
    ToggleNotice(usize),
    /// What the host read of this machine at the start: the operating system and the graphics adapter, for the debug
    /// report.
    Machine(Machine),
    /// Something done about the client's debug report, or the host's answer to what it asked.
    Report(ReportMessage),
    /// The window took the keyboard's focus, as it does when the person comes back to it from another application.
    Focused,
    /// The window lost the keyboard's focus: what it shows is no longer read as it comes.
    FocusLost,
    /// A mark or a link pressed in one of a server's messages.
    Clicked(PlaceId, Clicked),
    /// Files are dragged over the window, or have left it.
    FilesHovered(bool),
    /// Files let go over the window.
    FilesDropped(Vec<std::path::PathBuf>),
    /// A press outside a role's members, or Escape there.
    CloseRole,
    /// The Chat button on a call's header: the call's channel's messages open beside it, or close.
    ToggleCallChat,
    /// A press outside a call's chat over the call, or Escape there.
    CloseCallChat,
    /// The splitter on a call's chat column's edge.
    CallChatWidth(PanePick),
    /// The desktop session is ending: a close from now on is the system's, and quits at once.
    SessionEnding,
    /// The system asks Chatkord to end, as a session's end does: it quits at once, a call with it, never asking.
    Ended,
    /// Whether the window shows on the display now, or is minimised, covered whole or in the tray.
    Shown(bool),
    /// An entry of the tray icon's menu.
    FromTray(TrayPick),
    /// A microphone picked in the call's device menu or the settings page, none for the system's default.
    PickMicrophone(Option<Picked>),
    /// A speaker picked there, none for the system's default.
    PickSpeaker(Option<Picked>),
    /// "Play a test sound" on the settings page.
    PlayTestSound,
    /// How the microphone is processed, set and kept: a switch, a choice, a slider let go.
    SetProcessing(VoiceProcessing),
    /// The same while a slider is dragged: applied at once, kept once it is let go.
    TuneProcessing(VoiceProcessing),
    /// "Set from my room": the gate on, its threshold over the room's quietest moment of the last seconds.
    GateFromRoom,
    /// "Test" or "Stop test": one's voice played back a little late, the call muted meanwhile.
    TestMicrophone,
    /// A camera picked on the settings page; none for the first the system lists.
    PickCamera(Option<Picked>),
    /// The picture a camera is asked for, picked there.
    PickCameraResolution(choices::CameraResolution),
    /// The pictures a second a camera is asked for, picked there.
    PickCameraRate(choices::CameraRate),
    /// "Mirror my camera" switched.
    ToggleMirror,
    /// "Test camera" or "Stop test": the camera shown in the preview, or let go.
    TestCamera,
    /// What the probe behind that page said.
    Probed(ProbeHeard),
    /// What the voice settings page says of this subject, put away.
    DismissProbeNotice(Subject),
    /// This device's encoders for a screen, in the order they are tried, found once the engine is up; none is a
    /// device that cannot share.
    Encoders(Vec<EncoderOption>),
    /// Which picker a share is chosen in on this system, said once by the host.
    Chooser(Chooser),
    /// A server signed out or forgotten here whose token the keyring would not forget: its place says so, or one is put
    /// back, signed out.
    TokenLeft(Server, String),
    /// Something done with the floating card.
    Float(FloatMessage),
    /// What the host told of Chatkord's own update, or a person asked of it.
    Update(UpdateMessage),
}

/// Everything the screens ask of a host, the one tree it carries out: a server's asks under [`Ask::At`], the door's
/// and its sign-in's among them.
#[derive(Debug)]
pub enum Ask {
    /// Carry out what this server's place asks.
    At(PlaceId, place::Ask),
    /// Keep a choice of this device for the next run.
    Keep(Keep),
    /// Start, steer or stop the probe that measures the microphone while the voice settings are shown.
    Probe(ProbeAsk),
    /// Keep which server is open, for the next run.
    RememberOpen(ServerUrl),
    /// Report [`Message::Tick`] at this time.
    WakeAt(Instant),
    /// The next call starts muted or deafened as this says; the host keeps it for the next run too.
    NextCall(State),
    /// Play this moment's sound on the chosen speaker, in a call or not.
    Chime(Chime),
    /// Close the window and end the program, every server parted.
    Quit,
    /// Take the window away and keep running in the tray, a call with it.
    Hide,
    /// Bring the window back, from the tray or the taskbar.
    Show,
    /// Read again how the system starts Chatkord, which another application may have changed; report
    /// [`Message::StartupRead`].
    ReadStartup,
    /// Read the notices the package carries; report [`Message::NoticesRead`].
    ReadNotices,
    /// Read the recent log for the debug report; report [`ReportMessage::Read`].
    ReadLog,
    /// Ask where to save the debug report and write exactly this text there; report [`ReportMessage::Saved`].
    SaveReport(Download),
    /// Check for, fetch or start Chatkord's own update, or read the wall clock it is checked by.
    Update(UpdateAsk),
    /// Show a desktop popup under this id, in place of one shown under it before; a click is
    /// [`Message::PopupClicked`]. Its title and body are already within what the desktop takes.
    Popup {
        /// The popup's id.
        id: PopupId,
        /// Its title, plain.
        title: String,
        /// Its body, plain.
        body: String,
    },
    /// Take the popup away: what it shows is read.
    Withdraw {
        /// The popup's id.
        id: PopupId,
    },
    /// Start or stop watching for this device going idle; report [`Message::IdleChanged`] on each change.
    Idle(IdleWatch),
    /// Read the loudness of who is heard in the call at this server only while the window shows: a window hidden or
    /// in the tray is told none. A call starts reading it.
    Levels {
        /// The server whose call it is.
        place: PlaceId,
        /// Whether the window shows now.
        shown: bool,
    },
}
