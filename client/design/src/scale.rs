//! Sizes and times, in logical pixels and durations: the host multiplies by the window's scale
//! factor, so a token means the same size on every screen.

use crate::listed;
use crate::role::Surface;
use crate::scheme::Scheme;

/// The smallest target a pointer or a finger is asked to hit.
pub const HIT_TARGET: f32 = 32.0;
/// A glyph button on a card in the strip under a pinned one, smaller than a hit target as Sharkord's strip draws it,
/// so a strip card's bar keeps every control.
pub const SMALL_TARGET: f32 = 24.0;

/// The widest a form is made, so a line of it stays easy to read and a sign-in is not a wall of fields.
pub const FORM_WIDTH: f32 = 420.0;

/// A state line's height: one line across the foot of the area it is about.
pub const STATE_LINE_HEIGHT: f32 = 32.0;
/// The dot of a state line, a toast and a callout, in the colour of their kind.
pub const STATUS_DOT: f32 = 8.0;
/// The room inside a callout's edge.
pub const CALLOUT_PAD: f32 = Space::M.px();
/// What a one-line composer takes at the foot of a channel, with its gutters, which a toast floats above.
pub const COMPOSER_ROOM: f32 = CONTROL_HEIGHT + 2.0 * Space::L.px() + Space::S.px();
/// The widest a toast grows, so a long sentence wraps rather than crossing a window.
pub const TOAST_MOST: f32 = 720.0;
/// The widest a centre panel's column is.
pub const CENTRE_WIDTH: f32 = 420.0;
/// A centre panel's round mark.
pub const CENTRE_MARK: f32 = 64.0;
/// The round mark of a column's smaller empty state.
pub const CENTRE_MARK_SMALL: f32 = 40.0;
/// The badge on a control of a call that failed.
pub const FAULT_BADGE: f32 = 14.0;

/// A text field's and a button's height; an icon-only button is as wide.
pub const CONTROL_HEIGHT: f32 = 40.0;

