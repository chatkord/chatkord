//! Sentences that name something: a server, a person, a number. [`crate::Phrase`] holds the fixed words; these are
//! the ones a value goes into, said through the catalog's typed messages so every front end says them alike.

use crate::failure::{self, Failure};
use crate::words::{
    BanBodyValues, ChannelMentionsValues, ChannelMutedMentionsValues, DayLineValues, DayMonthValues, DropNoteValues,
    EmojiFullValues, FileNoteValues, FoundFileValues, FrozeValues, InviteTermsNotedValues, InviteTermsValues,
    KeyFramesCountValues, ListCommaValues, OpenPreviewValues, Phrase, PictureNoteValues, PinnedAtValues, PlacedValues,
    PlacedYouValues, PopupBodyChannelValues, PopupBodyDirectValues, PopupBodyThreadValues, PopupTitleValues,
    ReactionChipValues, ReactionChipYoursValues, ReducedBusyValues, ReducedCeilingValues, ReportLogCutValues,
    ShareRateOutOfRangeValues, SoundGoneValues, TakenOutOnValues, TakenValues, TransferTitleValues, TwoTypingValues,
    UnbanBodyValues, UntilValues, UploadProgressValues, UsedValues, VoiceUnreachedValues, WebhookPostedInValues, Words,
};
use proto::chatkord_v1::CloseReason;
use proto::rules::{HostingField, HostingRefused, Switch, TextRefused};

/// When something chosen for a while ends, where the person is, and the day it is now there: said by the clock on
/// that day, else as tomorrow's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ends {
    /// The moment it ends.
    pub at: chat::DateTime,
    /// The day it is now.
    pub today: chat::Date,
}

impl Ends {
    /// It ends today.
    fn today(self) -> bool {
        self.at.date() == self.today
    }
}

