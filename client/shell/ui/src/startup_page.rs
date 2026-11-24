//! Settings › Startup and window: whether Chatkord opens at sign-in, whether it then opens in the tray, and whether
//! closing the window keeps it running there.

use crate::account_page::{Open, pages};
use crate::labels::phrase;
use crate::{Renderer, TextClass, Theme, coloured, list_card, switch, switch_offered};
use choices::{AtSignIn, CloseTo, StartsIn, Startup};
use design::{Content, DEVICE_COLUMN_WIDTH, Space, TypeStyle};
use iced_core::{Element, Length, Widget};
use iced_widget::Column;
use screens::WindowMessage as Message;
use words::{Phrase, Words};

type Part<'a, R> = Element<'a, Message, Theme, R>;

pub(crate) fn view<'a, R: Renderer + 'a>(
    window: &'a screens::Window,
    fold: screens::frame::SettingsShown,
    startup: Startup,
    words: &'a Words,
) -> Part<'a, R> {
    let opens = startup.at_sign_in == AtSignIn::Opens;
    let card = list_card(
        phrase(Phrase::StartupAndWindow, words),
        vec![
            switch(&phrase(Phrase::AtSignIn, words), None, opens, Message::ToggleAtSignIn),
            switch_offered(
                &phrase(Phrase::StartMinimized, words),
                None,
                startup.starts_in == StartsIn::Tray,
                opens.then_some(Message::ToggleStartsIn),
            ),
            switch(
                &phrase(Phrase::CloseToTray, words),
                Some(&words.phrase(Phrase::CloseToTrayHint)),
                startup.close_to == CloseTo::Tray,
                Message::ToggleCloseTo,
            ),
        ],
        None,
    );
    let note = coloured(
        phrase(Phrase::NoTray, words).as_str().to_string(),
        TypeStyle::Small,
        TextClass::Content(Content::Secondary),
    );
    let content = Column::with_children([card, note._boxed()])
        .spacing(Space::S.px())
        .width(Length::Fill.max(DEVICE_COLUMN_WIDTH));
    let close = Message::CloseDevice;
    let frame = crate::settings_frame(
        &phrase(Phrase::SettingsLabel, words),
        pages(window, Open::Startup, words),
        phrase(Phrase::StartupAndWindow, words),
        None,
        content._boxed(),
        crate::SettingsWays {
            close: (phrase(Phrase::Close, words), close.clone()),
            shown: fold,
            back: (phrase(Phrase::SettingsBack, words), Message::SettingsList),
        },
    );
    crate::on_escape(frame, close, None)._boxed()
}
