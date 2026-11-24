//! The handle the caller holds, and what hears the stream for it on PipeWire's thread: the format the compositor
//! settles on, DMA-BUF buffers asked for, and each buffer a [`Frame`] in the shared slot.

use std::num::NonZeroU32;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::time::{Duration, Instant};

use pw::{Dequeued, State, StreamRef};

use super::clock::Clock;
use super::pods::{self, ModifierChoice, Negotiated};
use super::{DmaBuf, Modifier};
use crate::{CaptureError, Frame, Layout, Native, Shared};

/// A running capture of one node. Dropping it stops the stream.
pub struct Capture {
    shared: Arc<Shared>,
    session: pw::Session,
}

impl Capture {
    /// Opens `node` on the portal's connection `remote` and starts the stream; the compositor picks one of
    /// `modifiers`, which the GPU must import.
    pub fn open(
        remote: OwnedFd,
        node: u32,
        modifiers: &[Modifier],
        max_frames_per_second: NonZeroU32,
    ) -> Result<Self, CaptureError> {
        let offer = pods::offer(modifiers, max_frames_per_second.get())?;
        let shared = Shared::new();
        let intake = Intake {
            shared: shared.clone(),
            buffers: pods::buffers()?,
            layout: None,
            streaming: false,
            clock: Clock::default(),
            held: None,
        };
        let session =
            pw::Session::open(remote, node, pw::Role::Screen, &[&offer], intake).map_err(|why| match why {
                pw::Error::Unavailable(_) => CaptureError::Unavailable,
                pw::Error::Refused(_) | pw::Error::Pod => CaptureError::Connect,
            })?;
        Ok(Self { shared, session })
    }

    /// The newest frame, waiting up to `wait` for one. `Ok(None)` is a screen with nothing new, which the compositor
    /// reports by sending nothing; an error is the end of the capture.
    pub fn next(&self, wait: Duration) -> Result<Option<Frame>, CaptureError> {
        self.shared.take(wait)
    }

    /// What the source calls itself, once the compositor has said; KWin names a window's after its app.
    pub fn source_name(&self) -> Option<String> {
        self.session.source_name()
    }

    /// Frames the compositor sent that nobody took before the next one came.
    pub fn dropped(&self) -> u64 {
        self.shared.dropped()
    }

    /// Frames the system made, those nobody took included.
    pub fn delivered(&self) -> u64 {
        self.shared.delivered()
    }

    /// The widest gap between two frames' own times since it was last asked, which starts it over.
    pub fn take_widest_gap(&self) -> Duration {
        self.shared.take_widest_gap()
    }
}

/// Hears the stream: settles the format, then turns buffers into frames.
struct Intake {
    shared: Arc<Shared>,
    buffers: [Vec<u8>; 2],
    /// Set once a format is settled. A buffer before that is not a picture.
    layout: Option<(Layout, Modifier)>,
    streaming: bool,
    clock: Clock,
    /// The buffers the stream held when last logged: the count the compositor and this capture settled on.
    held: Option<u64>,
}

impl Intake {
    /// A format arrived, the first or a window's new size: the buffers are asked for again either way. The offer is
    /// never replaced, so the compositor, which fixes the modifier as the producer, can always renegotiate.
    fn settle(&mut self, stream: &StreamRef<'_>, settled: &Negotiated) -> Result<(), CaptureError> {
        self.layout = match settled.modifier {
            ModifierChoice::Absent => return Err(CaptureError::NoDmaBuf),
            // No picture until the compositor says which modifier it made the buffers with.
            ModifierChoice::Open => None,
            ModifierChoice::Fixed(modifier) => {
                Some((Layout { pixels: settled.pixels, width: settled.width, height: settled.height }, modifier))
            }
        };
        stream.update_params(&[&self.buffers[0], &self.buffers[1]]).map_err(|_| CaptureError::Stream)
    }
}

impl pw::Handler for Intake {
    fn state_changed(&mut self, state: State) {
        match state {
            // Before it ever streamed the compositor had nothing to agree to; after, it broke.
            State::Error => self.shared.end(if self.streaming { CaptureError::Stream } else { CaptureError::NoDmaBuf }),
            State::Streaming => self.streaming = true,
            State::Unconnected if self.streaming => self.shared.end(CaptureError::Ended),
            State::Unconnected | State::Connecting | State::Paused => {}
        }
    }

    fn param_changed(&mut self, stream: &StreamRef<'_>, id: u32, param: Option<&[u8]>) {
        let Some(param) = param.filter(|_| id == pw::FORMAT) else { return };
        if let Err(why) = pods::parse(param).and_then(|settled| self.settle(stream, &settled)) {
            self.shared.end(why);
        }
    }

    fn process(&mut self, stream: &StreamRef<'_>) {
        // The compositor lends a few buffers and stops sending while they are held, so only the newest is kept; each
        // older one goes back when it drops.
        let mut newest: Option<Dequeued<'_>> = None;
        let mut skipped = 0u64;
        while let Some(next) = stream.dequeue() {
            skipped += u64::from(newest.replace(next).is_some());
        }
        let buffers = stream.buffers();
        if self.held.replace(buffers) != Some(buffers) {
            report::note!(info, "capture: buffers settled with the compositor", buffers = buffers);
        }
        let (Some(newest), Some((layout, modifier))) = (newest, self.layout) else { return };
        // A buffer the compositor marked corrupt, or without a picture, goes back unread.
        let Some(plane) = newest.plane() else { return };
        let captured = self.clock.place(newest.time(), Instant::now());
        // `pw` checks the stride is not zero; a picture that fails it is not one.
        let Some(dmabuf) = DmaBuf::new(plane.fd, plane.offset, plane.stride, modifier, newest.lend()) else { return };
        self.shared.put(Frame::new(layout, Native::DmaBuf(dmabuf), captured), skipped);
    }
}
