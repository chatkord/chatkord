//! The call panel in the pane, and the room its bar takes.

use super::tools::share_button;
use crate::app::Part;
use crate::labels::phrase;
use crate::{CallButton, CallSize, Icon, Renderer, Surfaced, TextClass};
use design::{CALL_PANEL_HEIGHT, CallControl, Content, Popup, Radius, Space, Surface, TypeStyle};
use iced_core::{Alignment, Length, Padding, Widget};
use iced_widget::{Column, Row, Space as Gap, container};
use screens::{Call, Over, Place, PlaceMessage, Quality, WindowMessage as Message};
use words::{Phrase, Sentence, Words};

/// The window's call, in the pane of `shown`: where it is and the way out. Nothing when there is none. On another
/// server, the connection's popover opens that server instead, and Share opens the call's page and its panel.
pub(crate) fn call_panel<'a, R: Renderer + 'a>(place: &'a Place, shown: &'a Place, words: &'a Words) -> Part<'a, R> {
    let Some(call) = place.call().filter(|call| call.live()) else { return Gap::new()._boxed() };
    let id = place.id();
    let elsewhere = id != shown.id();
    let mut leave = CallButton::new(phrase(Phrase::LeaveTheCall, words), Icon::PhoneOff, CallControl::LeaveQuiet)
        .size(CallSize::Pane);
    if !call.leaving() {
        leave = leave.on_press(Message::At(id, PlaceMessage::Call(screens::CallMessage::Leave)));
    }
    let where_ = Column::with_children([
        crate::line(words.say(&Sentence::In(call.channel.name.clone())), TypeStyle::SmallStrong)._boxed(),
        crate::line(place.server().shown().to_string(), TypeStyle::Caption)
            .class(TextClass::Content(Content::Secondary))
            ._boxed(),
    ]);
    // The bars open the popover of how the connection is; they are quiet until the call has a look to colour them by.
    let open = call.open() == Some(Over::Connection);
    let mut bars = CallButton::new(
        phrase(Phrase::StatsTitle, words),
        Icon::Signal,
        CallControl::Signal { status: call.health().and_then(|health| health.quality).map(Quality::status), open },
    )
    .size(CallSize::Pane)
    .opens(Popup::Dialog, open);
    if call.joined() {
        bars = bars.on_press(if elsewhere {
            Message::Open(id)
        } else {
            Message::At(id, PlaceMessage::Call(screens::CallMessage::Stats))
        });
    }
    let share = share_button::<R>(place, call, CallSize::Pane, Message::PaneShare(id), words);
    let row = Row::with_children([
        bars.view::<R>(),
        where_.width(Length::Fill)._boxed(),
        // The two actions sit close together, as the canvas has them.
        Row::with_children([share, leave.view::<R>()]).spacing(Space::Xs.px())._boxed(),
    ])
    .spacing(Space::S.px())
    .align_y(Alignment::Center);
    let panel = container(row)
        .padding(Padding::from([0.0, Space::S.px() + Space::Xxs.px()]))
        .width(Length::Fill)
        .center_y(Length::Fixed(CALL_PANEL_HEIGHT))
        .class(Surfaced::Framed(Surface::Base, Radius::L));
    container(panel)
        .padding(Padding { left: Space::S.px(), right: Space::S.px(), bottom: Space::S.px(), top: 0.0 })
        ._boxed()
}

/// The room the stage keeps under it for the call bar; a bar shown only while wanted floats over the stage instead.
pub(crate) fn bar_room(call: &Call) -> f32 {
    screens::frame::bar_room(call.view().controls)
}
