//! A server's settings in the window: the pages this account may open, beside the open one: Overview, Roles,
//! Channels, Emoji, Invites and Members, each with what this account may do there.

use crate::labels::{label, phrase};
use crate::menu::{MenuCheck, menu_checks, menu_hung};
use crate::settings::Heading;
use crate::{
    AvatarKind, Button, Icon, IconClass, ListRow, MenuEntry, Renderer, SettingsGroup, SettingsPage, TextClass,
    TextInput, Theme, avatar_pictured, colour_picker, coloured, icon, line, menu_items, text,
};
use design::{
    AvatarSize, ButtonVariant, COLOUR_DOT, Content, DEVICE_COLUMN_WIDTH, FORM_WIDTH, IconSize, Popup, ROLE_LIST_WIDTH,
    Role, Space, TypeStyle,
};
use iced_core::image::Handle;
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Column, Row, container};
use proto::files::FileId;
use screens::channel_settings::Shows;
use screens::frame::{Frame, RolesShown};
use screens::{EmojiNamed, EmojiPageMessage, EmojiSettings};
use screens::{
    ListedMember, ListedRole, Loaded, MemberAction, MemberMenu, MemberTab, MembersMessage, NameFieldMessage, Overview,
    OverviewMessage, Page, Pick, Place, PlaceMessage, ReportLine, ReportMessage, RoleKind, RolePlace, RolesMessage,
    RolesView, ServerAbout, ServerSettings, WindowMessage as Message,
};
use words::{Phrase, Sentence, Text, Words};

type Part<'a, R> = Element<'a, Message, Theme, R>;

/// What a page of the server's settings sends, as the open place's.
pub(crate) fn page_message(place: &Place, message: screens::PageMessage) -> Message {
    Message::At(place.id(), PlaceMessage::Page(message))
}

/// The server's settings open on a page; `file` is a picture the host holds ready to paint (an emoji's).
pub(crate) fn view<'a, R: Renderer + 'a>(
    window: &'a screens::Window,
    frame: &Frame,
    place: &'a Place,
    open: &'a ServerSettings,
    paint: &crate::picture::Picture<'a, R>,
    file: &dyn Fn(FileId) -> Option<Handle>,
    words: &'a Words,
) -> Part<'a, R> {
    let id = place.id();
    let fold = window.settings_shown(frame);
    let pages = place
        .pages()
        .into_iter()
        .map(|page| SettingsPage {
            label: phrase(name(page), words),
            current: page == open.page(),
            message: Message::At(id, PlaceMessage::Page(screens::PageMessage::Settings(page))),
        })
        .collect();
    let pages = vec![SettingsGroup { heading: None, pages }];
    let titled = |page: Phrase| Heading::Titled(phrase(page, words));
    let (heading, action, content) = match (open.page(), open.overview(), place.roles()) {
        (Page::Overview, Some(page), _) => (titled(Phrase::Overview), None, overview(place, page, paint, words)),
        (Page::Roles, _, Some(roles)) => {
            roles_page(place, open, roles, window.roles_shown(frame).unwrap_or(RolesShown::Beside), words)
        }
        // A page and its model are made together; one without the other is drawn as nothing.
        (Page::Overview | Page::Roles, _, _) => (titled(name(open.page())), None, iced_widget::Space::new()._boxed()),
        (Page::Channels, _, _) => crate::channels_page::channels_page(
            place,
            open,
            window.channels_shown(frame).unwrap_or(Shows::Beside),
            file,
            words,
        ),
        (Page::Emoji, _, _) => {
            let (action, content) = emoji(place, open, file, words);
            (titled(Phrase::Emoji), Some(action), content)
        }
        (Page::Invites, _, _) => {
            let (heading, action, content) = invites(place, open, words);
            (titled(heading), action, content)
        }
        (Page::Members, _, _) => {
            (titled(Phrase::Members), None, members(place, open, frame.search_beside(fold), file, words))
        }
        (Page::About, _, _) => (titled(Phrase::About), None, about(place, open, words)),
        (Page::Webhooks, _, _) => match open.webhooks() {
            Some(hooks) => {
                let (action, content) = crate::webhooks_page::page(window, place, hooks, file, words);
                (titled(Phrase::Webhooks), Some(action), content)
            }
            // A page and its model are made together; one without the other is drawn as nothing.
            None => (titled(Phrase::Webhooks), None, iced_widget::Space::new()._boxed()),
        },
    };
    let close = Message::At(id, PlaceMessage::Page(screens::PageMessage::CloseSettings));
    // The roles and channels pages say what they say under the line about the order, as the canvas does; the rest
    // above all.
    let content = match open.page() {
        Page::Roles | Page::Channels => content,
        Page::Overview | Page::Emoji | Page::Invites | Page::Members | Page::About | Page::Webhooks => {
            said_above(place, open, content, words)
        }
    };
    let frame = crate::settings_frame(
        &label(place.server().shown()),
        pages,
        heading,
        action,
        content,
        crate::SettingsWays {
            close: (phrase(Phrase::Close, words), close.clone()),
            shown: fold,
            back: (phrase(Phrase::SettingsBack, words), Message::SettingsList),
        },
    );
    let frame = crate::on_escape(frame, close, None)._boxed();
    // The webhooks page's dialogs lie over the whole page.
    match open.webhooks().and_then(|hooks| crate::webhooks_page::dialog::<R>(place, hooks, words)) {
        Some((dialog, away)) => crate::dialog(frame, Some(dialog), away),
        None => frame,
    }
}

