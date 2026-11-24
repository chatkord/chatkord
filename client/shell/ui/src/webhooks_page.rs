//! Server settings › Webhooks: the server's webhooks in a table, each with Regenerate URL and Delete, New webhook as the
//! page's action, and the dialogs it is in: making one, its URL shown once, and the two confirmations.

use crate::labels::{label, phrase};
use crate::server_settings::page_message;
use crate::{
    AvatarKind, Button, MenuChoice, Renderer, Select, TextClass, TextInput, Theme, avatar_pictured, coloured, line,
};
use design::{AvatarSize, ButtonVariant, Content, SETTINGS_LIST_WIDTH, Space, TypeStyle};
use iced_core::{Element, Length, Widget};
use iced_widget::{Column, Row, container};
use screens::Loaded;
use screens::chat;
use screens::webhooks::{Act, Confirming, FormField, Listed, Made, Making, Message as Said, Stage, Webhooks};
use screens::{PageMessage, Place, Window, WindowMessage as Message};
use words::{Phrase, Sentence, Text, Words};

type Part<'a, R> = Element<'a, Message, Theme, R>;

fn say(place: &Place, message: Said) -> Message {
    page_message(place, PageMessage::Webhooks(message))
}

/// The page: New webhook as its action, and the line and the table as its content.
pub(crate) fn page<'a, R: Renderer + 'a>(
    window: &'a Window,
    place: &'a Place,
    open: &'a Webhooks,
    file: &dyn Fn(proto::files::FileId) -> Option<crate::Held>,
    words: &'a Words,
) -> (Part<'a, R>, Part<'a, R>) {
    let new = Button::new(phrase(Phrase::NewWebhook, words))
        .variant(ButtonVariant::Primary)
        .compact()
        .on_press(say(place, Said::Make))
        .view::<R>();
    let quiet = |said: String| coloured(said, TypeStyle::Body, TextClass::Content(Content::Secondary))._boxed();
    let intro = container(coloured(
        words.text(Text::WebhooksLine).into_owned(),
        TypeStyle::Small,
        TextClass::Content(Content::Secondary),
    ))
    .width(Length::Fill.max(SETTINGS_LIST_WIDTH))
    ._boxed();
    let list: Part<'a, R> = match open.list() {
        Loaded::Loading => iced_widget::Space::new()._boxed(),
        Loaded::Failed(_) => crate::server_settings::no_answer(place, words),
        Loaded::Ready(listed) => {
            let today = window.today();
            let rows = listed
                .iter()
                .map(|hook| {
                    let channel = place.channel_name(hook.channel()).map(|name| format!("#{name}")).unwrap_or_default();
                    let by = hook
                        .made_by()
                        .and_then(|user_id| place.name_of(user_id))
                        .unwrap_or_else(|| words.phrase(Phrase::FormerMember).into_owned());
                    let face = hook.made_by().map(|user_id| {
                        let picked = place.directory().person(user_id).and_then(|person| person.colour);
                        avatar_pictured(
                            label(&by),
                            AvatarKind::Called(screens::colour_of(user_id, picked)),
                            AvatarSize::Xs,
                            place.picture_of(user_id).and_then(file),
                        )
                    });
                    let by = Row::with_children(face.into_iter().chain([line(by, TypeStyle::Small)._boxed()]))
                        .spacing(Space::S.px())
                        .align_y(iced_core::Alignment::Center);
                    let made = chat::local(hook.created_at(), window.zone())
                        .map(|at| words.say(&Sentence::MadeOn { day: at.date(), today }))
                        .unwrap_or_default();
                    let button = |said: Phrase, variant, message: Said| {
                        Button::new(phrase(said, words))
                            .variant(variant)
                            .compact()
                            .on_press(say(place, message))
                            .view::<R>()
                    };
                    let actions = Row::with_children([
                        button(Phrase::RegenerateUrl, ButtonVariant::Secondary, Said::Regenerate(hook.id().clone())),
                        button(Phrase::Delete, ButtonVariant::Danger, Said::Delete(hook.id().clone())),
                    ])
                    .spacing(Space::Xs.px());
                    vec![
                        line(hook.name().to_owned(), TypeStyle::BodyStrong)._boxed(),
                        line(channel, TypeStyle::Small)._boxed(),
                        by._boxed(),
                        line(made, TypeStyle::Small)._boxed(),
                        container(actions).align_right(Length::Fill)._boxed(),
                    ]
                })
                .collect();
            // The name and the actions take half again an equal column's width, as the canvas sets them.
            let heads = [
                (Phrase::WebhookNameHead, 3),
                (Phrase::WebhookChannelHead, 2),
                (Phrase::WebhookMadeByHead, 2),
                (Phrase::WebhookMadeHead, 2),
            ]
            .map(|(head, share)| (words.phrase(head).to_string(), share))
            .into_iter()
            .chain([(String::new(), 3)])
            .collect();
            crate::table::weighted_table(
                phrase(Phrase::Webhooks, words),
                heads,
                rows,
                Some(quiet(words.phrase(Phrase::NoWebhooks).into_owned())),
            )
        }
    };
    (new, Column::with_children([intro, list]).spacing(Space::L.px())._boxed())
}

