//! What a failure says to a person: one of `link`'s, or one of the call's as a [`Failure`]. The words are this
//! crate's table's, so a translation reaches them, a refusal's by its code and a close reason's by its reason. `Display` on the producers' errors stays an
//! operator's.

use crate::sentence::Sentence;
use crate::words::Phrase;
use proto::chatkord_v1::{CloseReason, ErrorCode};
use proto::files::{FileKind, FileRefusal};
use terms::{Ended, FileError, LinkError, ReportError, ServerIs};

/// Which end of a major mismatch to update, when the server said its own.
pub fn other_major(server: ServerIs) -> Phrase {
    match server {
        ServerIs::Newer => Phrase::UpdateToJoin,
        ServerIs::Older => Phrase::ServerTooOld,
        ServerIs::Unsaid => Phrase::OtherVersion,
    }
}

/// What `failed` says.
pub fn said(failed: &LinkError) -> Sentence {
    match failed {
        LinkError::Connect => Sentence::Phrase(Phrase::NothingAnswers),
        LinkError::NoSuchHost => Sentence::Phrase(Phrase::NoSuchHost),
        LinkError::NoNetwork => Sentence::Phrase(Phrase::NoNetwork),
        LinkError::Offline => Sentence::Phrase(Phrase::NotConnectedThere),
        LinkError::Unanswered => Sentence::Phrase(Phrase::NoGreeting),
        LinkError::Lost => Sentence::Phrase(Phrase::ConnectionEnded),
        LinkError::Malformed => Sentence::Phrase(Phrase::ServerUnreadable),
        LinkError::OtherMajor(server) => Sentence::Phrase(other_major(*server)),
        LinkError::NotChatkord => Sentence::Phrase(Phrase::NotAChatkordServer),
        LinkError::Untrusted => Sentence::Phrase(Phrase::Untrusted),
        LinkError::Unvouched(_) => Sentence::Phrase(Phrase::SelfSigned),
        LinkError::KeyChanged { .. } => Sentence::Phrase(Phrase::KeyChanged),
        LinkError::OldEncryption => Sentence::Phrase(Phrase::OldEncryption),
        LinkError::Refused(code) => Sentence::Phrase(refusal(*code)),
        LinkError::Closed(gone) => ended(gone),
        LinkError::Timeout => Sentence::Phrase(Phrase::NoAnswerInTime),
        LinkError::TooLarge => Sentence::Phrase(Phrase::ServerCouldntDoIt),
    }
}

/// How a connection ended: the close reason's words, else the refusal the server wrote just before, else that it
/// stopped answering, else that it ended. The reason says what happened to the session, so it wins.
pub fn ended(gone: &Ended) -> Sentence {
    match (gone.reason.and_then(closed), &gone.refusal) {
        (Some(phrase), _) => Sentence::Phrase(phrase),
        (None, Some(code)) => Sentence::Phrase(refusal(*code)),
        (None, None) if gone.silent => Sentence::Phrase(Phrase::ServerStoppedAnswering),
        (None, None) => Sentence::Phrase(Phrase::ConnectionEnded),
    }
}

/// The words of a refusal, by its code.
pub fn refusal(code: ErrorCode) -> Phrase {
    match code {
        ErrorCode::Unspecified => Phrase::RefusedUnsaid,
        ErrorCode::ProtoVersionMismatch => Phrase::OtherVersion,
        ErrorCode::UnknownOp => Phrase::RefusedUnknownOp,
        ErrorCode::BadFrame => Phrase::RefusedUnreadable,
        ErrorCode::Unauthenticated => Phrase::RefusedSignedOut,
        ErrorCode::NotAuthorized => Phrase::RefusedNotAllowed,
        ErrorCode::RateLimited => Phrase::RefusedTooSoon,
        ErrorCode::ServerUnavailable => Phrase::RefusedUnavailable,
        ErrorCode::Internal => Phrase::ServerCouldntDoIt,
        ErrorCode::NotFound => Phrase::RefusedNotFound,
        ErrorCode::InvalidRequest => Phrase::RefusedInvalid,
        ErrorCode::AlreadyInVoice => Phrase::RefusedAlreadyInVoice,
        ErrorCode::AlreadyExists => Phrase::RefusedNameTaken,
        ErrorCode::StoreFull => Phrase::RefusedStoreFull,
        ErrorCode::ListChanged => Phrase::RefusedListChanged,
        ErrorCode::NoVoiceAddress => Phrase::RefusedNoVoiceAddress,
    }
}

