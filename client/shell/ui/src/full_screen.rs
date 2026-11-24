//! A watched screen over the whole display: its picture letterboxed on black, whose screen it is at the bottom left,
//! how to leave at the top centre and the way out at the top right. All but the picture go, with the pointer, while
//! the pointer rests; the keyboard brings the way out back.

use crate::typography::text;
use crate::{FocusScope, Labelled, Renderer, Surfaced, Theme};
use design::{SemanticRole, Semantics, Space, TypeStyle};
use iced_core::{Element, Length, Padding, Widget, alignment, mouse};
use iced_widget::{Stack, container, mouse_area};
use screens::{CallMessage, FullView, PlaceMessage, WindowMessage as Message};
use words::{Phrase, Sentence, Words};

type Part<'a, R> = Element<'a, Message, Theme, R>;

/// `full`'s screen over the whole window, `picture` the host's picture of it.
pub(crate) fn view<'a, R: Renderer + 'a>(
    full: FullView<'a>,
    picture: Element<'a, Message, Theme, R>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let place = full.place;
    let at = move |message: CallMessage| Message::At(place, PlaceMessage::Call(message));
    let ground = mouse_area(container(picture).width(Length::Fill).height(Length::Fill).class(Surfaced::Backdrop))
        .on_double_click(at(CallMessage::LeaveFullScreen))
        .on_move(move |_| at(CallMessage::Stirred));
    // The pointer goes with the controls; while they show, the picture's own (a hand while zoomed) is kept.
    let ground: Part<'a, R> =
        if full.controls { ground._boxed() } else { ground.interaction(mouse::Interaction::Hidden)._boxed() };

    let leave = crate::Tool {
        icon: crate::Icon::Minimize,
        label: crate::labels::phrase(Phrase::LeaveFullScreen, words),
        message: at(CallMessage::LeaveFullScreen),
        held: false,
    };
    let zoom = crate::call::zoom_tool(place, full.screen.user_id, full.zoom.is_some(), words);
    let way_out = crate::card_bar(vec![zoom, leave], None, design::TILE_LEAST);
    let hint: Part<'a, R> = container(
        container(text(words.phrase(Phrase::PressEscToLeave).to_string(), TypeStyle::Small))
            .padding(Padding::from([Space::Xs.px(), Space::M.px()]))
            .class(Surfaced::ScrimPill),
    )
    .align_x(alignment::Horizontal::Center)
    .align_y(alignment::Vertical::Top)
    .padding(Space::S.px())
    .width(Length::Fill)
    .height(Length::Fill)
    ._boxed();
    let whose = words.say(&Sentence::ScreenOf(full.screen.name.to_string()));
    let said = crate::labels::label(&words.say(&Sentence::FullScreenOf(full.screen.name.to_string())));
    let name: Part<'a, R> =
        container(crate::screen_card::name_pill(whose, full.zoom.map(screens::Lens::percent), words))
            .align_x(alignment::Horizontal::Left)
            .align_y(alignment::Vertical::Bottom)
            .padding(design::PILL_INSET)
            .width(Length::Fill)
            .height(Length::Fill)
            ._boxed();
    let controls: Part<'a, R> = Stack::with_children([hint, name, way_out])._boxed();
    let shown: Part<'a, R> = FocusScope::new(crate::reveal_when(ground, controls, full.controls))._boxed();
    let shown: Part<'a, R> = Labelled::new(shown, Semantics { role: SemanticRole::Dialog, label: said })._boxed();
    crate::on_escape(shown, at(CallMessage::LeaveFullScreen), None)._boxed()
}
