//! The call: what it holds, what it shows and the messages it takes, with the room, voices, shares and sound it is
//! made of.

use crate::discussion::{Compose, Discussion, leave, timeline_of};
use crate::place::{
    Ask, Channel, ChannelStanding, Control, Lobby, Message, Moment, Occupant, Owner, Panel, Place, SessionMessage,
    SignedIn, Status, Told,
};

use crate::SoundApp;
use crate::about::About;
use crate::call::camera::{CameraSent, CanCamera, OwnCamera};
use crate::call::health::{Health, Outgoing, OwnVideo, OwnWatch, RoundTrips};
use crate::call::room::{End, Event as RoomEvent, Room, Why};
use crate::call::share::{Can, ShareCard, ShareState, Shared, SoundSaid, Watched};
use crate::call::sound::Sound;
use crate::call::sources::{Chooser, Source, SourceKey, Sources, Tab};
use crate::call::voice::{Card, Pinned, Preview, Stage, Tile};
use crate::chime::Chime;
use crate::notice::Subject;
use crate::sign_in::Standing;
use crate::tell::{Action, Device, Faults, Telling};
use choices::{
    Audio, CallView, Ceiling, Kbits, OwnShare, People, PresetOrCustom, QualityPick, ShareQuality, State, TypedRefused,
    Volume,
};
use proto::rules::{Grant, Grants};
use proto::video::Video;
use std::collections::BTreeMap;
use std::time::Instant;
use terms::{Ended, MemberId};
use words::{Failure, Phrase, Sentence};

mod call_chat;
pub(crate) mod camera;
mod card_tools;
mod end;
pub(crate) mod fit;
pub(crate) mod floating;
mod grants;
pub(crate) mod health;
pub(crate) mod lens;
mod message;
mod model;
mod mute;
mod own;
mod popover;
pub(crate) mod quality;
pub(crate) mod room;
mod route;
pub(crate) mod share;
mod sharing;
pub(crate) mod sound;
mod sounds;
pub(crate) mod sources;
mod stage;
mod stats;
mod strip;
pub(crate) mod voice;
pub(crate) mod voice_settings;

pub use card_tools::*;
pub use end::*;
use grants::*;
pub use message::*;
pub use model::*;
pub use popover::*;
pub use sharing::*;
pub use stage::*;
pub use strip::*;
