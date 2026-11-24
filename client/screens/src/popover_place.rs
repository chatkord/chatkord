//! Where the popover about a screen's picture lies over a call: hung from its card, then moved the least distance that
//! keeps it whole inside the cards' area.

use design::{ABOUT_END_INSET, ABOUT_HANG, ABOUT_POPOVER_WIDTH};

/// A box in the call's coordinates, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// Its left edge.
    pub x: f32,
    /// Its top edge.
    pub y: f32,
    /// Its width.
    pub width: f32,
    /// Its height.
    pub height: f32,
}

impl Rect {
    /// The box with its corner at (`x`, `y`) and this `width` and `height`.
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }
}

/// The box of the popover about the picture on `card`, whose content is `height` tall: [`ABOUT_POPOVER_WIDTH`] wide,
/// [`ABOUT_HANG`] under the card's top with its end [`ABOUT_END_INSET`] in from the card's end, then moved the least
/// distance that keeps it inside `area`. No taller than `area`: a longer content scrolls in it.
pub fn popover_place(card: Rect, area: Rect, height: f32) -> Rect {
    let width = ABOUT_POPOVER_WIDTH.min(area.width.max(0.0));
    let height = height.min(area.height.max(0.0));
    let (x, y) = (card.x + card.width - ABOUT_END_INSET - width, card.y + ABOUT_HANG);
    Rect {
        x: x.min(area.x + area.width - width).max(area.x),
        y: y.min(area.y + area.height - height).max(area.y),
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect::new(20.0, 60.0, 1000.0, 700.0);

    #[test]
    fn it_hangs_under_the_cards_top_with_its_end_in_from_the_cards_end() {
        let card = Rect::new(100.0, 100.0, 400.0, 225.0);
        assert_eq!(popover_place(card, AREA, 150.0), Rect::new(230.0, 156.0, 260.0, 150.0));
    }

    #[test]
    fn near_the_areas_right_edge_it_moves_left_to_stay_whole() {
        let card = Rect::new(800.0, 100.0, 400.0, 225.0);
        let placed = popover_place(card, AREA, 150.0);
        assert_eq!((placed.x, placed.y), (AREA.x + AREA.width - 260.0, 156.0));
    }

    #[test]
    fn near_the_areas_foot_it_moves_up_to_stay_whole() {
        let card = Rect::new(100.0, 600.0, 400.0, 225.0);
        let placed = popover_place(card, AREA, 150.0);
        assert_eq!((placed.x, placed.y), (230.0, AREA.y + AREA.height - 150.0));
    }

    #[test]
    fn a_card_left_of_the_area_moves_it_right() {
        let card = Rect::new(-300.0, 100.0, 400.0, 225.0);
        assert_eq!(popover_place(card, AREA, 150.0).x, AREA.x);
    }

    #[test]
    fn taller_than_the_area_it_is_as_tall_as_the_area_and_at_its_top() {
        let card = Rect::new(100.0, 100.0, 400.0, 225.0);
        let placed = popover_place(card, AREA, 2_000.0);
        assert_eq!((placed.y, placed.height), (AREA.y, AREA.height));
    }

    #[test]
    fn an_area_narrower_than_the_popover_narrows_it() {
        let narrow = Rect::new(0.0, 0.0, 200.0, 400.0);
        let placed = popover_place(Rect::new(0.0, 0.0, 200.0, 100.0), narrow, 100.0);
        assert_eq!((placed.x, placed.width), (0.0, 200.0));
    }
}
