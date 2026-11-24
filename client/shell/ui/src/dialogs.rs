//! The dialogs a server's menu and its settings open over the window: the confirmations, inviting people, naming a
//! channel, the owner's set-up and the server's Notification settings. Every word is the model's.

use crate::access_card::AccessSaid;
use crate::custom_choice::{CustomField, CustomNote, Taken, custom_choice};
use crate::labels::{label, phrase};
use crate::menu::{entry_rows, menu_hung, submenu};
use crate::{
    Button, Icon, Leading, ListRow, MenuChoice, MenuEntry, Renderer, SecretInput, Select, TextClass, TextInput, Theme,
    coloured, flush_card, list_card, text,
};
use design::{ButtonVariant, Content, Popup, Space, TypeStyle};
use iced_core::{Alignment, Element, Length, Widget};
use iced_widget::{Column, Row};
use proto::notify::Level;
use proto::rules::Kind;
use screens::{
    Access, Asking, BanAsk, Banning, ChannelProblem, CreatingRole, Days, DeleteInviteRefused, Dialog, DialogMessage,
    InviteMessage, InviteProblem, Inviting, Lobby, Making, MakingMessage, Naming, Page, Place, Proof, Refused,
    Resetting, RoleProblem, SetUp, SetUpMessage, Uses, UsesChoice, UsesMessage, Window, WindowMessage as Message,
};
use screens::{MuteFor, NotifyMessage, PlaceMessage, level_phrase};
use std::rc::Rc;
use words::{Phrase, Sentence, Text, Words};

type Part<'a, R> = Element<'a, Message, Theme, R>;

fn ask(said: DialogMessage) -> Message {
    Message::Dialog(said)
}

/// The open server's Notification settings, while its dialog is open, and what Escape and a press outside send: its
/// Mute row with the durations hung under its button, how much it notifies, and what it suppresses.
pub(crate) fn notify<'a, R: Renderer + 'a>(window: &'a Window, words: &'a Words) -> Option<(Part<'a, R>, Message)> {
    let place = window.open()?;
    let dialog = place.notify_dialog()?;
    let id = place.id();
    let said = move |message: NotifyMessage| Message::At(id, PlaceMessage::Notify(message));
    let name = place.server().shown().to_string();
    let settings = place.notifying().map(|notifying| notifying.server).unwrap_or_default();
    let muted = window.wall().is_some_and(|now| place.server_muted(now));
    let row_title = if muted { Sentence::UnmuteServer(name.clone()) } else { Sentence::MuteServer(name.clone()) };
    let row_title = words.say(&row_title);
    let named = Column::with_children([
        text(row_title.clone(), TypeStyle::BodyStrong)._boxed(),
        coloured(
            words.phrase(Phrase::MuteServerNote).into_owned(),
            TypeStyle::Small,
            TextClass::Content(Content::Secondary),
        )
        ._boxed(),
    ])
    .spacing(Space::Xxs.px())
    .width(Length::Fill);
    let button = if muted {
        Button::new(phrase(Phrase::UnmuteAction, words))
    } else {
        Button::new(phrase(Phrase::MuteAction, words))
            .icon_after(Icon::ChevronDown)
            .opens(Popup::Menu, dialog.mute_menu)
    };
    let button = button.variant(ButtonVariant::Secondary).compact().on_press(said(NotifyMessage::MuteRow)).view::<R>();
    let durations = dialog.mute_menu.then(|| {
        let entries = MuteFor::ALL
            .into_iter()
            .map(|span| MenuEntry {
                label: phrase(span.phrase(), words),
                message: said(NotifyMessage::MuteServer(span)),
                danger: false,
                rule_before: false,
                icon: None,
            })
            .collect();
        submenu(label(&row_title), entry_rows(entries))
    });
    let button = menu_hung(button, durations, said(NotifyMessage::CloseMuteMenu));
    let mute = Row::with_children([named._boxed(), button]).spacing(Space::M.px()).align_y(Alignment::Center);
    let mute = list_card(label(&row_title), vec![mute._boxed()], None);
    let about = phrase(Phrase::NotifyMeAbout, words);
    let heading =
        coloured(about.as_str().to_uppercase(), TypeStyle::CaptionStrong, TextClass::Content(Content::Secondary));
    let levels = Level::ALL
        .into_iter()
        .map(|level| {
            let mut row = ListRow::<Message, R>::new(phrase(level_phrase(level), words))
                .radio(level == settings.level)
                .on_press(said(NotifyMessage::ServerLevel(level)));
            if level == Level::Mentions {
                row = row.noted(&words.phrase(Phrase::NotifyMentionsNote));
            }
            row.view()
        })
        .collect();
    let levels = flush_card(about, levels);
    let suppress = |said_as: Phrase, on: bool, message: NotifyMessage| {
        crate::switch(&phrase(said_as, words), None, on, said(message))
    };
    let suppressed = list_card(
        phrase(Phrase::SuppressEveryone, words),
        vec![
            suppress(Phrase::SuppressEveryone, settings.suppress_everyone, NotifyMessage::SuppressEveryone),
            suppress(Phrase::SuppressRoles, settings.suppress_roles, NotifyMessage::SuppressRoles),
        ],
        None,
    );
    let levels = Column::with_children([heading._boxed(), levels]).spacing(Space::S.px());
    let body = Column::with_children([mute, levels._boxed(), suppressed]).spacing(Space::L.px());
    let close = said(NotifyMessage::CloseDialog);
    let done =
        Button::new(phrase(Phrase::Done, words)).variant(ButtonVariant::Primary).on_press(close.clone()).view::<R>();
    let title = label(&words.say(&Sentence::NotificationsFrom(name)));
    Some((crate::dialog_panel(title, body._boxed(), vec![done], close.clone(), words), close))
}

