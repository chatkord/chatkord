//! The icons: a subset of Lucide as SVG, drawn in one colour of a role. An icon is never the only name of a control:
//! a control that is only an icon carries a label and shows it in a tooltip.

use crate::Theme;
use design::{IconSize, Rgb, Role};
use iced_core::Length;
use iced_widget::svg::{self, Handle};
use std::sync::LazyLock;

design::listed! {
    /// An icon of the set, drawn by [`icon`]: what it stands for is said where it is drawn, never by it alone.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum Icon {
        /// The microphone, open: the mute control while one is heard.
        Mic,
        /// The microphone, muted: the mute control while muted, and a muted member's mark.
        MicOff,
        /// Hearing the call: the deafen control while one hears, and a voice place's mark on the rail.
        Headphones,
        /// Deafened: the deafen control while deafened, and a deafened member's mark.
        HeadphonesOff,
        /// The server's channels: the button that opens the pane's drawer while the pane is folded.
        PanelLeft,
        /// A voice channel, beside its name.
        Volume,
        /// Sound off, in the set for a person's volume.
        VolumeOff,
        /// A gauge at its end: this computer cannot keep up with a stream.
        Gauge,
        /// A chip: a picture made or taken apart by the graphics hardware.
        Cpu,
        /// Code brackets: a picture made or taken apart by the processor.
        Code,
        /// Pause watching a stream, on a card's bar.
        Pause,
        /// Resume watching a paused stream, in Pause's place.
        Play,
        /// Leave the call.
        PhoneOff,
        /// Share a screen.
        ScreenShare,
        /// Stop one's own share.
        ScreenShareOff,
        /// A camera: the call bar's button, and a person whose camera is on in the side pane.
        Video,
        /// A camera turned off: the call bar's button while it is.
        VideoOff,
        /// A screen being shared: a share's card, its seat and its panel.
        Monitor,
        /// Pin a screen to fill the stage.
        Pin,
        /// Unpin the screen that fills the stage.
        PinOff,
        /// A watched screen over the whole display.
        Maximize,
        /// Leave the full screen.
        Minimize,
        /// A watched screen in a window of its own, or back in its card.
        PictureInPicture,
        /// Zoom on a watched screen shown large.
        ZoomIn,
        /// Back to the whole of a zoomed screen.
        ZoomOut,
        /// From the floating card back to its call.
        GoToCall,
        /// The floating card back to where it starts.
        ResetPosition,
        /// The way back to the list a conversation or a thread was opened from.
        ArrowLeft,
        /// Follow a thread: its header's button while it is not followed.
        Bell,
        /// Stop following a thread: its header's button while it is followed.
        BellOff,
        /// The threads, beside their row in the side pane and over their list.
        MessagesSquare,
        /// A text channel, in the set for chat.
        Hash,
        /// A category, on the empty state that says channels live in categories.
        Folder,
        /// A private text channel: the hash with a small lock (not one of Lucide's).
        HashLock,
        /// A private voice channel: the speaker with a small lock (not one of Lucide's).
        VolumeLock,
        /// Opens what lies under a control: a menu or a device list; under a pinned card, puts the strip away.
        ChevronDown,
        /// Under a pinned card, shows the strip again.
        ChevronUp,
        /// Back to where a page was opened from, as a list's page shown alone.
        ChevronLeft,
        /// Leads on to more, in the set for a nested menu.
        ChevronRight,
        /// Closes a dialog, a notice or the settings.
        Close,
        /// Adds: a server on the rail, a step of zoom.
        Plus,
        /// Takes away: a step of zoom.
        Minus,
        /// Edits a value by hand: the colour picker's entry.
        Pencil,
        /// Sends what the composer holds.
        Send,
        /// Search, in the set for chat.
        Search,
        /// Opens the settings.
        Settings,
        /// Members, in the set for the members sidebar.
        Users,
        /// One person, in the set for a profile.
        User,
        /// Sign out, in the set for the account.
        LogOut,
        /// The chosen row of a device menu.
        Check,
        /// An error, in the set for a notice.
        CircleAlert,
        /// A warning, in the set for a notice.
        TriangleAlert,
        /// About: a share's figures.
        Info,
        /// Work under way, turned by the spinner.
        Spinner,
        /// Try again, in the set for a failed load.
        Refresh,
        /// The connection is lost.
        WifiOff,
        /// Shows a hidden password.
        Eye,
        /// Hides a shown password.
        EyeOff,
        /// An update to install.
        Download,
        /// What may not be changed: a role at or above one's own place.
        Lock,
        /// What a drag moves: a role that may change its place.
        Grip,
        /// More of what may be done: a member's other actions.
        More,
        /// The connection's four bars, as the canvas draws them (not one of Lucide's).
        Signal,
        /// The product's own mark (four bars of a voice), not one of Lucide's.
        Logo,
        /// Attach a file to a message, and the drop that attaches one.
        Paperclip,
        /// The emoji picker's way in, and its group of faces.
        Smile,
        /// React to a message.
        SmilePlus,
        /// Reply to a message.
        Reply,
        /// A call's chat: the button ending the top bar, or a folded call's header, that opens its channel's messages beside it.
        MessageSquare,
        /// A file that is not a picture.
        File,
        /// A picture sent as a file, where it is listed by its name.
        Image,
        /// Delete what a row holds: a server's emoji.
        Trash,
        /// Upload a file of one's own: a server's emoji.
        Upload,
        /// A mention: a reply that mentions its author, and @everyone and @here in the chooser.
        AtSign,
        /// The emoji picked lately.
        Clock,
        /// The emoji picker's people and body.
        Hand,
        /// The emoji picker's animals and nature.
        Leaf,
        /// The emoji picker's food and drink.
        Apple,
        /// The emoji picker's activities.
        Dice,
        /// The emoji picker's objects.
        Lightbulb,
        /// The emoji picker's travel and places.
        Plane,
        /// The emoji picker's symbols.
        Heart,
        /// The emoji picker's flags.
        Flag,
        /// The language field: the language the window is said in.
        Globe,
    }
}

