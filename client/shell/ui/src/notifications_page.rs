//! Settings › Notifications: whether a message that would alert shows a desktop popup, whether the account's status
//! holds every alert back, and whether the window's own sounds play.

use crate::account_page::{Open, pages};
use crate::labels::phrase;
use crate::{Button, Renderer, TextClass, Theme, coloured, list_card, switch, text};
use choices::{EventSounds, MessageSounds, Popups};
use design::{ButtonVariant, Content, DEVICE_COLUMN_WIDTH, Space, TypeStyle};
use iced_core::{Alignment, Element, Length, Widget};
use iced_widget::{Column, Row, container};
use screens::{StatusMessage, WindowMessage as Message};
use words::{Phrase, Words};

type Part<'a, R> = Element<'a, Message, Theme, R>;

/// The Do not disturb row: its name and whether the account's status holds every alert back, until when, with the
/// button that opens the status where it is chosen.
fn do_not_disturb<'a, R: Renderer + 'a>(window: &'a screens::Window, words: &'a Words) -> Part<'a, R> {
    let secondary = TextClass::Content(Content::Secondary);
    let lines: Vec<Part<'a, R>> = vec![
        text(words.phrase(Phrase::DoNotDisturb).into_owned(), TypeStyle::BodyStrong)._boxed(),
        coloured(words.say(&window.do_not_disturb_note()), TypeStyle::Small, secondary)._boxed(),
    ];
    let choose = Button::new(phrase(Phrase::ChooseStatus, words))
        .variant(ButtonVariant::Secondary)
        .on_press(Message::Status(StatusMessage::Choose))
        .view::<R>();
    Row::with_children([Column::with_children(lines).spacing(Space::Xxs.px()).width(Length::Fill)._boxed(), choose])
        .spacing(Space::M.px())
        .align_y(Alignment::Center)
        ._boxed()
}

pub(crate) fn view<'a, R: Renderer + 'a>(
    window: &'a screens::Window,
    fold: screens::frame::SettingsShown,
    words: &'a Words,
) -> Part<'a, R> {
    let rows = vec![
        switch(
            &phrase(Phrase::ShowPopups, words),
            Some(&words.phrase(Phrase::ShowPopupsNote)),
            window.popups() == Popups::On,
            Message::TogglePopups,
        ),
        do_not_disturb(window, words),
        switch(
            &phrase(Phrase::EventSounds, words),
            None,
            window.event_sounds() == EventSounds::On,
            Message::ToggleEventSounds,
        ),
        switch(
            &phrase(Phrase::MessageSounds, words),
            Some(&words.phrase(Phrase::MessageSoundsNote)),
            window.message_sounds() == MessageSounds::On,
            Message::ToggleMessageSounds,
        ),
    ];
    let cards = container(list_card(phrase(Phrase::Notifications, words), rows, None))
        .width(Length::Fill.max(DEVICE_COLUMN_WIDTH));
    let close = Message::CloseDevice;
    let frame = crate::settings_frame(
        &phrase(Phrase::SettingsLabel, words),
        pages(window, Open::Notifications, words),
        phrase(Phrase::Notifications, words),
        None,
        cards._boxed(),
        crate::SettingsWays {
            close: (phrase(Phrase::Close, words), close.clone()),
            shown: fold,
            back: (phrase(Phrase::SettingsBack, words), Message::SettingsList),
        },
    );
    crate::on_escape(frame, close, None)._boxed()
}
