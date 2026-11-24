//! One person's card in the call, in the grid and floating.

use super::tools::{bar_tool, zoomable};
use crate::app::Part;
use crate::discussion::Handles;
use crate::labels::phrase;
use crate::picture::{Painted, Picture};
use crate::{Icon, Renderer, ScreenCard, Surfaced};
use iced_core::Length;
use iced_core::Widget;
use iced_widget::{container, stack};
use proto::video::Video;
use screens::{
    Call, Card, FloatMessage, Over, Pinned, Place, PlaceMessage, Preview, Received, ShareCard, ShareState,
    WindowMessage as Message,
};
use words::{Phrase, Sentence, Words};

/// One card at `size`, in the grid, on the stage or in the strip: a person, this account's own screen, or someone's
/// screen offered or playing here, with its bar of controls over it on hover. A person's chosen picture is painted
/// once `handles` says the host holds it, their initials until then.
#[expect(clippy::too_many_arguments, reason = "a card reads the call, its place and both of the host's painters")]
pub(crate) fn card_view<'a, R: Renderer + 'a>(
    place: &'a Place,
    call: &'a Call,
    card: Card<'a>,
    size: (f32, f32),
    alone: bool,
    handles: &Handles<'_>,
    picture: &Picture<'a, R>,
    words: &'a Words,
) -> Part<'a, R> {
    let id = place.id();
    let at = |message: PlaceMessage| Message::At(id, message);
    let paused_of = |pin: Pinned| crate::Paused {
        said: words.phrase(Phrase::Paused).to_string(),
        resume: phrase(Phrase::ResumeStream, words),
        on_resume: at(PlaceMessage::Call(screens::CallMessage::TogglePause(pin))),
    };
    let shown: Part<'a, R> = match card {
        // A right-click on someone else in the call opens their popover.
        Card::Person(tile) => {
            let press = tile
                .user_id
                .as_ref()
                .filter(|_| !tile.own)
                .map(|user_id| Message::Person(id, user_id.clone(), screens::PersonFrom::Card));
            // Another person's camera in its own window leaves their card saying so, as a screen's does.
            let out = tile.user_id.as_ref().is_some_and(|user_id| !tile.own && call.is_out(user_id, Video::Camera));
            let camera = tile.camera.pictured().then(|| match tile.user_id.as_ref() {
                Some(user_id) if !tile.own => picture(Painted::Camera { user_id: user_id.clone() }),
                Some(_) | None => picture(Painted::OwnCamera(place.mirror(), design::TILE_RADIUS)),
            });
            // Another person's camera, paused for this viewer: its last picture under the veil a screen has, the
            // veil beneath the name so the name stays whole.
            let paused = tile
                .user_id
                .as_ref()
                .filter(|_| !tile.own && tile.camera == screens::CameraSeen::Paused)
                .map(|user_id| Pinned::Person(user_id.clone()))
                .map(|pin| crate::screen_card::paused_veil(paused_of(pin)));
            let camera = match (camera, paused) {
                (Some(camera), Some(veil)) => {
                    Some(stack([camera, veil]).width(Length::Fill).height(Length::Fill)._boxed())
                }
                (camera, _) => camera,
            };
            let camera = if out {
                Some(crate::screen_card::out_face(words.say(&Sentence::CameraOut(tile.name.clone())), size))
            } else {
                camera
            };
            let chosen = tile
                .user_id
                .as_ref()
                .and_then(|user_id| place.picture_of(user_id))
                .filter(|file| handles(*file).is_some())
                .map(|file| picture(Painted::Picture { picture: file, radius: design::Radius::Full, faint: false }));
            crate::voice_tile(tile, size, camera, chosen, alone, press, words)
        }
        Card::OwnScreen(Preview::Shown) => crate::screen_card(
            words.phrase(Phrase::YourScreen).to_string(),
            size,
            ScreenCard::Playing {
                picture: picture(Painted::OwnScreen(design::TILE_RADIUS)),
                reconnecting: None,
                menu: None,
                double_click: None,
                zoom: None,
                unheard: None,
                behind: None,
                paused: None,
            },
            words,
        ),
        Card::OwnScreen(Preview::Waiting) => {
            let said = words.phrase(Phrase::ShareStarting).to_string();
            crate::screen_card(words.phrase(Phrase::YourScreen).to_string(), size, ScreenCard::Starting { said }, words)
        }
        Card::OwnScreen(Preview::Unshown) => crate::screen_card(
            words.phrase(Phrase::YourScreen).to_string(),
            size,
            ScreenCard::Own {
                said: words.phrase(Phrase::OwnScreenUnshown).to_string(),
                stop: at(PlaceMessage::Call(screens::CallMessage::ToggleShare)),
            },
            words,
        ),
        // Every share plays in its card from its start.
        Card::Screen { sharer, user_id, received } => crate::screen_card(
            words.say(&Sentence::ScreenOf(sharer.name.clone())),
            size,
            match received {
                // A picture paused before it was watched has none to hold, and only Resume starts it.
                Received::Waiting if call.is_paused(user_id, Video::Screen) => ScreenCard::Playing {
                    picture: iced_widget::Space::new()._boxed(),
                    reconnecting: None,
                    double_click: None,
                    zoom: None,
                    menu: None,
                    unheard: None,
                    behind: None,
                    paused: Some(paused_of(Pinned::Screen(user_id.clone()))),
                },
                Received::Waiting => ScreenCard::Starting { said: words.phrase(Phrase::ShareWaiting).to_string() },
                Received::Playing(_) if call.is_out(user_id, Video::Screen) => {
                    ScreenCard::Out { said: words.say(&Sentence::ScreenOut(sharer.name.clone())) }
                }
                Received::Playing(card) => ScreenCard::Playing {
                    picture: zoomable(id, call.zoom(user_id), |lens| {
                        picture(Painted::Screen { user_id: user_id.clone(), lens, corners: design::TILE_RADIUS })
                    }),
                    reconnecting: reconnecting(card, &sharer.name, words),
                    double_click: Some(at(PlaceMessage::Call(screens::CallMessage::FullScreen(user_id.clone())))),
                    zoom: call.zoom(user_id).map(screens::Lens::percent),
                    // A right-click opens the popover of its sound, as one on a person opens theirs; a sound not
                    // carried has none to set.
                    menu: (call.shares_sound(user_id) && call.unheard(user_id).is_none())
                        .then(|| at(PlaceMessage::Call(screens::CallMessage::ShareSound(user_id.clone())))),
                    unheard: call.unheard(user_id).map(|failed| words.say(&words::failure::of(failed))),
                    behind: (card.state == ShareState::Behind).then(|| words.phrase(Phrase::CantKeepUp).into_owned()),
                    paused: (card.state == ShareState::Paused).then(|| paused_of(Pinned::Screen(user_id.clone()))),
                },
                Received::Unshown(failed) => ScreenCard::Unshown { said: words.say(&words::failure::of(failed)) },
            },
            words,
        ),
    };
    let Some(pin) = card.pin() else { return shown };
    let member = match card {
        Card::Person(tile) => tile.user_id.as_ref(),
        Card::Screen { user_id, .. } => Some(user_id),
        Card::OwnScreen(_) => None,
    };
    let tools = call
        .card_tools(&card)
        .into_iter()
        .filter_map(|tool| bar_tool(place, call, &card, &pin, member, tool, words))
        .collect();
    let opened = about_opened(call, &card);
    let revealed = crate::reveal(shown, crate::card_bar(tools, None, size.1), opened);
    // The popover about its picture hangs from this card, and lies over the whole call.
    if opened { container(revealed).id(crate::layer::ABOUT_CARD.clone())._boxed() } else { revealed }
}

