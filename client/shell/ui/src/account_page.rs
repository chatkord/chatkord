//! The person's own account page: the username, the name others see and the colour, then the Sign-in section with the
//! password, signing out here and leaving. Drawn in the place of the pane and the content, like a server's settings.
//! The password is changed in a dialog of its own (`password_dialog`).

use crate::door::named_aside;
use crate::labels::{label, phrase};
use crate::{
    AvatarKind, Button, Renderer, RowTone, SettingsGroup, SettingsPage, Surfaced, TextInput, Theme, colour_picker,
    flush_card, settings_body, settings_row, settings_section, text,
};
use design::{ButtonVariant, CONTROL_HEIGHT, FORM_WIDTH, INPUT_RADIUS, Space, Surface, TypeStyle};
use iced_core::{Element, Length, Padding, Widget};
use iced_widget::{Column, container};
use screens::{
    Account, AccountMessage, DevicePage, NameFieldMessage, PageMessage, Pick, Place, PlaceMessage,
    WindowMessage as Message,
};
use words::{Phrase, Sentence, Words};

type Part<'a, R> = Element<'a, Message, Theme, R>;

/// The page of the person's own settings that is shown.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Open {
    Account,
    Appearance,
    VoiceAndVideo,
    Notifications,
    Startup,
    About,
}

/// The pages the person's own settings list: the account under the server's name while it is signed in, then the pages
/// of this device; with no server signed in, those alone.
pub(crate) fn pages(window: &screens::Window, open: Open, words: &Words) -> Vec<SettingsGroup<Message>> {
    let page =
        |label: Phrase, current: bool, message: Message| SettingsPage { label: phrase(label, words), current, message };
    let startup = window
        .startup()
        .map(|_| page(Phrase::StartupAndWindow, open == Open::Startup, Message::OpenDevice(DevicePage::Startup)));
    let account = window.open().filter(|place| place.signed_in()).map(|place| {
        let own = |message: PageMessage| Message::At(place.id(), PlaceMessage::Page(message));
        SettingsGroup {
            heading: Some(label(place.server().shown())),
            pages: vec![page(Phrase::Account, open == Open::Account, own(PageMessage::OpenAccount))],
        }
    });
    account
        .into_iter()
        .chain([SettingsGroup {
            heading: Some(phrase(Phrase::ThisDevice, words)),
            pages: vec![
                page(Phrase::Appearance, open == Open::Appearance, Message::OpenDevice(DevicePage::Appearance)),
                page(
                    Phrase::VoiceAndVideo,
                    open == Open::VoiceAndVideo,
                    Message::OpenDevice(DevicePage::VoiceAndVideo),
                ),
                page(
                    Phrase::Notifications,
                    open == Open::Notifications,
                    Message::OpenDevice(DevicePage::Notifications),
                ),
            ]
            .into_iter()
            .chain(startup)
            .chain([page(Phrase::About, open == Open::About, Message::OpenDevice(DevicePage::AboutChatkord))])
            .collect(),
        }])
        .collect()
}

pub(crate) fn view<'a, R: Renderer + 'a>(
    window: &'a screens::Window,
    fold: screens::frame::SettingsShown,
    place: &'a Place,
    open: &'a Account,
    paint: &crate::picture::Picture<'a, R>,
    words: &'a Words,
) -> Part<'a, R> {
    let id = place.id();
    let server = place.server().shown().to_string();
    let page = move |said: PageMessage| Message::At(id, PlaceMessage::Page(said));
    let username = place.identity().unwrap_or_default();
    let fixed = container(text(username.to_string(), TypeStyle::Body))
        .padding(Padding::from([0.0, Space::M.px()]))
        .center_y(Length::Fixed(CONTROL_HEIGHT))
        .width(Length::Fill)
        .class(Surfaced::Framed(Surface::Raised, INPUT_RADIUS));
    let said = open.said(&server);
    let field = TextInput::new(phrase(Phrase::NameOthersSee, words), open.typed())
        .placeholder(username)
        .invalid(said.as_ref().is_some_and(screens::FieldSaid::refuses))
        .on_input(move |typed| page(PageMessage::Name(NameFieldMessage::Typed(typed))))
        .on_submit(page(PageMessage::Name(NameFieldMessage::Save)))
        .view::<R>();
    let mut fields = vec![
        named_aside(Phrase::UsernameLabel, Phrase::CantBeChanged, None, fixed._boxed(), words),
        named_aside(
            Phrase::NameOthersSee,
            Phrase::Optional,
            said.map(|said| crate::field_said(&said, words)),
            field,
            words,
        ),
    ];
    let kind = place.profile_preview().map_or(AvatarKind::Person, AvatarKind::Called);
    let row = crate::picture_row::person_picture_row(
        place.shown().unwrap_or(username),
        kind,
        open.picture(),
        paint,
        move |message| page(PageMessage::Picture(message)),
        words,
    );
    let named = crate::field_label(words.phrase(Phrase::ProfilePicture).to_string());
    fields.push(Column::with_children([named, row]).spacing(Space::Xs.px())._boxed());
    // A server that keeps no colour (`profile-colour`) has no row for it.
    if let Some(picker) = open.colour() {
        let picker = colour_picker(
            &phrase(Phrase::YourColour, words),
            picker,
            move |message| page(PageMessage::ProfileColour(message)),
            words,
        );
        // The picture row above shows the colour on the initials.
        fields.push(picker);
    }
    let fields = Column::with_children(fields).spacing(Space::L.px()).width(Length::Fill.max(FORM_WIDTH));
    let rows = vec![
        settings_row(
            words.phrase(Phrase::PasswordLabel).to_string(),
            None,
            Button::new(phrase(Phrase::ChangePasswordAction, words))
                .compact()
                .on_press(page(PageMessage::Account(AccountMessage::Change)))
                .view::<R>(),
            RowTone::Plain,
        ),
        settings_row(
            words.say(&Sentence::SignOutOf(server.clone())),
            Some(words.phrase(Phrase::OnThisDevice).to_string()),
            Button::new(phrase(Phrase::SignOutAction, words))
                .compact()
                .on_press(Message::Pick(id, Pick::SignOut))
                .view::<R>(),
            RowTone::Plain,
        ),
        settings_row(
            words.say(&Sentence::Leave(server)),
            None,
            Button::new(phrase(Phrase::LeaveAction, words))
                .variant(ButtonVariant::Danger)
                .compact()
                .on_press(Message::Pick(id, Pick::Leave))
                .view::<R>(),
            RowTone::Danger,
        ),
    ];
    let heading = phrase(Phrase::SignInHeading, words);
    // The card is as wide as the form it follows, so the page reads as one column.
    let sign_in = container(settings_section(heading.clone(), None, flush_card(heading, rows)))
        .width(Length::Fill.max(FORM_WIDTH));
    let content = settings_body(vec![fields._boxed(), sign_in._boxed()]);
    let close = page(PageMessage::CloseAccount);
    let frame = crate::settings_frame(
        &phrase(Phrase::SettingsLabel, words),
        pages(window, Open::Account, words),
        phrase(Phrase::Account, words),
        None,
        content,
        crate::SettingsWays {
            close: (phrase(Phrase::Close, words), close.clone()),
            shown: fold,
            back: (phrase(Phrase::SettingsBack, words), Message::SettingsList),
        },
    );
    crate::on_escape(frame, close, None)._boxed()
}