/// A dialog about a server: what it asks, a note when it ends this window's call, the password when leaving, Cancel
/// and the button that does it; or inviting people.
pub(crate) fn view<'a, R: Renderer + 'a>(
    window: &'a Window,
    dialog: &'a Dialog,
    picture: &crate::picture::Picture<'a, R>,
    handles: &crate::discussion::Handles<'_>,
    words: &'a Words,
) -> Option<Part<'a, R>> {
    let place = window.places().iter().find(|place| place.id() == dialog.place())?;
    let name = place.server().shown().to_string();
    let muted = |said: String| coloured(said, TypeStyle::Body, TextClass::Content(Content::Secondary))._boxed();
    let call = dialog
        .call()
        .map(|channel| text(words.say(&Sentence::AlsoLeavesCall(channel.to_string())), TypeStyle::Body)._boxed());
    let mut body: Vec<Part<'a, R>> = Vec::new();
    let mut keep = Some(Phrase::Cancel);
    let mut alert = true;
    let (title, doing, ready) = match dialog.asking() {
        Asking::Invite(inviting) => return Some(invite(&name, place, inviting, words)),
        Asking::Name(naming) => return Some(name_channel(&name, naming, words)),
        Asking::MakeChannel(making) => return Some(make_channel(place, &name, making, handles, words)),
        Asking::CreateRole(creating) => return Some(create_role(&name, creating, words)),
        Asking::DeleteRole(deleting) => {
            body.push(muted(words.text(Text::DeleteRoleBody).to_string()));
            let role = deleting.name().to_string();
            body.extend(
                deleting.problem().map(|problem| crate::refusal(role_problem(&name, role.clone(), problem, words))),
            );
            let doing = Button::new(label(&words.say(&Sentence::DeleteRoleAction(role.clone()))))
                .variant(ButtonVariant::Danger)
                .loading(deleting.asking());
            let doing = if deleting.asking() { doing } else { doing.on_press(ask(DialogMessage::Confirm)) };
            (Sentence::DeleteRoleTitle(role), Some(doing), !deleting.asking())
        }
        Asking::Kick(kicking) => {
            let who = kicking.name().to_string();
            body.push(muted(words.say(&Sentence::KickBody(who.clone()))));
            let refused = kicking.refused().map(|refused| match refused {
                Refused::NotAllowed => words.say(&Sentence::CantKick(who.clone())),
                Refused::TooManyTries => words.phrase(Phrase::TooManyTries).to_string(),
                Refused::WrongPassword | Refused::NoAnswer => words.say(&Sentence::NoAnswer(name.clone())),
            });
            body.extend(refused.map(crate::refusal));
            let doing = Button::new(label(&words.say(&Sentence::KickMember(who.clone()))))
                .variant(ButtonVariant::Danger)
                .loading(kicking.asking());
            let doing = if kicking.asking() { doing } else { doing.on_press(ask(DialogMessage::Confirm)) };
            (Sentence::KickTitle(who), Some(doing), !kicking.asking())
        }
        Asking::Ban(banning) => {
            let (title, doing, ready) = ban(&name, banning, &mut body, words);
            (title, Some(doing), ready)
        }
        Asking::SetUp(first) => return Some(set_up(&name, first, picture, words)),
        Asking::DeleteChannel(deleting) => {
            body.push(muted(words.phrase(deleting.body()).to_string()));
            body.extend(deleting.problem().map(|problem| crate::refusal(words.say(&problem.said(&name)))));
            let doing = Button::new(label(&words.say(&deleting.action())))
                .variant(ButtonVariant::Danger)
                .loading(deleting.asking());
            let doing = if deleting.asking() { doing } else { doing.on_press(ask(DialogMessage::Confirm)) };
            (deleting.title(), Some(doing), !deleting.asking())
        }
        Asking::DeleteEmoji(deleting) => {
            let name = deleting.name().to_string();
            body.push(muted(words.say(&Sentence::DeleteEmojiBody(name.clone()))));
            let doing = Button::new(phrase(Phrase::DeleteEmojiAction, words))
                .variant(ButtonVariant::Danger)
                .on_press(ask(DialogMessage::Confirm));
            (Sentence::DeleteEmojiTitle(name), Some(doing), true)
        }
        Asking::DeleteMessage(deleting) => {
            let author =
                deleting.author().map_or_else(|| words.phrase(Phrase::FormerMember).into_owned(), str::to_owned);
            body.extend(
                deleting.line().map(|line| {
                    text(words.say(&Sentence::DeleteMessageLine(line.to_owned())), TypeStyle::Body)._boxed()
                }),
            );
            body.push(muted(words.phrase(Phrase::DeleteMessageNote).into_owned()));
            let doing = Button::new(phrase(Phrase::Delete, words))
                .variant(ButtonVariant::Danger)
                .on_press(ask(DialogMessage::Confirm));
            (Sentence::DeleteMessageTitle(author), Some(doing), true)
        }
        Asking::DeleteCategory(deleting) => {
            body.push(muted(words.say(&deleting.body())));
            body.extend(deleting.problem().map(|problem| crate::refusal(words.say(&problem.said(&name)))));
            let doing =
                Button::new(phrase(Phrase::Delete, words)).variant(ButtonVariant::Danger).loading(deleting.asking());
            let doing = if deleting.asking() { doing } else { doing.on_press(ask(DialogMessage::Confirm)) };
            (deleting.title(), Some(doing), !deleting.asking())
        }
        Asking::Reset(resetting) => {
            let (title, doing, done) = reset(&name, resetting, &mut body, words);
            if done {
                keep = None;
                alert = false;
            }
            (title, Some(doing), !resetting.asking())
        }
        Asking::Transfer(transferring) => {
            let who = transferring.name().to_string();
            body.push(muted(words.say(&Sentence::TransferBody(who.clone()))));
            body.push(proof_field(&name, transferring.proof(), None, words));
            let doing = Button::new(label(&words.say(&Sentence::TransferAction(who.clone()))))
                .variant(ButtonVariant::Primary)
                .loading(transferring.proof().asking());
            let ready = transferring.proof().ready();
            let doing = if ready { doing.on_press(ask(DialogMessage::Confirm)) } else { doing };
            (Sentence::TransferTitle { name: who, server: name }, Some(doing), ready)
        }
        Asking::DeleteInvite(deleting) => {
            body.push(muted(words.phrase(Phrase::DeleteInviteBody).to_string()));
            let refused = deleting.refused().map(|refused| match refused {
                DeleteInviteRefused::NotYours => words.phrase(Phrase::CantDeleteInvite).to_string(),
                DeleteInviteRefused::TooManyTries => words.phrase(Phrase::TooManyTries).to_string(),
                DeleteInviteRefused::NoAnswer => words.say(&Sentence::NoAnswer(name.clone())),
            });
            body.extend(refused.map(crate::refusal));
            keep = Some(Phrase::KeepIt);
            let doing = Button::new(phrase(Phrase::DeleteInvite, words))
                .variant(ButtonVariant::Danger)
                .loading(deleting.asking());
            let doing = if deleting.asking() { doing } else { doing.on_press(ask(DialogMessage::Confirm)) };
            (Sentence::Phrase(Phrase::DeleteInviteTitle), Some(doing), !deleting.asking())
        }
        Asking::SignOut => {
            body.push(muted(words.phrase(Phrase::SignOutBody).to_string()));
            body.extend(call);
            let doing = Button::new(phrase(Phrase::SignOutAction, words))
                .variant(ButtonVariant::Primary)
                .on_press(ask(DialogMessage::Confirm));
            (Sentence::SignOutTitle(name), Some(doing), true)
        }
        Asking::Quit => {
            body.extend(dialog.call().map(|channel| muted(words.say(&Sentence::QuitLeavesCall(channel.to_string())))));
            let doing = Button::new(phrase(Phrase::QuitAction, words))
                .variant(ButtonVariant::Primary)
                .on_press(ask(DialogMessage::Confirm));
            (Sentence::Phrase(Phrase::QuitTitle), Some(doing), true)
        }
        Asking::Remove => {
            body.push(muted(words.text(Text::RemoveBody).to_string()));
            body.extend(call);
            let doing = Button::new(label(&words.say(&Sentence::RemoveAction(name.clone()))))
                .variant(ButtonVariant::Danger)
                .on_press(ask(DialogMessage::Confirm));
            (Sentence::RemoveTitle(name), Some(doing), true)
        }
        Asking::Leave(proof) => {
            body.push(muted(words.say(&Sentence::LeaveBody(name.clone()))));
            body.extend(call);
            body.push(proof_field(&name, proof, None, words));
            let doing = Button::new(label(&words.say(&Sentence::Leave(name.clone()))))
                .variant(ButtonVariant::Danger)
                .loading(proof.asking());
            let doing = if proof.ready() { doing.on_press(ask(DialogMessage::Confirm)) } else { doing };
            (Sentence::LeaveTitle(name), Some(doing), proof.ready())
        }
        Asking::OwnerLeaves => {
            body.push(muted(words.phrase(Phrase::OwnerLeaveBody).to_string()));
            alert = false;
            // "Choose a new owner" opens the members, where Make owner is.
            let members = place.pages().contains(&Page::Members);
            let doing = members.then(|| {
                Button::new(phrase(Phrase::ChooseNewOwner, words))
                    .variant(ButtonVariant::Primary)
                    .on_press(ask(DialogMessage::Confirm))
            });
            (Sentence::YouOwn(name), doing, members)
        }
    };
    let mut actions: Vec<Part<'a, R>> = keep
        .map(|keep| {
            Button::new(phrase(keep, words))
                .variant(ButtonVariant::Secondary)
                .on_press(ask(DialogMessage::Cancel))
                .view::<R>()
        })
        .into_iter()
        .collect();
    actions.extend(doing.map(|doing| doing.view::<R>()));
    let body = Column::with_children(body).spacing(design::DIALOG_GAP)._boxed();
    let title = label(&words.say(&title));
    let panel = if alert {
        crate::alert_panel(title, body, actions, ask(DialogMessage::Cancel), words)
    } else {
        crate::dialog_panel(title, body, actions, ask(DialogMessage::Cancel), words)
    };
    // Escape cancels; a press outside is the model's to weigh (a typed password keeps the dialog).
    Some(crate::on_escape(panel, ask(DialogMessage::Cancel), ready.then_some(ask(DialogMessage::Confirm)))._boxed())
}

