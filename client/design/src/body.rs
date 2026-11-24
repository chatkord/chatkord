//! How a message's styled text is drawn beside its words, as the canvas's Chat board draws Discord's markdown: code
//! smaller and in its own box, a quote behind a bar, a mention on a halo, a server's emoji as a picture in the line.

/// Inline code's size, in proportion to the message's.
pub const CODE_INLINE_EM: f32 = 0.86;
/// A block of code's size, in proportion to the message's.
pub const CODE_BLOCK_EM: f32 = 0.9;
/// A server emoji's side in a message's line, in proportion to the message's size: 19.25 px at 14 px, inside its
/// 21 px line.
pub const CUSTOM_EMOJI_EM: f32 = 1.375;
/// The bar a quote's lines stand behind, in logical pixels.
pub const QUOTE_BAR: f32 = 4.0;
/// The halo round a mention, a channel or inline code beyond its letters, each side, in logical pixels.
pub const MARK_PAD: f32 = 2.0;
/// The space between a body's blocks (a paragraph, a quote, a block of code), in logical pixels.
pub const BLOCK_GAP: f32 = 2.0;
/// The corners of a mention's, a channel's, inline code's and a spoiler's ground, in logical pixels.
pub const MARK_RADIUS: f32 = 4.0;
