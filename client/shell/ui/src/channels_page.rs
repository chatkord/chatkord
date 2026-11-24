//! Server settings › Channels: every category and channel in the server's order, which a drag or Alt and an arrow key
//! changes, beside the chosen channel: its name, its category, who sees it and who writes in it, and its words one by
//! one under Advanced permissions. Where the two do not fit, the list alone, or the channel opened from it with Back.

use crate::access_card::{AccessSaid, access_card};
use crate::add_menu::{AddSaid, Hang, add_menu};
use crate::choice::{Verdict, chip, verdicts};
use crate::labels::{label, phrase};
use crate::server_settings::{no_answer, page_message, said_above};
use crate::settings::Heading;
use crate::whom::Naming;
use crate::{Button, Icon, IconClass, ListRow, Renderer, TextClass, TextInput, Theme, coloured, icon, text};
use design::{
    ButtonVariant, Content, FORM_WIDTH, IconSize, PillKind, ROLE_LIST_WIDTH, ROW_WORDS_LEAST, Role, SemanticRole,
    Semantics, Space, TypeStyle,
};
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Column, Row, container};
use proto::rules::Kind;
use screens::channel_settings::{
    AccessViews, Adding, AdvancedView, ChannelSettings, EditorView, Fold, Message as Channels, Off, Shows, Target,
    TargetKind, WordRow,
};
use screens::{ChannelAction, ChannelList, ListEntry, Lobby, NameFieldMessage, Place, PlaceMessage, ServerSettings};
use screens::{PageMessage, WindowMessage as Message};
use words::{Phrase, Sentence, Text, Words};

type Part<'a, R> = Element<'a, Message, Theme, R>;

/// What the page sends, as the open place's.
fn said(place: &Place, message: Channels) -> Message {
    page_message(place, PageMessage::Channels(message))
}

/// The channels page as `shows` says: its heading, its actions, and what it shows.
pub(crate) fn channels_page<'a, R: Renderer + 'a>(
    place: &'a Place,
    open: &'a ServerSettings,
    shows: Shows,
    file: &dyn Fn(proto::files::FileId) -> Option<crate::Held>,
    words: &'a Words,
) -> (Heading<Message>, Option<Part<'a, R>>, Part<'a, R>) {
    let titled = || Heading::Titled(phrase(Phrase::Channels, words));
    let (list, settings) = match (place.lobby(), place.channel_settings()) {
        (Some(Lobby::Ready(list)), Some(settings)) => (list, settings),
        (Some(Lobby::Failed(_)), _) => {
            return (titled(), None, said_above(place, open, no_answer(place, words), words));
        }
        (Some(Lobby::Ready(_) | Lobby::Loading) | None, _) => {
            return (titled(), None, iced_widget::Space::new()._boxed());
        }
    };
    let editor = || editor(place, settings, list, file, words);
    if shows == Shows::Channel {
        let back = said(place, Channels::Back);
        return (Heading::Back(phrase(Phrase::Channels, words), back), None, said_above(place, open, editor(), words));
    }
    let id = place.id();
    let channel = Button::new(phrase(Phrase::CreateChannel, words)).variant(ButtonVariant::Primary).compact();
    // A channel lives in a category: with none listed there is nowhere to make one.
    let channel = if list.categories.is_empty() { channel } else { channel.on_press(Message::CreateChannel(id)) };
    let actions = Row::with_children([
        Button::new(phrase(Phrase::CreateCategory, words))
            .variant(ButtonVariant::Secondary)
            .compact()
            .on_press(Message::CreateCategory(id))
            .view::<R>(),
        channel.view::<R>(),
    ])
    .spacing(Space::S.px())
    .align_y(Alignment::Center);
    if list.categories.is_empty() {
        let panel = screens::CentrePanel::without_categories();
        let empty = crate::centre::<Message, R>(&panel, |_| Message::CreateCategory(id), words);
        return (titled(), Some(actions._boxed()), empty);
    }
    let hint = coloured(
        words.phrase(Phrase::ChannelsDragLine).to_string(),
        TypeStyle::Small,
        TextClass::Content(Content::Secondary),
    )
    .width(Length::Fill.max(design::SETTINGS_LIST_WIDTH));
    let order = channel_list(place, settings, list, words);
    let body = match shows {
        Shows::Beside => {
            Row::with_children([container(order).width(Length::Fixed(ROLE_LIST_WIDTH))._boxed(), editor()])
                .spacing(Space::Xl.px())
                ._boxed()
        }
        Shows::List | Shows::Channel => order,
    };
    let content = Column::with_children([hint._boxed(), said_above(place, open, body, words)]).spacing(Space::L.px());
    (titled(), Some(actions._boxed()), content._boxed())
}