/// The current password, as leaving, making someone the server owner and a ban ask it, with why the server refused
/// it: `not_allowed` where the dialog has its own words for an ask no longer allowed, else that the server did not
/// answer.
fn proof_field<'a, R: Renderer + 'a>(
    server: &str,
    proof: &'a Proof,
    not_allowed: Option<&Sentence>,
    words: &'a Words,
) -> Part<'a, R> {
    let input = SecretInput::new(phrase(Phrase::YourPassword, words), proof.password())
        .revealed(proof.revealed())
        .invalid(proof.refused() == Some(Refused::WrongPassword))
        .on_input(|typed| ask(DialogMessage::Password(typed)))
        .on_submit(ask(DialogMessage::Confirm));
    let mut field: Vec<Part<'a, R>> = vec![
        coloured(words.phrase(Phrase::YourPassword), TypeStyle::Label, TextClass::Content(Content::Primary))._boxed(),
        crate::password_field::<Message, R>(input, ask(DialogMessage::Reveal), words),
    ];
    let refused = proof.refused().map(|refused| match refused {
        Refused::WrongPassword => words.phrase(Phrase::PasswordDoesntMatch).to_string(),
        Refused::TooManyTries => words.phrase(Phrase::TooManyTries).to_string(),
        Refused::NotAllowed => match not_allowed {
            Some(said) => words.say(said),
            None => words.say(&Sentence::NoAnswer(server.to_string())),
        },
        Refused::NoAnswer => words.say(&Sentence::NoAnswer(server.to_string())),
    });
    field.extend(refused.map(crate::refusal));
    Column::with_children(field).spacing(Space::Xs.px())._boxed()
}

