//! Where the producers' errors meet `screens`: each is mapped to the [`Failure`] a person can be told, with no
//! wildcard arm, so a new error stops the build until it has one. What a `Failure` says is `screens`' alone.

use crate::screen::Fail;
use ::sound::share::SoundError;
use ::sound::speaker::PlayError;
use capture::CaptureError;
use media::EncodeError;
use proto::chatkord_v1::ErrorCode;
use terms::LinkError;
use webrtc::{DevicesRefused, VoiceRefused};
use words::Failure;

/// A capture that did not start or stopped.
pub fn capture(why: CaptureError) -> Failure {
    match why {
        CaptureError::Connect => Failure::ScreenUnreachable,
        CaptureError::NoDmaBuf => Failure::ScreenNeedsCopy,
        CaptureError::Format => Failure::ScreenSizeUnshareable,
        CaptureError::Stream => Failure::CaptureStopped,
        CaptureError::Unavailable => Failure::CaptureNeedsPipeWire,
        CaptureError::Ended => Failure::ShareEnded,
    }
}

/// A camera that would not open or stopped giving pictures.
pub fn camera(why: capture::camera::CameraError) -> Failure {
    match why {
        capture::camera::CameraError::Denied => Failure::CameraDenied,
        capture::camera::CameraError::Busy => Failure::CameraBusy,
        capture::camera::CameraError::Gone => Failure::CameraGone,
        capture::camera::CameraError::Format => Failure::CameraFormat,
        capture::camera::CameraError::Unavailable => Failure::CameraUnavailable,
    }
}

/// An encode that could not be opened or went on.
pub fn encode(why: EncodeError) -> Failure {
    match why {
        EncodeError::Settings | EncodeError::Frames | EncodeError::Open => Failure::EncoderRefusedSize,
        EncodeError::Device => Failure::VideoHardwareClosed,
        EncodeError::FrameSize => Failure::ScreenResized,
        EncodeError::Screen => Failure::VideoHardwareNeedsCopy,
        EncodeError::Upload
        | EncodeError::Planar
        | EncodeError::Convert
        | EncodeError::Input
        | EncodeError::OtherCodec
        | EncodeError::DeviceLost
        | EncodeError::Stalled
        | EncodeError::Encoder(_) => Failure::EncodeStopped,
    }
}

/// A screen's run that ended before it had a report.
pub fn screen(why: &Fail) -> Failure {
    match why {
        Fail::Capture(why) => capture(*why),
        Fail::Encode(why) => encode(*why),
        Fail::Silent => Failure::ScreenSilent,
        Fail::Camera(why) => camera(*why),
    }
}

/// A share's sound that did not start.
pub fn sound(why: SoundError) -> Failure {
    match why {
        #[cfg(target_os = "linux")]
        SoundError::Unavailable => Failure::ShareSoundNeedsPipeWire,
        #[cfg(not(target_os = "linux"))]
        SoundError::Unavailable => Failure::ShareSoundNeedsNewerWindows,
        SoundError::Stream => Failure::ShareSoundStream,
    }
}

/// A voice step that did not happen.
pub fn voice(why: VoiceRefused) -> Failure {
    match why {
        VoiceRefused::Offer => Failure::VoiceOffer,
        VoiceRefused::Engine => Failure::VoiceEngine,
        VoiceRefused::AlreadySending => Failure::AlreadySending,
        VoiceRefused::Sound(refused) => devices(&refused),
        VoiceRefused::Link(failed) => Failure::Link(failed),
        VoiceRefused::Stalled(_) => Failure::EngineStalled,
        VoiceRefused::Unreached { host, port } => Failure::VoiceUnreached { host, port },
    }
}

/// A watched screen that could not be taken: one the server cannot carry now says so, the rest as any voice step.
pub fn watch(why: VoiceRefused) -> Failure {
    match why {
        VoiceRefused::Link(LinkError::Refused(ErrorCode::ServerUnavailable)) => Failure::ScreenNotCarried,
        refused => voice(refused),
    }
}