/// The list in its order, in a card: each category a small heading, each channel its kind's mark and its name, the
/// chosen one selected; a press chooses one.
fn channel_list<'a, R: Renderer + 'a>(
    place: &'a Place,
    settings: &ChannelSettings,
    list: &'a ChannelList,
    words: &'a Words,
) -> Part<'a, R> {
    let (chosen, chosen_category) = (settings.chosen(), settings.chosen_category());
    let rows = list.in_order().into_iter().enumerate().map(|(at, entry)| {
        // A channel stands in from its category's heading, so the list shows whose it is.
        let channel = |id: &terms::ChannelId, name: &str, plain, locked| {
            let inset = Space::L.px();
            let row = ListRow::<Message, R>::new(label(name))
                .leading(crate::channel_list::kind_icon(plain, locked, list.sight(id), words))
                .selected(chosen == Some(id))
                .on_press(said(place, Channels::Choose(id.clone())));
            let around = Padding::from([Space::Xs.px() + Space::Xxs.px(), Space::Xs.px()]);
            container(row.view()).padding(Padding { left: around.left + inset, ..around })._boxed()
        };
        let row: Part<'a, R> = match entry {
            ListEntry::Category(category) => {
                let heading = crate::panel_parts::section_heading(&label(&category.name));
                // Ruled above and below like a channel's row, so it sits in the middle of its own.
                let heading =
                    container(heading).width(Length::Fill).padding(Padding::from([Space::S.px(), Space::S.px()]));
                // A press chooses it as it chooses a channel; focusable, so Alt and an arrow key move it too.
                let selected = chosen_category == Some(&category.id);
                let choose = said(place, Channels::ChooseCategory(category.id.clone()));
                let pressed = iced_widget::button(heading)
                    .class(crate::button::ButtonClass::Row { selected, tone: crate::button::RowTone::Plain })
                    .padding(0)
                    .on_press(choose.clone());
                let semantics = Semantics { role: SemanticRole::Button, label: label(&category.name) };
                let row = crate::button::actionable(pressed, semantics, Some(choose));
                container(row).padding(Padding::from([Space::Xxs.px(), Space::Xs.px()]))._boxed()
            }
            ListEntry::Text(text) => channel(&text.id, &text.name, Icon::Hash, Icon::HashLock),
            ListEntry::Voice(voice) => channel(&voice.id, &voice.name, Icon::Volume, Icon::VolumeLock),
        };
        let row = if at == 0 { row } else { Column::with_children([crate::panel_parts::hairline(), row])._boxed() };
        (entry.key(), row)
    });
    let list = crate::reorder(
        phrase(Phrase::ChannelsInOrder, words),
        rows.collect::<Vec<_>>(),
        move |moved| place.may_move(moved),
        move |moved| Message::At(place.id(), PlaceMessage::Lobby(screens::LobbyMessage::Move(moved))),
    );
    crate::table::card_around(list)
}

/// The field of what is chosen on the page, a channel's name or a category's, under `label`: saved by Enter or by
/// leaving it, with what the server said of the last one saved under it.
fn name_field<'a, R: Renderer + 'a>(
    place: &'a Place,
    label: Phrase,
    name: &'a screens::NameField,
    words: &'a Words,
) -> Part<'a, R> {
    let note = name.note().said(place.server().shown());
    let danger = note.as_ref().is_some_and(screens::FieldSaid::refuses);
    let field = TextInput::new(phrase(label, words), name.typed())
        .invalid(danger)
        .on_input(move |typed| said(place, Channels::Name(NameFieldMessage::Typed(typed))))
        .on_submit(said(place, Channels::Name(NameFieldMessage::Save)))
        .view::<R>();
    let field = crate::focus::on_leave(field, said(place, Channels::NameLeft))._boxed();
    let note = note.map(|said| crate::field_said(&said, words));
    container(crate::door::named(label, note, field, words)).width(Length::Fill.max(FORM_WIDTH))._boxed()
}