/// The widest a dialog is made.
pub const DIALOG_WIDTH: f32 = 480.0;
/// The room between a dialog's heading, the lines of its body and its buttons.
pub const DIALOG_GAP: f32 = Space::M.px() + Space::Xxs.px();
/// A menu's width.
pub const MENU_WIDTH: f32 = 260.0;
/// A submenu's width, beside the item that opens it.
pub const SUBMENU_WIDTH: f32 = 240.0;
/// The dot beside a channel with unread messages.
pub const UNREAD_DOT: f32 = 8.0;
/// The accent bar down the left of a message that calls the reader.
pub const CALLED_BAR: f32 = 2.0;
/// A count of mentions on its pill: its height.
pub const MENTION_BADGE: f32 = 18.0;
/// A menu item's height.
pub const MENU_ITEM_HEIGHT: f32 = 34.0;
/// The server rail at the window's edge.
pub const RAIL_WIDTH: f32 = 60.0;
/// The server pane beside the rail at its narrowest, and as it starts: what its account strip's controls need.
pub const PANE_LEAST: f32 = 248.0;
/// The widest the server pane may be dragged.
pub const PANE_MOST: f32 = 400.0;
/// The narrowest the content is left beside the rail and a docked pane: narrower, the pane folds into a drawer.
pub const CONTENT_LEAST: f32 = 360.0;
/// The members column beside the content as it starts, and after a reset.
pub const MEMBERS_WIDTH: f32 = 240.0;
/// The narrowest the members column may be dragged.
pub const MEMBERS_LEAST: f32 = 200.0;
/// The widest the members column may be dragged.
pub const MEMBERS_MOST: f32 = 360.0;
/// A field that sits at the end of a setting's row, as the Do not disturb choice does.
pub const SETTING_FIELD_WIDTH: f32 = 190.0;
/// The field of a custom value beside its slider: an invite's number of people, a share's bitrate.
pub const CUSTOM_FIELD_WIDTH: f32 = 72.0;
/// The room a field keeps clear at its end for the unit laid over it: "Verbindungen" at the small size, padded.
pub const FIELD_UNIT_ROOM: f32 = 104.0;
/// A field of a page's grid of them, two to a settings list's width.
pub const GRID_FIELD_WIDTH: f32 = (SETTINGS_LIST_WIDTH - Space::L.px()) / 2.0;
/// The thread beside the channel in the content area, as the canvas has it.
pub const THREAD_WIDTH: f32 = 360.0;
/// The least room a channel keeps beside its thread: with less, the members column goes while the thread is open, and
/// where the thread alone fits it covers the channel.
pub const THREAD_CHANNEL_LEAST: f32 = 420.0;
/// A call's chat column beside it as it starts, after a reset, and at its narrowest, as Sharkord's.
pub const CALL_CHAT_LEAST: f32 = 360.0;
/// The widest a call's chat column may be dragged, as Sharkord's.
pub const CALL_CHAT_MOST: f32 = 600.0;
/// The dot on a call header's Chat button while its channel holds unread messages, and the ring of ground round it.
pub const CALL_CHAT_DOT: f32 = 8.0;
/// The ring of ground round [`CALL_CHAT_DOT`], parting it from the button.
pub const CALL_CHAT_DOT_RING: f32 = 2.0;
/// The room either side of a text channel's messages and composer; [`MESSAGE_SIDE_NARROW`] in a narrow content.
pub const MESSAGE_SIDE: f32 = 20.0;
/// The same, where the content is narrower than [`MESSAGE_SIDE_NARROW_BELOW`].
pub const MESSAGE_SIDE_NARROW: f32 = 12.0;
/// The content's width under which a text channel's sides narrow.
pub const MESSAGE_SIDE_NARROW_BELOW: f32 = 480.0;
/// The space above and below each message, inside what its row measures.
pub const MESSAGE_PAD: f32 = 2.0;
/// The most lines the composer grows to before it scrolls.
pub const COMPOSER_ROWS: u16 = 8;
/// The grip over the rule between the pane and the content that a pointer drags to resize the pane.
pub const SPLITTER_GRIP: f32 = 8.0;
/// The accent line a drag draws where it would land: down a splitter's grip while held, across a list's gap.
pub const DRAG_LINE: f32 = 2.0;
/// A dashed edge's dash and the gap after it, as the rail's add tile draws them: an add button's edge.
pub const DASH: [f32; 2] = [4.0, 3.0];
/// How far the pointer moves with the button held before a press on a row becomes a drag of it.
pub const DRAG_START: f32 = 4.0;
/// How near a list's top or bottom edge a drag held there scrolls the list toward it.
pub const DRAG_EDGE: f32 = 32.0;
/// How fast a drag held at a list's edge scrolls it, in pixels a second; slower the farther from the edge.
pub const DRAG_SCROLL: f32 = 600.0;
/// The least window, inside the frame the system draws, in logical pixels before the zoom.
pub const WINDOW_LEAST: (f32, f32) = (360.0, 294.0);
/// The pane's header (the server's name) and the content's header.
pub const PANE_HEADER_HEIGHT: f32 = 48.0;
/// The account strip under the server pane.
pub const ACCOUNT_STRIP_HEIGHT: f32 = 52.0;
/// The list of a server's settings pages.
pub const SETTINGS_NAV_WIDTH: f32 = 220.0;
/// A settings page's pill in that list.
pub const NAV_ITEM_HEIGHT: f32 = 34.0;

/// A switch's track.
pub const SWITCH_WIDTH: f32 = 42.0;
/// The switch's track height.
pub const SWITCH_HEIGHT: f32 = 24.0;
/// The knob on it.
pub const SWITCH_KNOB: f32 = 20.0;
/// The room round a switch's knob: what the knob leaves of the track's height.
pub const SWITCH_PAD: f32 = (SWITCH_HEIGHT - SWITCH_KNOB) / 2.0;

/// A slider's rail, the line its handle runs along.
pub const SLIDER_RAIL: f32 = 4.0;
/// The round handle on it.
pub const SLIDER_HANDLE: f32 = 16.0;
/// A slider's height, its handle and the room a pointer takes it in.
pub const SLIDER_HEIGHT: f32 = 24.0;
/// The figure beside a volume's slider, room for "100 %".
pub const VOLUME_FIGURE: f32 = 44.0;

/// A radio mark's ring.
pub const RADIO_SIZE: f32 = 16.0;
/// The thickness of a radio mark's ring.
pub const RADIO_RING: f32 = 2.0;
/// A check box's side, beside a choice of which any are taken.
pub const CHECK_BOX: f32 = 16.0;
/// The edge of a check box not ticked.
pub const CHECK_BOX_EDGE: f32 = 1.5;
/// A check box's corners: rounded, never so far as to read as a radio's circle.
pub const CHECK_BOX_RADIUS: Radius = Radius::S;
/// The dot a chosen radio mark holds.
pub const RADIO_DOT: f32 = 6.0;

