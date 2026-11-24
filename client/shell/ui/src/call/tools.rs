//! The tools on a card and the buttons of the call bar.

use crate::app::Part;
use crate::labels::{label, phrase};
use crate::{CallButton, CallSize, Icon, Renderer};
use design::{CallControl, Popup};
use proto::video::Video;
use screens::{
    About, Call, Can, Card, CardTool, Device, Over, Pinned, Place, PlaceId, PlaceMessage, Received, Sending,
    ShareState, WindowMessage as Message,
};
use terms::MemberId;
use words::{Phrase, Words};

/// The share button, in the bar or the pane, sending `press`: it opens the panel that picks a screen, or stops the
/// share. Where this account cannot share it is dimmed and its tooltip says why.
pub(crate) fn share_button<'a, R: Renderer + 'a>(
    place: &Place,
    call: &Call,
    size: CallSize,
    press: Message,
    words: &'a Words,
) -> Part<'a, R> {
    let button = share_pressing(place, call, size, press, words);
    match call.sending() {
        Sending::No => button.opens(Popup::Dialog, call.open() == Some(Over::Share)).view::<R>(),
        Sending::Starting | Sending::Live(_) => button.view::<R>(),
    }
}

/// The same, sending `press` when it may be pressed.
fn share_pressing(place: &Place, call: &Call, size: CallSize, press: Message, words: &Words) -> CallButton<Message> {
    let sharing = *call.sending() != Sending::No;
    let can = place.can_share();
    let (said, glyph, control) = if sharing {
        (Phrase::StopSharing, Icon::ScreenShareOff, CallControl::Live)
    } else {
        (can.why_not().unwrap_or(Phrase::ShareScreen), Icon::ScreenShare, CallControl::Rest)
    };
    let mut button = CallButton::new(phrase(said, words), glyph, control).size(size);
    if let Some(failed) = call.fault(Device::Share) {
        button = button.failed(label(&words.say(failed)));
    }
    if (sharing || can == Can::Yes) && call.joined() {
        button = button.on_press(press);
    }
    button
}

/// The camera button, before Share: none where the server offers no camera, dimmed with why where this account may
/// not use one, tinted while the camera is on.
pub(crate) fn camera_button<'a, R: Renderer + 'a>(
    place: &Place,
    call: &Call,
    size: CallSize,
    words: &'a Words,
) -> Option<Part<'a, R>> {
    let can = place.can_camera();
    let on = *place.own_camera() != screens::OwnCamera::Off;
    let (said, glyph, control) = if on {
        (Phrase::StopCamera, Icon::Video, CallControl::Live)
    } else {
        (can.why_not().unwrap_or(Phrase::TurnOnCamera), Icon::VideoOff, CallControl::Rest)
    };
    let mut button = CallButton::new(phrase(said, words), glyph, control).size(size);
    if let Some(failed) = call.fault(Device::Camera) {
        button = button.failed(label(&words.say(failed)));
    }
    if (on || can == screens::CanCamera::Yes) && call.joined() {
        button = button.on_press(Message::At(place.id(), PlaceMessage::Call(screens::CallMessage::ToggleCamera)));
    }
    Some(button.view::<R>())
}