/// Resetting a member's password: asked first, then the temporary one with Copy and Done. Says whether it is done.
fn reset<'a, R: Renderer + 'a>(
    server: &str,
    resetting: &'a Resetting,
    body: &mut Vec<Part<'a, R>>,
    words: &'a Words,
) -> (Sentence, Button<'a, Message>, bool) {
    let who = resetting.name().to_string();
    let muted = |said: String| coloured(said, TypeStyle::Body, TextClass::Content(Content::Secondary))._boxed();
    if let Some(temporary) = resetting.temporary() {
        body.push(muted(words.say(&Sentence::HandOver(who.clone()))));
        let copy = if resetting.copied() { Phrase::CopiedMark } else { Phrase::Copy };
        body.push(crate::copy_field(
            temporary.expose().to_string(),
            phrase(copy, words),
            ask(DialogMessage::CopyTemporary),
        ));
        let done = Button::new(phrase(Phrase::Done, words))
            .variant(ButtonVariant::Primary)
            .on_press(ask(DialogMessage::Confirm));
        return (Sentence::TemporaryFor(who), done, true);
    }
    body.push(muted(words.say(&Sentence::ResetBody(who.clone()))));
    let refused = resetting.refused().map(|refused| match refused {
        Refused::NotAllowed => words.say(&Sentence::CantReset(who.clone())),
        Refused::TooManyTries => words.phrase(Phrase::TooManyTries).to_string(),
        Refused::WrongPassword | Refused::NoAnswer => words.say(&Sentence::NoAnswer(server.to_string())),
    });
    body.extend(refused.map(crate::refusal));
    let doing = Button::new(phrase(Phrase::ResetPasswordAction, words))
        .variant(ButtonVariant::Primary)
        .loading(resetting.asking());
    let doing = if resetting.asking() { doing } else { doing.on_press(ask(DialogMessage::Confirm)) };
    (Sentence::ResetTitle(who), doing, false)
}