/// The words of a close reason, or none where the server's refusal or the generic words say it better.
pub fn closed(reason: CloseReason) -> Option<Phrase> {
    match reason {
        CloseReason::SignedOut => Some(Phrase::ClosedSignedOut),
        CloseReason::SessionExpired => Some(Phrase::ClosedSessionExpired),
        CloseReason::Kicked => Some(Phrase::ClosedKicked),
        CloseReason::PasswordChanged => Some(Phrase::ClosedPasswordChanged),
        CloseReason::PasswordReset => Some(Phrase::ClosedPasswordReset),
        CloseReason::AccountDeleted => Some(Phrase::ClosedAccountDeleted),
        CloseReason::Banned => Some(Phrase::ClosedBanned),
        CloseReason::Moved => Some(Phrase::ClosedMoved),
        CloseReason::NotAllowed => Some(Phrase::ClosedNotAllowed),
        CloseReason::ChannelDeleted => Some(Phrase::ClosedChannelDeleted),
        CloseReason::TakenOut => Some(Phrase::ClosedTakenOut),
        CloseReason::OverRate => Some(Phrase::ClosedOverRate),
        CloseReason::Unreachable => Some(Phrase::ClosedUnreachable),
        CloseReason::ClientShutdown => Some(Phrase::ClosedClientShutdown),
        CloseReason::ServerStopping => Some(Phrase::ClosedServerStopping),
        // The server's refusal names the other version; a server fault names nothing a person can act on; a server
        // never ends a socket with `FELL_BEHIND`, only a client says it, of itself.
        CloseReason::VersionMismatch
        | CloseReason::Unspecified
        | CloseReason::Normal
        | CloseReason::ServerError
        | CloseReason::FellBehind => None,
    }
}

/// Why the window could not start, as far as a person can act on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    /// No graphics card here can show the window.
    NoGraphics,
    /// The window could not be opened.
    NoWindow,
    /// The first picture was not shown within the start's deadline.
    TooSlow,
    /// Anything else, which the log has.
    Other,
}

/// The words of a failed start.
pub fn start(failed: Start) -> Phrase {
    match failed {
        Start::NoGraphics => Phrase::StartNoGraphics,
        Start::NoWindow => Phrase::StartNoWindow,
        Start::TooSlow => Phrase::StartTooSlow,
        Start::Other => Phrase::StartFailed,
    }
}

/// What a file that was not sent or fetched says.
pub fn file(failed: &FileError) -> Sentence {
    match failed {
        FileError::Reach(failed) => said(failed),
        FileError::Refused { kind, refusal } => refused(*kind, *refusal),
        FileError::Unreadable { kind: FileKind::ServerPicture | FileKind::ProfilePicture | FileKind::Emoji } => {
            Sentence::Phrase(Phrase::PictureUnreadable)
        }
        FileError::Unreadable { kind: FileKind::Attachment | FileKind::Thumbnail } => {
            Sentence::Phrase(Phrase::FileUnreadable)
        }
        FileError::Disk(_) => Sentence::Phrase(Phrase::FileOnDevice),
    }
}