/// The column of cards on a settings page made of cards: this device's pages and a server's About.
pub const DEVICE_COLUMN_WIDTH: f32 = 440.0;
/// The camera's preview on the voice settings page: 16:9 across that column, inside its card's row and padding.
pub const CAMERA_PREVIEW_HEIGHT: f32 =
    (DEVICE_COLUMN_WIDTH - 2.0 * (Space::Xs.px() + Space::M.px() + Space::Xxs.px())) * 9.0 / 16.0;
/// The bar that shows how loud the microphone is.
pub const LEVEL_BAR_HEIGHT: f32 = 8.0;
/// The mark on that bar where the noise gate opens.
pub const LEVEL_MARK_WIDTH: f32 = 2.0;
/// The figure beside a slider in dB, wide enough for "-70 dB" in the mono face.
pub const DECIBEL_FIGURE: f32 = 52.0;
/// The circle that turns while something is waited for.
pub const SPINNER_SIZE: f32 = 28.0;
/// A list a settings page holds on its own, such as the members.
pub const SETTINGS_LIST_WIDTH: f32 = 620.0;
/// The most of a licence's text About Chatkord shows before it scrolls.
pub const LICENCE_TEXT_HEIGHT: f32 = 220.0;
/// The most of a debug report the dialog lists before it scrolls.
pub const REPORT_TEXT_HEIGHT: f32 = 320.0;

/// The swatch that opens the colour picker.
pub const COLOUR_SWATCH: (f32, f32) = (64.0, 40.0);
/// The colour picker's area of saturation and brightness.
pub const COLOUR_PICKER_AREA: (f32, f32) = (240.0, 150.0);
/// The hue strip under that area.
pub const COLOUR_PICKER_HUE_HEIGHT: f32 = 14.0;
/// The round handle on the area and on the strip.
pub const COLOUR_PICKER_HANDLE: f32 = 14.0;
/// The thickness of the ring round that handle.
pub const COLOUR_PICKER_HANDLE_RING: f32 = 2.0;
/// The panel that holds the colour picker.
pub const COLOUR_PICKER_WIDTH: f32 = 268.0;

/// The zoom's percent between its − and +, so the buttons stay put as it changes.
pub const ZOOM_FIGURE: f32 = 56.0;
/// The least width a row's name keeps beside its buttons; narrower, the buttons go on a line under it.
pub const ROW_WORDS_LEAST: f32 = 160.0;
/// One kind's list on the channels page.
pub const CHANNEL_LIST_WIDTH: f32 = 440.0;
/// The list of a server's roles on its Roles page, beside the editor of the role chosen.
pub const ROLE_LIST_WIDTH: f32 = 280.0;
/// A panel in the content area: a server's sign-in, its connecting and not-answering notices.
pub const PANEL_WIDTH: f32 = 380.0;

/// The bar at the open server's edge on the rail.
pub const RAIL_BAR: (f32, f32) = (4.0, 24.0);
/// The bar's height beside an item the pointer, or the keyboard's focus, is on.
pub const RAIL_BAR_HOVER: f32 = 8.0;
/// The dot on a server that needs a look.
pub const RAIL_DOT: f32 = 10.0;
/// A role's colour as a dot: in the list of roles, in a member's Roles menu and at the head of the role's editor.
pub const COLOUR_DOT: f32 = 10.0;
/// The mark on the picture of the server this window's call is on.
pub const RAIL_CALL_MARK: f32 = 18.0;
/// How faint a server not signed in is drawn.
pub const RAIL_DIM: f32 = 0.45;
/// How faint a member who is offline is drawn in the members column.
pub const OFFLINE_DIM: f32 = 0.5;
/// The dot that says a person's status, one size wherever it is: beside a word in their popover and the status menu,
/// and on their picture ([`PRESENCE_PICTURE`]), flush with its bottom right corner.
pub const PRESENCE_DOT: f32 = 10.0;
/// A person's picture wherever it carries their status dot (the members column, a conversation's row, the account
/// bar): one size, so the picture, the dot and its place on it are the same on every one.
pub const PRESENCE_PICTURE: AvatarSize = AvatarSize::M;
/// The disc cut from an away dot's top left, as a share of the dot's width: a crescent, so away is told by its shape as
/// well as its colour.
pub const AWAY_NOTCH_SHARE: f32 = 0.8;
/// How far up and left of the dot's centre the away notch's centre is, as a share of the dot's width.
pub const AWAY_NOTCH_AT_SHARE: f32 = 0.2;
/// The bar across a do not disturb dot, its width and height as shares of the dot's width, in its middle.
pub const DO_NOT_DISTURB_BAR_SHARE: (f32, f32) = (0.6, 0.2);

