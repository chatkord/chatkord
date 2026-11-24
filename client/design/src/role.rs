//! Every colour a screen may paint, named by what it is for. A view asks for a role, never for a
//! colour, so a scheme or an accent changes every screen at once and the contract can be checked
//! over all of them.

use crate::listed;

listed! {
    /// What things sit on, from the window's own background up to a menu over everything.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum Surface {
        /// The window.
        Base,
        /// Panels and cards on the window: the side pane with the channels, a voice tile.
        Raised,
        /// The frame round the panes: the server rail and the account strip.
        Chrome,
        /// What floats over the rest: menus, dialogs, tooltips, banners.
        Overlay,
        /// What is set into a surface: a text field.
        Inset,
        /// What a control or a row is filled with under the pointer. Visibly different from every other surface,
        /// so the answer to the pointer shows whichever surface the control sits on.
        Hover,
        /// The same while it is pressed, and visibly different again.
        Pressed,
    }
}

impl Surface {
    /// The surfaces a control sits on: what the pointer's fills must be told apart from.
    /// Not the chrome: nothing on it answers the pointer with a fill (a rail item answers with its own picture), and
    /// in the light scheme the hover fill would not show on it.
    pub const GROUNDS: [Surface; 4] = [Surface::Base, Surface::Raised, Surface::Overlay, Surface::Inset];
}

listed! {
    /// Text and icons on a surface.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum Content {
        /// Most text and icons.
        Primary,
        /// Text and icons that matter less: a hint, a count, a note.
        Secondary,
        /// What cannot be used now.
        Disabled,
    }
}

listed! {
    /// The edges a screen draws.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum Border {
        /// A divider: decoration, so it carries no contrast of its own.
        Subtle,
        /// The edge of a control a person must find, such as a text field's.
        Strong,
    }
}

listed! {
    /// The colours derived from a server's accent.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum AccentRole {
        /// A primary button, a selected item.
        Fill,
        /// The fill under the pointer.
        FillHover,
        /// The fill while it is pressed.
        FillPressed,
        /// Text and icons on any of the three fills.
        OnFill,
        /// A link, accented text on a surface.
        Text,
        /// A ring in the accent: a server connecting, round its picture in the rail. Keyboard focus draws none.
        Ring,
        /// A quiet ground for a note in the accent: an owner invite, a welcome.
        Tint,
    }
}

listed! {
    /// The kinds of news a screen gives.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum Status {
        /// Something went well, or is as it should be.
        Success,
        /// Something needs a look, but still works.
        Warning,
        /// Something failed, or is about to be lost.
        Danger,
        /// Something worth knowing, neither good nor bad.
        Info,
    }
}

listed! {
    /// The colours each status gives.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum StatusRole {
        /// A banner's or a badge's background.
        Fill,
        /// Text and icons on that fill.
        OnFill,
        /// The status said in text on a surface, such as a field's error.
        Text,
        /// A quiet ground for a box in that status: an invite that stopped working, a server that did not answer.
        Tint,
    }
}

listed! {
    /// A destructive action's button (remove, leave, stop): its fill, under the pointer and pressed, and its text.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum DestructiveRole {
        /// The button's fill.
        Fill,
        /// Its fill under the pointer.
        FillHover,
        /// Its fill while it is pressed.
        FillPressed,
        /// Its text and icon on any of the three.
        OnFill,
    }
}

/// One opaque colour a screen may paint. The translucent scrim laid over a picture is not a role
/// but [`crate::Palette::scrim`]; what is written on it is [`Role::OnScrim`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// What things sit on.
    Surface(Surface),
    /// Text and icons on a surface.
    Content(Content),
    /// An edge.
    Border(Border),
    /// A colour derived from the accent.
    Accent(AccentRole),
    /// A colour of a status.
    Status(Status, StatusRole),
    /// A destructive action's button.
    Destructive(DestructiveRole),
    /// Text and icons on the scrim over a picture, whatever the picture is.
    OnScrim,
    /// A muted or deafened mark on the scrim: a light red, so it reads over any picture.
    OnScrimDanger,
    /// A control on the scrim that cannot be used.
    OnScrimDisabled,
    /// Under a control on the scrim while the pointer is on it, and while it is pressed: the same in both schemes.
    ScrimHover,
    /// The same while it is pressed.
    ScrimPressed,
    /// What a button that cannot be used is filled with: a step above the panel, its label [`Content::Disabled`].
    DisabledFill,
    /// The ground a shared screen or a camera is drawn on: black in both schemes, so what is drawn on it
    /// ([`Role::OnScrim`] and its kin) reads the same in each.
    PictureGround,
    /// The edge of a control on [`Role::PictureGround`].
    PictureEdge,
    /// The ring around a person who is speaking.
    Speaking,
}

impl Role {
    /// Every role, once.
    pub fn all() -> impl Iterator<Item = Role> {
        let surfaces = Surface::ALL.into_iter().map(Role::Surface);
        let content = Content::ALL.into_iter().map(Role::Content);
        let borders = Border::ALL.into_iter().map(Role::Border);
        let accent = AccentRole::ALL.into_iter().map(Role::Accent);
        let status = Status::ALL
            .into_iter()
            .flat_map(|status| StatusRole::ALL.into_iter().map(move |part| Role::Status(status, part)));
        let destructive = DestructiveRole::ALL.into_iter().map(Role::Destructive);
        surfaces.chain(content).chain(borders).chain(accent).chain(status).chain(destructive).chain([
            Role::OnScrim,
            Role::OnScrimDanger,
            Role::OnScrimDisabled,
            Role::ScrimHover,
            Role::ScrimPressed,
            Role::DisabledFill,
            Role::PictureGround,
            Role::PictureEdge,
            Role::Speaking,
        ])
    }
}
