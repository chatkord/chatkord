//! The call's mute, deafen and share toggles for the bar and the panel.

use crate::app::Part;
use crate::labels::{label, phrase};
use crate::{CallButton, CallSize, Icon, Renderer};
use design::CallControl;
use screens::{Call, Device, Over, PlaceMessage, WindowMessage as Message};
use words::{Phrase, Words};

/// Mute and deafen, in the call bar and the account strip: the icon says how things are, the label what a press
/// does. Dimmed until the room's snapshot has arrived, so what they show is the server's word.
pub(crate) fn call_toggles<'a, R: Renderer + 'a>(
    id: screens::PlaceId,
    call: &Call,
    size: CallSize,
    words: &'a Words,
) -> Vec<Part<'a, R>> {
    let [mut mute, mut deafen] = toggles(call.state(), size, words);
    // Held, the icon still says the person's own mute, which is theirs to change; the ground says the hold.
    if call.held() {
        mute = mute.held(phrase(Phrase::HeldTip, words));
    }
    if let Some(failed) = call.fault(Device::Microphone) {
        mute = mute.failed(label(&words.say(failed)));
    }
    if size == CallSize::Bar {
        mute = mute.menu(
            phrase(Phrase::DeviceMenu, words),
            Message::At(id, PlaceMessage::Call(screens::CallMessage::Devices)),
            call.open() == Some(Over::Devices),
        );
    }
    if call.joined() {
        mute = mute.on_press(Message::At(id, PlaceMessage::Call(screens::CallMessage::ToggleMute)));
        deafen = deafen.on_press(Message::At(id, PlaceMessage::Call(screens::CallMessage::ToggleDeafen)));
    }
    vec![mute.view::<R>(), deafen.view::<R>()]
}

/// Mute and deafen outside a call: they set how the next call starts, drawn as the call's own are.
pub(crate) fn next_call_toggles<'a, R: Renderer + 'a>(state: choices::State, words: &'a Words) -> Vec<Part<'a, R>> {
    let [mute, deafen] = toggles(state, CallSize::Pane, words);
    vec![mute.on_press(Message::NextMute).view::<R>(), deafen.on_press(Message::NextDeafen).view::<R>()]
}

/// Mute and deafen as `state` has them, at `size`, pressing nothing yet.
fn toggles<Message: Clone>(state: choices::State, size: CallSize, words: &Words) -> [CallButton<Message>; 2] {
    // Deafening silences the microphone too, so a deafened account's microphone is shown off.
    let muted = state.mic_muted || state.deafened;
    let mute = CallButton::new(
        phrase(if muted { Phrase::UnmuteMic } else { Phrase::MuteMic }, words),
        if muted { Icon::MicOff } else { Icon::Mic },
        if muted { CallControl::Off } else { CallControl::Rest },
    )
    .size(size);
    let deafen = CallButton::new(
        phrase(if state.deafened { Phrase::UndeafenSelf } else { Phrase::DeafenSelf }, words),
        if state.deafened { Icon::HeadphonesOff } else { Icon::Headphones },
        if state.deafened { CallControl::Off } else { CallControl::Rest },
    )
    .size(size);
    [mute, deafen]
}
