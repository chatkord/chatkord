//! Audio controls: every sound in the call but one's own, each person's voice and each shared screen's sound on a row
//! with its own volume, as the canvas draws the panel under the call header.

use crate::layer::on_panel;
use crate::volume_row::{VolumeWords, volume_row};
use crate::{
    AvatarKind, Held, Icon, IconClass, Labelled, Renderer, TextClass, Theme, avatar_pictured, coloured, icon, text,
};
use design::{
    AUDIO_CONTROLS_WIDTH, AvatarSize, Content, IconSize, Label, MENU_RADIUS, PersonColour, Role, SOUND_NAME_WIDTH,
    SemanticRole, Semantics, Space, TypeStyle,
};
use iced_core::text::Wrapping;
use iced_core::{Alignment, Element, Length, Widget};
use iced_widget::{Column, Row, container, scrollable};

/// One sound on the panel.
pub struct SoundRow<'a, Message> {
    /// Whose sound it is, as their tile names them.
    pub name: Label,
    /// Their colour, for their picture.
    pub colour: PersonColour,
    /// The picture they chose, as the host holds it; none for their initials.
    pub picture: Option<Held>,
    /// It is the sound of their shared screen, not their voice: a screen's glyph follows the name.
    pub share: bool,
    /// The row's own words.
    pub words: VolumeWords,
    /// Its volume, a whole percent.
    pub percent: u16,
    /// It is silent on this device.
    pub muted: bool,
    /// What moving the slider sends.
    pub on_change: Box<dyn Fn(u16) -> Message + 'a>,
    /// What letting the slider go, or a key's step, sends.
    pub on_commit: Box<dyn Fn(u16) -> Message + 'a>,
    /// What Mute for me sends.
    pub on_mute: Message,
}

/// What the panel says, in the window's words.
pub struct AudioWords {
    /// Its title, and its name for a screen reader.
    pub title: Label,
    /// What it says with no one else in the call.
    pub empty: String,
}

/// The panel of `rows`, with `words` over them.
pub fn audio_controls<'a, Message: Clone + 'a, R: Renderer + 'a>(
    words: AudioWords,
    rows: Vec<SoundRow<'a, Message>>,
) -> Element<'a, Message, Theme, R> {
    let secondary = |said: String, style| coloured(said, style, TextClass::Content(Content::Secondary));
    let heading = text(words.title.as_str().to_string(), TypeStyle::BodyStrong);
    let body: Element<'a, Message, Theme, R> = if rows.is_empty() {
        secondary(words.empty, TypeStyle::Small)._boxed()
    } else {
        let rows: Vec<Element<'a, Message, Theme, R>> = rows.into_iter().map(sound_row).collect();
        scrollable(Column::with_children(rows).spacing(Space::S.px() + Space::Xxs.px()))
            .direction(scrollable::Direction::Vertical(crate::theme::scroll_bar()))
            ._boxed()
    };
    let column = Column::with_children([heading._boxed(), body])
        .spacing(Space::S.px() + Space::Xxs.px())
        .width(Length::Fill.max(AUDIO_CONTROLS_WIDTH - 2.0 * Space::L.px()));
    // Past the window's height the rows scroll: the layer holds the panel inside the window.
    Labelled::new(
        on_panel(column._boxed(), Space::L, MENU_RADIUS),
        Semantics { role: SemanticRole::Dialog, label: words.title },
    )
    ._boxed()
}

/// One sound: the picture and name of whose it is, then its volume.
fn sound_row<'a, Message: Clone + 'a, R: Renderer + 'a>(row: SoundRow<'a, Message>) -> Element<'a, Message, Theme, R> {
    let mut named = vec![text(row.name.as_str().to_string(), TypeStyle::Small).wrapping(Wrapping::None)._boxed()];
    if row.share {
        let quiet = IconClass::Role(Role::Content(Content::Secondary));
        named.push(icon(Icon::Monitor, IconSize::S, quiet)._boxed());
    }
    let name = container(Row::with_children(named).spacing(Space::Xs.px()).align_y(Alignment::Center))
        .width(Length::Fixed(SOUND_NAME_WIDTH))
        .clip(true);
    Row::with_children([
        avatar_pictured(row.name, AvatarKind::Called(row.colour), AvatarSize::S, row.picture),
        name._boxed(),
        volume_row(row.words, row.percent, row.muted, row.on_change, row.on_commit, row.on_mute),
    ])
    .spacing(Space::S.px() + Space::Xxs.px())
    .align_y(Alignment::Center)
    ._boxed()
}