/// A sentence with the value it names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sentence {
    /// Adding a server: no server answers at the host typed, which it holds as typed.
    NotFound(String),
    /// Adding a server: what answers at the host typed, which it holds, is not a Chatkord server.
    NotChatkord(String),
    /// A call whose media never connected, or whose voice address named nothing this device may reach.
    VoiceUnreached {
        /// The voice address as the server announced it: a name or an address.
        host: String,
        /// The port media was sent to.
        port: u16,
    },
    /// When an invite stops working, in seconds since the epoch.
    Until(i64),
    /// What this call has sent, as a size, in the connection popover.
    Up(String),
    /// What this call has received, as a size, in the connection popover.
    Down(String),
    /// An owner's invite previewed, that this account will own the server: its name, else its address.
    YoullOwn(String),
    /// The create-account form's heading: the server's name, else its address.
    CreateTitle(String),
    /// The sign-in form's heading: the server's name, else its address.
    SignInTitle(String),
    /// Spaces in a username, and the name with dots instead.
    Spaces(String),
    /// A username longer than any may be.
    NameTooLong,
    /// A username another account holds, asked whether it is this person's.
    Taken {
        /// The server's name, else its address.
        server: String,
        /// The username as typed.
        name: String,
    },
    /// A new password's placeholder: the fewest characters it may have.
    AtLeast,
    /// Under a new password too short: how many more characters it needs.
    ToGo(usize),
    /// An ask the server did not answer, said where the ask was made: the server's name, else its address.
    NoAnswer(String),
    /// The create-account form's way to sign in instead: the server's name, else its address.
    AlreadyHave(String),
    /// Sign-in refused for its username or password: the server's name, else its address.
    Wrong(String),
    /// Under the sign-in form, who can reset a forgotten password: the server's name, else its address.
    Forgot(String),
    /// While the account is being created: the server's name, else its address.
    Creating(String),
    /// While signing in: the server's name, else its address.
    SigningIn(String),
    /// While the server is being reached: the server's name, else its address.
    Connecting(String),
    /// The new-password form's body after a sign-in with a temporary one: the server's name, else its address.
    NewPasswordBody(String),
    /// The arrival note's title on a server this account just joined: the server's name, else its address.
    Welcome(String),
    /// The arrival note's body on joining: this account's display name as others see it.
    SeenAs(String),
    /// The arrival note's title, or the set-up dialog's, for the server's owner: the server's name, else its address.
    YouOwn(String),
    /// The title of the set-up dialog a server's owner sees first.
    SetUp(String),
    /// Why a remembered account must sign in again: how its last session ended.
    Ended {
        /// The server's name, else its address.
        server: String,
        /// The close reason the server gave; none says only that the sign-in ran out.
        reason: Option<CloseReason>,
    },
    /// A server that does not answer, as its panel's title.
    NotAnswering(String),
    /// When the next try is, in whole seconds.
    TryingIn(u64),
    /// The banner while a dropped connection reconnects: the server's name, else its address.
    Lost(String),
    /// Back after a drop that left this call.
    Back(String),
    /// The banner's button to join again the voice channel a drop left: its name.
    Rejoin(String),
    /// Where the account is: in this voice channel.
    In(String),
    /// A voice channel's row: a click joins it, or moves the call to it.
    JoinChannel(String),
    /// The row of the channel this account's call is in: a click opens the call.
    OpenChannel(String),
    /// A microphone or speaker that is not connected, and the device used instead (none: the system's default).
    SoundGone {
        /// The device's name, as the system names it.
        was: String,
        /// The device used instead, by its name; none for the system's default.
        now: Option<String>,
    },
    /// A microphone or speaker that was not connected and is again.
    SoundBack(String),
    /// A watched screen's decoder stopped and the processor decodes it now: the sharer's name.
    WatchedNowOnProcessor(String),
    /// A watched screen's decoder stopped and another decodes it now: the sharer's name.
    WatchedNowOnAnotherDecoder(String),
    /// The title of a call whose channel was deleted.
    ChannelWasDeleted(String),
    /// The title of a call that could not be made.
    CouldNotJoin(String),
    /// The notice that this account was given a role above its own: the server's name, else its address.
    RaisedOn(String),
    /// The Language field while the window matches the system's languages: the language they ask for, in its own
    /// name.
    MatchingSystem(String),
    /// Under Appearance's text size: the default size, in pixels.
    MessageSizeNote(u16),
    /// Under Appearance's space between message groups: the default space, in pixels.
    RunGapNote(u16),
    /// A spoiler shown in a message, for a screen reader: its words.
    SpoilerShown(String),
    /// A reaction under a message, for a screen reader: its emoji (a `:name:` for a server's own), how many reacted,
    /// and whether this account is one of them.
    Reaction {
        /// The emoji, or a server emoji's `:name:`.
        emoji: String,
        /// How many reacted with it.
        count: u32,
        /// This account reacted with it too.
        yours: bool,
    },
    /// Beside @everyone in the composer's chooser: the channel the message goes to.
    EveryoneNote(String),
    /// The heading of the composer's emoji chooser: what follows the colon.
    EmojiMatching(String),
    /// A pending file's way out of the composer's strip: its name.
    RemoveFile(String),
    /// A file's Save, for a screen reader: its name.
    SaveFile(String),
    /// Under "Drop to attach": the channel files go to, and the most a file may be, as a size.
    DropNote {
        /// The channel's name.
        channel: String,
        /// The most bytes a file may be, as the server's host set it.
        most: u64,
    },
    /// Server settings › Emoji, under its heading: the server's name, else its address.
    EmojiNote(String),
    /// Under a server emoji's name: who added it.
    AddedBy(String),
    /// A server emoji's rename, for a screen reader: its name.
    RenameEmoji(String),
    /// A server emoji's delete, for a screen reader: its name.
    DeleteEmoji(String),
    /// Server settings › Emoji, once the server holds the most it may: its name, else its address.
    EmojiFull(String),
    /// The confirmation before an emoji is deleted, its heading: the emoji's name.
    DeleteEmojiTitle(String),
    /// The same confirmation's body: the emoji's name.
    DeleteEmojiBody(String),
    /// The confirmation before a message is deleted, its heading: the name its writer is shown by.
    DeleteMessageTitle(String),
    /// The same confirmation's quoted line of the message's text.
    DeleteMessageLine(String),
    /// The heading of the panel of a server whose account was deleted: the server's name, else its address.
    NoLongerOn(String),
    /// The heading of the panel of a server that presented another key than the one this device trusts: the server's
    /// name, else its address.
    IdentityChanged(String),
    /// Why a server's panel asks for the password: this device signed out of it.
    SignedOutHere(String),
    /// The menu entry and account page's button that sign out: the server's name, else its address.
    SignOutOf(String),
    /// The sign-out confirmation's title: the server's name, else its address.
    SignOutTitle(String),
    /// The confirmation's title for taking a server off this device's list: the server's name, else its address.
    RemoveTitle(String),
    /// That confirmation's button: the server's name, else its address.
    RemoveAction(String),
    /// The deleted account's panel's one way on.
    RemoveFromHere(String),
    /// The menu entry and buttons that delete this account on the server: the server's name, else its address.
    Leave(String),
    /// The leave confirmation's title: the server's name, else its address.
    LeaveTitle(String),
    /// The leave confirmation's warning that the account is deleted: the server's name, else its address.
    LeaveBody(String),
    /// A confirmation's note when the server holds this window's call: the channel.
    AlsoLeavesCall(String),
    /// What closing the window during a call does: the call's channel.
    QuitLeavesCall(String),
    /// The note that a server's address was copied: the server's name, else its address.
    Copied(String),
    /// The title of a call's end card when the call ended for this account: the channel's name.
    YouLeft(String),
    /// "You’re out of Lounge": an administrator took the account out.
    OutOf(String),
    /// What an administrator did.
    TakenOutOf(String),
    /// The same, said while another server is open: the channel and the server it is on.
    TakenOutOn {
        /// The voice channel's name.
        channel: String,
        /// The server's name, else its address.
        server: String,
    },
    /// Adding a server that is already in this device's list: the server's name, else its address.
    AlreadyListed(String),
    /// The button that opens that already-listed server: the server's name, else its address.
    OpenServer(String),
    /// The invite dialog's title: the server.
    InviteTo(String),
    /// How many an invite admits.
    People(u32),
    /// A watched screen's heading: whose it is.
    ScreenOf(String),
    /// The card of a watched screen playing in a window of its own.
    ScreenOut(String),
    /// A watched screen over the whole display, as a screen reader names it: whose it is.
    FullScreenOf(String),
    /// The popover of how loud a shared screen's sound plays here, and its row in Audio controls: whose screen it is.
    SoundFromScreen(String),
    /// A watched screen's picture while its connection comes back.
    ReconnectingTo(String),
    /// One's own share's sound, from a window's app.
    SoundOf(String),
    /// One's own screen sent smaller than it is: the screen's size.
    ScaledFrom {
        /// The screen's width, in pixels.
        width: u32,
        /// The screen's height, in pixels.
        height: u32,
    },
    /// One's own screen sent below what was chosen because the rate chosen is too low for it: what it was (a rate, or a
    /// size and a rate, as the popover says them) and the limit, in bits a second.
    ReducedByCeiling {
        /// What the share was chosen to send.
        from: String,
        /// The limit, in bits a second.
        bits_per_second: u64,
    },
    /// The same, because the screen's content needs more than the rate chosen: what it was and that rate, in bits a
    /// second.
    ReducedByContent {
        /// What the share was chosen to send.
        from: String,
        /// The rate chosen, in bits a second.
        bits_per_second: u64,
    },
    /// The same under an automatic rate, so the connection is what the content exceeds: what it was.
    ReducedByConnection(String),
    /// The same, because the encoder could not keep up: what it was.
    ReducedByEncoder(String),
    /// How many days an invite works.
    ForDays(u32),
    /// What a link just made admits: how many, the role it gives, until when, and its note.
    InviteTerms {
        /// How many people it admits.
        uses: u32,
        /// The role it gives beside the default one, else the default role's name.
        role: String,
        /// When it stops working, in seconds since the epoch; said as its day in UTC.
        until: i64,
        /// The note its maker gave it; none for no note.
        note: Option<String>,
    },
    /// The day an invite stops working, in seconds since the epoch.
    Ends(i64),
    /// An invite's row in server settings: how many it has admitted of how many it may.
    Used {
        /// How many it has admitted.
        used: u32,
        /// How many it admits in all.
        of: u32,
    },
    /// The server keeps as many working invites as it may.
    TooManyInvites,
    /// A custom number of people an invite admits that is none: the most it admits.
    UsesOutOfRange,
    /// An invite's note or a channel's name longer than any may be.
    TextTooLong,
    /// The server holds as many channels as it may.
    TooManyChannels,
    /// The rename dialog's title: the voice channel's name as it is.
    RenameTitle(String),
    /// The delete-channel confirmation's title: the channel's name.
    DeleteChannelTitle(String),
    /// That confirmation's button: the channel's name.
    DeleteChannelAction(String),
    /// How many are in a voice channel's call.
    InTheCall(u32),
    /// Under a member's name on their row: their username and where they stand, and whether it is this account.
    Placed {
        /// Their username, without the `@` the row writes before it.
        handle: String,
        /// The names of every role they hold but the default one, highest first; none where they hold no other.
        roles: Vec<String>,
        /// Whether the row is this account's, which the row marks.
        you: bool,
    },
    /// Under a banned member's name on their row: their username.
    BannedHandle(String),
    /// The members page's list of members, by how many it holds.
    MembersCount(u32),
    /// Its list of banned members, by how many it holds; said without a number while it holds none.
    BannedCount(u32),
    /// A search of the members that finds no one: what is typed, trimmed.
    NoOneMatches(String),
    /// The menu of a member's roles: their display name.
    RolesFor(String),
    /// The menu of what else may be done to a member: their display name.
    MoreFor(String),
    /// The menu entry and button that sign a member out everywhere: their display name.
    KickMember(String),
    /// That confirmation's title: the member's display name.
    KickTitle(String),
    /// What it does: the member's display name.
    KickBody(String),
    /// Said once a member was signed out everywhere: their display name.
    Kicked(String),
    /// A kick the server refused as not allowed: the member's display name.
    CantKick(String),
    /// The menu entry and button that ban a member: their display name.
    BanMember(String),
    /// That confirmation's title: the member's display name.
    BanTitle(String),
    /// What a ban does.
    BanBody {
        /// The member's display name.
        name: String,
        /// The server's name, else its address.
        server: String,
    },
    /// A ban the server refused as not allowed: the member's display name.
    CantBan(String),
    /// The button that lifts a ban: the member's display name.
    UnbanMember(String),
    /// That confirmation's title: the member's display name.
    UnbanTitle(String),
    /// What lifting a ban does.
    UnbanBody {
        /// The member's display name.
        name: String,
        /// The server's name, else its address.
        server: String,
    },
    /// An unban the server refused as not allowed: the member's display name.
    CantUnban(String),
    /// A change to a role the server refused as not allowed: the role's name.
    CantChangeRole(String),
    /// A role at or above this account's place, which only someone higher can change: its name.
    RoleLocked(String),
    /// Where a role made now starts: the default role's name.
    CreateRoleBody(String),
    /// The delete-role confirmation's title: the role's name.
    DeleteRoleTitle(String),
    /// That confirmation's button: the role's name.
    DeleteRoleAction(String),
    /// The server holds as many roles as it may.
    TooManyRoles,
    /// A role change the server refused as not allowed: the member's display name.
    CantChangeRoles(String),
    /// A password reset the server refused as not allowed: the member's display name.
    CantReset(String),
    /// The reset-password confirmation's title: the member's display name.
    ResetTitle(String),
    /// That confirmation's warning that the member is signed out everywhere: their display name.
    ResetBody(String),
    /// The title over the temporary password a reset gave: the member's display name.
    TemporaryFor(String),
    /// How to give the temporary password to the member: their display name.
    HandOver(String),
    /// The transfer-ownership confirmation's title.
    TransferTitle {
        /// The display name of the member made owner.
        name: String,
        /// The server's name, else its address.
        server: String,
    },
    /// The transfer confirmation's note that only the new owner can give it back: their display name.
    TransferBody(String),
    /// The transfer confirmation's button: the new owner's display name.
    TransferAction(String),
    /// A sentence the server wrote, said as it is.
    Said(String),
    /// A fixed sentence, where a value of this type is wanted.
    Phrase(Phrase),
    /// A fixed sentence shown only as text, where a value of this type is wanted.
    Text(crate::Text),
    /// A share that ended without being stopped, or could not begin, and why.
    ShareStopped(Failure),
    /// A picture over the most a server takes, in megabytes.
    PictureTooLarge,
    /// A picture over the most pixels on a side a server takes.
    PictureTooWide,
    /// An emoji's picture over the most a server takes for one.
    EmojiTooLarge,
    /// One member writing, by name.
    OneTyping(String),
    /// Two members writing, by name.
    TwoTyping(String, String),
    /// The server's Notification settings dialog's heading: its name.
    NotificationsFrom(String),
    /// The dialog's Mute row: the server's name.
    MuteServer(String),
    /// The same row while muted.
    UnmuteServer(String),
    /// A channel's row with unread messages, for a reader of the screen: its name.
    ChannelUnread(String),
    /// A channel's row with mentions: its name and their count.
    ChannelMentions(String, u32),
    /// A muted channel's row with mentions: its name and their count.
    ChannelMutedMentions(String, u32),
    /// An upload's progress in the strip: bytes sent and its size.
    UploadProgress {
        /// Sent so far.
        sent: u64,
        /// Its size.
        size: u64,
    },
    /// A file over the most the server takes for one, in bytes, where the server said it.
    FileTooLarge(Option<u64>),
    /// The account's files at the most the server keeps for one account, in bytes, where the server said it.
    FilesUsed(Option<u64>),
    /// A monitor in our picker, by its place in the list (from 1).
    ScreenNumbered(usize),
    /// The release tag this build's source is at: `v` and its version.
    SourceTag(String),
    /// The commit a pre-release's source is at, abbreviated.
    SourceCommit(String),
    /// The debug report was saved under this file name.
    ReportDownloaded(String),
    /// The debug report's dialog lists a log's newest lines only: how many it lists, of how many the log holds.
    ReportLogCut {
        /// The lines listed.
        shown: u64,
        /// The lines the log holds.
        total: u64,
    },
    /// A newer version of Chatkord in place, by its version, which a restart runs.
    VersionReady(String),
    /// The same while a call holds the restart off.
    VersionWaits(String),
    /// A loudness or a gain, in whole decibels.
    Decibels(i8),
    /// The heading of the connection popover's round-trip graph: how many minutes back it reaches.
    RoundTripOver(u64),
    /// A mark under the connection popover's round-trip graph: how many minutes back it is.
    MinutesAgo(u64),
    /// The connection popover's key frames of this account's share: made, and asked for by its viewers.
    KeyFrames {
        /// By its encoder.
        made: u64,
        /// By its viewers.
        asked: u64,
    },
    /// A watched screen's freezes: how many, and the longest.
    Froze {
        /// How many times.
        times: u64,
        /// The longest freeze.
        longest: std::time::Duration,
    },
    /// A watched screen's frames its decoder refused.
    NotDecoded(u64),
    /// A size in pixels.
    PixelSize(u32, u32),
    /// The composer's name in a text channel, by the channel's name.
    MessageChannel(String),
    /// The composer's name beside a call, by its voice channel's name.
    MessageVoiceChannel(String),
    /// A file card's note: its size, then its kind from its name's extension, where it has one.
    FileNote {
        /// Its size in bytes.
        size: u64,
        /// The name it was sent with.
        name: String,
    },
    /// A name or a text a person or a server wrote, said as it is.
    Plain(String),
    /// When something was, said by the clock on the day it is and by the day on another.
    When {
        /// The moment, where the person is.
        at: chat::DateTime,
        /// The day it is now, where the person is; `None` until it is known, when the day is said in full.
        today: Option<chat::Date>,
    },
    /// The day something was made, a webhook: "Today" on the day it is, else its day and month.
    MadeOn {
        /// The day it was made, where the person is.
        day: chat::Date,
        /// The day it is now, where the person is; `None` until it is known, when the day is said in full.
        today: Option<chat::Date>,
    },
    /// A thread chip's or row's "last 21:06": when its last reply was.
    LastReply {
        /// The moment, where the person is.
        at: chat::DateTime,
        /// The day it is now, where the person is; `None` until it is known, when the day is said in full.
        today: Option<chat::Date>,
    },
    /// A thread chip's or row's count of replies.
    Replies(u32),
    /// The header's search field, named for the server it searches.
    SearchMessages(String),
    /// A search that finds nothing: what was typed.
    SearchNothing(String),
    /// Where a reply in a thread was found: the thread's channel.
    SearchInThread(String),
    /// A found file under Files: its size, the place it was sent in as the place is written, and the moment it was sent.
    FoundFile {
        /// Its size, in bytes.
        size: u64,
        /// Where it was sent: `#channel`, or the other person's name.
        place: String,
        /// The moment it was sent, where the person is.
        at: chat::DateTime,
        /// The day it is now, where the person is; `None` until it is known, when the day is said in full.
        today: Option<chat::Date>,
    },
    /// A found file's Jump, named for a screen reader: the file's name.
    JumpTo(String),
    /// A camera's heading and its window's title: whose it is.
    CameraOf(String),
    /// The card of a camera playing in a window of its own.
    CameraOut(String),
    /// An app's About: the webhook it posted by.
    Webhook(String),
    /// An app's About, in the channel its post is in.
    WebhookIn {
        /// The webhook's name.
        webhook: String,
        /// The channel's name.
        channel: String,
    },
    /// The dialog showing a webhook's URL: the webhook's name.
    WebhookUrlFor(String),
    /// Under that title: the channel it posts in, without its `#`.
    WebhookPostsIn(String),
    /// The title of regenerating a webhook's URL: its name.
    RegenerateWebhookTitle(String),
    /// The title of deleting a webhook: its name.
    DeleteWebhookTitle(String),
    /// Under a hosting number refused for being above its ceiling.
    HostingMost(HostingField),
    /// Under a hosting number refused for being below its minimum.
    HostingLeast(HostingField),
    /// Under the largest file refused for being above the per-person storage: what that is now, in MB.
    HostingWithin(u32),
    /// The composer's card while a link is fetched: the link's host.
    PreviewWaiting(String),
    /// A preview under a message, named for a screen reader and its press: the page's title and the link's host.
    OpenPreview {
        /// The page's title.
        title: String,
        /// The link's host.
        host: String,
    },
    /// The composer's placeholder in a conversation: the other person's name.
    MessagePerson(String),
    /// In place of the composer of a conversation whose other member is gone: their name.
    NoLongerHere(String),
    /// Over the first message of a conversation: the other person's name.
    ConversationStart(String),
    /// Under the status chosen while it has an end: "Until 22:30", "Until tomorrow 08:00".
    StatusUntil(Ends),
    /// Settings › Notifications' note while the account's status is Do not disturb, with its end if it has one.
    DoNotDisturbOn(Option<Ends>),
    /// Under the share's custom bitrate: the ceiling it makes, in bits a second.
    ShareRateAtMost(u64),
    /// Under the share's custom bitrate when the rate typed is none of the steps it takes, each in bits a second.
    ShareRateOutOfRange {
        /// The least rate.
        least: u64,
        /// The most rate.
        most: u64,
        /// The step between two rates.
        step: u64,
    },
    /// A desktop notification's title for a message in a channel or a thread: who wrote it, and the server.
    PopupTitle {
        /// The writer's name.
        author: String,
        /// The server's name, else its address.
        server: String,
    },
    /// A desktop notification's body for a channel's message.
    PopupChannel {
        /// The channel's name.
        channel: String,
        /// The message's text, plain.
        text: String,
    },
    /// A desktop notification's body for a conversation's message.
    PopupDirect {
        /// The server's name, else its address.
        server: String,
        /// The message's text, plain.
        text: String,
    },
    /// A desktop notification's body for a thread's reply.
    PopupThread {
        /// The channel's name.
        channel: String,
        /// The message's text, plain.
        text: String,
    },
    /// A pin's time in the pins panel: its day said against `today`, then the time.
    PinnedAt {
        /// When the message was sent, where the person is.
        at: chat::DateTime,
        /// The day it is now, where the person is; `None` until it is known, when the day is said in full.
        today: Option<chat::Date>,
    },
    /// The picture viewer's note: its size in pixels, then in bytes.
    PictureNote {
        /// Its width in pixels.
        width: u32,
        /// Its height in pixels.
        height: u32,
        /// Its size in bytes.
        size: u64,
    },
    /// Where this account may not send, by the channel's name.
    CannotSend(String),
    /// The welcome at the top of a channel's list, by its name.
    ChannelWelcome(String),
    /// Under it, what the list's top is.
    ChannelStart(String),
    /// The welcome at the top of a voice channel's chat beside its call, by the channel's name.
    VoiceChatWelcome(String),
    /// Under it, what the chat's top is.
    VoiceChatStart(String),
    /// A day line: `day`, said against `today`.
    Day {
        /// The day the messages under it were sent.
        day: chat::Date,
        /// The day it is now, where the person is; `None` until it is known, when the day is said in full.
        today: Option<chat::Date>,
    },
    /// The server holds as many categories as it may.
    TooManyCategories,
    /// Names said together as one list: "Regulars, Game hosts, Helpers".
    Listed(Vec<String>),
    /// The members column's list of who is signed in, by how many it holds.
    MembersOnline(u32),
    /// Its list of who is not, by how many it holds.
    MembersOffline(u32),
    /// A mute for everyone the server refused because the person had left the call: their name.
    NotMutedLeft(String),
    /// A take-out the server refused because the person had left the call: their name.
    NotTakenOutLeft(String),
    /// A category's "+" in the side pane: the category's name.
    CreateChannelIn(String),
    /// The rename confirmation's title for a text channel: its name, without the `#` it writes.
    RenameTextTitle(String),
    /// The delete confirmation's title for a text channel: its name.
    DeleteTextTitle(String),
    /// That confirmation's button: the text channel's name.
    DeleteTextAction(String),
    /// What deleting a category takes with it: how many channels it holds.
    DeleteCategoryBody(u32),
    /// The button that takes a role or a member off a channel's list: their name.
    RemoveTarget(String),
    /// The button that takes every choice a role or a member has in a channel off it: their name.
    RemoveFromChannel(String),
    /// A private channel's start, in place of who may read and write there: the channel's name, without its `#`.
    PrivateChannelStart(String),
}

