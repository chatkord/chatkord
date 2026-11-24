//! The dot that says a person's status, each by its shape as well as its colour: on their picture in the members
//! column, a conversation's row and the account bar, ringed in the ground it stands on; plain beside a word in their
//! popover and the account's status menu. One drawing, one size and one place on one size of picture for all of them.

use crate::{AvatarKind, Held, Renderer, Theme, avatar_pictured};
use design::{
    AWAY_NOTCH_AT_SHARE, AWAY_NOTCH_SHARE, DO_NOT_DISTURB_BAR_SHARE, Label, PRESENCE_DOT, PRESENCE_PICTURE, RING_WIDTH,
    Role, StatusCut, StatusMark, Surface, status_mark,
};
use iced_core::{Element, Length, Point, Rectangle, Size, Widget, mouse};
use iced_widget::canvas::{Canvas, Geometry, Path, Program, Stroke};
use iced_widget::stack;
use screens::StatusDot;
use std::f32::consts::{PI, TAU};

/// How many straight steps each arc of the away crescent is drawn in: at a dot's size, more show no difference.
const CRESCENT_STEPS: u16 = 16;

/// The mark the dot of `dot` is drawn as.
const fn mark(dot: StatusDot) -> StatusMark {
    match dot {
        StatusDot::Online => StatusMark::Online,
        StatusDot::Away => StatusMark::Away,
        StatusDot::DoNotDisturb => StatusMark::DoNotDisturb,
        StatusDot::Offline => StatusMark::Offline,
        StatusDot::Hollow => StatusMark::Hollow,
    }
}

/// Where a dot stands in its canvas.
#[derive(Clone, Copy)]
enum Placed {
    /// Beside a word: the canvas is the dot, plain.
    Alone,
    /// On a person's picture: the canvas is the picture, the dot flush with its right and bottom, ringed in the ground.
    Corner,
}

/// The dot, cut in the colour of the ground `on` it stands, [`PRESENCE_DOT`] wide.
struct Dot {
    mark: StatusMark,
    on: Surface,
    placed: Placed,
}

/// The dot of `dot` beside a word on the ground `on`, plain.
pub fn status_mark_dot<'a, Message: 'a, R: Renderer + 'a>(
    dot: StatusDot,
    on: Surface,
) -> Element<'a, Message, Theme, R> {
    Canvas::<Dot, Message, Theme, R>::new(Dot { mark: mark(dot), on, placed: Placed::Alone })
        .width(Length::Fixed(PRESENCE_DOT))
        .height(Length::Fixed(PRESENCE_DOT))
        ._boxed()
}

/// The picture of `name` as `kind`, [`PRESENCE_PICTURE`] big, with the dot of `dot` flush in its bottom right corner,
/// ringed in the ground `on` it stands; the picture alone where there is none. The picture is the person's own where
/// they chose one (`held`), else their initials.
pub fn person_with_status<'a, Message: 'a, R: Renderer + 'a>(
    name: Label,
    kind: AvatarKind,
    held: Option<Held>,
    dot: Option<StatusDot>,
    on: Surface,
) -> Element<'a, Message, Theme, R> {
    let picture = avatar_pictured(name, kind, PRESENCE_PICTURE, held);
    let Some(dot) = dot else { return picture };
    let side = PRESENCE_PICTURE.px();
    let corner = Canvas::<Dot, Message, Theme, R>::new(Dot { mark: mark(dot), on, placed: Placed::Corner })
        .width(Length::Fixed(side))
        .height(Length::Fixed(side));
    stack([picture, corner._boxed()])._boxed()
}

impl<Message, R: Renderer> Program<Message, Theme, R> for Dot {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &R,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry<R>> {
        let mut frame = iced_widget::canvas::Frame::new(renderer, bounds.size());
        let ground = theme.color(Role::Surface(self.on));
        let look = status_mark(self.mark);
        let fill = theme.color(look.fill);
        let radius = PRESENCE_DOT / 2.0;
        let centre = match self.placed {
            Placed::Alone => Point::new(radius, radius),
            Placed::Corner => {
                let side = PRESENCE_PICTURE.px();
                let centre = Point::new(side - radius, side - radius);
                // The ring's outer half lies past the picture, on the same ground, where the canvas clips it.
                frame.fill(&Path::circle(centre, radius + RING_WIDTH), ground);
                centre
            }
        };
        match look.cut {
            StatusCut::Whole => frame.fill(&Path::circle(centre, radius), fill),
            StatusCut::Bar => {
                frame.fill(&Path::circle(centre, radius), fill);
                let (wide, high) =
                    (PRESENCE_DOT * DO_NOT_DISTURB_BAR_SHARE.0, PRESENCE_DOT * DO_NOT_DISTURB_BAR_SHARE.1);
                let corner = Point::new(centre.x - wide / 2.0, centre.y - high / 2.0);
                frame.fill(&Path::rounded_rectangle(corner, Size::new(wide, high), (high / 2.0).into()), ground);
            }
            StatusCut::Hollow => {
                frame.fill(&Path::circle(centre, radius), ground);
                let edge = RING_WIDTH.min(PRESENCE_DOT / 5.0);
                frame.stroke(
                    &Path::circle(centre, radius - edge / 2.0),
                    Stroke::default().with_color(fill).with_width(edge),
                );
            }
            StatusCut::Notch => {
                let cut = PRESENCE_DOT * AWAY_NOTCH_SHARE / 2.0;
                let back = PRESENCE_DOT * AWAY_NOTCH_AT_SHARE;
                frame.fill(&crescent(centre, radius, Point::new(centre.x - back, centre.y - back), cut), fill);
            }
        }
        vec![frame.into_geometry()]
    }
}

/// The disc at `centre` of `radius` with the disc at `notch` of `cut` taken from it, its outline in straight steps:
/// the notch is not painted over, so nothing spills past the dot onto the picture under it.
fn crescent(centre: Point, radius: f32, notch: Point, cut: f32) -> Path {
    let on = |at: Point, round: f32, angle: f32| Point::new(at.x + round * angle.cos(), at.y + round * angle.sin());
    let (across, down) = (notch.x - centre.x, notch.y - centre.y);
    let apart = across.hypot(down);
    let toward = down.atan2(across);
    // Where the two circles cross, as an angle either side of the notch's direction from the dot's centre.
    let half = ((radius * radius + apart * apart - cut * cut) / (2.0 * radius * apart)).clamp(-1.0, 1.0).acos();
    let (start, end) = (toward + half, toward + TAU - half);
    let (from, to) = (on(centre, radius, end), on(centre, radius, start));
    let first = (from.y - notch.y).atan2(from.x - notch.x);
    let last = (to.y - notch.y).atan2(to.x - notch.x);
    // Of the notch's two arcs between the crossings, the one whose middle is inside the dot: the short one while the
    // notch's centre is outside the dot, the long one once it is inside.
    let short = (last - first + PI).rem_euclid(TAU) - PI;
    let middle = on(notch, cut, first + short / 2.0);
    let sweep =
        if (middle.x - centre.x).hypot(middle.y - centre.y) <= radius { short } else { short - TAU.copysign(short) };
    let steps = f32::from(CRESCENT_STEPS);
    Path::new(|path| {
        path.move_to(to);
        for step in 1..=CRESCENT_STEPS {
            path.line_to(on(centre, radius, start + (end - start) * f32::from(step) / steps));
        }
        for step in 1..=CRESCENT_STEPS {
            path.line_to(on(notch, cut, first + sweep * f32::from(step) / steps));
        }
        path.close();
    })
}