/// Whether the popover about the picture on `card` is open.
fn about_opened(call: &Call, card: &Card<'_>) -> bool {
    match card {
        Card::Screen { user_id, received: Received::Playing(_), .. } => call.about() == Some(*user_id),
        Card::OwnScreen(Preview::Shown) => call.open() == Some(Over::OwnPicture),
        Card::Person(tile) => tile.own && call.open() == Some(Over::OwnCamera),
        Card::OwnScreen(_) | Card::Screen { .. } => false,
    }
}

/// The floating card: the pinned screen of a call out of view, whole, this account's own included, with Go to the
/// call, Reset position and Close on its bar; a drag on it moves it.
pub(crate) fn floating_card<'a, R: Renderer + 'a>(
    card: screens::Floating<'a>,
    picture: &Picture<'a, R>,
    words: &'a Words,
) -> Part<'a, R> {
    // Someone's screen with their name, or this account's own as it goes out, titled as its own card is.
    let (title, picture, reconnecting) = match card.screen {
        screens::PinnedShare::Someone(screen) => (
            words.say(&Sentence::ScreenOf(screen.name.to_string())),
            picture(Painted::Screen {
                user_id: screen.user_id.clone(),
                lens: screens::Lens::WHOLE,
                corners: design::TILE_RADIUS,
            }),
            reconnecting(screen.card, screen.name, words),
        ),
        screens::PinnedShare::Own => {
            (words.phrase(Phrase::YourScreen).to_string(), picture(Painted::OwnScreen(design::TILE_RADIUS)), None)
        }
    };
    let shown = crate::screen_card(
        title,
        design::FLOAT_CARD,
        ScreenCard::Playing {
            picture,
            reconnecting,
            menu: None,
            double_click: None,
            zoom: None,
            unheard: None,
            behind: None,
            paused: None,
        },
        words,
    );
    let tool = |icon: Icon, said: Phrase, message: FloatMessage| crate::Tool {
        icon,
        label: phrase(said, words),
        message: Message::Float(message),
        held: false,
    };
    let tools = vec![
        tool(Icon::GoToCall, Phrase::GoToCall, FloatMessage::GoToCall),
        tool(Icon::ResetPosition, Phrase::ResetPosition, FloatMessage::Reset),
        tool(Icon::Close, Phrase::Close, FloatMessage::Close),
    ];
    let raised = container(crate::reveal(shown, crate::card_bar(tools, None, design::FLOAT_CARD.1), false))
        .class(Surfaced::Floating);
    crate::floating::floating(raised._boxed(), iced_core::Point::new(card.at.0, card.at.1), |to| {
        Message::Float(FloatMessage::To(to.x, to.y))
    })
}

/// What is said over the screen of `who` while it reconnects.
fn reconnecting(card: &ShareCard, who: &str, words: &Words) -> Option<String> {
    (card.state == ShareState::Reconnecting).then(|| words.say(&Sentence::ReconnectingTo(who.to_string())))
}