/// `content` under what the open page says.
pub(crate) fn said_above<'a, R: Renderer + 'a>(
    place: &Place,
    open: &ServerSettings,
    content: Part<'a, R>,
    words: &'a Words,
) -> Part<'a, R> {
    let id = place.id();
    crate::callouts_above(
        &open.notices(),
        design::SETTINGS_LIST_WIDTH,
        |subject| Message::At(id, PlaceMessage::Page(screens::PageMessage::DismissPageNotice(subject))),
        content,
        words,
    )
}

const fn name(page: Page) -> Phrase {
    match page {
        Page::Overview => Phrase::Overview,
        Page::Roles => Phrase::Roles,
        Page::Channels => Phrase::Channels,
        Page::Emoji => Phrase::Emoji,
        Page::Invites => Phrase::Invites,
        Page::Members => Phrase::Members,
        Page::About => Phrase::About,
        Page::Webhooks => Phrase::Webhooks,
    }
}

/// Overview: the server's picture, set once it is sent; its name, saved on Enter or when the page is left; and its
/// colour, saved when the picker closes.
fn overview<'a, R: Renderer + 'a>(
    place: &'a Place,
    open: &'a Overview,
    paint: &crate::picture::Picture<'a, R>,
    words: &'a Words,
) -> Part<'a, R> {
    let id = place.id();
    let note = open.name().note().said(place.server().shown());
    let danger = note.as_ref().is_some_and(screens::FieldSaid::refuses);
    let field = TextInput::new(phrase(Phrase::ServerName, words), open.name().typed())
        .placeholder(place.server().host())
        .invalid(danger)
        .on_input(move |typed| {
            Message::At(
                id,
                PlaceMessage::Page(screens::PageMessage::Overview(OverviewMessage::Name(NameFieldMessage::Typed(
                    typed,
                )))),
            )
        })
        .on_submit(Message::At(
            id,
            PlaceMessage::Page(screens::PageMessage::Overview(OverviewMessage::Name(NameFieldMessage::Save))),
        ))
        .view::<R>();
    let note = note.map(|said| crate::field_said(&said, words));
    let name = crate::door::named(Phrase::ServerName, note, field, words);
    let colour = colour_picker(
        &phrase(Phrase::ColourLabel, words),
        open.colour(),
        move |message| {
            Message::At(id, PlaceMessage::Page(screens::PageMessage::Overview(OverviewMessage::Colour(message))))
        },
        words,
    );
    // The server's picture, or its letter on the colour as it is being picked, so the pick shows where it will be seen.
    let picked = screens::colour::rgb(open.colour().picked());
    let picture = crate::picture_row::picture_row(
        place.server().shown(),
        picked,
        open.picture(),
        paint,
        move |message| Message::At(id, PlaceMessage::Page(screens::PageMessage::Picture(message))),
        words,
    );
    let form = Column::with_children([picture, name._boxed(), colour])
        .spacing(Space::L.px())
        .width(Length::Fill.max(FORM_WIDTH));
    let switches = switches(place, open, words);
    let hosting = crate::hosting_form::view(place, open.hosting(), words);
    let parts = std::iter::once(form._boxed()).chain(switches).chain(hosting);
    Column::with_children(parts).spacing(Space::L.px()).width(Length::Fill.max(design::SETTINGS_LIST_WIDTH))._boxed()
}

