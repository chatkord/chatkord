//! Settings › Appearance: the theme, the zoom, More contrast, the language and how messages are drawn on this device.
//! Every choice changes the whole window at once, so the window is its own preview; a message's size and spacing
//! show beside the choices on two runs drawn as the channel draws them. No accent is picked here, the window's is
//! always Chatkord's.

use crate::account_page::{Open, pages};
use crate::labels::{label, phrase};
use crate::typography::message_text;
use crate::{
    AvatarKind, Button, Icon, Renderer, Surfaced, TextClass, Theme, avatar, choice_track, coloured, list_card, switch,
    text,
};
use choices::{LookPick, ZoomPick};
use design::{
    AvatarSize, ButtonVariant, Content, Contrast, DEVICE_COLUMN_WIDTH, MessageScale, MessageSize, Preference, Radius,
    RunGap, Space, Surface, TypeStyle, ZOOM_FIGURE, Zoom,
};
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Column, Row, container};
use screens::{WindowMessage as Message, theme_phrase};
use words::{Phrase, Sentence, Text, Words};

type Part<'a, R> = Element<'a, Message, Theme, R>;

const THEMES: [Preference; 3] = [Preference::System, Preference::Light, Preference::Dark];

pub(crate) fn view<'a, R: Renderer + 'a>(
    window: &'a screens::Window,
    fold: screens::frame::SettingsShown,
    words: &'a Words,
) -> Part<'a, R> {
    let look = window.look();
    let pick = |pick: LookPick| Message::Look(pick);
    let theme = choice_track(
        phrase(Phrase::ThemeLabel, words),
        THEMES.map(|theme| (phrase(theme_phrase(theme), words), pick(LookPick::Theme(theme)))).to_vec(),
        THEMES.iter().position(|theme| *theme == look.theme).unwrap_or_default(),
    );
    let contrast = list_card(
        phrase(Phrase::MoreContrast, words),
        vec![switch(
            &phrase(Phrase::MoreContrast, words),
            None,
            look.contrast == Contrast::More,
            pick(LookPick::MoreContrast),
        )],
        None,
    );
    let previews = list_card(
        phrase(Phrase::ShowPreviews, words),
        vec![switch(
            &phrase(Phrase::ShowPreviews, words),
            Some(&words.text(Text::ShowPreviewsNote)),
            look.previews == choices::Previews::On,
            pick(LookPick::Previews),
        )],
        None,
    );
    let choices = Column::with_children([
        section(Phrase::ThemeLabel, theme, words),
        section(Phrase::ZoomLabel, zoom(look.zoom, words), words),
        contrast,
        section(Phrase::LanguageLabel, crate::language_field(window, words, Message::from), words),
        container(text(words.phrase(Phrase::MessagesHeading), TypeStyle::Subheading))
            .padding(Padding { top: Space::S.px(), ..Padding::ZERO })
            ._boxed(),
        section(Phrase::MessageSizeLabel, message_sizes(look.messages.size, words), words),
        section(Phrase::RunGapLabel, run_gaps(look.messages.gap, words), words),
        previews,
    ])
    .spacing(Space::L.px())
    .width(Length::Fill.max(DEVICE_COLUMN_WIDTH));
    let preview = section(Phrase::PreviewLabel, preview(look.messages, words), words);
    let choices = Row::with_children([
        choices._boxed(),
        container(preview).width(Length::Fill.max(DEVICE_COLUMN_WIDTH))._boxed(),
    ])
    .spacing(Space::Xxl.px())
    .wrap()
    .vertical_spacing(Space::Xl.px());
    let close = Message::CloseDevice;
    let frame = crate::settings_frame(
        &phrase(Phrase::SettingsLabel, words),
        pages(window, Open::Appearance, words),
        phrase(Phrase::Appearance, words),
        None,
        choices._boxed(),
        crate::SettingsWays {
            close: (phrase(Phrase::Close, words), close.clone()),
            shown: fold,
            back: (phrase(Phrase::SettingsBack, words), Message::SettingsList),
        },
    );
    crate::on_escape(frame, close, None)._boxed()
}

/// A part of the page under its field label.
fn section<'a, R: Renderer + 'a>(name: Phrase, content: Part<'a, R>, words: &'a Words) -> Part<'a, R> {
    Column::with_children([crate::field_label(words.phrase(name).to_string()), content])
        .spacing(Space::S.px() - Space::Xxs.px())
        ._boxed()
}

