//! Message search and link previews as the canvas draws them: the server bar's search field and the layer its results
//! fill, the card under a message whose link has a preview, and the card above the composer while a draft's link is
//! fetched.

/// The bar over the server's pane and content that holds the search: as tall as a pane's header, which it stands in
/// for over the pane, in logical pixels.
pub const SERVER_BAR_HEIGHT: f32 = crate::scale::PANE_HEADER_HEIGHT;
/// The search field in that bar, centred, in logical pixels.
pub const SEARCH_FIELD_WIDTH: f32 = 380.0;
/// That field's height, in logical pixels.
pub const SEARCH_FIELD_HEIGHT: f32 = 32.0;
/// The key hint at the end of that field, in logical pixels.
pub const SEARCH_HINT_HEIGHT: f32 = 22.0;
/// How far the search layer is kept from the window's edges, in logical pixels.
pub const SEARCH_LAYER_INSET: f32 = crate::scale::Space::L.px();
/// The least room Messages keep beside Files in the search layer; narrower, Files go under them.
pub const SEARCH_MESSAGES_LEAST: f32 = 380.0;
/// The least room Files keep beside Messages in the search layer, in logical pixels.
pub const SEARCH_FILES_LEAST: f32 = 260.0;
/// The room the layer's start, empty, failed or waiting state takes, in logical pixels.
pub const SEARCH_STATE_WIDTH: f32 = 420.0;

/// A link's preview under its message, in logical pixels.
pub const PREVIEW_CARD_WIDTH: f32 = 420.0;
/// The box a preview's picture is fitted within and never enlarged beyond, width and height, in logical pixels.
pub const PREVIEW_PICTURE: (f32, f32) = (396.0, 220.0);
/// How many lines a preview's title keeps.
pub const PREVIEW_TITLE_LINES: usize = 2;
/// How many lines a preview's description keeps.
pub const PREVIEW_DESCRIPTION_LINES: usize = 3;

/// The picture on that card, square, in logical pixels.
pub const PREVIEW_COMPOSER_PICTURE: f32 = 48.0;

/// The size a picture of `width` by `height` is drawn at inside [`PREVIEW_PICTURE`]: fitted, never enlarged, the
/// sides' ratio kept. A picture without a side is nothing.
pub fn preview_picture_fit(width: u32, height: u32) -> (f32, f32) {
    if width == 0 || height == 0 {
        return (0.0, 0.0);
    }
    #[expect(clippy::cast_precision_loss, reason = "a picture's side is far under 2^24")]
    let (w, h) = (width as f32, height as f32);
    let scale = (PREVIEW_PICTURE.0 / w).min(PREVIEW_PICTURE.1 / h).min(1.0);
    ((w * scale).round(), (h * scale).round())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_fitted_within_its_box_and_never_enlarged() {
        assert_eq!(preview_picture_fit(1600, 1200), (293.0, 220.0), "tall for the box: the height governs");
        assert_eq!(preview_picture_fit(2000, 500), (396.0, 99.0), "wide for the box: the width governs");
        assert_eq!(preview_picture_fit(100, 50), (100.0, 50.0), "small stays small");
        assert_eq!(preview_picture_fit(0, 50), (0.0, 0.0));
    }
}
