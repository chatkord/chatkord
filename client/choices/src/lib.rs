//! What a person chose for this device: how a share goes out, the camera, the microphone's processing, the window's
//! own sounds and look, the emoji picked lately, the members column, how a call is drawn, how Chatkord starts and
//! updates itself, and each member's volume. Plain values and the rules that hold within them; which screen offers a
//! choice and where it is kept are not decided here.
#![forbid(unsafe_code)]

mod auto_update;
mod call_chat;
mod call_view;
mod camera;
mod custom;
mod device;
mod emoji;
mod event_sounds;
mod facts;
mod hearing;
mod language;
mod look;
mod members;
mod message_sounds;
mod pane;
mod popups;
mod previews;
mod processing;
mod remembered;
mod share;
mod startup;
mod volume;

pub use auto_update::AutoUpdate;
pub use call_chat::{CallChat, CallChatShown, CallChatSpan, CallChatWidth};
pub use call_view::{CallView, Controls, OwnShare, People, ViewPick};
pub use camera::{CameraAsk, CameraRate, CameraResolution, CameraSettings, Mirror};
pub use custom::{Bounded, Preset, PresetOrCustom, TypedRefused};
pub use device::Picked;
pub use emoji::{EmojiPicks, RECENT_EMOJI};
pub use event_sounds::EventSounds;
pub use facts::debug_section;
pub use hearing::State;
pub use language::LanguageTag;
pub use look::{Look, LookPick, ZoomPick};
pub use members::{ColumnWidth, Members, MembersShown, MembersSpan, MembersWidth, Span};
pub use message_sounds::MessageSounds;
pub use pane::PaneWidth;
pub use popups::Popups;
pub use previews::Previews;
pub use processing::{
    AutomaticGain, EchoCancellation, InputGain, METER_FLOOR, NoiseGate, Suppression, Threshold, VoiceProcessing,
    on_the_meter,
};
pub use remembered::Remembered;
pub use share::{
    Ceiling, Codec, EncoderOption, EncodesOn, FrameRate, Kbits, QualityPick, Resolution, ShareQuality, SoundSwitches,
};
pub use startup::{AtSignIn, CloseTo, StartsIn, Startup, Tray};
pub use volume::{Audio, Loudness, Volume, Volumes};
