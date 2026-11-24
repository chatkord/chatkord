//! What a share and a camera have in common as one's own picture sent: a source of pictures and how they reach the
//! encoder. One loop (`screen::Started::run`) encodes whichever it is given, so the rate control, the steps down, the
//! fallback to another encoder and the rest at a rate the person never sees are one thing, decided once.

use crate::screen::{Fail, Ladder, Move, Preview, Stages, first_open};
use capture::Layout;
use media::{Changes, EncodeError, Encoded, Reopening, Session};
use net::Envelope;
use std::time::{Duration, Instant};

/// The size a source's pictures are, and what else makes them one kind: a screen's channel order, a camera's nothing.
/// A picture of another shape is a new size, taken once it has held.
pub trait Shape: Copy + PartialEq + Send {
    /// Pixels across and down.
    fn size(self) -> (u32, u32);
    /// The same kind of picture at `size`.
    fn sized(self, size: (u32, u32)) -> Self;
}

impl Shape for Layout {
    fn size(self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn sized(self, (width, height): (u32, u32)) -> Self {
        Self { width, height, ..self }
    }
}

impl Shape for (u32, u32) {
    fn size(self) -> (u32, u32) {
        self
    }

    fn sized(self, size: (u32, u32)) -> Self {
        size
    }
}

/// Where one's own pictures come from and how each is made ready for an encoder: the screen's capture, or the
/// camera's reader. It owns what the pictures need beside the encoder's [`Session`], which the loop owns and lends it.
pub trait Source: Send {
    /// One picture, as its source hands it over.
    type Picture: Send;
    /// What its pictures' size and kind are.
    type Shape: Shape;
    /// What the loop keeps for lending the source's own card its pictures, which lives on the loop's thread alone.
    type Lending: Default;

    /// The newest picture, waiting up to `wait` for one; none is a source with nothing new. A source that is asked for
    /// pictures nobody takes (the camera, hidden and unwatched) makes none, and says none.
    fn next(&mut self, wait: Duration) -> Result<Option<Self::Picture>, Fail>;

    /// What `picture` is.
    fn shape(picture: &Self::Picture) -> Self::Shape;

    /// When the system made `picture`.
    fn captured(picture: &Self::Picture) -> Instant;

    /// Tells `stages` what the source counted, and the widest gap between its pictures since it last did.
    fn count_into(&self, stages: &Stages);

    /// What the source calls itself, where it says.
    fn name(&self) -> Option<String> {
        None
    }

    /// The rates a stream of `size` at up to `frames_per_second` runs between, under the person's `ceiling`.
    fn envelope(size: (u32, u32), frames_per_second: u32, ceiling: Option<u64>) -> Envelope;

    /// Gives one's own card `picture`, which nothing encodes: `preview` where the host lent one, else the source's own.
    /// `rebuilds` counts how often the encoder's device was made again.
    fn lend(
        &mut self,
        lending: &mut Self::Lending,
        preview: Option<&Preview<'_>>,
        picture: &Self::Picture,
        session: &Session,
        rebuilds: u32,
    );

    /// Makes `picture` ready for `session`, gives one's own card what it would be given, and encodes it; whatever the
    /// encoder has finished.
    fn encode(
        &mut self,
        lending: &mut Self::Lending,
        session: &mut Session,
        preview: Option<&Preview<'_>>,
        picture: Self::Picture,
        rebuilds: u32,
    ) -> Result<Vec<Encoded>, Fail>;

    /// The pictures are of `shape` from now, sent as a stream of `stream`: `session` is asked for it, made by the
    /// next encode, or replaced where the source's own device must make the new size.
    fn follow(&mut self, session: &mut Session, shape: Self::Shape, stream: (u32, u32)) -> Result<(), EncodeError>;

    /// The encoder refused the stream `follow` asked for and the old one goes on, of `stream`: a source that makes its
    /// pictures that size makes them so again.
    fn withdrawn(&mut self, _: &mut Session, _: (u32, u32)) -> Result<(), EncodeError> {
        Ok(())
    }

    /// The encoder's device was made again at `session`: a source that draws into it opens anew on the new one, and
    /// says so.
    fn rebuilt(&mut self, _: &Session) -> Result<bool, Fail> {
        Ok(false)
    }

    /// Makes what was asked of `session` (a size, a rate, another encoder), going back first to the hardware encoder
    /// the stream fell from as `ladder` says; what the old stream still held and what changed.
    fn reopen(&mut self, session: &mut Session, ladder: &mut Ladder) -> Result<Reopening, EncodeError> {
        ladder.reopen(session)
    }

    /// Hands the stream on from the encoder that stopped as `failed`: a lost device is rebuilt once, then the next of
    /// `ladder` that opens takes over, on a key frame. What the new stream changed, and whether the device was
    /// rebuilt; the last refusal where none opens.
    fn hand_over(
        &mut self,
        session: &mut Session,
        ladder: &mut Ladder,
        failed: EncodeError,
    ) -> Result<(Changes, bool), EncodeError> {
        hand_over(session, ladder, failed)
    }
}

/// The hand-over of an encoder that works on its own device through `session`: the device is rebuilt once when it was
/// lost, else the next encoder of `ladder` that opens takes over, and a hardware one given up is gone back to at the
/// next reopen. Windows' capture draws into the encoder's device: every candidate there is on the one D3D11 device,
/// which a reopen on the same device keeps.
pub fn hand_over(
    session: &mut Session,
    ladder: &mut Ladder,
    failed: EncodeError,
) -> Result<(Changes, bool), EncodeError> {
    let running = session.running().2.clone();
    let mut refused = failed;
    let rebuild = ladder.failed(&running, failed) == Move::Rebuild;
    if rebuild {
        session.rebuild_device();
        match session.reopen() {
            Ok(made) => return Ok((made.changes, rebuild)),
            Err(why) => refused = why,
        }
    }
    let (made, _) = first_open(&mut ladder.later, refused, |encoder| {
        session.set_encoder(encoder)?;
        session.reopen()
    })?;
    Ok((made.changes, rebuild))
}