/// A ban or lifting one, proved by the password: its title, its main button, and whether that does something now.
fn ban<'a, R: Renderer + 'a>(
    server: &str,
    banning: &'a Banning,
    body: &mut Vec<Part<'a, R>>,
    words: &'a Words,
) -> (Sentence, Button<'a, Message>, bool) {
    let muted = |said: String| coloured(said, TypeStyle::Body, TextClass::Content(Content::Secondary))._boxed();
    let who = banning.name().to_string();
    let (name, server_name) = (who.clone(), server.to_string());
    let (title, said, action, not_allowed, variant) = match banning.ask() {
        BanAsk::Ban => (
            Sentence::BanTitle(who.clone()),
            Sentence::BanBody { name, server: server_name },
            Sentence::BanMember(who.clone()),
            Sentence::CantBan(who),
            ButtonVariant::Danger,
        ),
        BanAsk::Unban => (
            Sentence::UnbanTitle(who.clone()),
            Sentence::UnbanBody { name, server: server_name },
            Sentence::UnbanMember(who.clone()),
            Sentence::CantUnban(who),
            ButtonVariant::Primary,
        ),
    };
    body.push(muted(words.say(&said)));
    body.push(proof_field(server, banning.proof(), Some(&not_allowed), words));
    let doing = Button::new(label(&words.say(&action))).variant(variant).loading(banning.proof().asking());
    let ready = banning.proof().ready();
    let doing = if ready { doing.on_press(ask(DialogMessage::Confirm)) } else { doing };
    (title, doing, ready)
}

/// What went wrong with a role at the server named `server`, the role named `role`, in one sentence.
fn role_problem(server: &str, role: String, problem: RoleProblem, words: &Words) -> String {
    match problem {
        RoleProblem::TooLong => words.say(&Sentence::TextTooLong),
        RoleProblem::Characters => words.phrase(Phrase::PlainTextOnly).to_string(),
        RoleProblem::Full => words.say(&Sentence::TooManyRoles),
        RoleProblem::NotAllowed => words.say(&Sentence::CantChangeRole(role)),
        RoleProblem::TooManyTries => words.phrase(Phrase::TooManyTries).to_string(),
        RoleProblem::NoAnswer => words.say(&Sentence::NoAnswer(server.to_string())),
    }
}

/// A name for a new role at the server named `server`, which starts just above the default role.
fn create_role<'a, R: Renderer + 'a>(server: &str, creating: &'a CreatingRole, words: &'a Words) -> Part<'a, R> {
    let problem = creating.problem();
    let field = TextInput::new(phrase(Phrase::ChannelNameLabel, words), creating.name())
        .placeholder(words.phrase(Phrase::NewRole))
        .invalid(matches!(problem, Some(RoleProblem::TooLong | RoleProblem::Characters)))
        .on_input(|typed| ask(DialogMessage::Named(typed)))
        .on_submit(ask(DialogMessage::Confirm))
        .view::<R>();
    let mut lines = vec![
        coloured(words.phrase(Phrase::ChannelNameLabel), TypeStyle::Label, TextClass::Content(Content::Primary))
            ._boxed(),
        field,
    ];
    let above = creating.above().to_string();
    lines.extend(problem.map(|problem| crate::refusal(role_problem(server, above, problem, words))));
    let said = words.say(&Sentence::CreateRoleBody(creating.above().to_string()));
    let body = Column::with_children([
        coloured(said, TypeStyle::Body, TextClass::Content(Content::Secondary))._boxed(),
        Column::with_children(lines).spacing(Space::Xs.px())._boxed(),
    ])
    .spacing(design::DIALOG_GAP)
    ._boxed();
    let doing =
        Button::new(phrase(Phrase::CreateRole, words)).variant(ButtonVariant::Primary).loading(creating.asking());
    let doing = if creating.ready() { doing.on_press(ask(DialogMessage::Confirm)) } else { doing };
    let actions = vec![
        Button::new(phrase(Phrase::Cancel, words))
            .variant(ButtonVariant::Secondary)
            .on_press(ask(DialogMessage::Cancel))
            .view::<R>(),
        doing.view::<R>(),
    ];
    let title = phrase(Phrase::CreateRoleTitle, words);
    let panel = crate::dialog_panel(title, body, actions, ask(DialogMessage::Cancel), words);
    crate::on_escape(panel, ask(DialogMessage::Cancel), creating.ready().then_some(ask(DialogMessage::Confirm)))
        ._boxed()
}

/// A name typed at the server named `server`: a channel's new name, a new category's, or a category's new name.
fn name_channel<'a, R: Renderer + 'a>(server: &str, naming: &'a Naming, words: &'a Words) -> Part<'a, R> {
    let problem = naming.problem();
    let field_label = Phrase::ChannelNameLabel;
    let field = TextInput::new(phrase(field_label, words), naming.name())
        .placeholder(words.say(&naming.placeholder()))
        .invalid(matches!(problem, Some(ChannelProblem::TooLong | ChannelProblem::Characters)))
        .on_input(|typed| ask(DialogMessage::Named(typed)))
        .on_submit(ask(DialogMessage::Confirm))
        .view::<R>();
    let mut lines = vec![
        coloured(words.phrase(field_label), TypeStyle::Label, TextClass::Content(Content::Primary))._boxed(),
        field,
    ];
    lines.extend(problem.map(|problem| crate::refusal(words.say(&problem.said(server)))));
    let body = Column::with_children(lines).spacing(Space::Xs.px())._boxed();
    let doing = Button::new(phrase(naming.action(), words)).variant(ButtonVariant::Primary).loading(naming.asking());
    let doing = if naming.ready() { doing.on_press(ask(DialogMessage::Confirm)) } else { doing };
    let actions = vec![
        Button::new(phrase(Phrase::Cancel, words))
            .variant(ButtonVariant::Secondary)
            .on_press(ask(DialogMessage::Cancel))
            .view::<R>(),
        doing.view::<R>(),
    ];
    let title = label(&words.say(&naming.title()));
    let panel = crate::dialog_panel(title, body, actions, ask(DialogMessage::Cancel), words);
    crate::on_escape(panel, ask(DialogMessage::Cancel), naming.ready().then_some(ask(DialogMessage::Confirm)))._boxed()
}

