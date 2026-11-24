//! This device's Settings › About: this build's version, licence and source, where its update stands, and the projects
//! it is built with, each group folding open to where it was built from and its licence's text.

use crate::account_page::{Open, pages};
use crate::labels::phrase;
use crate::text_box::text_box;
use crate::{
    Button, Labelled, Renderer, TextClass, Theme, coloured, flush_card, fold, list_card, switch_offered, text,
};
use choices::AutoUpdate;
use design::{
    ButtonVariant, Content, DEVICE_COLUMN_WIDTH, LICENCE_TEXT_HEIGHT, SemanticRole, Semantics, Space, TypeStyle,
};
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Column, Row, container};
use screens::{Carried, ReportMessage, UpdateButton, UpdateCard, UpdateMessage, WindowMessage as Message};
use words::{Phrase, Sentence, Text, Words};

type Part<'a, R> = Element<'a, Message, Theme, R>;

pub(crate) fn view<'a, R: Renderer + 'a>(
    window: &'a screens::Window,
    fold_shown: screens::frame::SettingsShown,
    words: &'a Words,
) -> Part<'a, R> {
    let stated = window.stated();
    let source = match stated.source {
        Some(screens::Origin::Tag(tag)) => words.say(&Sentence::SourceTag(tag)),
        Some(screens::Origin::Commit(commit)) => words.say(&Sentence::SourceCommit(commit)),
        None => phrase(Phrase::SourceLocal, words).as_str().to_string(),
    };
    let facts = version_card(
        Phrase::AboutClient,
        vec![
            (Phrase::AboutVersion, stated.version),
            (Phrase::AboutProtocol, stated.protocol),
            (Phrase::AboutLicence, stated.licence.to_string()),
            (Phrase::AboutSource, source),
        ],
        words,
    );
    let mut parts = vec![heading_first(Phrase::AboutClient, words), facts, note(words.text(Text::FreeSoftware))];
    let servers = window.server_builds();
    if !servers.is_empty() {
        let rows = servers.into_iter().map(|(name, build)| fact(name, build.to_string())).collect();
        parts.push(heading(Phrase::AboutServers, words));
        parts.push(list_card(phrase(Phrase::AboutServers, words), rows, None));
    }
    parts.extend([
        heading(Phrase::Updates, words),
        updates(&window.update_card(), words),
        heading(Phrase::DebugReport, words),
        debug_report(window.downloaded(), window.report_refused(), words),
        heading(Phrase::BuiltWith, words),
        note(words.phrase(Phrase::BuiltWithBody)),
    ]);
    match window.carried() {
        Carried::Read { notices, open } => {
            let rows = notices.groups().iter().enumerate().map(|(at, group)| {
                let inside = (*open == Some(at)).then(|| opened(group.source(), group.text()));
                fold(group.name(), group.what(), group.licence(), Message::ToggleNotice(at), inside)
            });
            parts.push(flush_card(phrase(Phrase::BuiltWith, words), rows.collect()));
        }
        Carried::Absent => parts.push(note(words.phrase(Phrase::NoticesInPackage))),
        Carried::Unasked | Carried::Reading => {}
    }
    let content = Column::with_children(parts).spacing(Space::L.px()).width(Length::Fill.max(DEVICE_COLUMN_WIDTH));
    let close = Message::CloseDevice;
    let frame = crate::settings_frame(
        &phrase(Phrase::SettingsLabel, words),
        pages(window, Open::About, words),
        phrase(Phrase::About, words),
        None,
        content._boxed(),
        crate::SettingsWays {
            close: (phrase(Phrase::Close, words), close.clone()),
            shown: fold_shown,
            back: (phrase(Phrase::SettingsBack, words), Message::SettingsList),
        },
    );
    crate::on_escape(frame, close, None)._boxed()
}

