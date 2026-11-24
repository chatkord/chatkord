//! A link's preview: the card under a message, one button that opens the link (its host, title, description and a
//! picture fitted in its box), and the bar over the composer while a draft's link is read (its host and title, or that
//! the page is being read) with the way to take it out.

use crate::button::{ButtonClass, actionable};
use crate::labels::{label, phrase};
use crate::typography::{coloured, line};
use crate::{Button, Icon, Renderer, Surfaced, TextClass, Theme};
use design::{
    ButtonVariant, Content, Label, PREVIEW_CARD_WIDTH, PREVIEW_COMPOSER_PICTURE, PREVIEW_DESCRIPTION_LINES,
    PREVIEW_TITLE_LINES, Radius, Role, SemanticRole, Semantics, Space, Surface, TypeStyle, preview_picture_fit,
};
use iced_core::image::Handle;
use iced_core::{Alignment, ContentFit, Element, Length, Padding, Widget};
use iced_widget::{Column, Row, Space as Gap, container};
use words::{Phrase, Words};

type Part<'a, Message, R> = Element<'a, Message, Theme, R>;

/// A preview under a message.
pub struct LinkCard<Message> {
    /// The link's host.
    pub host: String,
    /// The page's title.
    pub title: String,
    /// The page's description; empty for none.
    pub description: String,
    /// The page's picture: its sides, and its thumbnail once the host holds it.
    pub picture: Option<CardPicture>,
    /// What a screen reader says of the press: it opens the page, named by its title and host.
    pub label: Label,
    /// What a press sends: the link opens as a link in the text does.
    pub open: Message,
}

/// The picture of a [`LinkCard`].
pub struct CardPicture {
    /// Its width in pixels.
    pub width: u32,
    /// Its height in pixels.
    pub height: u32,
    /// Its thumbnail, decoded; none while it is not here yet, which draws its box empty.
    pub thumbnail: Option<Handle>,
}

/// What the bar over the composer shows.
pub enum PreviewBar {
    /// The page is being read: the link's host.
    Loading(String),
    /// Made.
    Ready {
        /// The link's host.
        host: String,
        /// The page's title.
        title: String,
        /// The page's picture, small, once the host holds it.
        picture: Option<Handle>,
    },
}

/// `lines` of `content` at most, a longer text cut at its last whole line.
fn clamped<'a, Message: 'a, R: Renderer + 'a>(
    content: String,
    style: TypeStyle,
    class: TextClass,
    lines: usize,
) -> Part<'a, Message, R> {
    #[expect(clippy::cast_precision_loss, reason = "a handful of lines")]
    let most = style.line_height() * lines as f32;
    container(coloured(content, style, class)).height(Length::Shrink.max(most)).clip(true)._boxed()
}

/// A preview as a card: host, title, description and a picture, one button.
pub fn link_card<'a, Message: Clone + 'a, R: Renderer + 'a>(card: LinkCard<Message>) -> Part<'a, Message, R> {
    let mut lines: Vec<Part<'a, Message, R>> = vec![
        line(card.host, TypeStyle::Caption).class(TextClass::Content(Content::Secondary))._boxed(),
        clamped(card.title, TypeStyle::BodyStrong, TextClass::Accent, PREVIEW_TITLE_LINES),
    ];
    if !card.description.is_empty() {
        lines.push(clamped(
            card.description,
            TypeStyle::Small,
            TextClass::Content(Content::Secondary),
            PREVIEW_DESCRIPTION_LINES,
        ));
    }
    if let Some(picture) = card.picture {
        let (width, height) = preview_picture_fit(picture.width, picture.height);
        let shown: Part<'a, Message, R> = match picture.thumbnail {
            Some(handle) => iced_widget::image(handle)
                .content_fit(ContentFit::Cover)
                .width(Length::Fill)
                .height(Length::Fill)
                .border_radius(Radius::S.px())
                ._boxed(),
            None => Gap::new().width(Length::Fill).height(Length::Fill)._boxed(),
        };
        lines.push(
            container(container(shown).width(Length::Fixed(width)).height(Length::Fixed(height)))
                .padding(Padding { top: Space::Xs.px(), ..Padding::ZERO })
                ._boxed(),
        );
    }
    let framed = container(Column::with_children(lines).spacing(Space::Xs.px()))
        .padding(Space::M.px())
        .width(Length::Fill)
        .class(Surfaced::Framed(Surface::Inset, Radius::M));
    let pressed = iced_widget::button(framed).class(ButtonClass::Swatch).padding(0).on_press(card.open.clone());
    let button = actionable(pressed, Semantics { role: SemanticRole::Button, label: card.label }, Some(card.open));
    container(container(button).width(Length::Fill.max(PREVIEW_CARD_WIDTH)))
        .padding(Padding { top: Space::Xs.px() + Space::Xxs.px(), ..Padding::ZERO })
        ._boxed()
}

/// The bar over the composer for a draft's link, with the way to take it out.
pub fn preview_bar<'a, Message: Clone + 'a, R: Renderer + 'a>(
    bar: PreviewBar,
    remove: Message,
    words: &'a Words,
) -> Part<'a, Message, R> {
    let mut cells: Vec<Part<'a, Message, R>> = Vec::new();
    match bar {
        PreviewBar::Loading(said) => {
            cells.push(crate::spinner(Role::Content(Content::Secondary)));
            cells.push(
                line(said, TypeStyle::Small).class(TextClass::Content(Content::Secondary)).width(Length::Fill)._boxed(),
            );
        }
        PreviewBar::Ready { host, title, picture } => {
            if let Some(handle) = picture {
                cells.push(
                    iced_widget::image(handle)
                        .content_fit(ContentFit::Cover)
                        .width(Length::Fixed(PREVIEW_COMPOSER_PICTURE))
                        .height(Length::Fixed(PREVIEW_COMPOSER_PICTURE))
                        .border_radius(Radius::S.px())
                        ._boxed(),
                );
            }
            cells.push(
                Column::with_children([
                    line(host, TypeStyle::Caption).class(TextClass::Content(Content::Secondary))._boxed(),
                    line(title, TypeStyle::SmallStrong)._boxed(),
                ])
                .width(Length::Fill)
                ._boxed(),
            );
        }
    }
    cells.push(
        Button::new(phrase(Phrase::RemovePreview, words))
            .icon_only(Icon::Close)
            .variant(ButtonVariant::Quiet)
            .small()
            .on_press(remove)
            .view::<R>(),
    );
    let row = Row::with_children(cells).spacing(Space::S.px() + Space::Xxs.px()).align_y(Alignment::Center);
    let bar = container(row)
        .padding(Padding {
            top: Space::S.px(),
            bottom: Space::S.px(),
            left: Space::S.px() + Space::Xxs.px() + Space::Xxs.px(),
            right: Space::Xs.px(),
        })
        .width(Length::Fill)
        .class(Surfaced::On(Surface::Raised));
    Column::with_children([bar._boxed(), crate::panel_parts::hairline()])._boxed()
}

/// The words a screen reader gives a card's press: the page's title and the link's host.
pub(crate) fn card_label(title: &str, host: &str, words: &Words) -> Label {
    label(&words.say(&words::Sentence::OpenPreview { title: title.to_owned(), host: host.to_owned() }))
}