/// A category chosen in a channel's place, edited as a channel is: its name over its name's field, and Delete
/// category, asked first.
fn category_editor<'a, R: Renderer + 'a>(
    place: &'a Place,
    settings: &'a ChannelSettings,
    list: &'a ChannelList,
    words: &'a Words,
) -> Part<'a, R> {
    let chosen = settings.chosen_category().zip(settings.category_name());
    let listed = chosen.and_then(|(id, name)| Some((list.categories.iter().find(|listed| listed.id == *id)?, name)));
    let Some((listed, name)) = listed else { return iced_widget::Space::new()._boxed() };
    let delete = Button::new(phrase(Phrase::DeleteCategory, words))
        .variant(ButtonVariant::Danger)
        .compact()
        .on_press(Message::Category(place.id(), ChannelAction::Delete, listed.id.clone()))
        .view::<R>();
    Column::with_children([
        text(listed.name.clone(), TypeStyle::Subheading)._boxed(),
        name_field(place, Phrase::CategoryName, name, words),
        container(delete)._boxed(),
    ])
    .spacing(Space::L.px())
    .width(Length::Fill.max(design::SETTINGS_LIST_WIDTH))
    ._boxed()
}

/// The chosen channel: its mark and name over its name's field, its category, who sees it and writes in it, its
/// words one by one, and Delete channel.
fn editor<'a, R: Renderer + 'a>(
    place: &'a Place,
    settings: &'a ChannelSettings,
    list: &'a ChannelList,
    file: &dyn Fn(proto::files::FileId) -> Option<crate::Held>,
    words: &'a Words,
) -> Part<'a, R> {
    if settings.chosen_category().is_some() {
        return category_editor(place, settings, list, words);
    }
    let Some(editor) = place.channel_editor() else { return iced_widget::Space::new()._boxed() };
    let EditorView { channel, kind, name, category, access } = editor;
    let shown = match list.find(&channel) {
        Some(ListEntry::Text(text)) => text.name.clone(),
        Some(ListEntry::Voice(voice)) => voice.name.clone(),
        Some(ListEntry::Category(_)) | None => String::new(),
    };
    let mark = match kind {
        Kind::Text => Icon::Hash,
        Kind::Voice => Icon::Volume,
    };
    let title = Row::with_children([
        icon(mark, IconSize::S, IconClass::Role(Role::Content(Content::Secondary)))._boxed(),
        text(shown.clone(), TypeStyle::Subheading)._boxed(),
    ])
    .spacing(Space::S.px())
    .align_y(Alignment::Center);
    let name = name_field(place, Phrase::ChannelName, name, words);
    let category = crate::category_select::category_select(
        &list.categories,
        &category,
        settings.category_menu_open(),
        said(place, Channels::CategoryMenu),
        move |picked| said(place, Channels::Category(picked)),
        words,
    );
    let category = container(category).width(Length::Fill.max(FORM_WIDTH));
    let mut parts: Vec<Part<'a, R>> = vec![title._boxed(), name._boxed(), category._boxed()];
    if let Some(access) = access {
        parts.extend(access_parts(place, settings, access, file, words));
    }
    parts.push(
        container(
            Button::new(phrase(Phrase::DeleteChannel, words))
                .variant(ButtonVariant::Danger)
                .compact()
                .on_press(Message::Channel(place.id(), ChannelAction::Delete, channel))
                .view::<R>(),
        )
        ._boxed(),
    );
    Column::with_children(parts).spacing(Space::L.px()).width(Length::Fill.max(design::SETTINGS_LIST_WIDTH))._boxed()
}