/// A heading over a part of an About page, this device's or a server's.
pub(crate) fn heading<'a, R: Renderer + 'a>(said: Phrase, words: &'a Words) -> Part<'a, R> {
    heading_with(said, Space::S.px(), words)
}

/// The heading that opens the page: the title above it already gives the room.
fn heading_first<'a, R: Renderer + 'a>(said: Phrase, words: &'a Words) -> Part<'a, R> {
    heading_with(said, 0.0, words)
}

fn heading_with<'a, R: Renderer + 'a>(said: Phrase, top: f32, words: &'a Words) -> Part<'a, R> {
    let said = phrase(said, words);
    Labelled::new(
        container(text(said.as_str().to_string(), TypeStyle::Subheading)).padding(Padding { top, ..Padding::ZERO }),
        Semantics { role: SemanticRole::Heading, label: said },
    )
    ._boxed()
}

/// The Updates card: a line on where the update stands with the button it offers, and the switch that has Chatkord
/// check by itself, drawn off and unusable where updates are not offered.
fn updates<'a, R: Renderer + 'a>(card: &UpdateCard, words: &'a Words) -> Part<'a, R> {
    let line = crate::labels::label(&words.say(&card.line));
    let button = card.button.map(|button| {
        let (said, variant, message) = match button {
            UpdateButton::Check => (Phrase::CheckForUpdates, ButtonVariant::Secondary, UpdateMessage::Check),
            UpdateButton::Restart => (Phrase::UpdateRestart, ButtonVariant::Primary, UpdateMessage::Restart),
        };
        Button::new(phrase(said, words)).variant(variant).compact().on_press(Message::Update(message)).view::<R>()
    });
    let status = Row::with_children(
        std::iter::once(
            coloured(line.as_str().to_string(), TypeStyle::Body, TextClass::Content(Content::Primary))
                .width(Length::Fill)
                ._boxed(),
        )
        .chain(button),
    )
    .spacing(Space::M.px() + Space::Xxs.px())
    // As tall as its button, so the card keeps its height whether one is offered.
    .height(Length::Shrink.min(design::HIT_TARGET))
    .align_y(Alignment::Center);
    let status = Labelled::new(status, Semantics { role: SemanticRole::Status, label: line });
    let automatic = switch_offered(
        &phrase(Phrase::AutoUpdate, words),
        Some(&words.phrase(Phrase::AutoUpdateHint)),
        card.automatic == Some(AutoUpdate::On),
        card.automatic.map(|_| Message::Update(UpdateMessage::ToggleAutomatic)),
    );
    list_card(phrase(Phrase::Updates, words), vec![status._boxed(), automatic], None)
}

/// The Debug report card: the client's report with a line on what it holds, or on the file it was last saved as, and
/// the button that shows it.
fn debug_report<'a, R: Renderer + 'a>(downloaded: Option<&str>, refused: bool, words: &'a Words) -> Part<'a, R> {
    let line = match (refused, downloaded) {
        (true, _) => words.phrase(Phrase::ReportNotMade).into_owned(),
        (false, Some(file)) => words.say(&Sentence::ReportDownloaded(file.to_string())),
        (false, None) => words.phrase(Phrase::ClientDebugReportLine).into_owned(),
    };
    let card = ReportCard {
        name: Phrase::DebugReport,
        title: Phrase::ClientDebugReport,
        line,
        alert: refused,
        button: ReportButton::Press(Box::new(Message::Report(ReportMessage::Open))),
    };
    report_card(card, words)
}

/// What a debug report's card offers in place of its Download while the report is not asked for.
pub(crate) enum ReportButton {
    /// Download, sending this.
    Press(Box<Message>),
    /// The report is being made: the card says so instead of offering Download.
    Preparing,
}

/// A debug report's card, the client's or a server's: its title, a line under it, and the button that makes the report.
pub(crate) struct ReportCard {
    /// What the card's list is named, for a screen reader.
    pub(crate) name: Phrase,
    /// The report's title.
    pub(crate) title: Phrase,
    /// What it says under the title.
    pub(crate) line: String,
    /// The line says a refusal: it is said as an alert, in the danger colour.
    pub(crate) alert: bool,
    /// What stands at the card's end.
    pub(crate) button: ReportButton,
}

/// `card`, drawn the one way for either report.
pub(crate) fn report_card<'a, R: Renderer + 'a>(card: ReportCard, words: &'a Words) -> Part<'a, R> {
    let (role, class) = if card.alert {
        (SemanticRole::Alert, TextClass::Status(design::Status::Danger))
    } else {
        (SemanticRole::Status, TextClass::Content(Content::Secondary))
    };
    let what = Column::with_children([
        text(words.phrase(card.title), TypeStyle::BodyStrong)._boxed(),
        coloured(card.line.clone(), TypeStyle::Small, class)._boxed(),
    ])
    .spacing(Space::Xxs.px())
    .width(Length::Fill);
    let what = Labelled::new(what, Semantics { role, label: crate::labels::label(&card.line) });
    let end = match card.button {
        ReportButton::Press(message) => Button::new(phrase(Phrase::DownloadReport, words))
            .variant(ButtonVariant::Secondary)
            .compact()
            .on_press(*message)
            .view::<R>(),
        ReportButton::Preparing => {
            let said = phrase(Phrase::ReportPreparing, words);
            let shown = coloured(said.as_str().to_string(), TypeStyle::Small, TextClass::Content(Content::Secondary));
            Labelled::new(shown, Semantics { role: SemanticRole::Status, label: said })._boxed()
        }
    };
    let row =
        Row::with_children([what._boxed(), end]).spacing(Space::M.px() + Space::Xxs.px()).align_y(Alignment::Center);
    list_card(phrase(card.name, words), vec![row._boxed()], None)
}

/// The card of what a build is, the client's or a server's, drawn the one way: a row for each fact, named by a phrase.
pub(crate) fn version_card<'a, R: Renderer + 'a>(
    name: Phrase,
    facts: Vec<(Phrase, String)>,
    words: &'a Words,
) -> Part<'a, R> {
    let rows = facts.into_iter().map(|(fact_of, value)| fact(words.phrase(fact_of), value)).collect();
    list_card(phrase(name, words), rows, None)
}

/// A row of a card of facts, named by `name`: a phrase, or a server's own name.
fn fact<'a, R: Renderer + 'a>(name: impl Into<String>, value: String) -> Part<'a, R> {
    Row::with_children([
        coloured(name.into(), TypeStyle::BodyStrong, TextClass::Content(Content::Primary)).width(Length::Fill)._boxed(),
        coloured(value, TypeStyle::Mono, TextClass::Content(Content::Secondary))._boxed(),
    ])
    .spacing(Space::M.px() + Space::Xxs.px())
    .align_y(Alignment::Center)
    ._boxed()
}

/// A quiet line of the page.
fn note<'a, R: Renderer + 'a>(said: std::borrow::Cow<'a, str>) -> Part<'a, R> {
    coloured(said, TypeStyle::Small, TextClass::Content(Content::Secondary))._boxed()
}

/// An open group: where it was built from, then its licence's text, which scrolls past a height of its own.
fn opened<'a, R: Renderer + 'a>(source: Option<&'a str>, licence: &'a str) -> Part<'a, R> {
    let source = source.map(|source| coloured(source, TypeStyle::Small, TextClass::Content(Content::Secondary)));
    let shown = text_box(text(licence, TypeStyle::MonoCaption)._boxed(), LICENCE_TEXT_HEIGHT);
    Column::with_children(source.map(|source| source._boxed()).into_iter().chain([shown]))
        .spacing(Space::S.px())
        ._boxed()
}
