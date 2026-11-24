//! The debug report's dialog: what the file will hold, every section listed in full and a log by its newest lines, in a
//! box that scrolls, over Cancel and Download.

use crate::body_text::{BodyText, Ink, Stretch};
use crate::labels::phrase;
use crate::text_box::text_box;
use crate::{Button, Renderer, TextClass, coloured, fonts, refusal};
use design::{ButtonVariant, Content, REPORT_TEXT_HEIGHT, Space, TypeStyle};
use iced_core::{Element, Widget};
use iced_widget::Column;
use screens::{ReportSide, WindowMessage as Message};
use words::{Phrase, Text, Words};

type Part<'a, R> = Element<'a, Message, crate::Theme, R>;

/// The dialog while the report is shown, and what Escape and a press outside it send.
pub(crate) fn view<'a, R: Renderer + 'a>(
    window: &'a screens::Window,
    words: &'a Words,
) -> Option<(Part<'a, R>, Message)> {
    let screens::ReportDialog { side, shown, cancel: close, download: press } = window.report_dialog()?;
    let style = TypeStyle::MonoCaption;
    let listing = Stretch {
        text: std::borrow::Cow::Owned(shown.listing().text(|sentence| words.say(sentence))),
        said: None,
        font: fonts::font(style),
        size: None,
        ink: Ink::Text,
        ground: None,
        pad: 0.0,
        underline: false,
        strike: false,
        press: None,
        picture: None,
    };
    let listing = BodyText::new(vec![listing], style.size(), style.line_height(), fonts::font(style));
    let mut body = vec![
        coloured(words.text(Text::ReportBody), TypeStyle::Body, TextClass::Content(Content::Secondary))._boxed(),
        text_box(listing._boxed(), REPORT_TEXT_HEIGHT),
    ];
    if shown.failed() {
        body.push(refusal(words.phrase(Phrase::ReportNotSaved).into_owned()));
    }
    let download = Button::new(phrase(Phrase::DownloadReport, words)).variant(ButtonVariant::Primary);
    let download = if shown.saving() { download } else { download.on_press(press) };
    let actions = vec![
        Button::new(phrase(Phrase::Cancel, words))
            .variant(ButtonVariant::Secondary)
            .on_press(close.clone())
            .view::<R>(),
        download.view::<R>(),
    ];
    let body = Column::with_children(body).spacing(Space::M.px());
    let title = match side {
        ReportSide::Client => Phrase::ClientDebugReport,
        ReportSide::Server => Phrase::ServerDebugReport,
    };
    let panel = crate::dialog_panel(phrase(title, words), body._boxed(), actions, close.clone(), words);
    Some((panel, close))
}
