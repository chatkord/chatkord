//! Why a run could not host its window, and why a run ended without one: the operator's words in `Display`, the
//! person's through `words::failure`.

use std::fmt;

use iced_winit::winit::error::EventLoopError;
use words::failure::Start;

/// Why the window could not be hosted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, report::Public)]
pub enum Unhosted {
    /// The system made no window.
    NoWindow,
    /// The window gave no surface to paint on.
    NoSurface,
    /// The thread the GPU starts on could not be made, or ended without its GPU.
    NoThread,
    /// No GPU here can show the window.
    NoGpu,
    /// The GPU would not give a device with what the window needs.
    NoDevice,
    /// The surface offers no format whose values are not re-encoded.
    NoFormat,
    /// The device was lost three times within a minute.
    DeviceLostAgain,
    /// The scene could not be made.
    Scene,
}

impl Unhosted {
    /// What of it a person can act on.
    pub fn start(self) -> Start {
        match self {
            Self::NoGpu | Self::NoDevice | Self::NoFormat | Self::NoSurface | Self::DeviceLostAgain => {
                Start::NoGraphics
            }
            Self::NoWindow => Start::NoWindow,
            Self::NoThread | Self::Scene => Start::Other,
        }
    }
}

impl fmt::Display for Unhosted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoWindow => "the system made no window",
            Self::NoSurface => "the window gave no surface",
            Self::NoThread => "the start's thread could not be made or ended without its GPU",
            Self::NoGpu => "no GPU here can show the window",
            Self::NoDevice => "the GPU gave no device with what the window needs",
            Self::NoFormat => "the surface offers no format that is not re-encoded",
            Self::DeviceLostAgain => "the GPU device was lost again within a minute",
            Self::Scene => "the scene could not be made",
        })
    }
}

impl std::error::Error for Unhosted {}

/// Why a run ended without running: its event loop failed, or its window could not be hosted.
#[derive(Debug)]
pub enum Failure {
    /// The windowing system's event loop failed.
    Loop(EventLoopError),
    /// The window could not be hosted.
    Unhosted(Unhosted),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Loop(why) => write!(f, "the window's loop failed: {why}"),
            Self::Unhosted(why) => fmt::Display::fmt(why, f),
        }
    }
}

impl std::error::Error for Failure {}

impl From<EventLoopError> for Failure {
    fn from(why: EventLoopError) -> Self {
        Self::Loop(why)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use words::{Language, Words};

    const EVERY: [Unhosted; 8] = [
        Unhosted::NoWindow,
        Unhosted::NoSurface,
        Unhosted::NoThread,
        Unhosted::NoGpu,
        Unhosted::NoDevice,
        Unhosted::NoFormat,
        Unhosted::DeviceLostAgain,
        Unhosted::Scene,
    ];

    #[test]
    fn every_reason_a_window_was_not_hosted_has_a_sentence_in_every_language() {
        for language in Language::all() {
            let words = Words::new(language);
            for why in EVERY {
                let said = words.phrase(words::failure::start(why.start()));
                assert!(said.contains(' ') && !said.starts_with("start-"), "{} {why:?}: {said}", language.tag());
            }
        }
    }

    #[test]
    fn a_graphics_failure_is_not_said_as_a_window_failure() {
        assert_eq!(Unhosted::NoGpu.start(), Start::NoGraphics);
        assert_eq!(Unhosted::DeviceLostAgain.start(), Start::NoGraphics);
        assert_eq!(Unhosted::NoWindow.start(), Start::NoWindow);
    }
}