/// Making a channel at `place`, whose name is `server`: its kind, its name, its category and, where this account may
/// set it, who sees it and who writes in it.
fn make_channel<'a, R: Renderer + 'a>(
    place: &'a screens::Place,
    server: &str,
    making: &'a Making,
    handles: &crate::discussion::Handles<'_>,
    words: &'a Words,
) -> Part<'a, R> {
    let make = |said: MakingMessage| ask(DialogMessage::Making(said));
    let kinds = [(Phrase::KindText, Kind::Text), (Phrase::KindVoice, Kind::Voice)];
    let chosen = kinds.iter().position(|(_, kind)| *kind == making.kind()).unwrap_or_default();
    let kind = Column::with_children([
        crate::panel_parts::section_heading(&phrase(Phrase::ChannelKind, words))._boxed(),
        crate::choice_track(
            phrase(Phrase::KindOfChannel, words),
            kinds.map(|(said, kind)| (phrase(said, words), make(MakingMessage::Kind(kind)))).to_vec(),
            chosen,
        ),
    ])
    .spacing(Space::Xs.px() + Space::Xxs.px());
    let problem = making.problem();
    let field = TextInput::new(phrase(Phrase::ChannelNameLabel, words), making.name())
        .placeholder(words.phrase(making.placeholder()))
        .invalid(matches!(problem, Some(ChannelProblem::TooLong | ChannelProblem::Characters)))
        .on_input(|typed| ask(DialogMessage::Named(typed)))
        .on_submit(ask(DialogMessage::Confirm))
        .view::<R>();
    // The kind comes first and is usually left as it is: the name is what is typed at once.
    let field = crate::focus::take_focus(field)._boxed();
    let note = problem.map(|problem| crate::refusal(words.say(&problem.said(server))));
    let mut name = vec![crate::door::named(Phrase::ChannelNameLabel, None, field, words)];
    name.extend(note);
    let categories = match place.lobby() {
        Some(Lobby::Ready(listed)) => listed.categories.as_slice(),
        Some(Lobby::Loading | Lobby::Failed(_)) | None => &[],
    };
    let category = crate::category_select::category_select(
        categories,
        making.category(),
        making.category_menu_open(),
        make(MakingMessage::CategoryMenu),
        move |picked| make(MakingMessage::Category(picked)),
        words,
    );
    let mut body: Vec<Part<'a, R>> =
        vec![kind._boxed(), Column::with_children(name).spacing(Space::Xs.px())._boxed(), category];
    let views: Vec<_> = [Access::Private, Access::ReadOnly]
        .into_iter()
        .filter_map(|access| place.making_access(making, access))
        .collect();
    if !views.is_empty() {
        let naming = crate::whom::Naming::of(place, words, handles);
        body.push(crate::access_card::access_card(
            views,
            making.query(),
            &naming,
            move |said| {
                make(match said {
                    AccessSaid::Switch(access) => MakingMessage::Switch(access),
                    AccessSaid::Adding(access) => MakingMessage::Adding(access),
                    AccessSaid::Query(typed) => MakingMessage::Query(typed),
                    AccessSaid::Allow(access, whom) => MakingMessage::Allow(access, whom),
                    AccessSaid::Remove(access, whom) => MakingMessage::Remove(access, whom),
                })
            },
            words,
        ));
    }
    let body = Column::with_children(body).spacing(Space::L.px())._boxed();
    let doing =
        Button::new(phrase(Phrase::CreateChannel, words)).variant(ButtonVariant::Primary).loading(making.asking());
    let doing = if making.ready() { doing.on_press(ask(DialogMessage::Confirm)) } else { doing };
    let actions = vec![
        Button::new(phrase(Phrase::Cancel, words))
            .variant(ButtonVariant::Secondary)
            .on_press(ask(DialogMessage::Cancel))
            .view::<R>(),
        doing.view::<R>(),
    ];
    let title = phrase(Phrase::CreateChannelTitle, words);
    let panel = crate::dialog_panel(title, body, actions, ask(DialogMessage::Cancel), words);
    crate::on_escape(panel, ask(DialogMessage::Cancel), making.ready().then_some(ask(DialogMessage::Confirm)))._boxed()
}