/// A message's text size: a pill a step, in pixels, and the default under them.
fn message_sizes<'a, R: Renderer + 'a>(chosen: MessageSize, words: &'a Words) -> Part<'a, R> {
    let steps: Vec<MessageSize> = MessageSize::all().collect();
    let track = choice_track(
        phrase(Phrase::MessageSizeLabel, words),
        steps.iter().map(|size| (label(&size.px().to_string()), Message::Look(LookPick::MessageSize(*size)))).collect(),
        steps.iter().position(|size| *size == chosen).unwrap_or_default(),
    );
    noted(track, words.say(&Sentence::MessageSizeNote(MessageSize::DEFAULT.px())))
}

/// The space between a channel's runs: a pill a step, in pixels, and the default under them.
fn run_gaps<'a, R: Renderer + 'a>(chosen: RunGap, words: &'a Words) -> Part<'a, R> {
    let steps: Vec<RunGap> = RunGap::all().collect();
    let track = choice_track(
        phrase(Phrase::RunGapLabel, words),
        steps.iter().map(|gap| (label(&gap.px().to_string()), Message::Look(LookPick::RunGap(*gap)))).collect(),
        steps.iter().position(|gap| *gap == chosen).unwrap_or_default(),
    );
    noted(track, words.say(&Sentence::RunGapNote(RunGap::DEFAULT.px())))
}

/// A choice with its note under it.
fn noted<'a, R: Renderer + 'a>(choice: Part<'a, R>, note: String) -> Part<'a, R> {
    Column::with_children([choice, coloured(note, TypeStyle::Caption, TextClass::Content(Content::Secondary))._boxed()])
        .spacing(Space::Xs.px())
        ._boxed()
}

/// Two runs drawn as the channel draws them at `scale`, framed on the window's ground.
fn preview<'a, R: Renderer + 'a>(scale: MessageScale, words: &'a Words) -> Part<'a, R> {
    let mut rows: Vec<Part<'a, R>> = Vec::new();
    for run in screens::PREVIEW {
        let name = words.phrase(run.author).into_owned();
        for (at, line) in run.lines.iter().enumerate() {
            let words_of = message_text(words.phrase(*line).into_owned(), TypeStyle::Message, scale.size)._boxed();
            let (gutter, lines) = if at == 0 {
                let picture = avatar(label(&name), AvatarKind::Called(run.colour), AvatarSize::Author);
                let author = message_text(name.clone(), TypeStyle::Author, scale.size)._boxed();
                let head = crate::discussion::author_line(author, words.clock(run.at), scale.size);
                (Some(picture), vec![head, words_of])
            } else {
                (None, vec![words_of])
            };
            rows.push(crate::discussion::message_frame(gutter, lines, Space::L.px(), scale)._boxed());
        }
    }
    container(Column::with_children(rows))
        .padding(Padding { top: Space::Xs.px(), bottom: Space::M.px() + Space::Xxs.px(), ..Padding::ZERO })
        .width(Length::Fill)
        .class(Surfaced::Framed(Surface::Base, Radius::Xl))
        ._boxed()
}

/// The zoom: − and + round its percent on a track, Reset beside them while it is not 100 %, and its keys under them.
fn zoom<'a, R: Renderer + 'a>(zoom: Zoom, words: &'a Words) -> Part<'a, R> {
    let step = |name, glyph, to: Option<Zoom>, pick| {
        let button = Button::new(phrase(name, words)).icon_only(glyph).variant(ButtonVariant::Ghost).compact();
        match to {
            Some(_) => button.on_press(Message::Look(LookPick::Zoom(pick))),
            None => button,
        }
        .view::<R>()
    };
    let percent = container(text(words.percent(zoom.percent()), TypeStyle::Mono)).center_x(Length::Fixed(ZOOM_FIGURE));
    let track = container(
        Row::with_children([
            step(Phrase::ZoomOut, Icon::Minus, zoom.zoomed_out(), ZoomPick::Out),
            percent._boxed(),
            step(Phrase::ZoomIn, Icon::Plus, zoom.zoomed_in(), ZoomPick::In),
        ])
        .spacing(Space::Xxs.px())
        .align_y(Alignment::Center),
    )
    .padding(Space::Xs.px())
    .class(Surfaced::Framed(Surface::Raised, Radius::Full));
    let mut line = vec![track._boxed()];
    if zoom != Zoom::DEFAULT {
        let reset = Message::Look(LookPick::Zoom(ZoomPick::Reset));
        line.push(Button::new(phrase(Phrase::ZoomReset, words)).compact().on_press(reset).view());
    }
    Column::with_children([
        Row::with_children(line).spacing(Space::M.px()).align_y(Alignment::Center)._boxed(),
        coloured(words.phrase(Phrase::ZoomKeys), TypeStyle::Caption, TextClass::Content(Content::Secondary))._boxed(),
    ])
    .spacing(Space::Xs.px())
    ._boxed()
}
