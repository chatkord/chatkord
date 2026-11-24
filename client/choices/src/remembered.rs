//! Every choice this device keeps across runs, together.

use crate::{
    AutoUpdate, CameraSettings, EmojiPicks, EventSounds, LanguageTag, Look, MessageSounds, Picked, Popups,
    ShareQuality, Startup, State, VoiceProcessing,
};

/// What this device remembers, from the last run and the choices made since: for calls, and how the window looks
/// and speaks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Remembered {
    /// The microphone picked, none for the system's default.
    pub microphone: Option<Picked>,
    /// The speaker picked, none for the system's default.
    pub speaker: Option<Picked>,
    /// How the next call starts: as the account bar or the last call left it, kept across runs.
    pub next_call: State,
    /// How a share starts.
    pub share: ShareQuality,
    /// How the window looks.
    pub look: Look,
    /// Whether the window's own sounds play.
    pub event_sounds: EventSounds,
    /// Whether a message that would alert this account plays a sound.
    pub message_sounds: MessageSounds,
    /// Whether a message that would alert shows a desktop popup.
    pub popups: Popups,
    /// How the microphone is processed.
    pub processing: VoiceProcessing,
    /// How Chatkord starts and closes; none where this system offers no choice.
    pub startup: Option<Startup>,
    /// The camera chosen and how it is shown.
    pub camera: CameraSettings,
    /// The language chosen for the window; none to match the system's.
    pub language: Option<LanguageTag>,
    /// Whether Chatkord checks for a newer version by itself and downloads it.
    pub auto_update: AutoUpdate,
    /// The emoji picked lately and the skin tone hands are picked in.
    pub emoji: EmojiPicks,
}