/// The dialog the page is in, with what Escape and a press outside it send; none while it is the list alone.
pub(crate) fn dialog<'a, R: Renderer + 'a>(
    place: &'a Place,
    open: &'a Webhooks,
    words: &'a Words,
) -> Option<(Part<'a, R>, Message)> {
    match open.stage() {
        Stage::Listed => None,
        Stage::Making(making) => Some(making_dialog(place, making, words)),
        Stage::Made(made) => Some(made_dialog(place, made, words)),
        Stage::Confirming(confirming) => Some(confirm_dialog(place, confirming, words)),
    }
}

fn making_dialog<'a, R: Renderer + 'a>(
    place: &'a Place,
    making: &'a Making,
    words: &'a Words,
) -> (Part<'a, R>, Message) {
    let muted = |said: String| coloured(said, TypeStyle::Body, TextClass::Content(Content::Secondary))._boxed();
    let typing = move |typed| say(place, Said::Name(typed));
    let field = TextInput::new(phrase(Phrase::WebhookNameLabel, words), making.name())
        .placeholder(words.phrase(Phrase::WebhookExample))
        .invalid(making.name_refused())
        .on_input(typing)
        .on_submit(say(place, Said::Submit))
        .view::<R>();
    let field = crate::focus::take_focus(field)._boxed();
    let refused = |at: FormField| making.said_under(at).map(|said| crate::refusal(words.say(&said)));
    let name = crate::door::named(Phrase::WebhookNameLabel, refused(FormField::Name), field, words);
    let choices: Vec<MenuChoice<Message>> = place
        .webhook_channels()
        .unwrap_or_default()
        .into_iter()
        .map(|channel| MenuChoice {
            leading: None,
            label: label(&format!("#{}", channel.name)),
            note: None,
            chosen: making.channel() == Some(channel.id),
            message: say(place, Said::Channel(channel.id.clone())),
        })
        .collect();
    let shown = choices
        .iter()
        .find(|choice| choice.chosen)
        .map_or_else(|| phrase(Phrase::WebhookChannelLabel, words), |choice| choice.label.clone());
    let field = Select {
        leading: None,
        name: phrase(Phrase::WebhookChannelLabel, words),
        shown,
        choices,
        open: making.channel_menu_open(),
        toggle: say(place, Said::ChannelMenu),
        close: say(place, Said::ChannelMenu),
    }
    .view::<R>();
    let channel = crate::door::named(Phrase::WebhookChannelLabel, refused(FormField::Channel), field, words);
    let flip = place.webhook_everyone_turn(making).map(|turn| say(place, turn));
    let everyone = crate::switch_offered(
        &phrase(Phrase::MayMentionEveryone, words),
        Some(&words.phrase(Phrase::MayMentionEveryoneLine)),
        making.mention_everyone(),
        flip,
    );
    let everyone = crate::list_card(phrase(Phrase::MayMentionEveryone, words), vec![everyone], None);
    let body =
        Column::with_children([muted(words.phrase(Phrase::NewWebhookLine).into_owned()), name, channel, everyone])
            .spacing(design::DIALOG_GAP)
            ._boxed();
    let create =
        Button::new(phrase(Phrase::CreateWebhook, words)).variant(ButtonVariant::Primary).loading(making.asking());
    let create = if making.asking() { create } else { create.on_press(say(place, Said::Submit)) };
    let cancel = Button::new(phrase(Phrase::Cancel, words))
        .variant(ButtonVariant::Secondary)
        .on_press(say(place, Said::Cancel))
        .view::<R>();
    let close = say(place, Said::Cancel);
    (
        crate::dialog_panel(
            phrase(Phrase::NewWebhook, words),
            body,
            vec![cancel, create.view::<R>()],
            close.clone(),
            words,
        ),
        close,
    )
}