/// The day of a moment in UTC, as day of the month and month from 1 (Howard Hinnant's days-to-civil).
fn day_and_month(seconds: i64) -> (i64, i64) {
    let z = seconds.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (day, month)
}

impl Phrase {
    /// The label of a hosting number.
    pub const fn hosting_label(field: HostingField) -> Phrase {
        match field {
            HostingField::AccountFilesMax => Phrase::HostingAccountFilesMax,
            HostingField::AttachmentMax => Phrase::HostingAttachmentMax,
        }
    }

    /// A switch's name and the line under it.
    pub const fn switch(switch: Switch) -> (Phrase, Phrase) {
        match switch {
            Switch::Conversations => (Phrase::DirectMessages, Phrase::SwitchConversationsLine),
            Switch::Threads => (Phrase::Threads, Phrase::SwitchThreadsLine),
            Switch::LinkPreviews => (Phrase::SwitchLinkPreviews, Phrase::SwitchLinkPreviewsLine),
            Switch::Search => (Phrase::SwitchSearch, Phrase::SwitchSearchLine),
            Switch::Welcome => (Phrase::SwitchWelcome, Phrase::SwitchWelcomeLine),
        }
    }
}

impl Sentence {
    /// What a refused hosting number says under its field: `typed` is the number in the field, `within_now` what the
    /// number it must stay within is now.
    pub fn hosting_refused(refused: HostingRefused, typed: u32, within_now: u32) -> Sentence {
        match refused {
            HostingRefused::PublicAddress => Sentence::Phrase(Phrase::PublicAddressRefused),
            HostingRefused::OutOfRange(field) if typed < field.minimum() => Sentence::HostingLeast(field),
            HostingRefused::OutOfRange(field) => Sentence::HostingMost(field),
            HostingRefused::Above { .. } => Sentence::HostingWithin(within_now),
        }
    }