/// What the server's debug report not being had says: the pace's own words where the ask or the download was too soon.
pub fn report(failed: &ReportError) -> Sentence {
    let phrase = match failed {
        ReportError::Reach(LinkError::Refused(ErrorCode::RateLimited)) | ReportError::Refused(FileRefusal::TooMany) => {
            Phrase::ReportTooMany
        }
        ReportError::Reach(failed) => return said(failed),
        ReportError::Refused(FileRefusal::Unauthenticated) => Phrase::FileSignedOut,
        ReportError::Refused(FileRefusal::NotAuthorized) => refusal(ErrorCode::NotAuthorized),
        ReportError::Refused(FileRefusal::NotFound) => Phrase::ReportGone,
        ReportError::Refused(FileRefusal::Unavailable) => Phrase::ServerBusy,
        ReportError::Refused(FileRefusal::Internal) => Phrase::ServerCouldntDoIt,
        // The report's route answers no other status: a server that does is not speaking as ours.
        ReportError::Refused(
            FileRefusal::BadRequest
            | FileRefusal::LengthRequired
            | FileRefusal::TooLarge
            | FileRefusal::UnsupportedType
            | FileRefusal::Unprocessable
            | FileRefusal::Full
            | FileRefusal::AccountFull,
        ) => Phrase::ServerUnreadable,
    };
    Sentence::Phrase(phrase)
}

fn refused(kind: FileKind, refusal: FileRefusal) -> Sentence {
    let picture = matches!(kind, FileKind::ServerPicture | FileKind::ProfilePicture | FileKind::Emoji);
    let phrase = match (refusal, kind) {
        (FileRefusal::Unauthenticated, _) => Phrase::FileSignedOut,
        (FileRefusal::NotAuthorized, FileKind::ServerPicture) => Phrase::CantChangePicture,
        (FileRefusal::NotAuthorized, FileKind::ProfilePicture) => self::refusal(ErrorCode::NotAuthorized),
        (FileRefusal::NotAuthorized, FileKind::Attachment) => Phrase::CantAttachFiles,
        (FileRefusal::NotAuthorized, FileKind::Emoji) => Phrase::CantAddEmoji,
        (FileRefusal::NotAuthorized | FileRefusal::NotFound, FileKind::Thumbnail)
        | (FileRefusal::NotFound, FileKind::Attachment | FileKind::Emoji) => Phrase::FileGone,
        (FileRefusal::NotFound, FileKind::ServerPicture | FileKind::ProfilePicture) => Phrase::PictureGone,
        (FileRefusal::TooLarge, FileKind::ServerPicture | FileKind::ProfilePicture) => {
            return Sentence::PictureTooLarge;
        }
        (FileRefusal::TooLarge, FileKind::Emoji) => return Sentence::EmojiTooLarge,
        // No server tells its own limit, which its host sets: the sentence names none.
        (FileRefusal::TooLarge, FileKind::Attachment) => return Sentence::FileTooLarge(None),
        (FileRefusal::AccountFull, _) => return Sentence::FilesUsed(None),
        (FileRefusal::UnsupportedType, _) if picture => Phrase::NotAPicture,
        (FileRefusal::Unprocessable, _) if picture => return Sentence::PictureTooWide,
        (FileRefusal::Full, _) => Phrase::ServerFull,
        (FileRefusal::TooMany, _) => Phrase::TooManyRequests,
        (FileRefusal::Unavailable, _) => Phrase::ServerBusy,
        (FileRefusal::Internal, _) => Phrase::ServerCouldntDoIt,
        (FileRefusal::TooLarge, FileKind::Thumbnail)
        | (FileRefusal::UnsupportedType | FileRefusal::Unprocessable, _)
        | (FileRefusal::BadRequest | FileRefusal::LengthRequired, _) => Phrase::FileNotTaken,
    };
    Sentence::Phrase(phrase)
}