/// Features, each a part of the page: the heading, a switch for each thing the server may withdraw, and the line that
/// says what turning one off does.
fn switches<'a, R: Renderer + 'a>(place: &'a Place, open: &'a Overview, words: &'a Words) -> Vec<Part<'a, R>> {
    let toggles = open.switches();
    let id = place.id();
    let rows = toggles
        .iter()
        .map(|toggle| {
            let (name, line) = Phrase::switch(toggle.switch());
            let flip = (!toggle.asking()).then(|| {
                Message::At(
                    id,
                    PlaceMessage::Page(screens::PageMessage::Overview(OverviewMessage::Switch(
                        toggle.switch(),
                        !toggle.on(),
                    ))),
                )
            });
            crate::switch_offered(&phrase(name, words), Some(&words.phrase(line)), toggle.on(), flip)
        })
        .collect();
    let refused = toggles
        .iter()
        .find_map(|toggle| toggle.refused())
        .map(|failed| crate::refusal(words.say(&words::failure::said(failed))));
    let note = coloured(words.phrase(Phrase::SwitchesNote), TypeStyle::Small, TextClass::Content(Content::Secondary));
    let card = Column::with_children(
        std::iter::once(crate::list_card(phrase(Phrase::Features, words), rows, None)).chain(refused),
    )
    .spacing(Space::Xs.px());
    let width = Length::Fill.max(DEVICE_COLUMN_WIDTH);
    vec![
        crate::about_page::heading(Phrase::Features, words),
        card.width(width)._boxed(),
        container(note).width(width)._boxed(),
    ]
}

/// About: the server's release and protocol in the card the client's About draws, then the server's debug report under
/// the heading the client's has.
fn about<'a, R: Renderer + 'a>(place: &'a Place, open: &'a ServerSettings, words: &'a Words) -> Part<'a, R> {
    use crate::about_page::{heading, version_card};
    let facts = place
        .build()
        .map(|build| (Phrase::AboutVersion, build.to_string()))
        .into_iter()
        .chain(place.protocol().map(|version| (Phrase::AboutProtocol, screens::protocol_said(version))))
        .collect::<Vec<_>>();
    let facts = (!facts.is_empty()).then(|| {
        crate::settings_section(phrase(Phrase::AboutServer, words), None, version_card(Phrase::About, facts, words))
    });
    let report = open.about().map(|about| [heading(Phrase::DebugReport, words), server_report(place, about, words)]);
    Column::with_children(facts.into_iter().chain(report.into_iter().flatten()))
        .spacing(Space::L.px())
        .width(Length::Fill.max(DEVICE_COLUMN_WIDTH))
        ._boxed()
}

/// The card of the server's debug report, drawn as the client's is: Download asks the server for it.
fn server_report<'a, R: Renderer + 'a>(place: &'a Place, open: &'a ServerAbout, words: &'a Words) -> Part<'a, R> {
    use crate::about_page::{ReportButton, ReportCard, report_card};
    let report = open.report();
    let (line, alert) = match report.line() {
        ReportLine::Contains => (words.phrase(Phrase::ServerDebugReportLine).into_owned(), false),
        ReportLine::Downloaded(file) => (words.say(&Sentence::ReportDownloaded(file.clone())), false),
        ReportLine::Refused(said) => (words.say(said), true),
    };
    let button = if report.preparing() {
        ReportButton::Preparing
    } else {
        let asked = screens::ServerAboutMessage::Report(ReportMessage::Open);
        ReportButton::Press(Box::new(page_message(place, screens::PageMessage::About(asked))))
    };
    let card = ReportCard { name: Phrase::ServerDebugReport, title: Phrase::ServerDebugReport, line, alert, button };
    report_card(card, words)
}

/// The invites page: its heading, its main action, and what it shows.
/// Emoji: how many the server holds of the most and Upload emoji, as the page's action; then a card for each, a new
/// one being named first, and the one being named across its row.
fn emoji<'a, R: Renderer + 'a>(
    place: &'a Place,
    open: &'a ServerSettings,
    file: &dyn Fn(FileId) -> Option<Handle>,
    words: &'a Words,
) -> (Part<'a, R>, Part<'a, R>) {
    let id = place.id();
    let send = move |said: EmojiPageMessage| page_message(place, screens::PageMessage::Emoji(said));
    let list = place.emoji().all();
    let page = open.emoji();
    let held = u32::try_from(list.len()).unwrap_or(u32::MAX);
    let most = u32::try_from(proto::rules::MAX_EMOJI).unwrap_or(u32::MAX);
    let upload =
        page.filter(|page| page.doing() == screens::EmojiDoing::Nothing).map(|_| send(EmojiPageMessage::Upload));
    let action = crate::emoji_action(held, most, upload, words);
    let naming = page.and_then(EmojiSettings::naming);
    let mut cards = Vec::new();
    let mut at = None;
    if let Some((naming, EmojiNamed::New(new))) = naming.map(|naming| (naming, naming.named())) {
        let me = place.me().cloned();
        at = Some(0);
        cards.push(crate::EmojiCard {
            name: naming.draft().to_string(),
            picture: file(*new),
            added_by: place.shown().map_or_else(|| words.phrase(Phrase::FormerMember).into_owned(), str::to_string),
            added_colour: me.as_ref().map_or(design::PersonColour::Swatch(design::Swatch::ALL[0]), |me| {
                place.own_colour().unwrap_or_else(|| screens::colour_of(me, None))
            }),
            added_picture: me.as_ref().and_then(|me| place.picture_of(me)).and_then(file),
            rename: send(EmojiPageMessage::Cancel),
            delete: send(EmojiPageMessage::Cancel),
        });
    }
    for emoji in list {
        if naming.is_some_and(|naming| *naming.named() == EmojiNamed::Kept(emoji.id.clone())) {
            at = Some(cards.len());
        }
        let by = emoji.made_by.as_ref();
        let added_by = by
            .and_then(|user_id| place.name_of(user_id))
            .unwrap_or_else(|| words.phrase(Phrase::FormerMember).into_owned());
        let added_colour = by.map_or(design::PersonColour::Swatch(design::Swatch::ALL[0]), |user_id| {
            screens::colour_of(user_id, place.directory().person(user_id).and_then(|person| person.colour))
        });
        cards.push(crate::EmojiCard {
            name: emoji.name.as_str().to_string(),
            picture: file(emoji.file),
            added_by,
            added_colour,
            added_picture: by.and_then(|user_id| place.picture_of(user_id)).and_then(file),
            rename: send(EmojiPageMessage::Rename(emoji.id.clone())),
            delete: send(EmojiPageMessage::Delete(emoji.id.clone())),
        });
    }
    let renaming = naming.zip(at).map(|(naming, at)| crate::Renaming {
        at,
        draft: naming.draft(),
        on_change: Box::new(move |typed| {
            Message::At(id, PlaceMessage::Page(screens::PageMessage::Emoji(EmojiPageMessage::Draft(typed))))
        }),
        on_submit: send(EmojiPageMessage::Save),
        on_escape: send(EmojiPageMessage::Cancel),
        refused: naming.misshapen(),
        new: matches!(naming.named(), EmojiNamed::New(_)),
    });
    let refused = page.and_then(EmojiSettings::refused).map(|said| (words.say(said), send(EmojiPageMessage::Dismiss)));
    let content = crate::emoji_page(place.server().shown(), held, most, refused, cards, renaming, words);
    (action, content)
}

fn invites<'a, R: Renderer + 'a>(
    place: &'a Place,
    open: &'a ServerSettings,
    words: &'a Words,
) -> (Phrase, Option<Part<'a, R>>, Part<'a, R>) {
    let id = place.id();
    let new = Button::new(phrase(Phrase::NewInvite, words))
        .variant(ButtonVariant::Primary)
        .compact()
        .on_press(Message::Pick(id, Pick::Invite))
        .view::<R>();
    let quiet = |said: String| coloured(said, TypeStyle::Body, TextClass::Content(Content::Secondary))._boxed();
    let below = match open.invites() {
        Loaded::Loading => None,
        Loaded::Failed(_) => Some(no_answer(place, words)),
        Loaded::Ready(invites) => {
            let rows = invites
                .iter()
                .map(|invite| {
                    let note = invite.note.clone().unwrap_or_else(|| words.phrase(Phrase::NoNote).to_string());
                    let delete = Button::new(phrase(Phrase::Delete, words))
                        .variant(ButtonVariant::Danger)
                        .compact()
                        .on_press(Message::DeleteInvite(id, invite.id.clone()))
                        .view::<R>();
                    let role = place.role_list().granted_name(invite.role.as_ref(), words);
                    vec![
                        line(note, TypeStyle::Small)._boxed(),
                        line(role, TypeStyle::Small)._boxed(),
                        line(words.say(&Sentence::Used { used: invite.used, of: invite.of }), TypeStyle::Small)
                            ._boxed(),
                        line(words.say(&Sentence::Ends(invite.ends)), TypeStyle::Small)._boxed(),
                        container(delete).align_right(Length::Fill)._boxed(),
                    ]
                })
                .collect();
            let heads = [Phrase::NoteLabel, Phrase::RoleLabel, Phrase::UsedHead, Phrase::EndsHead]
                .map(|head| words.phrase(head).to_string())
                .into_iter()
                .chain([String::new()])
                .collect();
            Some(crate::table(
                phrase(Phrase::Invites, words),
                heads,
                rows,
                Some(quiet(words.phrase(Phrase::NoInvites).to_string())),
            ))
        }
    };
    (Phrase::Invites, Some(new), below.unwrap_or_else(|| iced_widget::Space::new()._boxed()))
}

/// Why a list is missing: the server did not answer.
pub(crate) fn no_answer<'a, R: Renderer + 'a>(place: &Place, words: &'a Words) -> Part<'a, R> {
    crate::refusal(words.say(&Sentence::NoAnswer(place.server().shown().to_string())))
}

/// The roles page: their order, which a drag or Alt and an arrow key changes, beside the chosen role; where the two
/// do not fit, the list alone, or the role opened from it with Back to the list in place of the page's heading.
fn roles_page<'a, R: Renderer + 'a>(
    place: &'a Place,
    open: &ServerSettings,
    roles: RolesView<'a>,
    shown: RolesShown,
    words: &'a Words,
) -> (Heading<Message>, Option<Part<'a, R>>, Part<'a, R>) {
    let id = place.id();
    let back = page_message(place, screens::PageMessage::Roles(RolesMessage::Back));
    let list = match roles.listed() {
        Loaded::Loading => {
            return (Heading::Titled(phrase(Phrase::Roles, words)), None, iced_widget::Space::new()._boxed());
        }
        Loaded::Failed(_) => return (Heading::Titled(phrase(Phrase::Roles, words)), None, no_answer(place, words)),
        Loaded::Ready(listed) => listed,
    };
    let editor = || role_editor(place, roles, words);
    if shown == RolesShown::Role {
        return (Heading::Back(phrase(Phrase::Roles, words), back), None, said_above(place, open, editor(), words));
    }
    let create = Button::new(phrase(Phrase::CreateRole, words))
        .variant(ButtonVariant::Primary)
        .compact()
        .on_press(Message::CreateRole(id))
        .view::<R>();
    let hint = format!("{} {}", words.phrase(Phrase::RolesHint), words.phrase(Phrase::RolesHintMove));
    let hint = coloured(hint, TypeStyle::Small, TextClass::Content(Content::Secondary))
        .width(Length::Fill.max(design::SETTINGS_LIST_WIDTH));
    let order = role_list(place, roles, list, words);
    let body = match shown {
        RolesShown::Beside => {
            Row::with_children([container(order).width(Length::Fixed(ROLE_LIST_WIDTH))._boxed(), editor()])
                .spacing(Space::Xl.px())
                ._boxed()
        }
        RolesShown::List | RolesShown::Role => order,
    };
    let body = said_above(place, open, body, words);
    let content = Column::with_children([hint._boxed(), body]).spacing(Space::L.px())._boxed();
    (Heading::Titled(phrase(Phrase::Roles, words)), Some(create), content)
}

/// The roles in their order, in a card: each with a grip where it moves, a lock where this account cannot change it,
/// its colour and its name, and the owner's and the default role's notes.
fn role_list<'a, R: Renderer + 'a>(
    place: &'a Place,
    roles: RolesView<'a>,
    listed: &'a [ListedRole],
    words: &'a Words,
) -> Part<'a, R> {
    let asker = place.role_place();
    let chosen = roles.chosen().map(|role| &role.id);
    let rows = listed.iter().enumerate().map(|(at, role)| {
        let mark = if role.movable(asker) {
            Some(Icon::Grip)
        } else if role.changeable(asker) {
            None
        } else {
            Some(Icon::Lock)
        };
        let mark: Part<'a, R> = match mark {
            Some(shown) => icon(shown, IconSize::S, IconClass::Role(Role::Content(Content::Secondary)))._boxed(),
            None => iced_widget::Space::new().width(Length::Fixed(IconSize::S.px()))._boxed(),
        };
        let marks = Row::with_children([mark, crate::list_row::colour_dot(role.colour.map(screens::colour::rgb))])
            .spacing(Space::S.px() + Space::Xxs.px())
            .align_y(Alignment::Center);
        let choose = page_message(place, screens::PageMessage::Roles(RolesMessage::Choose(role.id.clone())));
        let mut row = ListRow::<Message, R>::new(label(&role.name))
            .leading(marks._boxed())
            .selected(chosen == Some(&role.id))
            .on_press(choose);
        let note = match role.kind {
            RoleKind::Owner => Some(Phrase::ServerOwner),
            RoleKind::Default => Some(Phrase::Everyone),
            RoleKind::Made => None,
        };
        if let Some(note) = note {
            row = row.noted(words.phrase(note).as_ref());
        }
        // Each row as tall as the canvas's, its control set in from the card's edge as a headed card's are.
        let row = container(row.view()).padding(Padding::from([Space::Xs.px() + Space::Xxs.px(), Space::Xs.px()]));
        let row = if at == 0 {
            row._boxed()
        } else {
            Column::with_children([crate::panel_parts::hairline(), row._boxed()])._boxed()
        };
        (role.id.clone(), row)
    });
    let list = crate::reorder(
        phrase(Phrase::RolesHighestFirst, words),
        rows.collect::<Vec<_>>(),
        move |moved| roles.may_move(moved, asker),
        move |moved| {
            Message::At(place.id(), PlaceMessage::Page(screens::PageMessage::Roles(RolesMessage::Move(moved))))
        },
    );
    crate::table::card_around(list)
}

/// The chosen role: its colour and name over what may be changed of it, or why nothing may.
fn role_editor<'a, R: Renderer + 'a>(place: &'a Place, roles: RolesView<'a>, words: &'a Words) -> Part<'a, R> {
    let Some(role) = roles.chosen() else { return iced_widget::Space::new()._boxed() };
    let asker = place.role_place();
    let roles_message = |message: RolesMessage| page_message(place, screens::PageMessage::Roles(message));
    let quiet = |said: String| coloured(said, TypeStyle::Small, TextClass::Content(Content::Secondary))._boxed();
    let title = Row::with_children([
        container(crate::list_row::colour_dot(role.colour.map(screens::colour::rgb)))
            .center(Length::Fixed(COLOUR_DOT + Space::Xs.px()))
            ._boxed(),
        text(role.name.clone(), TypeStyle::Subheading)._boxed(),
    ])
    .spacing(Space::S.px())
    .align_y(Alignment::Center);
    let mut parts: Vec<Part<'a, R>> = vec![title._boxed()];
    let editor = roles.editor().filter(|_| role.changeable(asker));
    let Some(editor) = editor else {
        parts.push(match role.kind {
            RoleKind::Owner => quiet(words.text(Text::OwnerRoleBody).to_string()),
            RoleKind::Made | RoleKind::Default => quiet(words.say(&Sentence::RoleLocked(role.name.clone()))),
        });
        return Column::with_children(parts).spacing(Space::M.px()).width(Length::Fill)._boxed();
    };
    if role.kind == RoleKind::Default {
        parts.push(quiet(words.phrase(Phrase::DefaultRoleBody).to_string()));
    }
    let note = editor.name().note().said(place.server().shown());
    let danger = note.as_ref().is_some_and(screens::FieldSaid::refuses);
    let field = TextInput::new(phrase(Phrase::RoleName, words), editor.name().typed())
        .invalid(danger)
        .on_input(move |typed| roles_message(RolesMessage::Name(NameFieldMessage::Typed(typed))))
        .on_submit(roles_message(RolesMessage::Name(NameFieldMessage::Save)))
        .view::<R>();
    let field = crate::focus::on_leave(field, roles_message(RolesMessage::NameLeft))._boxed();
    let note = note.map(|said| crate::field_said(&said, words));
    let name = container(crate::door::named(Phrase::RoleName, note, field, words)).width(Length::Fill.max(FORM_WIDTH));
    parts.push(name._boxed());
    parts.push(crate::colour_picker::colour_picker_helped(
        &phrase(Phrase::ColourLabel, words),
        words.phrase(Phrase::RoleColourHelp).to_string(),
        editor.colour(),
        move |message| roles_message(RolesMessage::Colour(message)),
        words,
    ));
    let grants = place.grants();
    if !grants.holds_all(proto::rules::Grants::every()) {
        parts.push(quiet(words.phrase(Phrase::GiveOnlyHeld).to_string()));
    }
    for (heading, grants_of) in screens::WORD_GROUPS {
        let switches = grants_of
            .iter()
            .map(|grant| {
                let (name, line) = screens::grant_words(*grant);
                let on = role.grants.holds(*grant);
                let flip = grants.holds(*grant).then(|| roles_message(RolesMessage::Word(*grant, !on)));
                crate::switch_offered(&phrase(name, words), Some(&words.phrase(line)), on, flip)
            })
            .collect();
        let section = Column::with_children([
            crate::panel_parts::section_heading(&phrase(heading, words))._boxed(),
            crate::list_card(phrase(heading, words), switches, None),
        ])
        .spacing(Space::Xs.px());
        parts.push(section._boxed());
    }
    if role.movable(asker) {
        parts.push(
            container(
                Button::new(phrase(Phrase::DeleteRole, words))
                    .variant(ButtonVariant::Danger)
                    .compact()
                    .on_press(Message::DeleteRole(place.id()))
                    .view::<R>(),
            )
            ._boxed(),
        );
    }
    Column::with_children(parts).spacing(Space::L.px()).width(Length::Fill.max(design::SETTINGS_LIST_WIDTH))._boxed()
}

/// The members page: the members or the banned, as the pills pick, found by the search, each with what this account may
/// do to them.
fn members<'a, R: Renderer + 'a>(
    place: &'a Place,
    open: &'a ServerSettings,
    beside: bool,
    file: &dyn Fn(FileId) -> Option<Handle>,
    words: &'a Words,
) -> Part<'a, R> {
    let all = match open.members() {
        Loaded::Loading => return iced_widget::Space::new()._boxed(),
        Loaded::Failed(_) => return no_answer(place, words),
        Loaded::Ready(members) => members,
    };
    let finding = open.finding();
    let on = |message: MembersMessage| page_message(place, screens::PageMessage::Members(message));
    let (count, banned) = screens::member_counts(all);
    let tabs = crate::choice_track(
        phrase(Phrase::WhoToList, words),
        vec![
            (label(&words.say(&Sentence::MembersCount(count))), on(MembersMessage::Tab(MemberTab::Members))),
            (label(&words.say(&Sentence::BannedCount(banned))), on(MembersMessage::Tab(MemberTab::Banned))),
        ],
        match finding.tab() {
            MemberTab::Members => 0,
            MemberTab::Banned => 1,
        },
    );
    let search = crate::search_field(
        phrase(Phrase::SearchMembers, words),
        words.phrase(Phrase::SearchByName).as_ref(),
        phrase(Phrase::ClearSearch, words),
        finding.query(),
        move |typed| on(MembersMessage::Search(typed)),
        on(MembersMessage::ClearSearch),
    );
    let bar = if beside {
        Row::with_children([tabs, search]).spacing(Space::M.px()).align_y(Alignment::Center)._boxed()
    } else {
        Column::with_children([tabs, search]).spacing(Space::M.px())._boxed()
    };
    let asker = place.role_place();
    let rows = finding
        .shown(all)
        .into_iter()
        .map(|member| {
            let picture = place.picture_of(&member.user_id).and_then(file);
            member_row(place, open, asker, member, picture, words)
        })
        .collect();
    let empty = coloured(words.say(&finding.none_said()), TypeStyle::Body, TextClass::Content(Content::Secondary));
    let list = crate::list_card(phrase(Phrase::Members, words), rows, Some(empty._boxed()));
    Column::with_children([bar, list])
        .spacing(Space::M.px())
        .width(Length::Fill.max(design::SETTINGS_LIST_WIDTH))
        ._boxed()
}

/// A member's roles in one line, cut with an ellipsis, the whole list in its tooltip.
fn caption<'a, R: Renderer + 'a>(member: &ListedMember, place: &Place, words: &Words) -> Part<'a, R> {
    let said = line(words.say(&member.caption(place.role_list())), TypeStyle::Caption)
        .class(TextClass::Content(Content::Secondary));
    match member.roles_said(place.role_list()) {
        Some(all) => crate::tooltip(said._boxed(), &label(&words.say(&all)), iced_widget::tooltip::Position::Top),
        None => said._boxed(),
    }
}

fn member_row<'a, R: Renderer + 'a>(
    place: &'a Place,
    open: &'a ServerSettings,
    asker: RolePlace,
    member: &'a ListedMember,
    picture: Option<Handle>,
    words: &'a Words,
) -> Part<'a, R> {
    let id = place.id();
    let grants = place.grants();
    let who =
        Row::with_children([line(member.name.clone(), TypeStyle::BodyStrong)._boxed(), caption(member, place, words)])
            .spacing(Space::Xs.px())
            .align_y(Alignment::Center);
    let menu_of =
        |kind| page_message(place, screens::PageMessage::Members(MembersMessage::Menu(member.user_id.clone(), kind)));
    let open_menu = open.finding().menu().filter(|(who, _)| **who == member.user_id).map(|(_, kind)| kind);
    let act = |action: MemberAction| Message::Member(id, action, member.user_id.clone());
    let offered = member.roles(place.role_list(), asker, grants);
    let more = member.more(asker, grants);
    let mut buttons: Vec<Part<'a, R>> = Vec::new();
    if !offered.is_empty() {
        buttons.push(
            Button::new(phrase(Phrase::Roles, words))
                .icon_after(Icon::ChevronDown)
                .variant(ButtonVariant::Secondary)
                .compact()
                .opens(Popup::Menu, open_menu == Some(MemberMenu::Roles))
                .on_press(menu_of(MemberMenu::Roles))
                .view::<R>(),
        );
    }
    if !more.is_empty() {
        buttons.push(
            Button::new(label(&words.say(&Sentence::MoreFor(member.name.clone()))))
                .icon_only(Icon::More)
                .variant(ButtonVariant::Secondary)
                .compact()
                .opens(Popup::Menu, open_menu == Some(MemberMenu::More))
                .on_press(menu_of(MemberMenu::More))
                .view::<R>(),
        );
    }
    if let Some(unban) = member.unban(asker, grants) {
        buttons.push(
            Button::new(label(&words.say(&unban.said(&member.name))))
                .variant(ButtonVariant::Secondary)
                .compact()
                .on_press(act(unban))
                .view::<R>(),
        );
    }
    let items = open_menu.map(|kind| match kind {
        MemberMenu::Roles => menu_checks(
            label(&words.say(&Sentence::RolesFor(member.name.clone()))),
            offered
                .iter()
                .map(|role| MenuCheck {
                    label: label(&role.name),
                    colour: role.colour.map(screens::colour::rgb),
                    checked: role.held,
                    message: act(role.action()),
                })
                .collect(),
        ),
        MemberMenu::More => {
            let mut ruled = false;
            let entries = more
                .iter()
                .map(|action| {
                    let rule_before = action.danger() && !std::mem::replace(&mut ruled, action.danger());
                    MenuEntry {
                        label: label(&words.say(&action.said(&member.name))),
                        message: act(action.clone()),
                        danger: action.danger(),
                        rule_before,
                        icon: None,
                    }
                })
                .collect();
            menu_items(label(&member.name), entries)
        }
    });
    let buttons = Row::with_children(buttons).spacing(Space::Xs.px()).align_y(Alignment::Center);
    let close = page_message(place, screens::PageMessage::Members(MembersMessage::CloseMenu));
    let buttons = menu_hung(buttons._boxed(), items, close);
    let words = Row::with_children([
        avatar_pictured(
            label(&member.name),
            AvatarKind::Called(screens::colour_of(&member.user_id, member.colour)),
            AvatarSize::Row,
            picture,
        ),
        who._boxed(),
    ])
    .spacing(Space::M.px())
    .align_y(Alignment::Center);
    // The words keep their least beside the buttons, the picture before them aside.
    let indent = AvatarSize::Row.px() + Space::M.px();
    crate::beside(words._boxed(), buttons, design::ROW_WORDS_LEAST + indent, Space::M.px(), indent)
}