/// The room a field leaves at its end for a small button inside it (Paste).
pub const INLINE_ACTION_WIDTH: f32 = 72.0;
/// The welcome window's side panel: the logo, the name and what the product is.
pub const BRAND_PANE_WIDTH: f32 = 300.0;
/// The narrowest a person's tile in a call is worth drawing; below it the grid scrolls instead.
pub const TILE_LEAST: f32 = 160.0;

/// A call's round control, in the call bar and in the side pane (a hit target).
pub const CALL_BUTTON: f32 = 40.0;
/// The call bar's Leave, wider than the other controls.
pub const LEAVE_WIDTH: f32 = 58.0;
/// The arrow beside the call bar's mute, which opens the lists of microphones and speakers.
pub const CALL_ARROW_WIDTH: f32 = 28.0;
/// The menu of microphones and speakers.
pub const DEVICE_MENU_WIDTH: f32 = 300.0;
/// The hairline between groups of controls in the call bar.
pub const CALL_BAR_DIVIDER_HEIGHT: f32 = 28.0;
/// The hairline between a call pane's channel and its count.
pub const CALL_HEADER_DIVIDER_HEIGHT: f32 = 18.0;
/// The column a watched share's card sits in.
pub const SHARE_CARD_WIDTH: f32 = 300.0;
/// Where a name's pill sits from the corner of a person's tile or a screen's card.
pub const PILL_INSET: f32 = 10.0;
/// The connection popover over the call panel.
pub const STATS_POPOVER_WIDTH: f32 = 300.0;
/// The call panel in the side pane: its two lines of words, or a control if that is taller, between its padding. Fixed,
/// so what hangs over it (the connection popover) is placed from tokens alone.
pub const CALL_PANEL_HEIGHT: f32 =
    (TypeStyle::SmallStrong.line_height() + TypeStyle::Caption.line_height()).max(HIT_TARGET) + 2.0 * Space::S.px();
/// The call bar's padding, and the gap between its controls.
pub const STAGE_BAR_PAD: f32 = 6.0;
/// The room the stage leaves under itself for the call bar.
pub const STAGE_BAR_ROOM: f32 = 88.0;

/// The least a pinned card is left tall over the strip: shorter, the strip folds away by itself.
pub const STAGE_LEAST_HEIGHT: f32 = 160.0;
/// A person's tile in the strip under a watched screen.
pub const STRIP_TILE: (f32, f32) = (132.0, 76.0);
/// The floating card that keeps a pinned screen playing while its call is out of view.
pub const FLOAT_CARD: (f32, f32) = (320.0, 180.0);
/// Where the floating card starts, and goes back to on Reset position: this far from the right and bottom edges.
pub const FLOAT_INSET: f32 = 20.0;
/// A watched screen's own window, at its least: as small as a corner of the display while still a picture.
pub const POP_OUT_LEAST: (f32, f32) = (160.0, 90.0);
/// That window as it opens, at its largest: a quarter of the display's width, never past this.
pub const POP_OUT_MOST: (f32, f32) = (960.0, 540.0);
/// The edge of that window, which has no frame, that resizes it; inside it a drag moves the window.
pub const POP_OUT_GRIP: f32 = 8.0;
/// The panel that offers to share a screen.
pub const SHARE_PANEL_WIDTH: f32 = 384.0;
/// The popover that opens from a person's card, and the room the call's other panels keep.
pub const INFO_PANEL_WIDTH: f32 = 280.0;
/// The popover about a screen's picture.
pub const ABOUT_POPOVER_WIDTH: f32 = 260.0;
/// How far under its card's top the popover about a screen's picture hangs.
pub const ABOUT_HANG: f32 = 56.0;
/// How far in from its card's end that popover's end is.
pub const ABOUT_END_INSET: f32 = 10.0;
/// The room kept between that popover and the call bar under it.
pub const ABOUT_BAR_GAP: f32 = 8.0;
/// Audio controls, the panel of every sound in a call.
pub const AUDIO_CONTROLS_WIDTH: f32 = 360.0;
/// Whose sound a row of Audio controls is: the name and a screen's glyph, cut past it.
pub const SOUND_NAME_WIDTH: f32 = 96.0;