/// What a person can be told of a failure of the call, its devices or its screen; it is said only here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The server could not be asked, or refused.
    Link(LinkError),
    /// The capture of the screen would not connect.
    ScreenUnreachable,
    /// The compositor offers no picture the GPU can take without a copy, and none is made.
    ScreenNeedsCopy,
    /// The picture the screen gives has a size or layout that cannot be encoded.
    ScreenSizeUnshareable,
    /// The screen's capture stream failed.
    CaptureStopped,
    /// This system has no screen capture: on Linux, no PipeWire.
    CaptureNeedsPipeWire,
    /// The person or the desktop stopped the share, or the window shared went away.
    ShareEnded,
    /// The screen gave no picture within the time a share waits for its first.
    ScreenSilent,
    /// The encoder would not open at the screen's size and settings.
    EncoderRefusedSize,
    /// The video encoder's device did not open.
    VideoHardwareClosed,
    /// A picture came at another size than the encode was opened for.
    ScreenResized,
    /// The encoder's device cannot take the screen's pictures on the GPU, and no copy is made.
    VideoHardwareNeedsCopy,
    /// The encode failed while it ran.
    EncodeStopped,
    /// A share's sound cannot be captured on this Linux: it has no PipeWire.
    ShareSoundNeedsPipeWire,
    /// A share's sound cannot be captured on this Windows: it predates process loopback.
    ShareSoundNeedsNewerWindows,
    /// The share's sound capture would not open.
    ShareSoundStream,
    /// The share's sound capture ended (the sound server restarted, its device went) and has not come back yet.
    ShareSoundStopped,
    /// The system's privacy settings keep this application from the camera.
    CameraDenied,
    /// Another application holds the camera.
    CameraBusy,
    /// The camera was unplugged, or is no longer there.
    CameraGone,
    /// The camera offers no picture that can be encoded.
    CameraFormat,
    /// This system has no camera capture.
    CameraUnavailable,
    /// The server's offer for the call could not be read.
    VoiceOffer,
    /// The voice engine would not set up the call's connection on this device.
    VoiceEngine,
    /// The voice engine did not finish a step of the call's connection within its bound.
    EngineStalled,
    /// A step of the call panicked: the call ended, and the connection to its server is made again.
    CallFaulted,
    /// The call's media never connected, or the voice address named nothing this device may reach.
    VoiceUnreached {
        /// The voice address as the server announced it: a name or an address.
        host: String,
        /// The port media was sent to.
        port: u16,
    },
    /// The sound devices' loop failed twice in a row and was stopped: only a restart brings sound back.
    SoundStopped,
    /// A track of that kind is already being sent on the call.
    AlreadySending,
    /// The sound devices would not start on this device.
    SoundUnavailable,
    /// The sound device picked is not present.
    DeviceMissing,
    /// The sound device picked is present and would not be used.
    DeviceRefused,
    /// On Linux, no screen-cast portal is installed, so there is no picker.
    PickerNoPortal,
    /// On Windows, this version has no screen picker.
    PickerNeedsNewerWindows,
    /// This system has no screen picker.
    PickerUnavailable,
    /// The person closed the desktop's picker without choosing.
    NothingChosen,
    /// The desktop's picker failed or picked nothing.
    PickerFailed,
    /// No decoder on this device, the processor's included, takes the watched screen's stream.
    CannotDecode,
    /// The server could not carry the watched screen to this device now: its media budget is spent.
    ScreenNotCarried,
    /// The same for the watched screen's sound, its picture playing on.
    ShareSoundNotCarried,
}

