//! A message's files and pictures as the canvas draws Discord's: a picture's thumbnail and the grid of several, a
//! file's card, the composer's strip of files to send, the drop over a channel and the picture viewer.

/// One picture's thumbnail, width and height, in logical pixels.
pub const PICTURE_ONE: (f32, f32) = (180.0, 135.0);
/// The box several pictures share as a grid, width and height, in logical pixels.
pub const PICTURE_GRID: (f32, f32) = (300.0, 150.0);
/// A file's card under its message, in logical pixels.
pub const FILE_CARD_WIDTH: f32 = 360.0;
/// A file waiting in the composer's strip, in logical pixels.
pub const PENDING_WIDTH: f32 = 220.0;
/// The same, refused: wider, as its sentence takes two lines.
pub const PENDING_REFUSED_WIDTH: f32 = 280.0;
/// A pending file's picture or file mark, square, in logical pixels.
pub const PENDING_PREVIEW: f32 = 40.0;
/// The room a pending file keeps on its right for its way out, in logical pixels.
pub const PENDING_CLOSE_ROOM: f32 = 34.0;
/// The card that says a drop attaches, in logical pixels.
pub const DROP_CARD_WIDTH: f32 = 380.0;
/// The tile with the paperclip on that card, square, in logical pixels.
pub const DROP_TILE: f32 = 64.0;
/// The dashed accent edge of that card, in logical pixels.
pub const DROP_EDGE: f32 = 2.0;
/// The room the picture viewer keeps above the picture for its bar, in logical pixels.
pub const VIEWER_BAR_ROOM: f32 = 72.0;
