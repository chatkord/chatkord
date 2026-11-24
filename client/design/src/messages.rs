//! How large a text channel's messages are drawn and how far apart its runs stand, as a person picks them in
//! Settings › Appearance: Discord's chat font scaling and space between message groups. The zoom scales on top.

use crate::scale::TypeStyle;

/// The text sizes a person picks from, in logical pixels: Discord's chat font scaling.
const SIZES: [u16; 7] = [12, 14, 15, 16, 18, 20, 24];
/// The space above a run's first message a person picks from, in logical pixels: Discord's.
const GAPS: [u16; 5] = [0, 4, 8, 16, 24];

/// A step of a message's text size; the default is the size [`TypeStyle::Message`] is set at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MessageSize(usize);

impl Default for MessageSize {
    fn default() -> Self {
        MessageSize::DEFAULT
    }
}

impl MessageSize {
    /// 14 px: the body's size.
    pub const DEFAULT: MessageSize = MessageSize(1);

    /// Every step, smallest first.
    pub fn all() -> impl Iterator<Item = MessageSize> {
        (0..SIZES.len()).map(MessageSize)
    }

    /// The step of `px`, if it is one.
    pub fn of_px(px: u16) -> Option<MessageSize> {
        SIZES.iter().position(|step| *step == px).map(MessageSize)
    }

    /// The step in logical pixels.
    pub fn px(self) -> u16 {
        SIZES[self.0]
    }

    fn factor(self) -> f32 {
        f32::from(self.px()) / TypeStyle::Message.size()
    }

    /// The size `style` is set at: a message's own styles (its words, its author, its time) grow with the step in
    /// proportion to their size at the default; every other style is its own.
    pub fn size(self, style: TypeStyle) -> f32 {
        if style.in_message() { style.size() * self.factor() } else { style.size() }
    }

    /// The height of one line of `style`, grown as [`MessageSize::size`] grows it, in whole pixels.
    pub fn line_height(self, style: TypeStyle) -> f32 {
        if style.in_message() { (style.line_height() * self.factor()).round() } else { style.line_height() }
    }
}

/// A step of the space above a run's first message; the default is 16 px.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RunGap(usize);

impl Default for RunGap {
    fn default() -> Self {
        RunGap::DEFAULT
    }
}

impl RunGap {
    /// 16 px.
    pub const DEFAULT: RunGap = RunGap(3);

    /// Every step, least first.
    pub fn all() -> impl Iterator<Item = RunGap> {
        (0..GAPS.len()).map(RunGap)
    }

    /// The step of `px`, if it is one.
    pub fn of_px(px: u16) -> Option<RunGap> {
        GAPS.iter().position(|step| *step == px).map(RunGap)
    }

    /// The step in logical pixels.
    pub fn px(self) -> u16 {
        GAPS[self.0]
    }

    /// The space itself.
    pub fn space(self) -> f32 {
        f32::from(self.px())
    }
}

/// How a text channel's messages are drawn on this device: their text size and the space between their runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MessageScale {
    /// The text size.
    pub size: MessageSize,
    /// The space above a run's first message.
    pub gap: RunGap,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_steps_rise_and_start_at_discords_defaults() {
        let sizes: Vec<u16> = MessageSize::all().map(MessageSize::px).collect();
        let gaps: Vec<u16> = RunGap::all().map(RunGap::px).collect();
        assert!(sizes.windows(2).all(|pair| pair[0] < pair[1]), "{sizes:?}");
        assert!(gaps.windows(2).all(|pair| pair[0] < pair[1]), "{gaps:?}");
        assert_eq!(f32::from(MessageScale::default().size.px()), TypeStyle::Message.size());
        assert_eq!(MessageScale::default().gap.space(), 16.0);
        assert_eq!(MessageSize::of_px(18).map(MessageSize::px), Some(18));
        assert_eq!((MessageSize::of_px(13), RunGap::of_px(12)), (None, None));
        assert_eq!(RunGap::of_px(0).map(RunGap::space), Some(0.0));
    }

    #[test]
    fn a_messages_styles_grow_with_its_size_and_keep_their_leading_while_others_stay() {
        for size in MessageSize::all() {
            let px = f32::from(size.px());
            assert_eq!(size.size(TypeStyle::Message), px);
            assert_eq!(size.line_height(TypeStyle::Message), (px * 1.5).round(), "the default's 14 on 21");
            assert_eq!(size.size(TypeStyle::Author), px);
            assert!(size.line_height(TypeStyle::Stamp) >= size.size(TypeStyle::Stamp) * 1.25);
            assert_eq!(size.size(TypeStyle::Body), TypeStyle::Body.size());
            assert_eq!(size.line_height(TypeStyle::Caption), TypeStyle::Caption.line_height());
        }
        let default = MessageSize::DEFAULT;
        for style in TypeStyle::ALL {
            assert_eq!((default.size(style), default.line_height(style)), (style.size(), style.line_height()));
        }
    }
}
