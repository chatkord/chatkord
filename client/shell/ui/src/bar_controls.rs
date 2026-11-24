//! A view's header, and the controls that end the window's top bar or, where the bar is folded away with the pane, the
//! header of the view shown: the call's Voice options, Audio controls and Chat while the call shown is this account's
//! own, then the members column's toggle. A folded header holds the search's magnifier before them.

use crate::app::members_toggle;
use crate::labels::phrase;
use crate::{Button, Icon, Renderer, Surfaced, Theme};
use design::{ButtonVariant, Content, Popup, Radius, Space, Surface};
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Column, Row, Space as Gap, container, stack};
use screens::{Over, Place, PlaceMessage, WindowMessage as Message};
use words::{Phrase, Words};

type Part<'a, R> = Element<'a, Message, Theme, R>;

/// The button that opens the channels' drawer, at the start of a folded view's header.
pub(crate) fn channels_button<'a, R: Renderer + 'a>(words: &Words) -> Part<'a, R> {
    Button::new(phrase(Phrase::ShowChannelsButton, words))
        .icon_only(Icon::PanelLeft)
        .variant(ButtonVariant::Ghost)
        .compact()
        .on_press(Message::ToggleChannels)
        .view::<R>()
}

/// A button that ends the bar.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Control {
    VoiceOptions,
    AudioControls,
    CallChat,
    Members,
}

/// What ends the bar while the call shown is this account's own, in order; otherwise the members toggle alone.
const OWN_CALL: [Control; 4] = [Control::VoiceOptions, Control::AudioControls, Control::CallChat, Control::Members];

/// What ends the top bar: the call's controls where they are offered, then the members toggle, `members` while the
/// column shows.
pub(crate) fn end<'a, R: Renderer + 'a>(place: &'a Place, members: bool, words: &Words) -> Vec<Part<'a, R>> {
    let Some(call) = place.own_call() else { return vec![members_toggle(members, words)] };
    let said = |message: screens::CallMessage| Message::At(place.id(), PlaceMessage::Call(message));
    let opens = |button: Phrase, shown: Icon, over: Over, message: screens::CallMessage| {
        Button::new(phrase(button, words))
            .icon_only(shown)
            .variant(ButtonVariant::Ghost)
            .compact()
            .opens(Popup::Dialog, call.open() == Some(over))
            .on_press(said(message))
            .view::<R>()
    };
    OWN_CALL
        .map(|control| match control {
            Control::VoiceOptions => {
                opens(Phrase::VoiceOptions, Icon::Settings, Over::View, screens::CallMessage::ViewMenu)
            }
            Control::AudioControls => {
                opens(Phrase::AudioControls, Icon::Volume, Over::Audio, screens::CallMessage::AudioControls)
            }
            Control::CallChat => call_chat_toggle(place, place.call_chat().is_some(), words),
            Control::Members => members_toggle(members, words),
        })
        .into_iter()
        .collect()
}

/// How far the right edge of the bar's button that opens `over` stands from the bar's end, where one opens it.
pub(crate) fn opener_from_end(over: Over) -> Option<f32> {
    let control = match over {
        Over::View => Control::VoiceOptions,
        Over::Audio => Control::AudioControls,
        _ => return None,
    };
    let after = OWN_CALL.iter().rev().position(|each| *each == control)?;
    Some(EDGE + after as f32 * (design::HIT_TARGET + Space::Xs.px()))
}

/// What ends a folded view's header: the search's magnifier where the server offers one, then what ends the bar.
pub(crate) fn folded_end<'a, R: Renderer + 'a>(place: &'a Place, members: bool, words: &Words) -> Vec<Part<'a, R>> {
    let mut folded: Vec<Part<'a, R>> = crate::search_view::magnifier::<R>(place, words).into_iter().collect();
    folded.extend(end(place, members, words));
    folded
}

/// The room between a bar's or a header's edge and a button standing at it: the canvas pulls such a button into the
/// side the rest of the line keeps.
pub(crate) const EDGE: f32 = Space::L.px() - Space::Xxs.px();

/// A view's header, as the canvas draws each: `lead` (the buttons that lead away, to the channels or back to a list),
/// then `start` (a mark and a title, one of which fills the room), then `end` (its buttons), on one line
/// [`design::PANE_HEADER_HEIGHT`] tall. Its sides are [`design::MESSAGE_SIDE`], or [`EDGE`] where a button stands at
/// the edge.
pub(crate) fn header<'a, R: Renderer + 'a>(
    lead: Vec<Part<'a, R>>,
    start: Vec<Part<'a, R>>,
    end: Vec<Part<'a, R>>,
) -> Part<'a, R> {
    let side = |buttons: &[Part<'a, R>]| if buttons.is_empty() { design::MESSAGE_SIDE } else { EDGE };
    let (left, right) = (side(&lead), side(&end));
    let gap = Space::M.px() - Space::Xxs.px();
    let start = Row::with_children(lead.into_iter().chain(start)).spacing(gap).align_y(Alignment::Center);
    let end = Row::with_children(end).spacing(Space::Xs.px()).align_y(Alignment::Center);
    let row =
        Row::with_children([start.width(Length::Fill)._boxed(), end._boxed()]).spacing(gap).align_y(Alignment::Center);
    container(row)
        .padding(Padding { left, right, ..Padding::ZERO })
        .width(Length::Fill)
        .center_y(Length::Fixed(design::PANE_HEADER_HEIGHT))
        ._boxed()
}

/// A folded view's header under its rule: the way to the channels, `start`, then what ends a folded header.
pub(crate) fn folded_header<'a, R: Renderer + 'a>(
    start: Vec<Part<'a, R>>,
    place: &'a Place,
    members: bool,
    words: &Words,
) -> Part<'a, R> {
    let header = header(vec![channels_button::<R>(words)], start, folded_end(place, members, words));
    Column::with_children([header, crate::panel_parts::hairline()])._boxed()
}

/// The call's Chat button: `shown` while the call's chat is open, with a dot while it is shut over unread.
fn call_chat_toggle<'a, R: Renderer + 'a>(place: &'a Place, shown: bool, words: &Words) -> Part<'a, R> {
    let said = if shown { Phrase::HideCallChat } else { Phrase::ShowCallChat };
    let button = Button::new(phrase(said, words))
        .icon_only(Icon::MessageSquare)
        .variant(ButtonVariant::Ghost)
        .compact()
        .on_press(Message::ToggleCallChat)
        .view::<R>();
    if !place.call_chat_dot() {
        return button;
    }
    let ring = design::CALL_CHAT_DOT + 2.0 * design::CALL_CHAT_DOT_RING;
    let dot = container(Gap::new().width(design::CALL_CHAT_DOT).height(design::CALL_CHAT_DOT)).class(Surfaced::Fill {
        fill: design::Role::Content(Content::Primary),
        text: design::Role::Content(Content::Primary),
        radius: Radius::Full,
    });
    let ringed = container(dot).center(Length::Fixed(ring)).class(Surfaced::Fill {
        fill: design::Role::Surface(Surface::Base),
        text: design::Role::Content(Content::Primary),
        radius: Radius::Full,
    });
    let placed = container(ringed)
        .padding(Padding { top: Space::Xxs.px(), right: Space::Xxs.px(), ..Padding::ZERO })
        .align_right(Length::Fill);
    stack([button, placed._boxed()])._boxed()
}
