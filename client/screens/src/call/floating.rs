//! The floating card: a pinned share kept playing over the window while its call is out of view, dropped where it is
//! let go and held whole inside the window.

use crate::frame::Room;
use crate::{PinnedShare, PlaceId};
use design::{FLOAT_CARD, FLOAT_INSET};

/// Where the card is kept: its gaps to the right and bottom edges of the window, so a resize keeps it by
/// the corner it was nearest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatAt {
    right: f32,
    bottom: f32,
}

impl FloatAt {
    /// Where it starts, and goes back to on Reset position.
    pub const HOME: FloatAt = FloatAt { right: FLOAT_INSET, bottom: FLOAT_INSET };

    /// Kept with its top left at `to` in `room`, held whole inside it.
    pub fn dropped(to: (f32, f32), room: Room) -> FloatAt {
        FloatAt { right: room.width - to.0 - FLOAT_CARD.0, bottom: room.height - to.1 - FLOAT_CARD.1 }.held(room)
    }

    /// Its top left in `room`: held whole inside it, and at its top left in a room smaller than the card.
    pub fn place(self, room: Room) -> (f32, f32) {
        let held = self.held(room);
        ((room.width - FLOAT_CARD.0 - held.right).max(0.0), (room.height - FLOAT_CARD.1 - held.bottom).max(0.0))
    }

    fn held(self, room: Room) -> FloatAt {
        let hold = |gap: f32, most: f32| gap.min(most).max(0.0);
        FloatAt {
            right: hold(self.right, room.width - FLOAT_CARD.0),
            bottom: hold(self.bottom, room.height - FLOAT_CARD.1),
        }
    }
}

impl Default for FloatAt {
    fn default() -> FloatAt {
        FloatAt::HOME
    }
}

/// Whether the card shows while the call is out of view: Close hides it until the call's view is next shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Away {
    /// It shows.
    #[default]
    Shown,
    /// Closed since the call's view was last shown.
    Closed,
}

/// What a person did with the floating card.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FloatMessage {
    /// Dragged: its top left to this point in the window.
    To(f32, f32),
    /// Reset position.
    Reset,
    /// Close: hidden until the call's view is next left.
    Close,
    /// Go to the call: the call's server opens, and any page open over the call closes.
    GoToCall,
}

/// The floating card as it shows now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Floating<'a> {
    /// The place whose call it is in.
    pub place: PlaceId,
    /// The share it plays: someone's screen, or one's own.
    pub screen: PinnedShare<'a>,
    /// Its top left in the window.
    pub at: (f32, f32),
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDE: Room = Room { width: 1000.0, height: 700.0 };

    #[test]
    fn it_starts_and_resets_by_the_bottom_right_corner() {
        assert_eq!(FloatAt::default(), FloatAt::HOME);
        assert_eq!(FloatAt::HOME.place(WIDE), (1000.0 - 320.0 - 20.0, 700.0 - 180.0 - 20.0));
    }

    #[test]
    fn it_lands_where_it_is_dropped_and_keeps_its_gaps_to_the_bottom_right() {
        let at = FloatAt::dropped((100.0, 50.0), WIDE);
        assert_eq!(at.place(WIDE), (100.0, 50.0));
        let wider = Room { width: 1200.0, height: 800.0 };
        assert_eq!(at.place(wider), (300.0, 150.0), "the same gaps to the right and bottom edges");
    }

    #[test]
    fn a_drop_past_an_edge_is_held_whole_inside() {
        for (to, held) in [
            ((-50.0, -50.0), (0.0, 0.0)),
            ((2000.0, 2000.0), (680.0, 520.0)),
            ((-50.0, 2000.0), (0.0, 520.0)),
            ((f32::NAN, f32::NAN), (0.0, 0.0)),
        ] {
            assert_eq!(FloatAt::dropped(to, WIDE).place(WIDE), held, "{to:?}");
        }
    }

    #[test]
    fn a_smaller_room_moves_it_in_and_a_room_smaller_than_it_puts_it_at_the_top_left() {
        let at = FloatAt::dropped((0.0, 0.0), WIDE);
        let small = Room { width: 400.0, height: 300.0 };
        assert_eq!(at.place(small), (0.0, 0.0));
        assert_eq!(FloatAt::HOME.place(small), (60.0, 100.0));
        let tiny = Room { width: 200.0, height: 100.0 };
        for at in [at, FloatAt::HOME] {
            assert_eq!(at.place(tiny), (0.0, 0.0));
        }
        assert_eq!(at.place(WIDE), (0.0, 0.0), "the gaps it was dropped with come back with the room");
    }
}
