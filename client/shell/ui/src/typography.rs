//! Text in the scale. Every piece of text a component draws is one of `design::TypeStyle`, so the
//! size, the line height and the weight's font are set here once and not at each use.

use crate::{TextClass, Theme, fonts};
use design::{MessageSize, TypeStyle};
use iced_core::Pixels;
use iced_core::text::{LineHeight, Wrapping};
use iced_widget::text::{self, IntoFragment};

/// `content` in a type style, in the colour its container gives it: breaks at words, and inside a
/// word too where one is longer than the line, so nothing runs out of its box.
pub fn text<'a>(content: impl IntoFragment<'a>, style: TypeStyle) -> text::Text<'a, Theme> {
    text::Text::new(content)
        .size(Pixels(style.size()))
        .line_height(LineHeight::Absolute(Pixels(style.line_height())))
        .font(fonts::font(style))
        .wrapping(Wrapping::WordOrGlyph)
}

/// [`text`] in a message, set at the person's `size`: a message's own styles grow with it.
pub(crate) fn message_text<'a>(
    content: impl IntoFragment<'a>,
    style: TypeStyle,
    size: MessageSize,
) -> text::Text<'a, Theme> {
    text(content, style)
        .size(Pixels(size.size(style)))
        .line_height(LineHeight::Absolute(Pixels(size.line_height(style))))
}

/// One line of text that ends in an ellipsis where it does not fit, for a name in a list: the whole of it is
/// the control's label, and the control shows it in a tooltip.
pub(crate) fn line<'a>(content: impl IntoFragment<'a>, style: TypeStyle) -> text::Text<'a, Theme> {
    text(content, style).wrapping(Wrapping::None).ellipsis(iced_core::text::Ellipsis::End)
}

/// One underlined line in a type style, in the colour its container gives it, as a message's link is underlined.
pub(crate) fn underlined<'a, Message: 'a>(
    content: impl IntoFragment<'a>,
    style: TypeStyle,
) -> text::Rich<'a, (), Message, Theme> {
    text::Rich::with_spans([text::Span::new(content).underline(true)])
        .size(Pixels(style.size()))
        .line_height(LineHeight::Absolute(Pixels(style.line_height())))
        .font(fonts::font(style))
        .wrapping(Wrapping::None)
}

/// [`text`] in a colour of its own.
pub(crate) fn coloured<'a>(
    content: impl IntoFragment<'a>,
    style: TypeStyle,
    class: TextClass,
) -> text::Text<'a, Theme> {
    text(content, style).class(class)
}
