//! How the window looks on this device: the theme, the contrast, the zoom and how messages are drawn, as a person
//! picked them in Settings › Appearance. Its colour is not a choice: the window is always Chatkord's.

use crate::call_chat::CallChat;
use crate::call_view::{CallView, ViewPick};
use crate::members::Members;
use crate::pane::PaneWidth;
use crate::previews::Previews;
use design::{Contrast, MessageScale, MessageSize, Preference, RunGap, Zoom};

/// What a person picked; the default is the system's scheme, the standard contrast, no zoom and messages at 14 px.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Look {
    /// Light, dark, or the system's scheme.
    pub theme: Preference,
    /// Standard, or the More contrast switch on.
    pub contrast: Contrast,
    /// The scale of the whole window, in its steps.
    pub zoom: Zoom,
    /// The server pane's width, as its splitter left it.
    pub pane: PaneWidth,
    /// How a call is drawn, as its Voice options left it.
    pub call: CallView,
    /// The members column: open or closed, and its width.
    pub members: Members,
    /// A text channel's messages: their text size and the space between their runs.
    pub messages: MessageScale,
    /// A call's chat column: open or closed, and its width.
    pub call_chat: CallChat,
    /// Whether the first link of a message sent gets a preview.
    pub previews: Previews,
}

/// One choice on the Appearance page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LookPick {
    /// A theme chosen.
    Theme(Preference),
    /// The More contrast switch.
    MoreContrast,
    /// One step of the zoom.
    Zoom(ZoomPick),
    /// A switch of a call's Voice options.
    View(ViewPick),
    /// A step of a message's text size.
    MessageSize(MessageSize),
    /// A step of the space between a text channel's runs.
    RunGap(RunGap),
    /// The switch of link previews.
    Previews,
}

/// A step of the zoom: the page's buttons and the keys (Ctrl+=, Ctrl+-, Ctrl+0) alike.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomPick {
    /// One step larger; the largest holds.
    In,
    /// One step smaller; the smallest holds.
    Out,
    /// Back to no zoom.
    Reset,
}

impl Look {
    /// Changes the one choice `pick` names and leaves the others.
    pub fn pick(&mut self, pick: LookPick) {
        match pick {
            LookPick::Theme(theme) => self.theme = theme,
            LookPick::MoreContrast => {
                self.contrast = match self.contrast {
                    Contrast::Standard => Contrast::More,
                    Contrast::More => Contrast::Standard,
                };
            }
            LookPick::View(pick) => self.call.pick(pick),
            LookPick::MessageSize(size) => self.messages.size = size,
            LookPick::RunGap(gap) => self.messages.gap = gap,
            LookPick::Previews => self.previews = self.previews.toggled(),
            LookPick::Zoom(step) => {
                self.zoom = match step {
                    ZoomPick::In => self.zoom.zoomed_in().unwrap_or(self.zoom),
                    ZoomPick::Out => self.zoom.zoomed_out().unwrap_or(self.zoom),
                    ZoomPick::Reset => Zoom::DEFAULT,
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_pick_changes_its_own_choice_and_more_contrast_turns_on_and_off() {
        let mut look = Look::default();
        look.pick(LookPick::Theme(Preference::Light));
        assert_eq!(look, Look { theme: Preference::Light, ..Look::default() });
        look.pick(LookPick::MoreContrast);
        assert_eq!(look.contrast, Contrast::More);
        look.pick(LookPick::MoreContrast);
        assert_eq!(look.contrast, Contrast::Standard);
        let (size, gap) = (MessageSize::of_px(24).unwrap(), RunGap::of_px(0).unwrap());
        look.pick(LookPick::MessageSize(size));
        look.pick(LookPick::RunGap(gap));
        let messages = MessageScale { size, gap };
        assert_eq!(look, Look { theme: Preference::Light, messages, ..Look::default() });
    }

    #[test]
    fn link_previews_are_on_in_a_new_look_and_the_pick_flips_them_alone() {
        let mut look = Look::default();
        assert_eq!(look.previews, Previews::On);
        look.pick(LookPick::Previews);
        assert_eq!(look, Look { previews: Previews::Off, ..Look::default() });
        look.pick(LookPick::Previews);
        assert_eq!(look, Look::default());
    }

    #[test]
    fn a_zoom_steps_one_at_a_time_stops_at_the_ends_and_resets_to_none() {
        let mut look = Look::default();
        look.pick(LookPick::Zoom(ZoomPick::In));
        assert_eq!(look.zoom.percent(), 110);
        for _ in 0..20 {
            look.pick(LookPick::Zoom(ZoomPick::In));
        }
        assert_eq!(look.zoom.percent(), 200, "the top holds");
        look.pick(LookPick::Zoom(ZoomPick::Reset));
        assert_eq!(look, Look::default());
        for _ in 0..20 {
            look.pick(LookPick::Zoom(ZoomPick::Out));
        }
        assert_eq!(look.zoom.percent(), 67, "the bottom holds");
        assert_eq!(Look { zoom: Zoom::DEFAULT, ..look }, Look::default(), "nothing else moved");
    }
}
