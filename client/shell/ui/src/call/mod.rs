//! The call view: what a window shows of a call, apart from the frame around it.

mod cards;
mod devices;
mod panel;
mod sound;
mod toggles;
mod tools;
mod view;

pub(crate) use cards::floating_card;
pub(crate) use panel::{bar_room, call_panel};
pub(crate) use sound::{screen_sound, sound_controls};
pub(crate) use toggles::{call_toggles, next_call_toggles};
pub(crate) use tools::{zoom_tool, zoomable};
pub(crate) use view::call_view;
