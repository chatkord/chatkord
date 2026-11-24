//! The search layer: one large panel over the window with the server's name, the field and what was found, Messages
//! beside Files where there is room for both and Files under Messages where not; or, in their place, that nothing
//! was typed yet, that nothing matched, that the search did not answer, or that it is under way.

use crate::centre_panel::{Face, centre_panel};
use crate::labels::{label, phrase};
use crate::pins_panel::{Pin, pin_card};
use crate::typography::{coloured, line, text};
use crate::{Button, Icon, IconClass, Labelled, Renderer, Surfaced, TextClass, Theme, icon};
use design::{
    AccentRole, ButtonVariant, Content, DIALOG_RADIUS, IconSize, Label, Radius, Role, SEARCH_FILES_LEAST,
    SEARCH_MESSAGES_LEAST, SEARCH_STATE_WIDTH, SemanticRole, Semantics, Space, Status, Surface, TypeStyle,
};
use iced_core::{Alignment, Element, Length, Padding, Widget};
use iced_widget::{Column, Row, container, scrollable};
use words::{Phrase, Text, Words};

/// A file whose name matched, as the layer lists it.
pub struct FoundFile<Message> {
    /// Its name, as sent.
    pub name: String,
    /// Its size, the place it was sent in and the day, said.
    pub note: String,
    /// Whether it is a picture, marked as one.
    pub picture: bool,
    /// What Jump sends: the message it is attached to opens.
    pub jump: Message,
}

/// What the layer holds under its field.
pub enum SearchBody<'a, Message, R> {
    /// Nothing is typed yet.
    Start,
    /// Matches: messages each with the place it was written in, and files.
    Found {
        /// The messages, newest first.
        messages: Vec<(Option<String>, Pin<'a, Message, R>)>,
        /// The files, newest first; none where the server does not search them, and the section is left out.
        files: Option<Vec<FoundFile<Message>>>,
        /// What Show more sends, while older matches follow.
        more: Option<Message>,
    },
    /// Nothing matched what was typed.
    Nothing(String),
    /// The search did not answer; the message is what Try again sends.
    Failed(Message),
    /// The search is under way.
    Waiting,
}

/// The layer headed `title` in a window of `size`, kept `SEARCH_LAYER_INSET` from its edges, with `field` under the
/// heading, holding the focus as the layer opens, and `body` under that; its sections sit side by side where the layer
/// holds both.
pub fn search_layer<'a, Message: Clone + 'a, R: Renderer + 'a>(
    title: Label,
    field: Element<'a, Message, Theme, R>,
    body: SearchBody<'a, Message, R>,
    on_close: Message,
    size: iced_core::Size,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let inset = design::SEARCH_LAYER_INSET;
    let (across, high) = ((size.width - 2.0 * inset).max(0.0), (size.height - 2.0 * inset).max(0.0));
    let pad = Space::Xl.px();
    let inside = (across - 2.0 * pad).max(0.0);
    let shut = Button::new(phrase(Phrase::Close, words))
        .icon_only(Icon::Close)
        .variant(ButtonVariant::Ghost)
        .compact()
        .on_press(on_close)
        .view::<R>();
    let head =
        Row::with_children([text(title.as_str().to_string(), TypeStyle::Title).width(Length::Fill)._boxed(), shut])
            .align_y(Alignment::Center);
    // Opened to be typed in: the field, not the close button before it, has the focus.
    let field = crate::focus::take_focus(field)._boxed();
    // The close button is taller than the title's line: the room above and below the head is less by its overhang.
    let top = Column::with_children([head._boxed(), field]).spacing(Space::S.px()).padding(Padding {
        top: pad - Space::S.px(),
        bottom: Space::L.px(),
        left: pad,
        right: pad,
    });
    let said = |phrase: Phrase| Some(words.phrase(phrase).into_owned());
    let finds = || Some(words.text(Text::SearchFinds).into_owned());
    let below: Element<'a, Message, Theme, R> = match body {
        SearchBody::Start => centred(Face::Accent(Icon::Search), said(Phrase::SearchStart), finds(), Vec::new()),
        SearchBody::Nothing(query) => centred(
            Face::Accent(Icon::Search),
            Some(words.say(&words::Sentence::SearchNothing(query))),
            finds(),
            Vec::new(),
        ),
        SearchBody::Failed(retry) => {
            let again = Button::new(phrase(Phrase::TryAgain, words)).variant(ButtonVariant::Primary).on_press(retry);
            centred(
                Face::Status(Icon::CircleAlert, Status::Danger),
                said(Phrase::SearchFailed),
                None,
                vec![again.view::<R>()],
            )
        }
        SearchBody::Waiting => centred(Face::Waiting, said(Phrase::SearchWaiting), None, Vec::new()),
        SearchBody::Found { messages, files, more } => found(messages, files, more, inside, words),
    };
    let scrolled =
        scrollable(container(below).padding(Padding { top: Space::L.px(), bottom: pad, left: pad, right: pad }))
            .direction(iced_widget::scrollable::Direction::Vertical(crate::theme::scroll_bar()))
            .height(Length::Fill);
    let panel = container(Column::with_children([top._boxed(), crate::panel_parts::hairline(), scrolled._boxed()]))
        .width(Length::Fixed(across))
        .height(Length::Fixed(high))
        .class(Surfaced::Panel(design::Elevation::Overlay, DIALOG_RADIUS));
    Labelled::new(panel, Semantics { role: SemanticRole::Dialog, label: title })._boxed()
}