/// Each icon's file, named once: the name and the bytes compiled in are both made from it, and the match over
/// every variant leaves none without its file.
macro_rules! files {
    ($($icon:ident => $stem:literal,)+) => {
        impl Icon {
            /// The file the icon is, without its extension.
            pub const fn file_stem(self) -> &'static str {
                match self {
                    $(Icon::$icon => $stem,)+
                }
            }

            /// The SVG file, compiled in.
            pub(crate) const fn bytes(self) -> &'static [u8] {
                match self {
                    $(Icon::$icon => include_bytes!(concat!("../assets/icons/", $stem, ".svg")),)+
                }
            }
        }
    };
}

files! {
    Mic => "mic",
    MicOff => "mic-off",
    Headphones => "headphones",
    HeadphonesOff => "headphone-off",
    PanelLeft => "panel-left",
    Volume => "volume-2",
    VolumeOff => "volume-x",
    Gauge => "gauge",
    Cpu => "cpu",
    Code => "code",
    Pause => "pause",
    Play => "play",
    PhoneOff => "phone-off",
    ScreenShare => "screen-share",
    ScreenShareOff => "screen-share-off",
    Video => "video",
    VideoOff => "video-off",
    Monitor => "monitor",
    Pin => "pin",
    PinOff => "pin-off",
    Maximize => "maximize",
    Minimize => "minimize",
    PictureInPicture => "picture-in-picture-2",
    ZoomIn => "zoom-in",
    ZoomOut => "zoom-out",
    GoToCall => "arrow-down-left",
    ResetPosition => "rotate-ccw",
    ArrowLeft => "arrow-left",
    Bell => "bell",
    BellOff => "bell-off",
    MessagesSquare => "messages-square",
    Hash => "hash",
    Folder => "folder",
    HashLock => "hash-lock",
    VolumeLock => "volume-lock",
    ChevronDown => "chevron-down",
    ChevronUp => "chevron-up",
    ChevronLeft => "chevron-left",
    ChevronRight => "chevron-right",
    Close => "x",
    Plus => "plus",
    Minus => "minus",
    Pencil => "pencil",
    Send => "send-horizontal",
    Search => "search",
    Settings => "settings",
    Users => "users",
    User => "user",
    LogOut => "log-out",
    Check => "check",
    CircleAlert => "circle-alert",
    TriangleAlert => "triangle-alert",
    Info => "info",
    Spinner => "loader-circle",
    Refresh => "rotate-cw",
    WifiOff => "wifi-off",
    Eye => "eye",
    EyeOff => "eye-off",
    Download => "download",
    Lock => "lock",
    Grip => "grip-vertical",
    More => "ellipsis",
    Signal => "signal",
    Logo => "chatkord-mark",
    Paperclip => "paperclip",
    Smile => "smile",
    SmilePlus => "smile-plus",
    Reply => "reply",
    MessageSquare => "message-square",
    File => "file",
    Image => "image",
    Trash => "trash",
    Upload => "upload",
    AtSign => "at-sign",
    Clock => "clock",
    Hand => "hand",
    Leaf => "leaf",
    Apple => "apple",
    Dice => "dice-5",
    Lightbulb => "lightbulb",
    Plane => "plane",
    Heart => "heart",
    Flag => "flag",
    Globe => "globe",
}