/// The set-up dialog a server's owner sees first at the server named `name`: a name, a picture and a colour, saved
/// together or skipped.
fn set_up<'a, R: Renderer + 'a>(
    name: &str,
    first: &'a SetUp,
    paint: &crate::picture::Picture<'a, R>,
    words: &'a Words,
) -> Part<'a, R> {
    let picked = screens::colour::rgb(first.colour().picked());
    let shown = if first.typed().trim().is_empty() { name } else { first.typed() };
    let picture = crate::picture_row::picture_row(
        shown,
        picked,
        first.picture(),
        paint,
        |message| ask(DialogMessage::SetUp(SetUpMessage::Picture(message))),
        words,
    );
    let field = TextInput::new(phrase(Phrase::ServerName, words), first.typed())
        .placeholder(name.to_string())
        .on_input(|typed| ask(DialogMessage::SetUp(SetUpMessage::Name(typed))))
        .on_submit(ask(DialogMessage::Confirm))
        .view::<R>();
    // The picture comes first and is often left for later: the name is what is typed at once.
    let field = crate::focus::take_focus(field)._boxed();
    let colour = crate::colour_picker(
        &phrase(Phrase::ColourLabel, words),
        first.colour(),
        |message| ask(DialogMessage::SetUp(SetUpMessage::Colour(message))),
        words,
    );
    let body = Column::with_children([
        coloured(words.phrase(Phrase::SetUpBody), TypeStyle::Body, TextClass::Content(Content::Secondary))._boxed(),
        picture,
        crate::door::named(Phrase::ServerName, None, field, words),
        colour,
    ])
    .spacing(Space::M.px())
    ._boxed();
    let save = Button::new(phrase(Phrase::Save, words)).variant(ButtonVariant::Primary);
    let save = if first.ready() { save.on_press(ask(DialogMessage::Confirm)) } else { save };
    let actions = vec![
        Button::new(phrase(Phrase::SkipForNow, words))
            .variant(ButtonVariant::Secondary)
            .on_press(ask(DialogMessage::Cancel))
            .view::<R>(),
        save.view::<R>(),
    ];
    let title = label(&words.say(&Sentence::SetUp(name.to_string())));
    let panel = crate::dialog_panel(title, body, actions, ask(DialogMessage::Cancel), words);
    crate::on_escape(panel, ask(DialogMessage::Cancel), first.ready().then_some(ask(DialogMessage::Confirm)))._boxed()
}

