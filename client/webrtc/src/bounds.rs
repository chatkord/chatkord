//! The rates a send connection runs between. Each picture sent states its own, and the connection's are their sum,
//! so a camera started beside a share never takes the share's bounds for its own.

use libwebrtc::peer_connection::BitrateSettings;
use proto::chatkord_v1::TrackKind;
use std::collections::BTreeMap;

/// The rates one picture may run between, in bits per second.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rates {
    /// The least the estimate falls to for it.
    pub(crate) min: u64,
    /// The most it is ever sent at.
    pub(crate) max: u64,
}

/// Every picture's rates, by the track that sends it.
#[derive(Debug, Default)]
pub(crate) struct Bounds(BTreeMap<TrackKind, Rates>);

impl Bounds {
    /// States `kind`'s rates; the connection's settings now. The estimate starts at `start` only while `kind` is the
    /// one picture, since a start stated again would begin the estimate of the pictures already sent anew.
    pub(crate) fn state(&mut self, kind: TrackKind, rates: Rates, start: Option<u64>) -> BitrateSettings {
        self.0.insert(kind, rates);
        self.settings(start.filter(|_| self.0.len() == 1))
    }

    /// Forgets `kind`'s rates; the settings for the pictures left, none when `kind` stated none or none is left.
    pub(crate) fn forget(&mut self, kind: TrackKind) -> Option<BitrateSettings> {
        self.0.remove(&kind)?;
        (!self.0.is_empty()).then(|| self.settings(None))
    }

    fn settings(&self, start: Option<u64>) -> BitrateSettings {
        BitrateSettings {
            min_bitrate_bps: Some(self.0.values().map(|rates| rates.min).sum()),
            start_bitrate_bps: start,
            max_bitrate_bps: Some(self.0.values().map(|rates| rates.max).sum()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHARE: Rates = Rates { min: 1_000_000, max: 40_000_000 };
    const CAMERA: Rates = Rates { min: 150_000, max: 5_000_000 };

    fn read(settings: &BitrateSettings) -> (Option<u64>, Option<u64>, Option<u64>) {
        (settings.min_bitrate_bps, settings.start_bitrate_bps, settings.max_bitrate_bps)
    }

    #[test]
    fn two_pictures_sum_their_bounds_and_the_second_never_restarts_the_estimate() {
        let mut bounds = Bounds::default();
        let alone = bounds.state(TrackKind::ScreenVideo, SHARE, Some(8_000_000));
        assert_eq!(read(&alone), (Some(1_000_000), Some(8_000_000), Some(40_000_000)));
        let both = bounds.state(TrackKind::Camera, CAMERA, Some(1_400_000));
        assert_eq!(read(&both), (Some(1_150_000), None, Some(45_000_000)), "the share's bounds stay");
        let moved = bounds.state(TrackKind::ScreenVideo, Rates { min: 500_000, ..SHARE }, None);
        assert_eq!(read(&moved), (Some(650_000), None, Some(45_000_000)));
    }

    #[test]
    fn a_picture_that_ends_leaves_the_others_bounds() {
        let mut bounds = Bounds::default();
        bounds.state(TrackKind::ScreenVideo, SHARE, None);
        bounds.state(TrackKind::Camera, CAMERA, None);
        let left = bounds.forget(TrackKind::ScreenVideo).expect("the camera's");
        assert_eq!(read(&left), (Some(150_000), None, Some(5_000_000)));
        assert!(bounds.forget(TrackKind::Microphone).is_none(), "a track that stated none changes nothing");
        assert!(bounds.forget(TrackKind::Camera).is_none(), "no picture left");
    }
}
