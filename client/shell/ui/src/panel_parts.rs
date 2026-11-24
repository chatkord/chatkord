//! What the menus and popovers over a window are made of alike: a section's name in capitals, and the hairline that
//! sets a group off.

use crate::{Surfaced, TextClass, Theme, coloured};
use design::{Border, Content, Label, Radius, Role, TypeStyle};
use iced_core::{Length, Widget};
use iced_widget::{Space as Gap, container, text::Text};

/// A section's name in capitals, as the lists of the side pane are headed.
pub(crate) fn section_heading<'a>(name: &Label) -> Text<'a, Theme> {
    coloured(name.as_str().to_uppercase(), TypeStyle::CaptionStrong, TextClass::Content(Content::Secondary))
}

/// A one-pixel line across the width it is given.
pub(crate) fn hairline<'a, Message: 'a, R: crate::Renderer + 'a>() -> iced_core::Element<'a, Message, Theme, R> {
    container(Gap::new())
        .width(Length::Fill)
        .height(Length::Fixed(design::BORDER_WIDTH))
        .class(Surfaced::Fill {
            fill: Role::Border(Border::Subtle),
            text: Role::Content(Content::Primary),
            radius: Radius::None,
        })
        ._boxed()
}
