//! The pictures the host paints into a screen.

use crate::Theme;
use iced_core::Element;
use screens::WindowMessage as Message;
use terms::MemberId;

/// Which picture the host paints for a call.
#[derive(Clone, Debug, PartialEq)]
pub enum Painted {
    /// The screen shared by the person with this id, through its zoom.
    Screen {
        /// Its sharer's id.
        user_id: MemberId,
        /// How much of it shows, and which part.
        lens: screens::Lens,
        /// The corners of what it is shown in: its card's, or none over the whole display.
        corners: design::Radius,
    },
    /// This account's own screen, as it is sent, in a card with these corners.
    OwnScreen(design::Radius),
    /// The camera of the person with this id, filling their card.
    Camera {
        /// Its sender's id.
        user_id: MemberId,
    },
    /// This account's own camera, filling its card or the preview, mirrored as chosen (others never see it so), with
    /// the corners of what it is shown in.
    OwnCamera(choices::Mirror, design::Radius),
    /// A still or an icon in our own picker, which the host made, with the corners it is drawn with.
    Still(screens::Painting, design::Radius),
    /// A picture the host holds by its file and has ready: a server's or a person's.
    Picture {
        /// The file the picture is.
        picture: proto::files::FileId,
        /// Its corners: a server's rail shape, a person's round.
        radius: design::Radius,
        /// Faint while it is not the owner's yet, or the server is not signed in.
        faint: bool,
    },
}

/// A picture the host paints: made when a screen shows it.
pub type Picture<'a, R> = dyn Fn(Painted) -> Element<'a, Message, Theme, R> + 'a;
