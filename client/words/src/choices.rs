//! What each value a person chose for a share or the camera is called in the window: one method per kind on [`Words`],
//! so a row lists them as the language says them.

use crate::words::{Phrase, Words};
use choices::{CameraRate, CameraResolution, Ceiling, FrameRate, Resolution};

impl Words {
    /// A camera's picture size: "720p".
    pub fn camera_resolution(&self, resolution: CameraResolution) -> String {
        self.p(resolution.size().1)
    }

    /// A camera's rate: "30 fps".
    pub fn camera_rate(&self, rate: CameraRate) -> String {
        self.fps(rate.per_second())
    }

    /// A share's size: "Source", "1080p".
    pub fn resolution(&self, resolution: Resolution) -> String {
        resolution.short_side().map_or_else(|| self.phrase(Phrase::Source).into_owned(), |side| self.p(side))
    }

    /// A share's rate: "30 fps".
    pub fn frame_rate(&self, rate: FrameRate) -> String {
        self.fps(rate.per_second())
    }

    /// A share's ceiling, its row's heading carrying the unit: "4", "Automatic", "Custom".
    pub fn ceiling(&self, ceiling: Ceiling) -> String {
        match ceiling {
            Ceiling::Automatic => self.phrase(Phrase::Automatic).into_owned(),
            Ceiling::Custom(_) => self.phrase(Phrase::Custom).into_owned(),
            Ceiling::Mbit4 | Ceiling::Mbit8 | Ceiling::Mbit16 => {
                ceiling.bits_per_second().map(|bits| self.mbits(bits)).unwrap_or_default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use choices::Kbits;

    #[test]
    fn the_rows_read_as_the_canvas_has_them_each_ascending() {
        let words = Words::english();
        assert_eq!(Resolution::ALL.map(|size| words.resolution(size)), ["720p", "1080p", "1440p", "Source"]);
        assert_eq!(FrameRate::ALL.map(|rate| words.frame_rate(rate)), ["15 fps", "24 fps", "30 fps", "60 fps"]);
        assert_eq!(Ceiling::ALL.map(|ceiling| words.ceiling(ceiling)), ["4", "8", "16", "Automatic"]);
        assert_eq!(words.ceiling(Ceiling::Custom(Kbits::LEAST)), "Custom");
    }

    #[test]
    fn the_camera_rows_say_the_size_and_the_rate() {
        let words = Words::english();
        assert_eq!(CameraResolution::ALL.map(|size| words.camera_resolution(size)), ["1080p", "720p", "480p"]);
        assert_eq!(CameraRate::ALL.map(|rate| words.camera_rate(rate)), ["60 fps", "30 fps", "15 fps"]);
    }
}