/// Our own picker of what to share.
pub const PICKER_WIDTH: f32 = 720.0;
/// How tall the picker's grid of cards may be before it scrolls: the canvas's dialog is at most 620 tall.
pub const PICKER_GRID_HEIGHT: f32 = 418.0;
/// A card's corners in the picker.
pub const SOURCE_CARD_RADIUS: Radius = Radius::L;
/// The corners of the picture in a picker's card.
pub const SOURCE_STILL_RADIUS: Radius = Radius::M;
/// An application's icon beside its title in the picker.
pub const SOURCE_ICON: f32 = 20.0;
/// The corners of that icon.
pub const SOURCE_ICON_RADIUS: Radius = Radius::S;

/// Every scroll bar's clear rail.
pub const SCROLL_RAIL: f32 = 10.0;
/// The slim pill in the middle of a scroll bar's rail.
pub const SCROLLER: f32 = 4.0;

/// The row of a person listed under a voice channel.
pub const PERSON_ROW_HEIGHT: f32 = 28.0;
/// How far that row is set in.
pub const PERSON_ROW_INDENT: f32 = 26.0;

/// How long the pointer rests on something before its tooltip shows.
pub const TOOLTIP_DELAY: std::time::Duration = std::time::Duration::from_millis(600);
/// How long a plain toast stays, unless a server is picked or a menu opened first.
pub const TOAST_PLAIN_FOR: std::time::Duration = std::time::Duration::from_secs(4);
/// How long a toast with a status dot or an action stays, held while the pointer is on it.
pub const TOAST_NOTED_FOR: std::time::Duration = std::time::Duration::from_secs(10);
/// How long a full screen's controls and the pointer stay after the pointer or a key last moved, as Sharkord's do.
pub const FULL_SCREEN_CALM: std::time::Duration = std::time::Duration::from_secs(3);
/// How long a person's voice, or a server's connection, may be on its way before it says it is connecting: one as it
/// should go says nothing.
pub const CONNECTING_AFTER: std::time::Duration = std::time::Duration::from_secs(1);
/// How long "connecting" stays once shown: a state shown for less reads as a flicker.
pub const CONNECTING_HELD: std::time::Duration = std::time::Duration::from_millis(500);
/// How long the waiting circle takes to turn once.
pub const SPIN_TURN: std::time::Duration = std::time::Duration::from_secs(1);

/// An icon's stroke on its 24-unit grid at a size that is [`IconSize::heavy`], over Lucide's own 2.
pub const ICON_HEAVY_STROKE: f32 = 2.6;

listed! {
    /// How big an icon is drawn.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum IconSize {
        /// The glyph inside a mark on a picture: the rail's call mark.
        Mark,
        /// The alert circle before a refusal under a field.
        Field,
        /// The glyph before a line of a popover, in one column.
        Line,
        /// An icon beside a line of text, and an icon-only button's glyph.
        S,
        /// The call bar's controls.
        Bar,
        /// A button's icon beside its label.
        M,
        /// The mark on a card or a notice: a call's end, a server not answering.
        L,
    }
}

impl IconSize {
    /// The icon's side, in logical pixels.
    pub const fn px(self) -> f32 {
        match self {
            IconSize::Mark => 11.0,
            IconSize::Field => 12.0,
            IconSize::Line => 14.0,
            IconSize::S => 16.0,
            IconSize::Bar => 18.0,
            IconSize::M => 20.0,
            IconSize::L => 24.0,
        }
    }

    /// Whether it is drawn with [`ICON_HEAVY_STROKE`], so a glyph that small still reads.
    pub const fn heavy(self) -> bool {
        match self {
            IconSize::Mark | IconSize::Field | IconSize::Line => true,
            IconSize::S | IconSize::Bar | IconSize::M | IconSize::L => false,
        }
    }
}

