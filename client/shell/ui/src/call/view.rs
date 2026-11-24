//! The call's header and its view: the grid, the stage and the pinned body.

use super::cards::card_view;
use super::devices::{audio_panel, devices, view_panel};
use super::panel::bar_room;
use super::sound::share_sound_popover;
use super::toggles::call_toggles;
use super::tools::{about_popover, camera_button, share_button};
use crate::app::{Part, members_shown};
use crate::discussion::Handles;
use crate::labels::phrase;
use crate::picture::Picture;
use crate::{
    CallButton, CallSize, Icon, IconClass, Renderer, TextClass, bar_divider, coloured, device_menu_over, end_card,
    icon, stage_bar, state_line, voice_grid,
};
use design::{
    AUDIO_CONTROLS_WIDTH, CALL_BUTTON, CallControl, Content, INFO_PANEL_WIDTH, IconSize, Role, STAGE_BAR_PAD, Space,
    TypeStyle,
};
use iced_core::{Alignment, Length, Padding, Size, Widget, alignment};
use iced_widget::{Column, Row, container, scrollable, stack};
use screens::frame::{Frame, MembersColumn};
use screens::{Call, Card, Over, Pinned, Place, PlaceMessage, StateLine, Strip, Window, WindowMessage as Message};
use words::{Phrase, Words};

/// A folded call's header: the way to the channels, the channel's name (cut with an ellipsis where it is long, with no
/// count of who is in it) and the controls that end the bar.
fn call_header<'a, R: Renderer + 'a>(place: &'a Place, call: &'a Call, members: bool, words: &Words) -> Part<'a, R> {
    let start = vec![
        icon(Icon::Volume, IconSize::M, IconClass::Role(Role::Content(Content::Secondary)))._boxed(),
        crate::typography::line(call.channel.name.as_str(), TypeStyle::BodyStrong).width(Length::Fill)._boxed(),
    ];
    crate::bar_controls::folded_header(start, place, members, words)
}