/// What `failed` says.
pub fn of(failed: &Failure) -> Sentence {
    let phrase = match failed {
        Failure::Link(failed) => return said(failed),
        Failure::VoiceUnreached { host, port } => return Sentence::VoiceUnreached { host: host.clone(), port: *port },
        Failure::ScreenUnreachable => Phrase::ScreenUnreachable,
        Failure::ScreenNeedsCopy => Phrase::ScreenNeedsCopy,
        Failure::ScreenSizeUnshareable => Phrase::ScreenSizeUnshareable,
        Failure::CaptureStopped => Phrase::CaptureStopped,
        Failure::CaptureNeedsPipeWire => Phrase::CaptureNeedsPipeWire,
        Failure::ShareEnded => Phrase::ShareEnded,
        Failure::ScreenSilent => Phrase::ScreenSilent,
        Failure::EncoderRefusedSize => Phrase::EncoderRefusedSize,
        Failure::VideoHardwareClosed => Phrase::VideoHardwareClosed,
        Failure::ScreenResized => Phrase::ScreenResized,
        Failure::VideoHardwareNeedsCopy => return Sentence::Text(crate::Text::VideoHardwareNeedsCopy),
        Failure::EncodeStopped => Phrase::EncodeStopped,
        Failure::ShareSoundNeedsPipeWire => Phrase::ShareSoundNeedsPipeWire,
        Failure::ShareSoundNeedsNewerWindows => Phrase::ShareSoundNeedsNewerWindows,
        Failure::ShareSoundStream => Phrase::ShareSoundStream,
        Failure::ShareSoundStopped => Phrase::ShareSoundStopped,
        Failure::CameraDenied => Phrase::CameraDenied,
        Failure::CameraBusy => Phrase::CameraBusy,
        Failure::CameraGone => Phrase::CameraGone,
        Failure::CameraFormat => Phrase::CameraFormat,
        Failure::CameraUnavailable => Phrase::CameraUnavailable,
        Failure::VoiceOffer => Phrase::VoiceOffer,
        Failure::VoiceEngine => Phrase::VoiceEngine,
        Failure::EngineStalled => Phrase::EngineStalled,
        Failure::CallFaulted => Phrase::CallFaulted,
        Failure::SoundStopped => Phrase::SoundStopped,
        Failure::AlreadySending => Phrase::AlreadySending,
        Failure::SoundUnavailable => Phrase::SoundUnavailable,
        Failure::DeviceMissing => Phrase::DeviceMissing,
        Failure::DeviceRefused => Phrase::DeviceRefused,
        Failure::PickerNoPortal => Phrase::PickerNoPortal,
        Failure::PickerNeedsNewerWindows => Phrase::PickerNeedsNewerWindows,
        Failure::PickerUnavailable => Phrase::PickerUnavailable,
        Failure::NothingChosen => Phrase::NothingChosen,
        Failure::PickerFailed => Phrase::PickerFailed,
        Failure::CannotDecode => Phrase::CannotDecode,
        Failure::ScreenNotCarried => Phrase::ScreenNotCarried,
        Failure::ShareSoundNotCarried => Phrase::ShareSoundNotCarried,
    };
    Sentence::Phrase(phrase)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::chatkord_v1::ErrorCode;

    /// Every reason the wire names, by its numbers: a reason added to the schema is listed here by construction.
    fn every_reason() -> Vec<CloseReason> {
        (0..=i32::from(u8::MAX)).filter_map(|n| CloseReason::try_from(n).ok()).collect()
    }

    #[test]
    fn a_reason_with_words_of_its_own_is_said_before_the_servers_refusal() {
        let refusal = Some(ErrorCode::Unauthenticated);
        for reason in every_reason() {
            let gone = Ended { reason: Some(reason), refusal, silent: false, cursor: None };
            let expected = match closed(reason) {
                Some(phrase) => Sentence::Phrase(phrase),
                None => Sentence::Phrase(Phrase::RefusedSignedOut),
            };
            assert_eq!(ended(&gone), expected, "{reason:?}");
            assert_eq!(said(&LinkError::Closed(gone)), expected, "{reason:?}");
        }
        assert!(every_reason().len() > 15, "the schema's reasons were read");
    }

    #[test]
    fn every_refusal_code_has_words_of_its_own() {
        let codes: Vec<ErrorCode> = (0..=i32::from(u8::MAX)).filter_map(|n| ErrorCode::try_from(n).ok()).collect();
        assert!(codes.len() > 10, "the schema's codes were read");
        let words: std::collections::HashSet<String> =
            codes.iter().map(|code| said(&LinkError::Refused(*code)).in_english()).collect();
        assert_eq!(words.len(), codes.len(), "two codes say the same: {words:?}");
    }

    #[test]
    fn a_connection_that_failed_is_never_said_as_another_version() {
        let other = Phrase::OtherVersion.in_english();
        for failed in [LinkError::Offline, LinkError::Unanswered, LinkError::Lost, LinkError::Malformed] {
            assert_ne!(said(&failed).in_english(), other, "{failed:?}");
        }
    }

    #[test]
    fn nothing_answering_is_not_the_same_failure_as_a_wrong_thing_answering() {
        let pin = proto::pin::KeyPin::from_digest([0; proto::pin::KeyPin::LEN]);
        let words: Vec<_> = [
            LinkError::Connect,
            LinkError::NoSuchHost,
            LinkError::NoNetwork,
            LinkError::NotChatkord,
            LinkError::Untrusted,
            LinkError::Unvouched(pin),
            LinkError::KeyChanged { trusted: pin, presented: pin },
            LinkError::OldEncryption,
        ]
        .iter()
        .map(|failed| said(failed).in_english())
        .collect();
        for (at, one) in words.iter().enumerate() {
            assert!(words[at + 1..].iter().all(|other| other != one), "{one}");
            assert!(one.ends_with('.') && !one.contains("://"), "{one}");
        }
    }

    #[test]
    fn every_failed_start_is_its_own_sentence() {
        let said: Vec<_> = [Start::NoGraphics, Start::NoWindow, Start::TooSlow, Start::Other]
            .map(|failed| start(failed).in_english())
            .into();
        assert!(said.iter().all(|one| one.ends_with('.') && !one.contains("://")), "{said:?}");
        assert!(said[0] != said[1] && said[1] != said[2] && said[0] != said[2], "{said:?}");
    }

    #[test]
    fn a_connection_that_ended_saying_nothing_says_whether_it_went_silent() {
        let silent = Ended { reason: None, refusal: None, silent: true, cursor: None };
        assert_eq!(ended(&silent), Sentence::Phrase(Phrase::ServerStoppedAnswering));
        assert_eq!(ended(&Ended::default()), Sentence::Phrase(Phrase::ConnectionEnded));
    }

    #[test]
    fn every_file_refusal_is_its_own_short_sentence() {
        let mut said: Vec<_> = FileRefusal::ALL
            .iter()
            .map(|refusal| file(&FileError::Refused { kind: FileKind::ServerPicture, refusal: *refusal }).in_english())
            .collect();
        assert!(said.iter().all(|one| one.ends_with('.') && !one.contains("://") && one.len() < 100), "{said:?}");
        said.sort();
        said.dedup();
        assert_eq!(said.len(), FileRefusal::ALL.len() - 1, "only the two malformed asks share one");
    }

    #[test]
    fn every_report_refusal_is_a_short_sentence_and_the_pace_has_its_own_words() {
        for refusal in FileRefusal::ALL {
            let said = report(&ReportError::Refused(refusal)).in_english();
            assert!(said.ends_with('.') && !said.contains("://") && said.len() < 100, "{refusal:?}: {said}");
        }
        let pace = Sentence::Phrase(Phrase::ReportTooMany);
        assert_eq!(report(&ReportError::Refused(FileRefusal::TooMany)), pace);
        assert_eq!(report(&ReportError::Reach(LinkError::Refused(ErrorCode::RateLimited))), pace);
        assert_eq!(report(&ReportError::Reach(LinkError::Timeout)), said(&LinkError::Timeout));
    }

    #[test]
    fn a_picture_over_the_bounds_names_them() {
        let over = |refusal| file(&FileError::Refused { kind: FileKind::ServerPicture, refusal }).in_english();
        assert_eq!(over(FileRefusal::TooLarge), "That picture is over 2 MB. Choose a smaller one.");
        assert_eq!(
            over(FileRefusal::Unprocessable),
            "That picture is over 4096 pixels on a side. Choose a smaller one."
        );
    }
}