/// Inviting people to the server `place` named `name`: how many and for how long, the role it gives where the account
/// may give one, a note, then the link to copy.
fn invite<'a, R: Renderer + 'a>(name: &str, place: &'a Place, inviting: &'a Inviting, words: &'a Words) -> Part<'a, R> {
    let invite = |said: InviteMessage| ask(DialogMessage::Invite(said));
    let muted = |said: String, style| coloured(said, style, TextClass::Content(Content::Secondary))._boxed();
    let field_name =
        |said: Phrase| coloured(words.phrase(said), TypeStyle::Label, TextClass::Content(Content::Primary));
    let title = label(&words.say(&Sentence::InviteTo(name.to_string())));
    let (body, actions, ready) = match inviting.made() {
        Some(made) => {
            let copy = if made.copied() { Phrase::CopiedMark } else { Phrase::Copy };
            let link = Column::with_children([
                field_name(Phrase::InviteLinkLabel)._boxed(),
                crate::copy_field(
                    made.link().expose().to_string(),
                    phrase(copy, words),
                    invite(InviteMessage::CopyLink),
                ),
            ])
            .spacing(Space::Xs.px());
            let mut lines = vec![muted(words.phrase(Phrase::SendThisLink).to_string(), TypeStyle::Body), link._boxed()];
            lines.extend(inviting.summary(words, place.role_list()).map(|summary| muted(summary, TypeStyle::Caption)));
            let actions = vec![
                Button::new(phrase(Phrase::ManageInvites, words))
                    .variant(ButtonVariant::Secondary)
                    .on_press(invite(InviteMessage::Manage))
                    .view::<R>(),
                Button::new(phrase(Phrase::Done, words))
                    .variant(ButtonVariant::Primary)
                    .on_press(ask(DialogMessage::Confirm))
                    .view::<R>(),
            ];
            (lines, actions, true)
        }
        None => {
            let uses = invite_uses(inviting, words);
            let days = Days::ALL
                .map(|days| (label(&words.say(&Sentence::ForDays(days.count()))), invite(InviteMessage::Days(days))));
            let chosen_days = Days::ALL.iter().position(|days| *days == inviting.days()).unwrap_or_default();
            let problem = inviting.problem();
            let note_wrong = matches!(problem, Some(InviteProblem::NoteTooLong | InviteProblem::NoteCharacters));
            let note = TextInput::new(phrase(Phrase::NoteLabel, words), inviting.note())
                .placeholder(words.phrase(Phrase::WhoIsItFor))
                .invalid(note_wrong)
                .on_input(move |typed| ask(DialogMessage::Invite(InviteMessage::Note(typed))))
                .on_submit(ask(DialogMessage::Confirm))
                .view::<R>();
            let refused = problem.map(|problem| {
                let said = match problem {
                    InviteProblem::NoteTooLong => words.say(&Sentence::TextTooLong),
                    InviteProblem::NoteCharacters => words.phrase(Phrase::PlainTextOnly).to_string(),
                    InviteProblem::TooMany => words.say(&Sentence::TooManyInvites),
                    InviteProblem::TooManyTries => words.phrase(Phrase::TooManyTries).to_string(),
                    InviteProblem::RoleNotGivable => words.phrase(Phrase::InviteRoleRefused).to_string(),
                    InviteProblem::NoAnswer => words.say(&Sentence::NoAnswer(name.to_string())),
                };
                crate::refusal(said)
            });
            let mut lines =
                vec![uses, crate::choice(phrase(Phrase::WorksFor, words), days.to_vec(), chosen_days, true)];
            lines.extend(invite_role(inviting, place, words));
            lines.push(crate::door::named_aside(Phrase::NoteLabel, Phrase::Optional, refused, note, words));
            let create = Button::new(phrase(Phrase::CreateInviteLink, words))
                .variant(ButtonVariant::Primary)
                .loading(inviting.asking());
            let create = if inviting.ready() { create.on_press(ask(DialogMessage::Confirm)) } else { create };
            let actions = vec![
                Button::new(phrase(Phrase::Cancel, words))
                    .variant(ButtonVariant::Secondary)
                    .on_press(ask(DialogMessage::Cancel))
                    .view::<R>(),
                create.view::<R>(),
            ];
            (lines, actions, inviting.ready())
        }
    };
    let body = Column::with_children(body).spacing(design::DIALOG_GAP)._boxed();
    let panel = crate::dialog_panel(title, body, actions, ask(DialogMessage::Cancel), words);
    crate::on_escape(panel, ask(DialogMessage::Cancel), ready.then_some(ask(DialogMessage::Confirm)))._boxed()
}

/// How many people the invite admits: the presets and Custom in one track, and under it, while Custom is taken, the
/// slider and the number field of the one count, with why a number typed is none.
fn invite_uses<'a, R: Renderer + 'a>(inviting: &'a Inviting, words: &'a Words) -> Part<'a, R> {
    let invite = |said: InviteMessage| ask(DialogMessage::Invite(said));
    let presets = Uses::PRESETS
        .map(|uses| {
            (label(&words.say(&Sentence::People(uses.count()))), invite(InviteMessage::Uses(UsesMessage::Preset(uses))))
        })
        .into();
    let custom = (phrase(Phrase::Custom, words), invite(InviteMessage::Uses(UsesMessage::Custom)));
    let taken = match inviting.uses() {
        UsesChoice::Preset(uses) => {
            Taken::Preset(Uses::PRESETS.iter().position(|preset| preset == uses).unwrap_or_default())
        }
        UsesChoice::Custom(typed) => {
            let least = u16::try_from(Uses::LEAST.count()).unwrap_or(u16::MIN);
            let most = u16::try_from(Uses::MOST.count()).unwrap_or(u16::MAX);
            Taken::Custom(CustomField {
                name: phrase(Phrase::UsesLabel, words),
                steps: least..=most,
                at: u16::try_from(inviting.uses().slid_to().count()).unwrap_or(least),
                slid: Rc::new(|at: u16| ask(DialogMessage::Invite(InviteMessage::Uses(UsesMessage::Slid(at))))),
                typed,
                on_input: Box::new(|typed| ask(DialogMessage::Invite(InviteMessage::Uses(UsesMessage::Typed(typed))))),
                unit: None,
                on_submit: Some(ask(DialogMessage::Confirm)),
                note: inviting.uses_refused().map(|_| CustomNote::Refused(words.say(&Sentence::UsesOutOfRange))),
            })
        }
    };
    custom_choice(phrase(Phrase::CanBeUsedBy, words), presets, custom, taken, true)
}

/// The Role row: the role the invite gives beside the default one, a field that opens the roles this account may give,
/// the default role first; none where it may give none.
fn invite_role<'a, R: Renderer + 'a>(
    inviting: &'a Inviting,
    place: &'a Place,
    words: &'a Words,
) -> Option<Part<'a, R>> {
    let options = inviting.role_options(place)?;
    let invite = |said: InviteMessage| ask(DialogMessage::Invite(said));
    let dot = |colour: Option<proto::rules::Colour>| Leading::Dot(colour.map(screens::colour::rgb));
    let shown = options.iter().find(|option| option.chosen).map_or_else(|| label(""), |option| label(option.name));
    let leading = options.iter().find(|option| option.chosen).map(|option| dot(option.colour));
    let choices = options
        .iter()
        .map(|option| MenuChoice {
            leading: Some(dot(option.colour)),
            label: label(option.name),
            note: None,
            chosen: option.chosen,
            message: invite(InviteMessage::Role(option.role.clone())),
        })
        .collect();
    let field = Select {
        leading,
        name: phrase(Phrase::RoleLabel, words),
        shown,
        choices,
        open: inviting.role_menu_open(),
        toggle: invite(InviteMessage::RoleMenu),
        close: invite(InviteMessage::RoleMenu),
    }
    .view::<R>();
    Some(crate::door::named(Phrase::RoleLabel, None, field, words))
}