/// Access: its heading, the card of switches, whether this account stays in it, and Advanced permissions.
fn access_parts<'a, R: Renderer + 'a>(
    place: &'a Place,
    settings: &'a ChannelSettings,
    access: AccessViews,
    file: &dyn Fn(proto::files::FileId) -> Option<crate::Held>,
    words: &'a Words,
) -> Vec<Part<'a, R>> {
    let AccessViews { private, read_only, stay, advanced } = access;
    let naming = Naming::of(place, words, file);
    let quiet = |said: String| coloured(said, TypeStyle::Small, TextClass::Content(Content::Secondary))._boxed();
    let mut parts = vec![text(words.phrase(Phrase::Access).to_string(), TypeStyle::BodyStrong)._boxed()];
    let views = std::iter::once(private).chain(read_only).collect();
    parts.push(access_card(
        views,
        settings.query(),
        &naming,
        move |access_said| {
            said(
                place,
                match access_said {
                    AccessSaid::Switch(access) => Channels::Switch(access),
                    AccessSaid::Adding(access) => Channels::Adding(Adding::Access(access)),
                    AccessSaid::Query(typed) => Channels::Query(typed),
                    AccessSaid::Allow(access, whom) => Channels::Allow(access, whom),
                    AccessSaid::Remove(access, whom) => Channels::Remove(access, whom),
                },
            )
        },
        words,
    ));
    if stay {
        parts.push(quiet(words.phrase(Phrase::YouStay).to_string()));
    }
    let chevron = match advanced.fold {
        Fold::Folded => Icon::ChevronRight,
        Fold::Open => Icon::ChevronDown,
    };
    parts.push(disclosure(phrase(Phrase::AdvancedPermissions, words), chevron, said(place, Channels::Advanced)));
    if advanced.fold == Fold::Open {
        parts.extend(advanced_parts(place, settings, advanced, &naming, words));
    }
    parts
}

/// Advanced permissions, open: what it does, whose words show, why they may not be changed, and the words.
fn advanced_parts<'a, R: Renderer + 'a>(
    place: &'a Place,
    settings: &'a ChannelSettings,
    advanced: AdvancedView,
    naming: &Naming<'_>,
    words: &'a Words,
) -> Vec<Part<'a, R>> {
    let quiet = |said: String| coloured(said, TypeStyle::Small, TextClass::Content(Content::Secondary))._boxed();
    let AdvancedView { targets, adding, locked, removable, lacks, groups, .. } = advanced;
    let mut parts = vec![quiet(words.text(Text::AdvancedLine).to_string())];
    let chosen = targets.iter().find(|target| target.chosen).map(|target| naming.called(&target.name));
    let mut chips: Vec<Part<'a, R>> =
        targets.into_iter().map(|target| target_chip(place, target, naming, words)).collect();
    chips.push(add_menu(
        phrase(Phrase::Add, words),
        adding,
        settings.query(),
        Hang::End,
        naming,
        move |add| {
            said(
                place,
                match add {
                    AddSaid::Toggle => Channels::Adding(Adding::Advanced),
                    AddSaid::Query(typed) => Channels::Query(typed),
                    AddSaid::Pick(whom) => Channels::AddTarget(whom),
                },
            )
        },
        words,
    ));
    let chips = Row::with_children(chips).spacing(Space::Xs.px() + Space::Xxs.px()).align_y(Alignment::Center);
    let chips = crate::Labelled::new(
        chips.wrap().vertical_spacing(Space::Xs.px() + Space::Xxs.px()),
        Semantics { role: SemanticRole::RadioGroup, label: phrase(Phrase::WhoseWords, words) },
    );
    parts.push(chips._boxed());
    let locked = locked.map(|locked| words.say(&locked));
    parts.extend(locked.clone().map(quiet));
    if lacks {
        parts.push(quiet(words.phrase(Phrase::OnlyWordsYouHold).to_string()));
    }
    for (heading, rows) in groups {
        let rows = rows.iter().map(|row| word_row(place, row, locked.as_deref(), words)).collect();
        let group = Column::with_children([
            crate::panel_parts::section_heading(&phrase(heading, words))._boxed(),
            crate::flush_card(phrase(heading, words), rows),
        ])
        .spacing(Space::S.px());
        parts.push(group._boxed());
    }
    if removable && let Some(chosen) = chosen {
        parts.push(
            container(
                Button::new(label(&words.say(&Sentence::RemoveFromChannel(chosen.as_str().to_string()))))
                    .variant(ButtonVariant::Secondary)
                    .compact()
                    .on_press(said(place, Channels::RemoveTarget))
                    .view::<R>(),
            )
            ._boxed(),
        );
    }
    parts
}

