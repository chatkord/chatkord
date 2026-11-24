//! The sizes of a reply, a channel's pins and a server's emoji page, as the canvas draws Discord's: the bar over the
//! composer, the quoted line over a reply, the pinned messages' panel and the cards of Server settings › Emoji.

/// The bar over the composer while replying, in logical pixels.
pub const REPLY_BAR_HEIGHT: f32 = 34.0;
/// The quoted line over a reply, in logical pixels.
pub const QUOTED_LINE_HEIGHT: f32 = 20.0;
/// Where the quoted line starts in a row: its connector's foot over the middle of the author's picture.
pub const QUOTED_INDENT: f32 = 19.0;

/// The line from a reply's picture up to the message it quotes: a stroke rising from its foot, turning, and running on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Connector {
    /// Its box's width, in logical pixels.
    pub width: f32,
    /// Its box's height, in logical pixels.
    pub height: f32,
    /// How far in from its box's left it rises, in logical pixels.
    pub inset: f32,
    /// How far below the box's top it runs across, in logical pixels.
    pub across: f32,
    /// The radius of its turn, in logical pixels.
    pub turn: f32,
    /// Its stroke's width, in logical pixels.
    pub stroke: f32,
}

/// The canvas's connector: 29 by 13, rising 1 in, across 3 below its top, turning on 5, stroked 1.5 wide.
pub const REPLY_CONNECTOR: Connector =
    Connector { width: 29.0, height: 13.0, inset: 1.0, across: 3.0, turn: 5.0, stroke: 1.5 };

/// The pinned messages' panel, in logical pixels.
pub const PINS_PANEL_WIDTH: f32 = 380.0;

/// A card of Server settings › Emoji: three to a row of the page's widest list.
pub const EMOJI_CARD_WIDTH: f32 = 294.0;
/// The height of an emoji's card, in logical pixels.
pub const EMOJI_CARD_HEIGHT: f32 = 56.0;
/// A server emoji's picture on its card, in logical pixels.
pub const EMOJI_CARD_PICTURE: f32 = 32.0;
/// The widest the page's cards run, in logical pixels: three cards and the gaps between them.
pub const EMOJI_LIST_WIDTH: f32 = 900.0;