    /// What a webhook's refused name says under its field.
    pub const fn webhook_name_refused(refused: TextRefused) -> Sentence {
        match refused {
            TextRefused::Empty => Sentence::Phrase(Phrase::WebhookNameNeeded),
            TextRefused::TooLong => Sentence::TextTooLong,
            TextRefused::ControlCharacter => Sentence::Phrase(Phrase::PlainTextOnly),
        }
    }
}

impl Words {
    /// `at` said by the clock when it is on `today`, else by its day.
    fn when(&self, at: chat::DateTime, today: Option<chat::Date>) -> String {
        if today == Some(at.date()) { self.clock(at) } else { self.say(&Sentence::Day { day: at.date(), today }) }
    }

    /// `names` said as a list joined by commas: "Regulars, Game hosts, Helpers".
    fn comma_list(&self, names: &[String]) -> String {
        let mut names = names.iter().rev();
        let Some(last) = names.next() else { return String::new() };
        names.fold(last.clone(), |rest, first| {
            self.list_comma(ListCommaValues { first: first.into(), rest: rest.into() })
        })
    }

    /// What `sentence` says.
    pub fn say(&self, sentence: &Sentence) -> String {
        match sentence {
            Sentence::NotFound(host) => self.not_found(host),
            Sentence::NotChatkord(host) => self.not_chatkord(host),
            Sentence::VoiceUnreached { host, port } => {
                self.voice_unreached(VoiceUnreachedValues { host: host.into(), port: port.to_string().into() })
            }
            Sentence::Up(size) => self.sent_size(size),
            Sentence::Down(size) => self.received_size(size),
            Sentence::ScreenNumbered(n) => self.screen_numbered(*n),
            Sentence::SourceTag(tag) => self.source_tag(tag),
            Sentence::SourceCommit(commit) => self.source_commit(commit),
            Sentence::ReportDownloaded(file) => self.report_downloaded(file),
            Sentence::ReportLogCut { shown, total } => {
                self.report_log_cut(ReportLogCutValues { shown: (*shown).into(), total: (*total).into() })
            }
            Sentence::VersionReady(version) => self.update_version_ready(version),
            Sentence::VersionWaits(version) => self.update_version_waits(version),
            Sentence::Decibels(db) => self.decibels(*db),
            Sentence::RoundTripOver(minutes) => self.round_trip_over(*minutes),
            Sentence::MinutesAgo(minutes) => self.minutes_ago(*minutes),
            Sentence::KeyFrames { made, asked } => {
                self.key_frames_count(KeyFramesCountValues { made: (*made).into(), asked: (*asked).into() })
            }
            Sentence::Froze { times, longest } => {
                self.froze(FrozeValues { times: (*times).into(), seconds: self.seconds(*longest).into() })
            }
            Sentence::NotDecoded(frames) => self.not_decoded(*frames),
            Sentence::PixelSize(width, height) => self.dimensions(*width, *height),
            Sentence::MessageChannel(channel) => self.message_channel(channel),
            Sentence::MessageVoiceChannel(channel) => self.message_voice_channel(channel),
            Sentence::FileNote { size, name } => match kind_of(name) {
                Some(kind) => self.file_note(FileNoteValues { size: self.bytes(*size).into(), kind: kind.into() }),
                None => self.bytes(*size),
            },
            Sentence::Plain(text) => text.clone(),
            Sentence::When { at, today } => self.when(*at, *today),
            Sentence::LastReply { at, today } => self.last_reply(self.when(*at, *today)),
            Sentence::Replies(count) => self.reply_count(*count),
            Sentence::SearchMessages(server) => self.search_messages(server),
            Sentence::SearchNothing(query) => self.search_nothing(query),
            Sentence::SearchInThread(channel) => self.search_in_thread(channel),
            Sentence::FoundFile { size, place, at, today } => self.found_file(FoundFileValues {
                size: self.bytes(*size).into(),
                place: place.into(),
                day: self.say(&Sentence::Day { day: at.date(), today: *today }).into(),
            }),
            Sentence::JumpTo(name) => self.jump_to(name),
            Sentence::CameraOf(name) => self.camera_of(name),
            Sentence::CameraOut(name) => self.camera_out(name),
            Sentence::Webhook(name) => self.webhook_posted_by(name),
            Sentence::WebhookIn { webhook, channel } => {
                self.webhook_posted_in(WebhookPostedInValues { name: webhook.into(), channel: channel.into() })
            }
            Sentence::WebhookUrlFor(name) => self.webhook_url_for(name),
            Sentence::WebhookPostsIn(channel) => self.webhook_posts_in(channel),
            Sentence::RegenerateWebhookTitle(name) => self.regenerate_webhook_title(name),
            Sentence::DeleteWebhookTitle(name) => self.delete_webhook_title(name),
            Sentence::HostingMost(field) => self.hosting_most(self.count(field.ceiling().into())),
            Sentence::HostingLeast(field) => self.hosting_least(self.count(field.minimum().into())),
            Sentence::HostingWithin(most) => self.hosting_within(self.count((*most).into())),
            Sentence::PreviewWaiting(host) => self.preview_waiting(host),
            Sentence::OpenPreview { title, host } => {
                self.open_preview(OpenPreviewValues { title: title.into(), host: host.into() })
            }
            Sentence::MessagePerson(name) => self.message_person(name),
            Sentence::NoLongerHere(name) => self.no_longer_here(name),
            Sentence::ConversationStart(name) => self.conversation_start(name),
            Sentence::StatusUntil(ends) if ends.today() => self.status_until(self.clock(ends.at)),
            Sentence::StatusUntil(ends) => self.status_until_tomorrow_at(self.clock(ends.at)),
            Sentence::DoNotDisturbOn(None) => self.phrase(Phrase::DoNotDisturbOn).into_owned(),
            Sentence::DoNotDisturbOn(Some(ends)) if ends.today() => self.do_not_disturb_on_until(self.clock(ends.at)),
            Sentence::DoNotDisturbOn(Some(ends)) => self.do_not_disturb_on_until_tomorrow(self.clock(ends.at)),
            Sentence::ShareRateAtMost(bits) => self.share_rate_at_most(self.mbits(*bits)),
            Sentence::ShareRateOutOfRange { least, most, step } => {
                self.share_rate_out_of_range(ShareRateOutOfRangeValues {
                    least: self.mbits(*least).into(),
                    most: self.mbits(*most).into(),
                    step: self.mbits(*step).into(),
                })
            }
            Sentence::PopupTitle { author, server } => {
                self.popup_title(PopupTitleValues { author: author.into(), server: server.into() })
            }
            Sentence::PopupChannel { channel, text } => {
                self.popup_body_channel(PopupBodyChannelValues { channel: channel.into(), text: text.into() })
            }
            Sentence::PopupDirect { server, text } => {
                self.popup_body_direct(PopupBodyDirectValues { server: server.into(), text: text.into() })
            }
            Sentence::PopupThread { channel, text } => {
                self.popup_body_thread(PopupBodyThreadValues { channel: channel.into(), text: text.into() })
            }
            Sentence::PinnedAt { at, today } => self.pinned_at(PinnedAtValues {
                day: self.say(&Sentence::Day { day: at.date(), today: *today }).into(),
                time: self.clock(*at).into(),
            }),
            Sentence::PictureNote { width, height, size } => self.picture_note(PictureNoteValues {
                dimensions: self.dimensions(*width, *height).into(),
                size: self.bytes(*size).into(),
            }),
            Sentence::CannotSend(channel) => self.cannot_send(channel),
            Sentence::ChannelStart(channel) => self.channel_start(channel),
            Sentence::VoiceChatStart(channel) => self.voice_chat_start(channel),
            Sentence::ChannelWelcome(channel) => self.channel_welcome(channel),
            Sentence::VoiceChatWelcome(channel) => self.voice_chat_welcome(channel),
            Sentence::Day { day, today } if today.as_ref() == Some(day) => self.phrase(Phrase::Today).into_owned(),
            Sentence::Day { day, today: Some(today) } if day.tomorrow().ok().as_ref() == Some(today) => {
                self.phrase(Phrase::Yesterday).into_owned()
            }
            Sentence::Day { day, .. } => self.day_line(DayLineValues {
                weekday: day.weekday().to_monday_one_offset().into(),
                day: day.day().into(),
                month: day.month().into(),
            }),
            Sentence::MadeOn { day, today } if today.as_ref() == Some(day) => self.phrase(Phrase::Today).into_owned(),
            Sentence::MadeOn { day, .. } => {
                self.day_month(DayMonthValues { day: day.day().into(), month: day.month().into() })
            }
            Sentence::Until(at) => {
                let (day, month) = day_and_month(*at);
                self.until(UntilValues { day: day.into(), month: month.into() })
            }
            Sentence::YoullOwn(server) => self.youll_own(server),
            Sentence::CreateTitle(server) => self.create_title(server),
            Sentence::SignInTitle(server) => self.sign_in_title(server),
            Sentence::Spaces(name) => self.spaces(name),
            Sentence::NameTooLong => self.no_more_than(proto::rules::MAX_HANDLE_BYTES),
            Sentence::Taken { server, name } => self.taken(TakenValues { name: name.into(), server: server.into() }),
            Sentence::AtLeast => self.at_least(proto::rules::MIN_CHOSEN_CHARS),
            Sentence::ToGo(left) => self.to_go(*left),
            Sentence::NoAnswer(server) => self.no_answer(server),
            Sentence::AlreadyHave(server) => self.already_have(server),
            Sentence::Wrong(server) => self.wrong(server),
            Sentence::Forgot(server) => self.forgot(server),
            Sentence::Creating(server) => self.creating(server),
            Sentence::SigningIn(server) => self.signing_in(server),
            Sentence::Connecting(server) => self.connecting(server),
            Sentence::NewPasswordBody(server) => self.new_password_body(server),
            Sentence::Welcome(server) => self.welcome(server),
            Sentence::SeenAs(name) => self.seen_as(name),
            Sentence::YouOwn(server) => self.you_own(server),
            Sentence::SetUp(server) => self.set_up(server),
            Sentence::Ended { server, reason } => self.ended(server, *reason),
            Sentence::NotAnswering(server) => self.not_answering(server),
            Sentence::TryingIn(seconds) => self.trying_in(*seconds),
            Sentence::Lost(server) => self.lost(server),
            Sentence::Back(channel) => self.back_after_drop(channel),
            Sentence::Rejoin(channel) => self.rejoin(channel),
            Sentence::In(channel) => self.in_channel(channel),
            Sentence::JoinChannel(channel) => self.join_channel(channel),
            Sentence::OpenChannel(channel) => self.open_channel(channel),
            Sentence::SoundGone { was, now: Some(now) } => {
                self.sound_gone(SoundGoneValues { now: now.into(), was: was.into() })
            }
            Sentence::SoundGone { was, now: None } => self.sound_gone_to_default(was),
            Sentence::SoundBack(name) => self.sound_back(name),
            Sentence::WatchedNowOnProcessor(name) => self.watched_now_on_processor(name),
            Sentence::WatchedNowOnAnotherDecoder(name) => self.watched_now_on_another_decoder(name),
            Sentence::ChannelWasDeleted(channel) => self.channel_was_deleted(channel),
            Sentence::CouldNotJoin(channel) => self.could_not_join(channel),
            Sentence::RaisedOn(server) => self.raised_on(server),
            Sentence::MatchingSystem(language) => self.language_system_named(language),
            Sentence::MessageSizeNote(default) => self.message_size_note(*default),
            Sentence::RunGapNote(default) => self.run_gap_note(*default),
            Sentence::SpoilerShown(text) => self.spoiler_shown(text),
            Sentence::Reaction { emoji, count, yours: false } => {
                self.reaction_chip(ReactionChipValues { emoji: emoji.into(), count: (*count).into() })
            }
            Sentence::Reaction { emoji, count, yours: true } => {
                self.reaction_chip_yours(ReactionChipYoursValues { emoji: emoji.into(), count: (*count).into() })
            }
            Sentence::EveryoneNote(channel) => self.everyone_note(channel),
            Sentence::EmojiMatching(query) => self.emoji_matching(query),
            Sentence::RemoveFile(name) => self.remove_file(name),
            Sentence::SaveFile(name) => self.save_file(name),
            Sentence::DropNote { channel, most } => {
                self.drop_note(DropNoteValues { channel: channel.into(), most: self.limit(*most).into() })
            }
            Sentence::EmojiNote(server) => self.emoji_note(server),
            Sentence::AddedBy(name) => self.added_by(name),
            Sentence::RenameEmoji(name) => self.rename_emoji(name),
            Sentence::DeleteEmoji(name) => self.delete_emoji(name),
            Sentence::EmojiFull(server) => self
                .emoji_full(EmojiFullValues { server: server.as_str().into(), most: proto::rules::MAX_EMOJI.into() }),
            Sentence::DeleteEmojiTitle(name) => self.delete_emoji_title(name),
            Sentence::DeleteEmojiBody(name) => self.delete_emoji_body(name),
            Sentence::DeleteMessageTitle(author) => self.delete_message_title(author),
            Sentence::DeleteMessageLine(line) => self.delete_message_line(line),
            Sentence::NoLongerOn(server) => self.no_longer_on(server),
            Sentence::IdentityChanged(server) => self.identity_changed(server),
            Sentence::SignedOutHere(server) => self.signed_out_here(server),
            Sentence::SignOutOf(server) => self.sign_out_of(server),
            Sentence::SignOutTitle(server) => self.sign_out_title(server),
            Sentence::RemoveTitle(server) => self.remove_title(server),
            Sentence::RemoveAction(server) => self.remove_action(server),
            Sentence::RemoveFromHere(server) => self.remove_from_here(server),
            Sentence::Leave(server) => self.leave_server(server),
            Sentence::LeaveTitle(server) => self.leave_title(server),
            Sentence::LeaveBody(server) => self.leave_body(server),
            Sentence::AlsoLeavesCall(channel) => self.also_leaves_call(channel),
            Sentence::QuitLeavesCall(channel) => self.quit_leaves_call(channel),
            Sentence::Copied(server) => self.copied(server),
            Sentence::YouLeft(channel) => self.you_left(channel),
            Sentence::OutOf(channel) => self.out_of(channel),
            Sentence::TakenOutOf(channel) => self.taken_out_of(channel),
            Sentence::TakenOutOn { channel, server } => {
                self.taken_out_on(TakenOutOnValues { channel: channel.into(), server: server.into() })
            }
            Sentence::AlreadyListed(server) => self.already_listed(server),
            Sentence::OpenServer(server) => self.open_server(server),
            Sentence::InviteTo(server) => self.invite_to(server),
            Sentence::ScreenOf(name) => self.screen_of(name),
            Sentence::ScreenOut(name) => self.screen_out(name),
            Sentence::FullScreenOf(name) => self.full_screen_of(name),
            Sentence::SoundFromScreen(name) => self.sound_from_screen(name),
            Sentence::ReconnectingTo(name) => self.reconnecting_to(name),
            Sentence::SoundOf(name) => self.sound_of(name),
            Sentence::ScaledFrom { width, height } => self.scaled_from(self.dimensions(*width, *height)),
            Sentence::ReducedByCeiling { from, bits_per_second } => self
                .reduced_ceiling(ReducedCeilingValues { from: from.into(), rate: self.rate(*bits_per_second).into() }),
            Sentence::ReducedByContent { from, bits_per_second } => {
                self.reduced_busy(ReducedBusyValues { from: from.into(), rate: self.rate(*bits_per_second).into() })
            }
            Sentence::ReducedByConnection(from) => self.reduced_busy_connection(from),
            Sentence::ReducedByEncoder(from) => self.reduced_encoder(from),
            Sentence::People(count) => self.people(*count),
            Sentence::ForDays(count) => self.for_days(*count),
            Sentence::InviteTerms { uses, role, until, note } => {
                let (day, month) = day_and_month(*until);
                let people = self.people(*uses);
                match note {
                    Some(note) => self.invite_terms_noted(InviteTermsNotedValues {
                        day: day.into(),
                        month: month.into(),
                        note: note.into(),
                        people: people.into(),
                        role: role.into(),
                    }),
                    None => self.invite_terms(InviteTermsValues {
                        day: day.into(),
                        month: month.into(),
                        people: people.into(),
                        role: role.into(),
                    }),
                }
            }
            Sentence::Ends(at) => {
                let (day, month) = day_and_month(*at);
                self.day_month(DayMonthValues { day: day.into(), month: month.into() })
            }
            Sentence::Used { used, of } => self.used(UsedValues { of: (*of).into(), used: (*used).into() }),
            Sentence::TooManyInvites => self.too_many_invites(proto::rules::MAX_LIVE_INVITES),
            Sentence::UsesOutOfRange => self.uses_out_of_range(proto::rules::MAX_INVITE_USES),
            Sentence::TextTooLong => self.no_more_than(proto::rules::MAX_NAME_CHARS),
            Sentence::TooManyChannels => self.too_many_channels(proto::rules::MAX_CHANNELS),
            Sentence::RenameTitle(name) => self.rename_title(name),
            Sentence::DeleteChannelTitle(name) => self.delete_channel_title(name),
            Sentence::DeleteChannelAction(name) => self.delete_channel_action(name),
            Sentence::InTheCall(count) => self.in_the_call(*count),
            Sentence::Placed { handle, roles, you } => match (roles.is_empty(), you) {
                (false, true) => {
                    let place = self.comma_list(roles);
                    self.placed_you(PlacedYouValues { handle: handle.into(), place: place.into() })
                }
                (false, false) => {
                    let place = self.comma_list(roles);
                    self.placed(PlacedValues { handle: handle.into(), place: place.into() })
                }
                (true, true) => self.handle_you(handle),
                (true, false) => self.handle_only(handle),
            },
            Sentence::Listed(names) => self.comma_list(names),
            Sentence::TooManyCategories => self.too_many_categories(proto::rules::MAX_CATEGORIES),
            Sentence::MembersOnline(count) => self.members_online(*count),
            Sentence::MembersOffline(count) => self.members_offline(*count),
            Sentence::NotMutedLeft(name) => self.not_muted_left(name),
            Sentence::NotTakenOutLeft(name) => self.not_taken_out_left(name),
            Sentence::CreateChannelIn(category) => self.create_channel_in(category),
            Sentence::RenameTextTitle(name) => self.rename_text_title(name),
            Sentence::DeleteTextTitle(name) => self.delete_text_title(name),
            Sentence::DeleteTextAction(name) => self.delete_text_action(name),
            Sentence::DeleteCategoryBody(count) => self.delete_category_body(*count),
            Sentence::RemoveTarget(name) => self.remove_target(name),
            Sentence::RemoveFromChannel(name) => self.remove_from_channel(name),
            Sentence::PrivateChannelStart(channel) => self.private_channel_start(channel),
            Sentence::BannedHandle(handle) => self.handle_banned(handle),
            Sentence::MembersCount(count) => self.members_count(*count),
            Sentence::BannedCount(0) => self.phrase(Phrase::Banned).into_owned(),
            Sentence::BannedCount(count) => self.banned_count(*count),
            Sentence::NoOneMatches(query) => self.no_one_matches(query),
            Sentence::RolesFor(name) => self.roles_for(name),
            Sentence::MoreFor(name) => self.more_for(name),
            Sentence::KickMember(name) => self.kick_member(name),
            Sentence::KickTitle(name) => self.kick_title(name),
            Sentence::KickBody(name) => self.kick_body(name),
            Sentence::Kicked(name) => self.kicked(name),
            Sentence::CantKick(name) => self.cant_kick(name),
            Sentence::BanMember(name) => self.ban_member(name),
            Sentence::BanTitle(name) => self.ban_title(name),
            Sentence::BanBody { name, server } => {
                self.ban_body(BanBodyValues { name: name.into(), server: server.into() })
            }
            Sentence::CantBan(name) => self.cant_ban(name),
            Sentence::UnbanMember(name) => self.unban_member(name),
            Sentence::UnbanTitle(name) => self.unban_title(name),
            Sentence::UnbanBody { name, server } => {
                self.unban_body(UnbanBodyValues { name: name.into(), server: server.into() })
            }
            Sentence::CantUnban(name) => self.cant_unban(name),
            Sentence::CantChangeRole(role) => self.cant_change_role(role),
            Sentence::RoleLocked(role) => self.role_locked(role),
            Sentence::CreateRoleBody(role) => self.create_role_body(role),
            Sentence::DeleteRoleTitle(role) => self.delete_role_title(role),
            Sentence::DeleteRoleAction(role) => self.delete_role_action(role),
            Sentence::TooManyRoles => self.too_many_roles(proto::rules::MAX_ROLES),
            Sentence::CantChangeRoles(name) => self.cant_change_roles(name),
            Sentence::CantReset(name) => self.cant_reset(name),
            Sentence::ResetTitle(name) => self.reset_title(name),
            Sentence::ResetBody(name) => self.reset_body(name),
            Sentence::TemporaryFor(name) => self.temporary_for(name),
            Sentence::HandOver(name) => self.hand_over(name),
            Sentence::TransferTitle { name, server } => {
                self.transfer_title(TransferTitleValues { name: name.into(), server: server.into() })
            }
            Sentence::TransferBody(name) => self.transfer_body(name),
            Sentence::TransferAction(name) => self.transfer_action(name),
            Sentence::Said(said) => said.clone(),
            Sentence::Phrase(phrase) => self.phrase(*phrase).into_owned(),
            Sentence::Text(text) => self.text(*text).into_owned(),
            Sentence::ShareStopped(failed) => self.share_stopped(self.say(&failure::of(failed))),
            Sentence::PictureTooLarge => self.picture_too_large(self.limit(proto::files::PICTURE_UPLOAD_BYTES)),
            Sentence::PictureTooWide => self.picture_too_wide(proto::files::PICTURE_UPLOAD_SIDE),
            Sentence::EmojiTooLarge => self.emoji_too_large(self.limit(proto::rules::EMOJI_UPLOAD_BYTES)),
            Sentence::OneTyping(name) => self.one_typing(name.as_str()),
            Sentence::TwoTyping(first, second) => {
                self.two_typing(TwoTypingValues { first: first.as_str().into(), second: second.as_str().into() })
            }
            Sentence::ChannelUnread(channel) => self.channel_unread(channel.as_str()),
            Sentence::ChannelMentions(channel, count) => self
                .channel_mentions(ChannelMentionsValues { channel: channel.as_str().into(), count: (*count).into() }),
            Sentence::ChannelMutedMentions(channel, count) => self.channel_muted_mentions(ChannelMutedMentionsValues {
                channel: channel.as_str().into(),
                count: (*count).into(),
            }),
            Sentence::NotificationsFrom(server) => self.notifications_from(server.as_str()),
            Sentence::MuteServer(server) => self.mute_server(server.as_str()),
            Sentence::UnmuteServer(server) => self.unmute_server(server.as_str()),
            Sentence::UploadProgress { sent, size } => self.upload_progress(UploadProgressValues {
                sent: self.bytes(*sent).into(),
                size: self.bytes(*size).into(),
            }),
            Sentence::FileTooLarge(Some(most)) => self.file_too_large(self.limit(*most)),
            Sentence::FileTooLarge(None) => self.phrase(Phrase::FileTooLargeUnsaid).into_owned(),
            Sentence::FilesUsed(Some(most)) => self.files_used(self.limit(*most)),
            Sentence::FilesUsed(None) => self.phrase(Phrase::FilesUsedUnsaid).into_owned(),
        }
    }