/// A target's chip: its mark, its name and, for the default role, "Everyone" after it.
fn target_chip<'a, R: Renderer + 'a>(
    place: &'a Place,
    target: Target,
    naming: &Naming<'_>,
    words: &'a Words,
) -> Part<'a, R> {
    let name = naming.name(&target.name);
    let mut content: Vec<Part<'a, R>> =
        vec![naming.mark(target.look, &target.name), text(name.as_str().to_string(), TypeStyle::SmallStrong)._boxed()];
    if target.kind == TargetKind::Everyone {
        content.push(
            coloured(
                words.phrase(Phrase::Everyone).to_string(),
                TypeStyle::Caption,
                TextClass::Content(Content::Secondary),
            )
            ._boxed(),
        );
    }
    let content = Row::with_children(content).spacing(Space::Xs.px() + Space::Xxs.px()).align_y(Alignment::Center);
    chip(name, content._boxed(), target.chosen, said(place, Channels::Target(target.whom)))
}

/// A word's row: its name and what it gives, beside Deny, Inherit and Allow, the one now chosen.
fn word_row<'a, R: Renderer + 'a>(
    place: &'a Place,
    row: &WordRow,
    locked: Option<&str>,
    words: &'a Words,
) -> Part<'a, R> {
    let (name, line) = screens::grant_words(row.word);
    let quiet = |said: String| coloured(said, TypeStyle::Small, TextClass::Content(Content::Secondary))._boxed();
    let mut lines = vec![
        text(words.phrase(name).to_string(), TypeStyle::BodyStrong)._boxed(),
        quiet(words.phrase(line).to_string()),
    ];
    if row.locks_out {
        lines.push(quiet(words.phrase(Phrase::WouldNotSee).to_string()));
    }
    let lines = Column::with_children(lines).spacing(Space::Xxs.px());
    let kinds =
        [(Phrase::Deny, PillKind::Denied), (Phrase::Inherit, PillKind::Kept), (Phrase::Allow, PillKind::Allowed)];
    let chosen = row.choices.iter().position(|choice| choice.state == row.now).unwrap_or_default();
    let options = row
        .choices
        .iter()
        .zip(kinds)
        .map(|(choice, (said_as, kind))| Verdict {
            label: phrase(said_as, words),
            kind,
            message: choice.off.is_none().then(|| said(place, Channels::Set(row.word, choice.state))),
            why_not: choice.off.map(|off| match off {
                Off::Lacks => phrase(Phrase::OnlyWordsYouHold, words),
                Off::LocksOut => phrase(Phrase::WouldNotSee, words),
                Off::Locked => locked.map_or_else(|| phrase(Phrase::OnlyWordsYouHold, words), label),
            }),
        })
        .collect();
    let control = verdicts(phrase(name, words), options, chosen);
    let row = crate::beside(lines._boxed(), control, ROW_WORDS_LEAST, Space::M.px() + Space::Xxs.px(), 0.0);
    container(row).padding(crate::ROW_PADDING).width(Length::Fill)._boxed()
}

/// A heading that folds what is under it away or open: its chevron and its name, set close to the page's edge.
fn disclosure<'a, R: Renderer + 'a>(said: design::Label, chevron: Icon, toggle: Message) -> Part<'a, R> {
    let content = Row::with_children([
        icon(chevron, IconSize::S, IconClass::Role(Role::Content(Content::Secondary)))._boxed(),
        text(said.as_str().to_string(), TypeStyle::BodyStrong)._boxed(),
    ])
    .spacing(Space::Xs.px() + Space::Xxs.px())
    .align_y(Alignment::Center);
    let style = TypeStyle::BodyStrong;
    let inner = iced_widget::button(content)
        .class(crate::ButtonClass::Button { variant: ButtonVariant::Ghost, loading: false })
        .padding(Padding {
            top: (design::HIT_TARGET - style.line_height()) / 2.0,
            bottom: (design::HIT_TARGET - style.line_height()) / 2.0,
            left: Space::Xxs.px(),
            right: Space::S.px() + Space::Xxs.px(),
        })
        .on_press(toggle.clone());
    let semantics = Semantics { role: SemanticRole::Button, label: said };
    crate::button::actionable(inner, semantics, Some(toggle))
}