/// A sound of the window's that did not play.
pub fn played(why: PlayError) -> Failure {
    match why {
        PlayError::Unavailable | PlayError::Closed => Failure::SoundUnavailable,
        PlayError::Refused => Failure::DeviceRefused,
    }
}

/// A sound device step that did not happen.
pub fn devices(why: &DevicesRefused) -> Failure {
    match why {
        DevicesRefused::Unavailable => Failure::SoundUnavailable,
        DevicesRefused::Missing => Failure::DeviceMissing,
        DevicesRefused::Refused => Failure::DeviceRefused,
    }
}

/// The screen picker, which picked nothing.
pub fn picked(why: share::Refused) -> Failure {
    match why {
        #[cfg(target_os = "linux")]
        share::Refused::Unavailable => Failure::PickerNoPortal,
        #[cfg(windows)]
        share::Refused::Unavailable => Failure::PickerNeedsNewerWindows,
        #[cfg(not(any(target_os = "linux", windows)))]
        share::Refused::Unavailable => Failure::PickerUnavailable,
        share::Refused::Cancelled => Failure::NothingChosen,
        share::Refused::Failed => Failure::PickerFailed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every class a mapping gives is said in a sentence of its own words.
    fn said(failure: &Failure) -> String {
        let said = words::Words::english().said(&words::failure::of(failure));
        assert!(said.ends_with('.'), "{failure:?}: {said}");
        said
    }

    #[test]
    fn every_capture_and_encode_error_is_said() {
        let captures = [
            CaptureError::Connect,
            CaptureError::NoDmaBuf,
            CaptureError::Format,
            CaptureError::Stream,
            CaptureError::Unavailable,
            CaptureError::Ended,
        ];
        let words: Vec<_> = captures.map(|why| said(&capture(why))).into();
        assert!(words.iter().enumerate().all(|(at, one)| !words[at + 1..].contains(one)), "each its own: {words:?}");
        for why in [
            EncodeError::Settings,
            EncodeError::Device,
            EncodeError::Frames,
            EncodeError::Open,
            EncodeError::FrameSize,
            EncodeError::Upload,
            EncodeError::Screen,
            EncodeError::Convert,
            EncodeError::Input,
            EncodeError::OtherCodec,
            EncodeError::DeviceLost,
            EncodeError::Stalled,
            EncodeError::Encoder(media::AvError::Other(-1)),
        ] {
            said(&encode(why));
        }
        assert_eq!(said(&screen(&Fail::Silent)), "The screen sent no picture in that time.");
    }

    #[test]
    fn every_sound_voice_device_and_picker_refusal_is_said() {
        for why in [SoundError::Unavailable, SoundError::Stream] {
            said(&sound(why));
        }
        for why in [DevicesRefused::Unavailable, DevicesRefused::Missing, DevicesRefused::Refused] {
            said(&devices(&why));
        }
        for why in [share::Refused::Unavailable, share::Refused::Cancelled, share::Refused::Failed] {
            said(&picked(why));
        }
        for why in [VoiceRefused::Offer, VoiceRefused::Engine, VoiceRefused::AlreadySending] {
            said(&voice(why));
        }
        let stalled = voice(VoiceRefused::Stalled(webrtc::Step::CreateOffer));
        assert_eq!(said(&stalled), "Your device did not answer in time.", "whichever step it was: that is the log's");
        let missing = voice(VoiceRefused::Sound(DevicesRefused::Missing));
        assert_eq!(missing, Failure::DeviceMissing, "a device's refusal inside a voice step is the device's");
        let refused = LinkError::Refused(ErrorCode::Unspecified);
        assert_eq!(said(&voice(VoiceRefused::Link(refused))), "The server refused without saying why.", "by its code");
    }

    #[test]
    fn a_screen_the_server_cannot_carry_is_said_in_words_of_its_own() {
        let busy = LinkError::Refused(ErrorCode::ServerUnavailable);
        assert_eq!(said(&watch(VoiceRefused::Link(busy))), "The server can’t send you this stream now.");
        let other = LinkError::Refused(ErrorCode::NotAuthorized);
        assert_eq!(
            said(&watch(VoiceRefused::Link(other))),
            "Your account may not do that.",
            "any other refusal as a voice step's"
        );
    }
}