/// The control of `card`'s bar that the model's `tool` stands for, sending what it does; none where the card has
/// nothing for it to act on.
pub(crate) fn bar_tool(
    place: &Place,
    call: &Call,
    card: &Card<'_>,
    pin: &Pinned,
    member: Option<&MemberId>,
    tool: CardTool,
    words: &Words,
) -> Option<crate::Tool<Message>> {
    let id = place.id();
    let at = |message: screens::CallMessage| Message::At(id, PlaceMessage::Call(message));
    let made = |icon: Icon, said: Phrase, message: Message| crate::Tool {
        icon,
        label: phrase(said, words),
        message,
        held: false,
    };
    Some(match tool {
        CardTool::About => match card {
            Card::OwnScreen(_) => made(Icon::Info, Phrase::AboutPicture, at(screens::CallMessage::ToggleOwnInfo)),
            Card::Screen { user_id, .. } => {
                made(Icon::Info, Phrase::AboutPicture, at(screens::CallMessage::ToggleInfo((*user_id).clone())))
            }
            Card::Person(tile) if tile.own => {
                made(Icon::Info, Phrase::AboutPicture, at(screens::CallMessage::ToggleOwnCameraInfo))
            }
            Card::Person(_) => return None,
        },
        CardTool::PictureInPicture => {
            let video = match card {
                Card::Person(_) => Video::Camera,
                Card::Screen { .. } | Card::OwnScreen(_) => Video::Screen,
            };
            made(
                Icon::PictureInPicture,
                Phrase::PictureInPicture,
                at(screens::CallMessage::PopOut(member?.clone(), video)),
            )
        }
        CardTool::ClosePictureInPicture => {
            made(Icon::PictureInPicture, Phrase::ClosePictureInPicture, at(screens::CallMessage::PopIn))
        }
        CardTool::Volume => match card {
            Card::Person(_) => {
                made(Icon::Volume, Phrase::Volume, Message::Person(id, member?.clone(), screens::PersonFrom::Card))
            }
            Card::Screen { received, .. } => crate::Tool {
                // A paused screen's sound is paused with it, so its Volume waits for Resume.
                held: matches!(received, Received::Playing(held) if held.state == ShareState::Paused),
                ..made(Icon::Volume, Phrase::Volume, at(screens::CallMessage::ShareSound(member?.clone())))
            },
            Card::OwnScreen(_) => return None,
        },
        CardTool::Pause => made(Icon::Pause, Phrase::PauseStream, at(screens::CallMessage::TogglePause(pin.clone()))),
        CardTool::Resume => made(Icon::Play, Phrase::ResumeStream, at(screens::CallMessage::TogglePause(pin.clone()))),
        CardTool::Zoom => zoom_tool(id, member?, call.zoom(member?).is_some(), words),
        CardTool::FullScreen => {
            made(Icon::Maximize, Phrase::FullScreen, at(screens::CallMessage::FullScreen(member?.clone())))
        }
        CardTool::Pin if call.pinned() == Some(pin) => {
            made(Icon::PinOff, Phrase::UnpinScreen, at(screens::CallMessage::Unpin))
        }
        CardTool::Pin => made(Icon::Pin, Phrase::PinScreen, at(screens::CallMessage::Pin(pin.clone()))),
        CardTool::StopSharing => made(Icon::ScreenShareOff, Phrase::StopSharing, at(screens::CallMessage::ToggleShare)),
    })
}

/// The popover about the picture of the card whose About is open: a watched screen's as this viewer gets it, or this
/// account's own as it goes out.
pub(crate) fn about_popover<'a, R: Renderer + 'a>(place: &Place, call: &Call, words: &'a Words) -> Option<Part<'a, R>> {
    if let Some(about) = call.about() {
        return call.cards().into_iter().find_map(|card| match card {
            Card::Screen { sharer, user_id, received: Received::Playing(shown) } if user_id == about => {
                Some(crate::info_panel::<Message, R>(&About::watched(shown, &sharer.name, words), None, words))
            }
            Card::Person(_) | Card::OwnScreen(_) | Card::Screen { .. } => None,
        });
    }
    if call.open() == Some(Over::OwnCamera) {
        return call.own_camera_about(words).map(|about| crate::info_panel::<Message, R>(&about, None, words));
    }
    let id = place.id();
    call.own_about(words).filter(|_| call.open() == Some(Over::OwnPicture)).map(|about| {
        let picked = call.own_sound_apps().map(|(apps, picked)| {
            crate::share_panel::sound_apps(
                apps,
                picked,
                |app| Message::At(id, PlaceMessage::Call(screens::CallMessage::PickSoundApp(app))),
                words,
            )
        });
        crate::info_panel::<Message, R>(&about, picked, words)
    })
}

/// The toggle of the zoom on the screen of `user_id`, in place `id`: a magnifier with a plus, with a minus while `on`.
pub(crate) fn zoom_tool(id: PlaceId, user_id: &MemberId, on: bool, words: &Words) -> crate::Tool<Message> {
    let (icon, said) = if on { (Icon::ZoomOut, Phrase::StopZooming) } else { (Icon::ZoomIn, Phrase::ZoomPicture) };
    let message = Message::At(id, PlaceMessage::Call(screens::CallMessage::ToggleZoom(user_id.clone())));
    crate::Tool { icon, label: phrase(said, words), message, held: false }
}

/// The host's picture through `lens`, drawn by `drawn`: while zoom is on, the wheel over it zooms and a drag moves it.
pub(crate) fn zoomable<'a, R: Renderer + 'a>(
    id: PlaceId,
    lens: Option<screens::Lens>,
    drawn: impl FnOnce(screens::Lens) -> Part<'a, R>,
) -> Part<'a, R> {
    let Some(lens) = lens else { return drawn(screens::Lens::WHOLE) };
    let call = move |message| Message::At(id, PlaceMessage::Call(message));
    crate::zoomed::zoomed(
        drawn(lens),
        move |notches, at, area| {
            call(screens::CallMessage::ZoomWheel { notches, at: (at.x, at.y), area: (area.width, area.height) })
        },
        move |by, area| call(screens::CallMessage::Pan { by: (by.x, by.y), area: (area.width, area.height) }),
    )
}