/// The call's place: a grid of everyone under the top bar, with the call bar floating at the bottom; or, once it is
/// over, what happened and the ways on.
#[expect(
    clippy::too_many_arguments,
    reason = "the call's view reads the window, its shape and both of the host's painters"
)]
pub(crate) fn call_view<'a, R: Renderer + 'a>(
    window: &'a Window,
    place: &'a Place,
    call: &'a Call,
    size: Size,
    shaped: &Frame,
    handles: &Handles<'_>,
    picture: &Picture<'a, R>,
    words: &'a Words,
) -> Part<'a, R> {
    let folded = shaped.folded();
    let (encoders, view) = (window.encoders(), window.look().call);
    let id = place.id();
    // Folded, the top bar is away with the pane and the call's own header holds the way to the channels and its
    // controls; otherwise the stage starts under the bar, which is outside the call's `size`.
    let header = folded.then(|| call_header(place, call, members_shown(place, shaped), words));
    let head = shaped.top_room() - shaped.top;
    let stage_height = size.height - head;

    let state = StateLine::of(place);
    let room_taken = if state.is_some() { crate::STATE_LINE_ROOM } else { 0.0 };
    let body: Part<'a, R> = if let Some(card) = call.end_card(place.grants()) {
        let switch = Message::At(id, PlaceMessage::Lobby(screens::LobbyMessage::Join(call.channel.id.clone())));
        end_card(&card, switch, Message::At(id, PlaceMessage::Call(screens::CallMessage::Leave)), call.leaving(), words)
    } else if !call.joined() {
        container(coloured(words.phrase(Phrase::JoiningCall), TypeStyle::Body, TextClass::Content(Content::Secondary)))
            .center(Length::Fill)
            ._boxed()
    } else {
        // The room the grid has: the stage's, less the gaps and the space the call bar floats in.
        let width = (size.width - 2.0 * Space::L.px()).max(0.0);
        let height = (stage_height - Space::L.px() - bar_room(call) - room_taken).max(0.0);
        if let Some(pinned) = call.pinned() {
            let stage = screens::Stage::of(stage_height);
            pinned_body(place, call, pinned, handles, picture, (width, height), stage, words)
        } else {
            container(voice_grid::<_, Message, R>(&call.cards(), width, height, |card, size, alone| {
                card_view(place, call, card, size, alone, handles, picture, words)
            }))
            .padding(Padding {
                left: Space::L.px(),
                right: Space::L.px(),
                top: Space::L.px(),
                bottom: bar_room(call) + room_taken,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            ._boxed()
        }
    };
    let mut stage: Vec<Part<'a, R>> = header.into_iter().collect();
    stage.push(container(body).width(Length::Fill).height(Length::Fill)._boxed());
    let page: Part<'a, R> = match state {
        // The line stands in the room kept above the call bar, with its rule on its top edge.
        Some(shown) => stack([
            Column::with_children(stage)._boxed(),
            container(state_line::<Message, R>(&shown, words))
                .align_bottom(Length::Fill)
                .padding(Padding { bottom: bar_room(call), ..Padding::ZERO })
                ._boxed(),
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        ._boxed(),
        None => Column::with_children(stage)._boxed(),
    };
    let mut bar = None;
    if call.live() {
        // What is heard, what is shown, and leaving, each set apart as the canvas draws them.
        let mut controls = call_toggles::<R>(id, call, CallSize::Bar, words);
        controls.push(bar_divider());
        controls.extend(camera_button::<R>(place, call, CallSize::Bar, words));
        let toggle = Message::At(place.id(), PlaceMessage::Call(screens::CallMessage::ToggleShare));
        controls.push(share_button::<R>(place, call, CallSize::Bar, toggle, words));
        controls.push(bar_divider());
        let mut leave = CallButton::new(phrase(Phrase::LeaveTheCall, words), Icon::PhoneOff, CallControl::Leave)
            .size(CallSize::Bar)
            .wide();
        if !call.leaving() {
            leave = leave.on_press(Message::At(id, PlaceMessage::Call(screens::CallMessage::Leave)));
        }
        controls.push(leave.view::<R>());
        bar = Some(
            container(stage_bar(controls))
                .align_x(Alignment::Center)
                .align_y(alignment::Vertical::Bottom)
                .padding(Padding { bottom: Space::Xl.px() - Space::Xs.px(), ..Padding::ZERO })
                .width(Length::Fill)
                .height(Length::Fill)
                ._boxed(),
        );
    }
    let screen: Part<'a, R> = match (bar, call.view().controls) {
        (Some(bar), choices::Controls::Always) => stack([page, bar]).width(Length::Fill).height(Length::Fill)._boxed(),
        (Some(bar), choices::Controls::WhileHovered) => crate::reveal(page, bar, call.bar_held()),
        (None, _) => page,
    };
    // The menu hangs over the call bar, which floats in the middle at the bottom.
    let bar_top = size.height - (Space::Xl.px() - Space::Xs.px()) - (CALL_BUTTON + 2.0 * STAGE_BAR_PAD);
    let menu = call.sound().filter(|_| call.open() == Some(Over::Devices)).map(|sound| devices(sound, words));
    let screen = device_menu_over(
        screen,
        menu,
        (size.width / 2.0, bar_top - Space::S.px()),
        Message::At(id, PlaceMessage::Call(screens::CallMessage::CloseOver)),
    );
    // The share panel hangs over the call bar as the device menu does.
    let panel = (call.open() == Some(Over::Share)).then(|| {
        let rows = crate::quality_rows(
            call.quality(),
            encoders,
            crate::Bitrate { rate: call.rate(), note: call.rate_note() },
            |pick| Message::At(id, PlaceMessage::Call(screens::CallMessage::Quality(pick))),
            move |rate| Message::At(id, PlaceMessage::Call(screens::CallMessage::Rate(rate))),
            words,
        );
        crate::share_panel::<Message, R>(
            phrase(Phrase::ShareToCall, words),
            rows,
            phrase(Phrase::ChooseScreen, words),
            Message::At(id, PlaceMessage::Call(screens::CallMessage::ChooseScreen)),
        )
    });
    let screen = device_menu_over(
        screen,
        panel,
        (size.width / 2.0, bar_top - Space::S.px()),
        Message::At(id, PlaceMessage::Call(screens::CallMessage::CloseOver)),
    );
    // The popover about a picture lies over the whole call, inside the cards' area and clear of the call bar.
    let top = head + Space::L.px();
    let area = iced_core::Rectangle::new(
        iced_core::Point::new(Space::L.px(), top),
        Size::new((size.width - 2.0 * Space::L.px()).max(0.0), (bar_top - design::ABOUT_BAR_GAP - top).max(0.0)),
    );
    let screen = crate::menu::popover_over_card(
        screen,
        about_popover::<R>(place, call, words),
        area,
        Message::At(id, PlaceMessage::Call(screens::CallMessage::CloseOver)),
    );
    // A person's popover opens where they were right-clicked; from the keyboard, at the stage's top right.
    let keyed =
        iced_core::Vector::new(size.width - INFO_PANEL_WIDTH - Space::L.px() - Space::S.px(), head + Space::L.px());
    let screen = crate::menu_at_pointer(
        screen,
        share_sound_popover(place, call, words),
        keyed,
        Message::At(id, PlaceMessage::Call(screens::CallMessage::CloseOver)),
    );
    // Audio controls and Voice options hang under their buttons at the end of the bar or the folded header, their right
    // edges on the buttons', kept inside the call's column.
    let docked = match (shaped.members, shaped.call_chat) {
        (Some(MembersColumn::Docked(width)), _) | (_, Some(MembersColumn::Docked(width))) => {
            width + design::BORDER_WIDTH
        }
        _ => 0.0,
    };
    let panel_width = AUDIO_CONTROLS_WIDTH.min(size.width - 2.0 * Space::L.px());
    let from_end = call.open().and_then(crate::bar_controls::opener_from_end).unwrap_or(crate::bar_controls::EDGE);
    let right = (from_end - docked).min(size.width - panel_width - Space::L.px()).max(Space::L.px());
    let corner = iced_core::Vector::new(
        size.width - panel_width - right,
        if folded { head + Space::S.px() } else { Space::S.px() },
    );
    crate::menu_at(
        screen,
        match call.open() {
            Some(Over::Audio) => Some(audio_panel(place, call, handles, words)),
            Some(Over::View) => Some(view_panel(view, words)),
            _ => None,
        },
        corner,
        Message::At(id, PlaceMessage::Call(screens::CallMessage::CloseOver)),
    )
}

/// A pinned card fills the stage and the other cards stand in a strip under it, each with its bar.
#[expect(clippy::too_many_arguments, reason = "the stage reads the call, its place and both of the host's painters")]
fn pinned_body<'a, R: Renderer + 'a>(
    place: &'a Place,
    call: &'a Call,
    pinned: &Pinned,
    handles: &Handles<'_>,
    picture: &Picture<'a, R>,
    (width, height): (f32, f32),
    height_is: screens::Stage,
    words: &'a Words,
) -> Part<'a, R> {
    let (stage, strip): (Vec<Card<'a>>, Vec<Card<'a>>) =
        call.cards().into_iter().partition(|card| card.pin().as_ref() == Some(pinned));
    let gap = Space::M.px();
    let shown = !strip.is_empty() && call.strip(height_is) == Strip::Shown;
    let staged = if shown { (width, (height - design::STRIP_TILE.1 - gap).max(0.0)) } else { (width, height) };
    // The stage's toggle shows while the pointer is over it or the keyboard is in it, as a card's bar does.
    let toggle = (!strip.is_empty()).then(|| {
        let (icon, said) =
            if shown { (Icon::ChevronDown, Phrase::HideStrip) } else { (Icon::ChevronUp, Phrase::ShowStrip) };
        let message = Message::At(place.id(), PlaceMessage::Call(screens::CallMessage::ToggleStrip(height_is)));
        crate::stage_toggle(crate::Tool { icon, label: phrase(said, words), message, held: false })
    });
    let stage = stage.into_iter().next().map(|card| {
        let card = card_view(place, call, card, staged, true, handles, picture, words);
        match toggle {
            Some(toggle) => crate::reveal(card, toggle, false),
            None => card,
        }
    });
    let strip = shown.then(|| {
        let cells: Vec<Part<'a, R>> = strip
            .into_iter()
            .map(|card| card_view(place, call, card, design::STRIP_TILE, false, handles, picture, words))
            .collect();
        let strip = scrollable(Row::with_children(cells).spacing(gap))
            .direction(scrollable::Direction::Horizontal(crate::theme::scroll_bar()))
            .width(Length::Shrink);
        // Centred under the stage, as the canvas draws it; a strip wider than the stage scrolls.
        container(strip).height(Length::Fixed(design::STRIP_TILE.1)).center_x(Length::Fill)._boxed()
    });
    container(Column::with_children(stage.into_iter().chain(strip)).spacing(gap))
        .padding(Padding { left: Space::L.px(), right: Space::L.px(), top: Space::L.px(), bottom: bar_room(call) })
        .width(Length::Fixed(width + 2.0 * Space::L.px()))
        .height(Length::Fixed(height + Space::L.px() + bar_room(call)))
        ._boxed()
}