listed! {
    /// How big a person's or a server's picture is.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum AvatarSize {
        /// A person beside a quoted line: a reply's author, who added a server's emoji.
        Mark,
        /// A person listed under a voice channel.
        Xs,
        /// A small picture beside a line of text.
        S,
        /// A person on a list's row (a server's members).
        Row,
        /// A step above a row's.
        M,
        /// A server's picture on the rail.
        Rail,
        /// The author of a run of messages, beside its first.
        Author,
        /// A server's picture on a panel about it.
        L,
        /// A person's own picture on their profile, beside their colour; a server's on Overview and in its set-up.
        Profile,
        /// A person on a tile in a call, and alone on it.
        Tile,
        /// A person alone in a call, on their tile.
        Alone,
    }
}

impl AvatarSize {
    /// The picture's side, in logical pixels.
    pub const fn px(self) -> f32 {
        match self {
            AvatarSize::Mark => 16.0,
            AvatarSize::Xs => 20.0,
            AvatarSize::S => 24.0,
            AvatarSize::Row => 28.0,
            AvatarSize::M => 32.0,
            AvatarSize::Rail | AvatarSize::Author => 40.0,
            AvatarSize::L => 48.0,
            AvatarSize::Profile => 56.0,
            AvatarSize::Tile => 72.0,
            AvatarSize::Alone => 104.0,
        }
    }

    /// The type its initials are set in: the largest that sits well inside the picture.
    pub const fn initials(self) -> TypeStyle {
        match self {
            AvatarSize::Mark | AvatarSize::Xs | AvatarSize::S => TypeStyle::Caption,
            AvatarSize::Row => TypeStyle::SmallStrong,
            AvatarSize::M => TypeStyle::BodyStrong,
            AvatarSize::Rail | AvatarSize::Author | AvatarSize::L => TypeStyle::Title,
            AvatarSize::Profile => TypeStyle::Heading,
            AvatarSize::Tile => TypeStyle::Display,
            AvatarSize::Alone => TypeStyle::DisplayLarge,
        }
    }
}

/// The width of a control's edge.
pub const BORDER_WIDTH: f32 = 1.0;

/// The width of an emphasised edge: a focused field's, a chosen card's, the ring round a rail picture or a presence
/// dot. Keyboard focus on any other control shows as its hover fill, with no ring.
pub const RING_WIDTH: f32 = 2.0;

/// A ring that glows: its width, and a halo of its own colour at `alpha` blurred over `blur`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glow {
    /// The ring's width, in logical pixels.
    pub width: f32,
    /// How far its halo spreads, in logical pixels.
    pub blur: f32,
    /// How strong its halo is: 0 is none, 255 the ring's own colour.
    pub alpha: u8,
}

/// Round someone who is speaking.
pub const SPEAKING_GLOW: Glow = Glow { width: 3.0, blur: 18.0, alpha: 115 };

/// A line graph over time: its plot, its line's width, the scale's column at the plot's right, and the row of marks
/// under it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trace {
    /// The plot's height, in logical pixels; it takes the width the scale's column leaves.
    pub plot: f32,
    /// The line's width, in logical pixels.
    pub stroke: f32,
    /// The scale's column at the plot's right, its numbers inset by [`Space::S`] less [`Space::Xxs`].
    pub scale: f32,
    /// The marks' row under the plot, the gap above it included.
    pub marks: f32,
}

/// The round trip of the last minutes, in the connection popover, as the canvas draws Discord's: a 64 px plot, a
/// 1.5 px line, a 36 px scale and a 20 px row of minutes.
pub const ROUND_TRIP_TRACE: Trace = Trace { plot: 64.0, stroke: 1.5, scale: 36.0, marks: 20.0 };

/// Gaps and padding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Space {
    /// The smallest gap: between lines of one small group, or added to a larger step.
    Xxs,
    /// The second step.
    Xs,
    /// The third step.
    S,
    /// The fourth step.
    M,
    /// The fifth step.
    L,
    /// The sixth step.
    Xl,
    /// The widest gap.
    Xxl,
}

impl Space {
    /// The gap, in logical pixels.
    pub const fn px(self) -> f32 {
        match self {
            Space::Xxs => 2.0,
            Space::Xs => 4.0,
            Space::S => 8.0,
            Space::M => 12.0,
            Space::L => 16.0,
            Space::Xl => 24.0,
            Space::Xxl => 32.0,
        }
    }
}

/// Corner rounding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Radius {
    /// Square corners.
    None,
    /// The least rounding.
    S,
    /// The second step.
    M,
    /// The third step.
    L,
    /// The fourth step.
    Xl,
    /// The most rounding short of a pill.
    Xxl,
    /// A pill or a circle: half the shorter side, whatever the size.
    Full,
}

