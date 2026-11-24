//! The emoji a person reacts and writes with, as the canvas draws Discord's: the chips under a message, the tools over
//! a hovered one, the picker, and the composer's chooser after `@`, `#` and `:`.

use crate::look::Interaction;
use crate::role::{AccentRole, Border, Content, Role, Surface};

/// A reaction chip's height, in logical pixels.
pub const CHIP_HEIGHT: f32 = 26.0;
/// A unicode emoji's size on a chip, in logical pixels.
pub const CHIP_EMOJI: f32 = 15.0;
/// A server emoji's side on a chip, in logical pixels.
pub const CHIP_PICTURE: f32 = 18.0;
/// The space between a chip's emoji and its count, in logical pixels.
pub const CHIP_GAP: f32 = 6.0;

/// A hovered message's tools: each button's side, in logical pixels.
pub const HOVER_TOOL: f32 = 30.0;
/// The room round and between a hovered message's tools, in logical pixels.
pub const HOVER_TOOLS_PAD: f32 = 2.0;

/// The emoji picker's width, in logical pixels.
pub const EMOJI_PICKER_WIDTH: f32 = 316.0;
/// The emoji picker's height, its search, tabs and a few rows of the grid, in logical pixels.
pub const EMOJI_PICKER_HEIGHT: f32 = 420.0;
/// The room round and between the emoji picker's parts, in logical pixels.
pub const EMOJI_PICKER_PAD: f32 = 10.0;
/// The space between the emoji picker's parts, in logical pixels.
pub const EMOJI_PICKER_GAP: f32 = 8.0;
/// The emoji picker's search and skin tone button: their height, the tone button's side, in logical pixels.
pub const EMOJI_PICKER_FIELD: f32 = 34.0;
/// How many emoji the picker's grid sets in a row.
pub const EMOJI_GRID_COLUMNS: usize = 6;
/// A cell of the picker's grid: its height, in logical pixels.
pub const EMOJI_CELL: f32 = 40.0;
/// A unicode emoji's size in the picker's grid, in logical pixels.
pub const EMOJI_CELL_GLYPH: f32 = 22.0;
/// A server emoji's side in the picker's grid, in logical pixels.
pub const EMOJI_CELL_PICTURE: f32 = 26.0;
/// The hand on the picker's skin tone button, in logical pixels.
pub const EMOJI_TONE_GLYPH: f32 = 16.0;
/// The server's own tab in the picker: its initial's tile, in logical pixels.
pub const EMOJI_SERVER_TILE: f32 = 20.0;

/// The composer's chooser: its width, in logical pixels.
pub const CHOOSER_WIDTH: f32 = 340.0;
/// The composer's chooser: the most it grows to before it scrolls, in logical pixels.
pub const CHOOSER_HEIGHT: f32 = 460.0;
/// The room round the chooser's rows, in logical pixels.
pub const CHOOSER_PAD: f32 = 6.0;
/// The column a chooser row's picture, mark or emoji stands in, in logical pixels.
pub const CHOOSER_LEADING: f32 = 24.0;
/// A unicode emoji's size in a chooser row, in logical pixels.
pub const CHOOSER_GLYPH: f32 = 20.0;
/// A server emoji's side in a chooser row, in logical pixels.
pub const CHOOSER_PICTURE: f32 = 22.0;

/// The roles a reaction chip is painted with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChipLook {
    /// Its ground.
    pub fill: Role,
    /// Its count.
    pub text: Role,
    /// Its edge.
    pub edge: Role,
}

/// A reaction chip, one's own or not, in an interaction: one's own is ringed in the accent on its tint with the
/// accent's text, as Discord marks the reactions a person gave; another's answers the pointer with the hover fill.
pub const fn chip(mine: bool, interaction: Interaction) -> ChipLook {
    match (mine, interaction) {
        (_, Interaction::Disabled) => ChipLook {
            fill: Role::Surface(Surface::Raised),
            text: Role::Content(Content::Disabled),
            edge: Role::Border(Border::Subtle),
        },
        // The accent's text is kept on the tint it was derived for; the ring answers the pointer instead.
        (true, Interaction::Idle | Interaction::Loading) => ChipLook {
            fill: Role::Accent(AccentRole::Tint),
            text: Role::Accent(AccentRole::Text),
            edge: Role::Accent(AccentRole::Ring),
        },
        (true, Interaction::Hover | Interaction::Pressed) => ChipLook {
            fill: Role::Accent(AccentRole::Tint),
            text: Role::Accent(AccentRole::Text),
            edge: Role::Accent(AccentRole::Fill),
        },
        (false, Interaction::Idle | Interaction::Loading) => ChipLook {
            fill: Role::Surface(Surface::Raised),
            text: Role::Content(Content::Primary),
            edge: Role::Border(Border::Subtle),
        },
        (false, Interaction::Hover) => ChipLook {
            fill: Role::Surface(Surface::Hover),
            text: Role::Content(Content::Primary),
            edge: Role::Border(Border::Strong),
        },
        (false, Interaction::Pressed) => ChipLook {
            fill: Role::Surface(Surface::Pressed),
            text: Role::Content(Content::Primary),
            edge: Role::Border(Border::Strong),
        },
    }
}
