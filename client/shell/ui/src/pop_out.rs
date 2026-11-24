//! A watched screen or a camera in a window of its own: its picture letterboxed on black, whose it is at the bottom
//! left, and under the pointer its bar at the top right: Volume while a screen has sound, or for a camera the person's,
//! and Close. A screen's sound popover opens under the bar.

use crate::{FocusScope, Labelled, Renderer, Surfaced, Theme};
use design::{SemanticRole, Semantics, Space};
use iced_core::{Element, Length, Size, Vector, Widget, alignment};
use iced_widget::{Stack, container};
use proto::video::Video;
use screens::{CallMessage, PersonFrom, PlaceMessage, PoppedOut, WindowMessage as Message};
use words::{Phrase, Sentence, Words};

type Part<'a, R> = Element<'a, Message, Theme, R>;

/// `out`'s screen or camera filling a window of `size`, `picture` the host's picture of it, and over it the popover of
/// the person's volume while a camera's Volume is open.
pub(crate) fn view<'a, R: Renderer + 'a>(
    out: PoppedOut<'a>,
    size: Size,
    picture: Part<'a, R>,
    person_volume: Option<Part<'a, R>>,
    words: &'a Words,
) -> Part<'a, R> {
    let place = out.place.id();
    let shown_out = out.screen;
    let at = move |message: CallMessage| Message::At(place, PlaceMessage::Call(message));
    let ground: Part<'a, R> =
        container(picture).width(Length::Fill).height(Length::Fill).class(Surfaced::Backdrop)._boxed();
    let tool =
        |icon, said, message| crate::Tool { icon, label: crate::labels::phrase(said, words), message, held: false };
    // A screen's sound is its own; a camera has none, so its Volume is the person's.
    let volume = match shown_out.video {
        Video::Screen => shown_out
            .sound
            .map(|_| tool(crate::Icon::Volume, Phrase::Volume, at(CallMessage::ShareSound(shown_out.user_id.clone())))),
        Video::Camera => Some(tool(
            crate::Icon::Volume,
            Phrase::Volume,
            Message::Person(place, shown_out.user_id.clone(), PersonFrom::Card),
        )),
    };
    let sound_open = shown_out.sound.is_some_and(|sound| sound.open);
    let open = sound_open || person_volume.is_some();
    let close = tool(crate::Icon::Close, Phrase::Close, at(CallMessage::PopIn));
    let bar = crate::card_bar(volume.into_iter().chain([close]).collect(), None, design::TILE_LEAST);
    let whose = match shown_out.video {
        Video::Screen => words.say(&Sentence::ScreenOf(shown_out.name.to_string())),
        Video::Camera => words.say(&Sentence::CameraOf(shown_out.name.to_string())),
    };
    let name: Part<'a, R> = container(crate::screen_card::name_pill(whose.clone(), None, words))
        .align_x(alignment::Horizontal::Left)
        .align_y(alignment::Vertical::Bottom)
        .padding(design::PILL_INSET)
        .width(Length::Fill)
        .height(Length::Fill)
        ._boxed();
    let shown: Part<'a, R> = Stack::with_children([crate::reveal(ground, bar, open), name])._boxed();
    // Its sound's popover hangs under the bar (its inset, its pill of hit targets, a gap), from the right edge.
    let bar_bottom = Space::S.px() + design::HIT_TARGET + 2.0 * Space::Xxs.px() + Space::S.px();
    let under = Vector::new((size.width - design::INFO_PANEL_WIDTH - Space::S.px()).max(0.0), bar_bottom);
    let popover = person_volume
        .or_else(|| sound_open.then(|| crate::call::screen_sound(out.place, shown_out.user_id, shown_out.name, words)));
    let shown = crate::menu_at(FocusScope::new(shown)._boxed(), popover, under, at(CallMessage::CloseOver));
    Labelled::new(shown, Semantics { role: SemanticRole::Dialog, label: crate::labels::label(&whose) })._boxed()
}