impl Radius {
    /// The radius, in logical pixels.
    pub const fn px(self) -> f32 {
        match self {
            Radius::None => 0.0,
            Radius::S => 6.0,
            Radius::M => 10.0,
            Radius::L => 14.0,
            Radius::Xl => 16.0,
            Radius::Xxl => 20.0,
            // Renderers clamp a radius to half the shorter side.
            Radius::Full => 9999.0,
        }
    }
}

listed! {
    /// How heavy a text is set.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum Weight {
        /// Running text.
        Regular,
        /// A field's name.
        Medium,
        /// Stressed text and titles.
        SemiBold,
        /// Between semi-bold and extra-bold.
        Bold,
        /// The product's name and the largest initials.
        ExtraBold,
    }
}

/// Which typeface a text is set in: the one for words, or the monospaced one for numbers and addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    /// For words.
    Sans,
    /// Monospaced, for numbers and addresses.
    Mono,
}

listed! {
    /// The type scale. Every text a screen shows is one of these.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum TypeStyle {
        /// The smallest text: a badge's word, a status line, a note.
        Caption,
        /// A field's note beside its name: "✓ Long enough".
        CaptionStrong,
        /// A field's name, above it.
        Label,
        /// A line smaller than the body: an error under a field, a footer, a menu's item.
        Small,
        /// A menu's item that removes or leaves something.
        SmallStrong,
        /// Running text.
        Body,
        /// Running text that is stressed: a row's name, a field's name on a page.
        BodyStrong,
        /// A number or an address: a latency, a time, a server's address.
        Mono,
        /// A long monospaced text, and an id beside a name: a licence's text and its id.
        MonoCaption,
        /// A dialog's or a panel's title.
        Title,
        /// A page's or a card's heading.
        Heading,
        /// The title of a page of settings, over its content on the line that holds the page's actions.
        PageTitle,
        /// A part's heading within a page: About Chatkord's "Built with".
        Subheading,
        /// The initials on a person's tile in a call.
        Display,
        /// The same, alone on it.
        DisplayLarge,
        /// The product's name beside its logo.
        Wordmark,
        /// A message's words in a text channel: the body's size, a little more leading for running lines.
        Message,
        /// The name over a run of one author's messages.
        Author,
        /// A message's time beside its author, and in the gutter of one that carries a run on.
        Stamp,
    }
}

impl TypeStyle {
    /// The type's size, in logical pixels; a message's own styles at the default [`crate::MessageSize`].
    pub const fn size(self) -> f32 {
        match self {
            TypeStyle::Caption | TypeStyle::CaptionStrong | TypeStyle::MonoCaption => 12.0,
            TypeStyle::Stamp => 11.0,
            TypeStyle::Label | TypeStyle::Small | TypeStyle::SmallStrong | TypeStyle::Mono => 13.0,
            TypeStyle::Body | TypeStyle::BodyStrong | TypeStyle::Message | TypeStyle::Author => 14.0,
            TypeStyle::Subheading => 17.0,
            TypeStyle::Title => 18.0,
            TypeStyle::Heading | TypeStyle::PageTitle | TypeStyle::Wordmark => 22.0,
            TypeStyle::Display => 28.0,
            TypeStyle::DisplayLarge => 40.0,
        }
    }

    /// The height of one line of it, in logical pixels.
    pub const fn line_height(self) -> f32 {
        match self {
            TypeStyle::Stamp => 14.0,
            TypeStyle::Caption | TypeStyle::CaptionStrong => 17.0,
            TypeStyle::Label | TypeStyle::Small | TypeStyle::SmallStrong | TypeStyle::Mono | TypeStyle::MonoCaption => {
                18.0
            }
            TypeStyle::Body | TypeStyle::BodyStrong => 20.0,
            TypeStyle::Message | TypeStyle::Author => 21.0,
            TypeStyle::Title | TypeStyle::Subheading => 24.0,
            TypeStyle::Heading | TypeStyle::PageTitle | TypeStyle::Wordmark => 28.0,
            TypeStyle::Display => 36.0,
            TypeStyle::DisplayLarge => 50.0,
        }
    }