fn made_dialog<'a, R: Renderer + 'a>(place: &'a Place, made: &'a Made, words: &'a Words) -> (Part<'a, R>, Message) {
    let hook: &Listed = made.webhook();
    let channel = place.channel_name(hook.channel()).unwrap_or_default().to_owned();
    let copy = if made.copied() { Phrase::CopiedMark } else { Phrase::Copy };
    let url = Column::with_children([
        coloured(words.phrase(Phrase::WebhookUrl), TypeStyle::Label, TextClass::Content(Content::Primary))._boxed(),
        crate::copy_field(made.url().expose().to_string(), phrase(copy, words), say(place, Said::CopyUrl)),
    ])
    .spacing(Space::Xs.px());
    let body = Column::with_children([
        coloured(
            words.say(&Sentence::WebhookPostsIn(channel)),
            TypeStyle::Body,
            TextClass::Content(Content::Secondary),
        )
        ._boxed(),
        url._boxed(),
        crate::warning(words.phrase(Phrase::WebhookUrlOnce).into_owned()),
    ])
    .spacing(design::DIALOG_GAP)
    ._boxed();
    let done = Button::new(phrase(Phrase::Done, words))
        .variant(ButtonVariant::Primary)
        .on_press(say(place, Said::Cancel))
        .view::<R>();
    let close = say(place, Said::Cancel);
    let title = label(&words.say(&Sentence::WebhookUrlFor(hook.name().to_owned())));
    (crate::dialog_panel(title, body, vec![done], close.clone(), words), close)
}

fn confirm_dialog<'a, R: Renderer + 'a>(
    place: &'a Place,
    confirming: &'a Confirming,
    words: &'a Words,
) -> (Part<'a, R>, Message) {
    let name = confirming.webhook().name().to_owned();
    let (title, line_, action, variant) = match confirming.act() {
        Act::Regenerate => (
            Sentence::RegenerateWebhookTitle(name),
            Phrase::RegenerateWebhookBody,
            Phrase::RegenerateUrl,
            ButtonVariant::Primary,
        ),
        Act::Delete => (
            Sentence::DeleteWebhookTitle(name),
            Phrase::DeleteWebhookBody,
            Phrase::DeleteWebhook,
            ButtonVariant::Danger,
        ),
    };
    let mut lines: Vec<Part<'a, R>> =
        vec![coloured(words.phrase(line_), TypeStyle::Body, TextClass::Content(Content::Secondary))._boxed()];
    lines.extend(confirming.refused().map(|failed| crate::refusal(words.say(&words::failure::said(failed)))));
    let body = Column::with_children(lines).spacing(design::DIALOG_GAP)._boxed();
    let doing = Button::new(phrase(action, words)).variant(variant).loading(confirming.asking());
    let doing = if confirming.asking() { doing } else { doing.on_press(say(place, Said::Confirm)) };
    let cancel = Button::new(phrase(Phrase::Cancel, words))
        .variant(ButtonVariant::Secondary)
        .on_press(say(place, Said::Cancel))
        .view::<R>();
    let close = say(place, Said::Cancel);
    let title = label(&words.say(&title));
    (crate::alert_panel(title, body, vec![cancel, doing.view::<R>()], close.clone(), words), close)
}