    fn ended(&self, server: &str, reason: Option<CloseReason>) -> String {
        let expired = || self.phrase(Phrase::SignInExpired).into_owned();
        let Some(reason) = reason else { return expired() };
        match reason {
            CloseReason::SignedOut => self.signed_out_elsewhere(server),
            CloseReason::AccountDeleted => self.account_deleted_on(server),
            CloseReason::Kicked | CloseReason::PasswordChanged | CloseReason::PasswordReset | CloseReason::Banned => {
                failure::closed(reason).map_or_else(expired, |phrase| self.phrase(phrase).into_owned())
            }
            // A session that ran out, or one the door was sent to for a reason that is not about the session.
            CloseReason::SessionExpired
            | CloseReason::Unspecified
            | CloseReason::Normal
            | CloseReason::ServerError
            | CloseReason::FellBehind
            | CloseReason::VersionMismatch
            | CloseReason::Moved
            | CloseReason::NotAllowed
            | CloseReason::ChannelDeleted
            | CloseReason::TakenOut
            | CloseReason::OverRate
            | CloseReason::Unreachable
            | CloseReason::ClientShutdown
            | CloseReason::ServerStopping => expired(),
        }
    }
}

/// A file's kind as its name's extension says it, upper case: "PDF" for `notes.pdf`; none without one.
fn kind_of(name: &str) -> Option<String> {
    let extension = std::path::Path::new(name).extension()?.to_str()?;
    (!extension.is_empty()).then(|| extension.to_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Language;

    #[test]
    fn a_day_is_named_in_utc() {
        let words = Words::english();
        // 2026-11-01T00:00:00Z and the second before it, the epoch, and a leap day.
        assert_eq!(words.said(&Sentence::Until(1_793_491_200)), "· until 1 November");
        assert_eq!(words.said(&Sentence::Up("12.4 MB".into())), "\u{2191} 12.4 MB");
        assert_eq!(words.said(&Sentence::Down("48.1 MB".into())), "\u{2193} 48.1 MB");
        assert_eq!(words.said(&Sentence::Until(1_793_491_199)), "· until 31 October");
        assert_eq!(words.said(&Sentence::Until(0)), "· until 1 January");
        assert_eq!(words.said(&Sentence::Until(951_782_400)), "· until 29 February");
    }

    #[test]
    fn talk_and_alerts_are_said_as_the_canvas_has_them() {
        let words = Words::english();
        let on = |day: i8, hour: i8| chat::DateTime::constant(2026, 11, day, hour, 6, 0, 0);
        let day = on(15, 0).date();
        let today = Some(day);
        assert_eq!(words.said(&Sentence::When { at: on(15, 21), today }), "21:06");
        assert_eq!(words.said(&Sentence::When { at: on(14, 21), today }), "Yesterday");
        assert_eq!(words.said(&Sentence::MadeOn { day, today }), "Today");
        assert_eq!(words.said(&Sentence::MadeOn { day: on(2, 9).date(), today }), "2 November");
        assert_eq!(words.said(&Sentence::LastReply { at: on(15, 21), today }), "last 21:06");
        assert_eq!(words.said(&Sentence::Replies(1)), "1 reply");
        assert_eq!(words.said(&Sentence::Replies(12)), "12 replies");
        assert_eq!(words.said(&Sentence::SearchNothing("xyzzy".into())), "Nothing found for “xyzzy”.");
        assert_eq!(words.said(&Sentence::SearchInThread("general".into())), "#general › thread");
        assert_eq!(words.said(&Sentence::SearchMessages("Owls".into())), "Search Owls");
        assert_eq!(
            words.said(&Sentence::PreviewWaiting("blog.example.invalid".into())),
            "Getting a preview of blog.example.invalid…"
        );
        assert_eq!(
            words.said(&Sentence::OpenPreview { title: "Game night".into(), host: "blog.example.invalid".into() }),
            "Open Game night, blog.example.invalid"
        );
        assert_eq!(words.said(&Sentence::NoLongerHere("Bo".into())), "Bo is no longer on this server.");
        assert_eq!(
            words.said(&Sentence::ConversationStart("Bo".into())),
            "This is the start of your conversation with Bo."
        );
        assert_eq!(words.said(&Sentence::MessagePerson("Bo".into())), "Message Bo");
        let tonight = Ends { at: on(15, 22), today: day };
        let morning = Ends { at: on(16, 8), today: day };
        assert_eq!(words.said(&Sentence::StatusUntil(tonight)), "Until 22:06");
        assert_eq!(words.said(&Sentence::StatusUntil(morning)), "Until tomorrow 08:06");
        assert_eq!(
            words.said(&Sentence::DoNotDisturbOn(Some(morning))),
            "On until tomorrow 08:06: no popups or sounds on any of your devices."
        );
        assert_eq!(
            words.said(&Sentence::DoNotDisturbOn(Some(tonight))),
            "On until 22:06: no popups or sounds on any of your devices."
        );
        assert_eq!(words.said(&Sentence::DoNotDisturbOn(None)), "On: no popups or sounds on any of your devices.");
        assert_eq!(
            words.said(&Sentence::ShareRateOutOfRange { least: 500_000, most: 40_000_000, step: 500_000 }),
            "Pick a rate from 0.5 to 40 Mbit/s, in steps of 0.5."
        );
        assert_eq!(
            words.said(&Sentence::ShareRateAtMost(2_500_000)),
            "At most 2.5 Mbit/s; less when the screen or your connection needs less."
        );
        let from = "3840 \u{d7} 2160 \u{b7} 60 fps".to_string();
        assert_eq!(
            words.said(&Sentence::ReducedByCeiling { from: from.clone(), bits_per_second: 4_000_000 }),
            "Reduced from 3840 \u{d7} 2160 \u{b7} 60 fps: 4 Mbit/s limit"
        );
        assert_eq!(
            words.said(&Sentence::ReducedByContent { from: "60 fps".into(), bits_per_second: 8_000_000 }),
            "Reduced from 60 fps: content exceeds 8 Mbit/s"
        );
        assert_eq!(
            words.said(&Sentence::ReducedByConnection("60 fps".into())),
            "Reduced from 60 fps: content exceeds the connection"
        );
        assert_eq!(
            words.said(&Sentence::ReducedByEncoder(from)),
            "Reduced from 3840 \u{d7} 2160 \u{b7} 60 fps: encoder overloaded"
        );
        let title = Sentence::PopupTitle { author: "Bo".into(), server: "Night Owls".into() };
        assert_eq!(words.said(&title), "Bo in Night Owls");
        assert_eq!(words.said(&Sentence::Plain("Bo".into())), "Bo");
        let body = |sentence| words.said(&sentence);
        assert_eq!(body(Sentence::PopupChannel { channel: "general".into(), text: "hi".into() }), "#general · hi");
        assert_eq!(
            body(Sentence::PopupDirect { server: "Night Owls".into(), text: "hi".into() }),
            "Night Owls · direct message: hi"
        );
        assert_eq!(
            body(Sentence::PopupThread { channel: "general".into(), text: "hi".into() }),
            "Thread in #general · hi"
        );
        assert_eq!(words.phrase(Phrase::YouColon), "You:");
        assert_eq!(words.phrase(Phrase::ShowPopupsNote), "When a message would alert you and Chatkord isn’t in front.");
    }

    #[test]
    fn a_day_said_before_today_is_known_is_its_date_never_today_or_yesterday() {
        let words = Words::english();
        let on = |day: i8, hour: i8| chat::DateTime::constant(2026, 11, day, hour, 6, 0, 0);
        let today = None;
        assert_eq!(words.said(&Sentence::Day { day: on(14, 0).date(), today }), "Saturday 14 November");
        assert_eq!(words.said(&Sentence::When { at: on(15, 21), today }), "Sunday 15 November");
        assert_eq!(words.said(&Sentence::LastReply { at: on(15, 21), today }), "last Sunday 15 November");
        assert_eq!(words.said(&Sentence::MadeOn { day: on(15, 0).date(), today }), "15 November");
        assert_eq!(words.said(&Sentence::PinnedAt { at: on(15, 21), today }), "Sunday 15 November · 21:06");
        let found = Sentence::FoundFile { size: 2048, place: "#general".into(), at: on(15, 21), today };
        assert!(words.said(&found).ends_with("· #general · Sunday 15 November"));
        for edge in [chat::Date::MAX, chat::Date::MIN] {
            for day in [Some(edge), edge.yesterday().ok(), edge.tomorrow().ok()].into_iter().flatten() {
                let said = words.said(&Sentence::Day { day, today });
                assert!(said != "Today" && said != "Yesterday", "{day} said {said}");
                assert_ne!(words.said(&Sentence::MadeOn { day, today }), "Today", "{day}");
            }
        }
    }

    #[test]
    fn a_member_s_row_names_their_username_and_place_and_banned_says_its_number_only_when_one_is() {
        let words = Words::english();
        let placed = |roles: &[&str], you| {
            let roles = roles.iter().map(|role| (*role).to_string()).collect();
            words.said(&Sentence::Placed { handle: "ada".into(), roles, you })
        };
        assert_eq!(placed(&["Owner"], true), "@ada · Owner · you");
        assert_eq!(placed(&["Regulars", "Game hosts", "Helpers"], false), "@ada · Regulars, Game hosts, Helpers");
        assert_eq!(placed(&[], true), "@ada · you");
        assert_eq!(placed(&[], false), "@ada");
        assert_eq!(words.said(&Sentence::BannedHandle("cy".into())), "@cy · Banned");
        assert_eq!(words.said(&Sentence::MembersCount(4)), "Members (4)");
        assert_eq!(words.said(&Sentence::BannedCount(1)), "Banned (1)");
        assert_eq!(words.said(&Sentence::BannedCount(0)), "Banned");
        assert_eq!(words.said(&Sentence::NoOneMatches("zz".into())), "No one matches “zz”.");
        assert_eq!(words.said(&Sentence::TooManyRoles), "A server holds at most 250 roles.");
    }

    #[test]
    fn a_private_channel_s_start_says_it_is_private_and_a_category_s_delete_what_it_takes() {
        let words = Words::english();
        assert_eq!(
            words.said(&Sentence::PrivateChannelStart("staff".into())),
            "This is the start of the private #staff channel."
        );
        assert_eq!(words.said(&Sentence::DeleteCategoryBody(0)), "It holds no channels, so only its name goes.");
        assert_eq!(words.said(&Sentence::DeleteCategoryBody(1)), "Its channel and its messages are deleted too.");
        assert_eq!(words.said(&Sentence::DeleteCategoryBody(4)), "Its 4 channels and their messages are deleted too.");
        assert_eq!(words.said(&Sentence::MembersOnline(3)), "Online — 3");
        assert_eq!(
            words.said(&Sentence::NotMutedLeft("grace".into())),
            "grace couldn’t be muted for everyone: they left the call."
        );
    }

    #[test]
    fn matching_the_system_names_the_language_it_asks_for() {
        let words = Words::english();
        assert_eq!(words.said(&Sentence::MatchingSystem("English".into())), "Match system (English)");
    }

    #[test]
    fn an_invite_s_terms_name_one_person_or_more_its_day_and_its_note() {
        let words = Words::english();
        let terms = |uses, note: Option<&str>| {
            let role = "Regulars".to_string();
            words.said(&Sentence::InviteTerms { uses, role, until: 1_793_491_200, note: note.map(str::to_string) })
        };
        assert_eq!(terms(5, Some("for Grace")), "5 people · Regulars · until 1 November · for Grace");
        assert_eq!(terms(1, None), "1 person · Regulars · until 1 November");
        assert_eq!(words.said(&Sentence::UsesOutOfRange), "Pick a number from 1 to 100.");
        assert_eq!(words.said(&Sentence::Ends(1_793_491_200)), "1 November");
        assert_eq!(words.said(&Sentence::Used { used: 2, of: 5 }), "2 of 5");
        assert_eq!(words.said(&Sentence::ForDays(1)), "1 day");
        assert_eq!(words.said(&Sentence::TooManyInvites), "There are already 100 working invites. Delete one first.");
        assert_eq!(words.said(&Sentence::TooManyChannels), "There are already 500 channels. Delete one first.");
    }

    #[test]
    fn a_device_that_goes_and_comes_back_is_said_by_name() {
        let words = Words::english();
        let gone = |now: Option<&str>| Sentence::SoundGone { was: "Headset".into(), now: now.map(str::to_string) };
        assert_eq!(words.said(&gone(Some("Laptop"))), "“Headset” isn’t connected, so Chatkord uses “Laptop”.");
        assert_eq!(words.said(&gone(None)), "“Headset” isn’t connected, so Chatkord uses the default.");
        assert_eq!(words.said(&Sentence::SoundBack("Headset".into())), "“Headset” is connected again.");
    }

    #[test]
    fn each_ending_has_its_own_sentence_and_the_rest_ran_out() {
        let words = Words::english();
        let said = |reason| words.said(&Sentence::Ended { server: "Book club".into(), reason });
        let ran_out = said(None);
        for reason in [
            CloseReason::SignedOut,
            CloseReason::PasswordChanged,
            CloseReason::PasswordReset,
            CloseReason::Kicked,
            CloseReason::AccountDeleted,
            CloseReason::Banned,
        ] {
            assert!(said(Some(reason)) != ran_out, "{reason:?}");
        }
        assert_eq!(said(Some(CloseReason::SessionExpired)), ran_out);
    }

    #[test]
    fn a_refused_hosting_number_says_which_way_it_missed() {
        let words = Words::english();
        let said = |refused, typed, within| words.said(&Sentence::hosting_refused(refused, typed, within));
        let files = HostingField::AccountFilesMax;
        assert_eq!(said(HostingRefused::OutOfRange(HostingField::AttachmentMax), 0, 0), "At least 1 MB.");
        assert_eq!(said(HostingRefused::OutOfRange(HostingField::AttachmentMax), 8192, 0), "No more than 4,096 MB.");
        assert_eq!(said(HostingRefused::OutOfRange(files), 2_000_000, 0), "No more than 1,048,576 MB.");
        let above = HostingRefused::Above { field: HostingField::AttachmentMax, within: files };
        assert_eq!(said(above, 2000, 1024), "No more than the per-person storage, 1,024 MB.");
        assert_eq!(words.said(&Sentence::Phrase(Phrase::RefusedStoreFull)), "The server’s disk is full.");
    }

    #[test]
    fn a_freeze_writes_its_seconds_with_the_catalog_s_decimal() {
        let words = Words::new(Language::given(
            "nl",
            "Test",
            "language-name = Test\nfigure-decimal = {\",\"}\nfigure-group = {\".\"}\nfroze = { $times }x, { $seconds } s\n",
        ));
        let froze = Sentence::Froze { times: 2, longest: std::time::Duration::from_millis(2_040) };
        assert_eq!(words.said(&froze), "2x, 2,0 s");
    }
}