    /// How heavy it is set.
    pub const fn weight(self) -> Weight {
        match self {
            TypeStyle::Caption
            | TypeStyle::Small
            | TypeStyle::Body
            | TypeStyle::Mono
            | TypeStyle::MonoCaption
            | TypeStyle::Message
            | TypeStyle::Stamp => Weight::Regular,
            TypeStyle::Label => Weight::Medium,
            TypeStyle::CaptionStrong
            | TypeStyle::SmallStrong
            | TypeStyle::BodyStrong
            | TypeStyle::Author
            | TypeStyle::Title
            | TypeStyle::Heading
            | TypeStyle::Subheading => Weight::SemiBold,
            TypeStyle::PageTitle => Weight::Bold,
            TypeStyle::Wordmark | TypeStyle::Display | TypeStyle::DisplayLarge => Weight::ExtraBold,
        }
    }

    /// Whether it is one of a message's own styles, which [`crate::MessageSize`] sets at the person's size.
    pub const fn in_message(self) -> bool {
        matches!(self, TypeStyle::Message | TypeStyle::Author | TypeStyle::Stamp)
    }

    /// The typeface it is set in.
    pub const fn face(self) -> Face {
        match self {
            TypeStyle::Mono | TypeStyle::MonoCaption | TypeStyle::Stamp => Face::Mono,
            TypeStyle::Message
            | TypeStyle::Author
            | TypeStyle::Caption
            | TypeStyle::CaptionStrong
            | TypeStyle::Label
            | TypeStyle::Small
            | TypeStyle::SmallStrong
            | TypeStyle::Wordmark
            | TypeStyle::Body
            | TypeStyle::BodyStrong
            | TypeStyle::Title
            | TypeStyle::Heading
            | TypeStyle::PageTitle
            | TypeStyle::Subheading
            | TypeStyle::Display
            | TypeStyle::DisplayLarge => Face::Sans,
        }
    }
}

/// A drop shadow, in logical pixels, black at `alpha`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    /// How far below the shape it falls, in logical pixels.
    pub offset_y: f32,
    /// How far it spreads, in logical pixels.
    pub blur: f32,
    /// How dark it is: 0 is none, 255 black.
    pub alpha: u8,
}

/// How far something stands above the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Elevation {
    /// On the window itself.
    Flat,
    /// A panel or a card on the window.
    Raised,
    /// Floating over everything: a menu, a dialog, a tooltip.
    Overlay,
}

impl Elevation {
    /// The surface it is painted on.
    pub const fn surface(self) -> Surface {
        match self {
            Elevation::Flat => Surface::Base,
            Elevation::Raised => Surface::Raised,
            Elevation::Overlay => Surface::Overlay,
        }
    }

    /// A panel shows its height by its surface; what floats over the window shares the window's
    /// colour, so it casts a shadow in both schemes. A light panel, nearly white, needs one too.
    pub const fn shadow(self, scheme: Scheme) -> Option<Shadow> {
        match (scheme, self) {
            (_, Elevation::Flat) | (Scheme::Dark, Elevation::Raised) => None,
            (Scheme::Light, Elevation::Raised) => Some(Shadow { offset_y: 1.0, blur: 3.0, alpha: 26 }),
            (_, Elevation::Overlay) => Some(Shadow { offset_y: 20.0, blur: 48.0, alpha: 89 }),
        }
    }
}

/// A dot of the typing line's three, round.
pub const TYPING_DOT: f32 = 5.0;
/// The room between them.
pub const TYPING_DOT_GAP: f32 = 3.0;
/// How long one dot takes to brighten and fade, as the canvas's typing line.
pub const TYPING_PULSE: std::time::Duration = std::time::Duration::from_millis(1200);
/// How much later each dot pulses than the one before it.
pub const TYPING_STAGGER: std::time::Duration = std::time::Duration::from_millis(200);
/// A dot's opacity between pulses, of its full colour.
pub const TYPING_DIM: f32 = 0.35;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scales_grow_and_text_fits_its_line() {
        let spaces = [Space::Xxs, Space::Xs, Space::S, Space::M, Space::L, Space::Xl, Space::Xxl].map(Space::px);
        assert!(spaces.windows(2).all(|pair| pair[0] < pair[1]));
        for style in TypeStyle::ALL {
            assert!(style.line_height() >= style.size() * 1.25, "{style:?}");
            if !matches!(style, TypeStyle::Display | TypeStyle::DisplayLarge) {
                assert!(style.line_height() <= HIT_TARGET, "{style:?}: a line fits a target");
            }
        }
    }
}
