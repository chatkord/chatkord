//! A picture of a person or a server: its initials on a circle for a person, a rounded square for a server. A person's
//! is a quiet disc; a server's is in its own colour, so the rail tells servers apart without a picture.

use crate::typography::text;
use crate::{Labelled, Renderer, Surfaced, Theme};
use design::{AvatarSize, Label, PersonColour, RAIL_RADIUS, Radius, Rgb, SemanticRole, Semantics, plain_avatar};
use iced_core::{Element, Length, Widget};
use iced_widget::container;
use screens::initials;

/// Whose picture it is, which sets its shape and its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AvatarKind {
    /// Someone with no colour known, such as an app or a former member: round, in [`plain_avatar`]'s look.
    Person,
    /// In a call: on the colour of this person, who the call's tiles and the channel's rows know by it.
    Called(PersonColour),
    /// Someone offline, in the members column: their colour, faint.
    Offline(PersonColour),
    /// In the server's own colour, as picked.
    Server(Rgb),
}

/// The picture a person chose, as the host holds it ready to paint: a square, drawn round.
pub type Held = iced_widget::image::Handle;

/// The initials of `name` on a picture of `size`; the name is also what a screen reader says.
pub fn avatar<'a, Message: 'a, R: Renderer + 'a>(
    name: Label,
    kind: AvatarKind,
    size: AvatarSize,
) -> Element<'a, Message, Theme, R> {
    picture(name, kind, size, false)
}

/// [`avatar`] with the person's own `picture` where they chose one and the host holds it: drawn round at every size,
/// faint where the person is offline, the initials where there is none.
pub fn avatar_pictured<'a, Message: 'a, R: Renderer + 'a>(
    name: Label,
    kind: AvatarKind,
    size: AvatarSize,
    picture: Option<Held>,
) -> Element<'a, Message, Theme, R> {
    let Some(held) = picture.filter(|_| !matches!(kind, AvatarKind::Server(_))) else {
        return avatar(name, kind, size);
    };
    let side = Length::Fixed(size.px());
    let faint = if matches!(kind, AvatarKind::Offline(_)) { design::OFFLINE_DIM } else { 1.0 };
    let image = iced_widget::image(held)
        .content_fit(iced_core::ContentFit::Cover)
        .opacity(faint)
        .border_radius(size.px() / 2.0)
        .width(side)
        .height(side)
        ._boxed();
    Labelled::new(image, Semantics { role: SemanticRole::Image, label: name })._boxed()
}

/// A server's rail picture drawn faint: a server not signed in.
pub(crate) fn faint_server<'a, Message: 'a, R: Renderer + 'a>(
    name: Label,
    colour: Rgb,
) -> Element<'a, Message, Theme, R> {
    picture(name, AvatarKind::Server(colour), AvatarSize::Rail, true)
}

fn picture<'a, Message: 'a, R: Renderer + 'a>(
    name: Label,
    kind: AvatarKind,
    size: AvatarSize,
    dim: bool,
) -> Element<'a, Message, Theme, R> {
    let class = match kind {
        AvatarKind::Person => {
            let look = plain_avatar();
            Surfaced::Fill { fill: look.fill, text: look.text, radius: Radius::Full }
        }
        AvatarKind::Called(colour) => Surfaced::Person { colour, radius: Radius::Full, faint: false },
        AvatarKind::Offline(colour) => Surfaced::Person { colour, radius: Radius::Full, faint: true },
        AvatarKind::Server(colour) => Surfaced::Server { colour, radius: RAIL_RADIUS, dim },
    };
    let side = Length::Fixed(size.px());
    let letters = match kind {
        AvatarKind::Person | AvatarKind::Called(_) | AvatarKind::Offline(_) => initials(name.as_str()),
        AvatarKind::Server(_) => initials(name.as_str()).chars().take(1).collect(),
    };
    let circle =
        container(text(letters, size.initials()).wrapping(iced_core::text::Wrapping::None)).center(side).class(class);
    Labelled::new(circle, Semantics { role: SemanticRole::Image, label: name })._boxed()
}
