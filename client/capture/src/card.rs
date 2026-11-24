//! The size one's own screen is drawn at for its card: fitted to the card, its texture rounded up so a card resized by a
//! few pixels draws into the texture it has.

/// The step a card's texture grows by, so a card resized by a few pixels is drawn into the texture it has.
const STEP: u32 = 64;

/// A picture of one's own screen for a card: the texture it is drawn into and the picture's own size inside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card {
    /// The texture: the card's size rounded up to `STEP`, never larger than the screen rounded so.
    pub texture: (u32, u32),
    /// The screen fitted inside the card, keeping its shape and never larger than it was.
    pub picture: (u32, u32),
}

/// One's own screen of `source` drawn for a card of `within` pixels; none for an empty screen or card.
pub fn card(source: (u32, u32), within: (u32, u32)) -> Option<Card> {
    let step = |side: u32| side.div_ceil(STEP) * STEP;
    let rounded = (step(within.0), step(within.1));
    let picture = fit(source, rounded)?;
    Some(Card { texture: (rounded.0.min(step(source.0)), rounded.1.min(step(source.1))), picture })
}

/// The largest even size inside the second with the shape of the first, never larger than it; none for an empty one.
pub(crate) fn fit((width, height): (u32, u32), (most_width, most_height): (u32, u32)) -> Option<(u32, u32)> {
    if width == 0 || height == 0 || most_width < 2 || most_height < 2 {
        return None;
    }
    let scale = (f64::from(most_width) / f64::from(width)).min(f64::from(most_height) / f64::from(height)).min(1.0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the scale is in 0..=1, so a side is never above the u32 it came from"
    )]
    let side = |value: u32| ((f64::from(value) * scale) as u32 & !1).max(2);
    Some((side(width), side(height)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_still_keeps_its_shape_inside_the_card_and_is_never_larger_than_the_picture() {
        assert_eq!(fit((3840, 2160), (320, 180)), Some((320, 180)));
        assert_eq!(fit((1080, 1920), (320, 180)), Some((100, 180)));
        assert_eq!(fit((200, 100), (320, 180)), Some((200, 100)), "a small window is not blown up");
        assert_eq!(fit((0, 100), (320, 180)), None);
    }

    #[test]
    fn a_card_s_texture_grows_by_steps_and_holds_the_screen_fitted() {
        let drawn = card((3840, 2160), (300, 170)).unwrap();
        assert_eq!(drawn.texture, (320, 192));
        assert_eq!(drawn.picture, (320, 180));
        assert_eq!(card((3840, 2160), (310, 175)).unwrap().texture, drawn.texture, "a few pixels keep the texture");
        assert_eq!(card((200, 100), (640, 360)).unwrap(), Card { texture: (256, 128), picture: (200, 100) });
        assert_eq!(card((0, 100), (320, 180)), None);
    }
}