/// One handle per icon, made once: the renderer keeps what it rasterised under the handle.
static HANDLES: LazyLock<[Handle; Icon::ALL.len()]> =
    LazyLock::new(|| Icon::ALL.map(|icon| Handle::from_memory(icon.bytes())));

/// The colour an icon is drawn in: a role, or a colour already resolved (a server's own).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconClass {
    /// The role's colour in the theme's palette, so it follows the scheme and the contrast.
    Role(Role),
    /// A colour already resolved, painted as given.
    Rgb(Rgb),
}

/// The same, stroked with [`design::ICON_HEAVY_STROKE`] for the sizes that ask for it.
static HEAVY: LazyLock<[Handle; Icon::ALL.len()]> = LazyLock::new(|| Icon::ALL.map(heavy));

fn heavy(icon: Icon) -> Handle {
    let stroke = format!("stroke-width=\"{}\"", design::ICON_HEAVY_STROKE);
    Handle::from_memory(String::from_utf8_lossy(icon.bytes()).replace("stroke-width=\"2\"", &stroke).into_bytes())
}

/// The picture of `icon`, loaded once.
pub(crate) fn handle(icon: Icon) -> Handle {
    HANDLES[icon as usize].clone()
}

/// `icon` at `size` in `class`.
pub fn icon<'a>(icon: Icon, size: IconSize, class: IconClass) -> svg::Svg<'a, Theme> {
    let handle = if size.heavy() { HEAVY[icon as usize].clone() } else { handle(icon) };
    svg::Svg::new(handle).width(Length::Fixed(size.px())).height(Length::Fixed(size.px())).class(class)
}

impl svg::Catalog for Theme {
    type Class<'a> = IconClass;

    fn default<'a>() -> Self::Class<'a> {
        IconClass::Role(Role::Content(design::Content::Primary))
    }

    fn style(&self, class: &Self::Class<'_>, _status: svg::Status) -> svg::Style {
        svg::Style {
            color: Some(match class {
                IconClass::Role(role) => self.color(*role),
                IconClass::Rgb(rgb) => crate::color::color(*rgb),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_an_svg_at_its_place() {
        for (at, icon) in Icon::ALL.into_iter().enumerate() {
            assert_eq!(icon as usize, at, "{icon:?}");
            assert!(icon.bytes().starts_with(b"<svg"), "{icon:?}");
        }
    }
}