fn centred<'a, Message: Clone + 'a, R: Renderer + 'a>(
    face: Face,
    title: Option<String>,
    body: Option<String>,
    actions: Vec<Element<'a, Message, Theme, R>>,
) -> Element<'a, Message, Theme, R> {
    container(container(centre_panel(face, title, body, actions)).width(Length::Fixed(SEARCH_STATE_WIDTH)))
        .center_x(Length::Fill)
        .padding(Padding { top: Space::Xl.px(), ..Padding::ZERO })
        ._boxed()
}

/// The sections: Messages with their count and Show more, and Files with theirs where the server searches them.
fn found<'a, Message: Clone + 'a, R: Renderer + 'a>(
    messages: Vec<(Option<String>, Pin<'a, Message, R>)>,
    files: Option<Vec<FoundFile<Message>>>,
    more: Option<Message>,
    inside: f32,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let secondary = TextClass::Content(Content::Secondary);
    let heading = |said: Phrase, count: usize| {
        let count = words.count(u64::try_from(count).unwrap_or(u64::MAX));
        Row::with_children([
            text(phrase(said, words).as_str().to_string(), TypeStyle::BodyStrong)._boxed(),
            coloured(count, TypeStyle::BodyStrong, secondary)._boxed(),
        ])
        .spacing(Space::Xs.px())
        .padding(Padding::from([Space::Xxs.px(), Space::Xs.px()]))
        ._boxed()
    };
    let none = |said: Phrase| {
        container(coloured(words.phrase(said).into_owned(), TypeStyle::Small, secondary))
            .padding(Padding::from([0.0, Space::Xs.px()]))
            ._boxed()
    };
    let mut first: Vec<Element<'a, Message, Theme, R>> = vec![heading(Phrase::MessagesHeading, messages.len())];
    if messages.is_empty() {
        first.push(none(Phrase::NoMessagesFound));
    }
    first.extend(messages.into_iter().map(|(place, pin)| pin_card(pin, place, words)));
    if let Some(more) = more {
        let button = Button::new(phrase(Phrase::ShowMore, words)).variant(ButtonVariant::Secondary).on_press(more);
        first.push(container(button.view::<R>()).center_x(Length::Fill)._boxed());
    }
    let first = Column::with_children(first).spacing(Space::S.px());
    let Some(files) = files else { return first.width(Length::Fill)._boxed() };
    let mut second: Vec<Element<'a, Message, Theme, R>> = vec![heading(Phrase::FilesHeading, files.len())];
    if files.is_empty() {
        second.push(none(Phrase::NoFilesFound));
    }
    second.extend(files.into_iter().map(|file| file_row(file, words)));
    let second = Column::with_children(second).spacing(Space::S.px());
    let gap = Space::L.px();
    if inside >= SEARCH_MESSAGES_LEAST + gap + SEARCH_FILES_LEAST {
        Row::with_children([
            first.width(Length::FillPortion(3))._boxed(),
            second.width(Length::FillPortion(2))._boxed(),
        ])
        .spacing(gap)
        .align_y(Alignment::Start)
        ._boxed()
    } else {
        Column::with_children([first.width(Length::Fill)._boxed(), second.width(Length::Fill)._boxed()])
            .spacing(gap)
            ._boxed()
    }
}

fn file_row<'a, Message: Clone + 'a, R: Renderer + 'a>(
    file: FoundFile<Message>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let jump = Button::new(phrase(Phrase::Jump, words))
        .variant(ButtonVariant::Secondary)
        .small()
        .on_press(file.jump)
        .view::<R>();
    let said = Column::with_children([
        line(file.name.clone(), TypeStyle::BodyStrong)._boxed(),
        coloured(file.note, TypeStyle::Stamp, TextClass::Content(Content::Secondary))._boxed(),
    ])
    .spacing(Space::Xxs.px())
    .width(Length::Fill);
    let kind = if file.picture { Icon::Image } else { Icon::File };
    let row = Row::with_children([
        icon(kind, IconSize::L, IconClass::Role(Role::Accent(AccentRole::Text)))._boxed(),
        said._boxed(),
        jump,
    ])
    .spacing(Space::M.px())
    .align_y(Alignment::Center);
    let framed = container(row)
        .padding(Padding::from([Space::S.px() + Space::Xxs.px(), Space::M.px()]))
        .width(Length::Fill)
        .class(Surfaced::Framed(Surface::Inset, Radius::M));
    Labelled::new(framed, Semantics { role: SemanticRole::ListItem, label: label(&file.name) })._boxed()
}
