//! The ScreenCast portal, as far as picking goes.

use crate::pick::{Cursor, Kind, Refused, Source, Want};
use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType, Stream};
use ashpd::desktop::{PersistMode, Session};
use ashpd::enumflags2::BitFlags;
use ashpd::{Error, desktop::ResponseError};
use std::os::fd::OwnedFd;

impl Source {
    fn from_portal(stream: &Stream) -> Self {
        let kind = stream.source_type().map(|kind| match kind {
            SourceType::Monitor => Kind::Monitor,
            SourceType::Window => Kind::Window,
            SourceType::Virtual => Kind::Virtual,
        });
        let size =
            stream.size().and_then(|(width, height)| Some((u32::try_from(width).ok()?, u32::try_from(height).ok()?)));
        Self { node_id: stream.pipe_wire_node_id(), kind, size }
    }
}

impl Refused {
    fn of(error: &Error) -> Self {
        match error {
            Error::Response(ResponseError::Cancelled) => Self::Cancelled,
            Error::PortalNotFound(_) => Self::Unavailable,
            _ => Self::Failed,
        }
    }
}

/// A pick that is still live: the portal keeps the share's permission until it is closed.
pub struct Selection {
    /// What was picked.
    pub source: Source,
    /// A connection to the PipeWire daemon that is allowed to open `source`.
    pub remote: OwnedFd,
    session: Session<Screencast>,
}

impl Selection {
    /// Ends the share's permission with the portal.
    pub async fn close(self) {
        let _ = self.session.close().await;
    }
}

/// Asks the person to pick, in the desktop's own picker.
pub async fn choose(want: Want, cursor: Cursor) -> Result<Selection, Refused> {
    ask(want, cursor).await.map_err(|error| Refused::of(&error))
}

async fn ask(want: Want, cursor: Cursor) -> Result<Selection, Error> {
    let portal = Screencast::new().await?;
    let session = portal.create_session(Default::default()).await?;
    // Every step after the session exists runs in this block, so every way out of it closes the session;
    // the portal's own session has no `Drop` that does.
    let opened = async {
        let sources: BitFlags<SourceType> = match want {
            Want::Monitor => SourceType::Monitor.into(),
            Want::Window => SourceType::Window.into(),
            Want::Either => SourceType::Monitor | SourceType::Window,
        };
        let cursor = match cursor {
            Cursor::Hidden => CursorMode::Hidden,
            Cursor::Embedded => CursorMode::Embedded,
        };
        portal
            .select_sources(
                &session,
                SelectSourcesOptions::default()
                    .set_sources(sources)
                    .set_cursor_mode(cursor)
                    .set_multiple(false)
                    .set_persist_mode(PersistMode::DoNot),
            )
            .await?;
        let started = portal.start(&session, None, Default::default()).await?.response()?;
        let Some(stream) = started.streams().first() else {
            return Err(Error::NoResponse);
        };
        let source = Source::from_portal(stream);
        let remote = portal.open_pipe_wire_remote(&session, Default::default()).await?;
        Ok((source, remote))
    }
    .await;
    match opened {
        Ok((source, remote)) => Ok(Selection { source, remote, session }),
        Err(error) => {
            let _ = session.close().await;
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ashpd::desktop::screencast::StreamBuilder;

    #[test]
    fn a_stream_the_portal_describes_becomes_a_source() {
        let stream = StreamBuilder::new(42).size((1920, 1080)).source_type(SourceType::Monitor).build();
        assert_eq!(
            Source::from_portal(&stream),
            Source { node_id: 42, kind: Some(Kind::Monitor), size: Some((1920, 1080)) }
        );
        let stream = StreamBuilder::new(7).source_type(SourceType::Window).build();
        assert_eq!(Source::from_portal(&stream), Source { node_id: 7, kind: Some(Kind::Window), size: None });
    }

    #[test]
    fn a_size_that_is_not_positive_is_not_claimed() {
        let stream = StreamBuilder::new(1).size((-1, 1080)).build();
        assert_eq!(Source::from_portal(&stream).size, None);
        assert_eq!(Source::from_portal(&stream).kind, None);
    }

    #[test]
    fn a_closed_picker_and_a_missing_portal_are_told_apart_from_a_failure() {
        assert_eq!(Refused::of(&Error::Response(ResponseError::Cancelled)), Refused::Cancelled);
        assert_eq!(Refused::of(&Error::Response(ResponseError::Other)), Refused::Failed);
        assert_eq!(Refused::of(&Error::NoResponse), Refused::Failed);
    }
}
