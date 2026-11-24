//! How loud one sound plays on this device: Mute for me, the slider and its figure, as a person's popover and Audio
//! controls draw it.

use crate::{Button, Icon, Renderer, Theme, slider, text};
use design::{ButtonVariant, Label, Space, TypeStyle, VOLUME_FIGURE};
use iced_core::{Alignment, Element, Length, Widget};
use iced_widget::{Row, container};

/// What the row says, in the window's words.
pub struct VolumeWords {
    /// The slider's name for a screen reader.
    pub slider: Label,
    /// The toggle's name: Mute for me, or Unmute for me while silent.
    pub mute: Label,
    /// Where the slider is, as a figure.
    pub figure: String,
}

/// The row for one sound at `percent`, `muted` while it is silent here: the toggle sends `on_mute`, the slider
/// `on_change` while it moves and `on_commit` where it is let go.
pub fn volume_row<'a, Message: Clone + 'a, R: Renderer + 'a>(
    words: VolumeWords,
    percent: u16,
    muted: bool,
    on_change: impl Fn(u16) -> Message + 'a,
    on_commit: impl Fn(u16) -> Message + 'a,
    on_mute: Message,
) -> Element<'a, Message, Theme, R> {
    let glyph = if muted { Icon::VolumeOff } else { Icon::Volume };
    let toggle = Button::new(words.mute).icon_only(glyph).variant(ButtonVariant::Ghost).compact().on_press(on_mute);
    Row::with_children([
        toggle.view::<R>(),
        slider(words.slider, 0..=100, percent, on_change, on_commit),
        container(text(words.figure, TypeStyle::Mono)).align_right(Length::Fixed(VOLUME_FIGURE))._boxed(),
    ])
    .spacing(Space::S.px() + Space::Xxs.px())
    .align_y(Alignment::Center)
    .width(Length::Fill)
    ._boxed()
}
