//! What only the host can do with what an errand brings back: put a picture where its toolkit paints it, draw the
//! application's icon, and close, hide, show or bring forward its window.

use proto::files::FileId;
use proto::video::Video;
use screens::{Painting, PlaceId};
use terms::MemberId;

/// A still of a source our own picker lists, as RGBA to make a picture of once.
#[derive(Clone, Debug)]
pub struct Still {
    /// The id the picker's model paints it by.
    pub painting: Painting,
    /// Its width in pixels.
    pub width: u32,
    /// Its height in pixels.
    pub height: u32,
    /// Its pixels, four bytes each.
    pub rgba: Vec<u8>,
}

/// The host's side of the errands: what its toolkit owns. Called from the runner's threads, so it must not block.
pub trait Host: Send + Sync + 'static {
    /// The slot `user_id`'s picture of `video` arrives in, made the first time a call puts one there.
    fn slot(&self, user_id: &MemberId, video: Video) -> call::Slot;
    /// The call is over: every slot [`Host::slot`] made goes with it.
    fn clear_slots(&self);
    /// The slot this account's own screen is previewed in.
    fn own_screen(&self) -> call::Slot;
    /// The slot this account's own camera is previewed in, in a call or the settings' test.
    fn own_camera(&self) -> call::Slot;
    /// The GPU the window paints with, which a decoder decodes onto.
    fn painting(&self) -> picture::Painting;
    /// Holds `square` to paint wherever `picture` is shown, for `place`; answers each picture let go to make room, by
    /// the place it was held for.
    fn hold_picture(&self, place: PlaceId, picture: FileId, square: portrait::Square) -> Vec<(PlaceId, FileId)>;
    /// Holds a fetched file's picture (an emoji, a thumbnail, a picture sent), decoded within its kind's bounds, to
    /// paint wherever `file` is shown, for `place`; answers each picture let go to make room, by the place it was held
    /// for.
    fn hold_file(&self, place: PlaceId, file: FileId, picture: portrait::Decoded) -> Vec<(PlaceId, FileId)>;
    /// `place` was taken off the device or signed out: every picture held for it alone goes.
    fn clear_pictures(&self, place: PlaceId);
    /// Holds our own picker's stills in place of every one held before.
    fn hold_stills(&self, stills: Vec<Still>);
    /// The application's icon `side` pixels square, as RGBA; none when it cannot be drawn.
    fn icon(&self, side: u32) -> Option<Vec<u8>>;
    /// The window is to close, and the run to end.
    fn quit(&self);
    /// The window is to go, the scene running on in the tray.
    fn hide(&self);
    /// The window is to show.
    fn show(&self);
    /// The window is to come forward, with the desktop's token to take the focus where it gave one.
    fn come_forward(&self, activation: Option<platform::activation::Activation>);
}
