//! A colour picked freely: a server's, by an admin, and a person's own. The picker keeps hue,
//! saturation and brightness, so the hue survives a trip through grey or black; what it hands on is `#RRGGBB`. The
//! one conversion from the wire's colour to the one a screen draws is here too.

use proto::rules::Colour;
use terms::LinkError;

/// The colour a screen draws for one the wire carries.
pub fn rgb(colour: Colour) -> design::Rgb {
    let [r, g, b] = colour.rgb();
    design::Rgb::new(r, g, b)
}

/// Hue in degrees, `0..360`; saturation and brightness, `0..=1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsv {
    /// The hue, in degrees.
    pub hue: f64,
    /// From grey at 0 to the full hue at 1.
    pub saturation: f64,
    /// The brightness, from black at 0 to full at 1.
    pub value: f64,
}

impl Hsv {
    fn of(colour: Colour) -> Hsv {
        let [r, g, b] = colour.rgb().map(|channel| f64::from(channel) / 255.0);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let delta = max - min;
        let sector = if delta == 0.0 {
            0.0
        } else if max == r {
            ((g - b) / delta).rem_euclid(6.0)
        } else if max == g {
            (b - r) / delta + 2.0
        } else {
            (r - g) / delta + 4.0
        };
        Hsv { hue: sector * 60.0, saturation: if max == 0.0 { 0.0 } else { delta / max }, value: max }
    }

    /// The colour it is.
    pub fn colour(self) -> Colour {
        let channel = |n: f64| {
            let k = (n + self.hue / 60.0).rem_euclid(6.0);
            let level = self.value - self.value * self.saturation * k.min(4.0 - k).clamp(0.0, 1.0);
            // In 0..=1 by construction, so the cast cannot leave a u8.
            #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "clamped to 0..=255 first")]
            let byte = (level * 255.0).round().clamp(0.0, 255.0) as u8;
            byte
        };
        Colour::new(channel(5.0), channel(3.0), channel(1.0))
    }
}

/// What the person does with the picker.
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    /// The swatch was pressed: the picker opens, or closes as Done would.
    Toggle,
    /// A point of the saturation and brightness area, each as a fraction of its side: left to right more colour, top to
    /// bottom less light.
    Area {
        /// Across, from the left edge at 0; past an edge is the edge.
        x: f64,
        /// Down, from the top edge at 0; past an edge is the edge.
        y: f64,
    },
    /// A point of the hue strip, as a fraction of its length.
    Hue(f64),
    /// The Hex field's text.
    Typed(String),
    /// Back to the colour of one that picked none.
    Reset,
    /// Done, Enter in Hex, Escape or leaving the page: the picker closes, and a change is kept.
    Done,
}

/// The picker, open or closed, over the colour as last saved.
#[derive(Clone, Debug, PartialEq)]
pub struct Picker {
    /// The pick as last saved; none for "picked none".
    saved: Option<Colour>,
    /// The colour of one that picked none: Chatkord's, or the account's swatch.
    fallback: Colour,
    hsv: Hsv,
    typed: String,
    open: bool,
    /// What `saved` was before each change asked and not yet answered, the first asked first: where to go back to when
    /// the server refuses one.
    unanswered: Vec<Option<Colour>>,
}

impl Picker {
    /// A closed picker on `saved`, or on `fallback` when none was picked.
    pub fn new(saved: Option<Colour>, fallback: Colour) -> Picker {
        let shown = saved.unwrap_or(fallback);
        Picker { saved, fallback, hsv: Hsv::of(shown), typed: shown.to_string(), open: false, unanswered: Vec::new() }
    }

    /// The colour as it stands, picked or not yet saved.
    pub fn picked(&self) -> Colour {
        self.hsv.colour()
    }

    /// Where the area's point and the strip's stand: the hue kept through a grey or black.
    pub fn hsv(&self) -> Hsv {
        self.hsv
    }

    /// The Hex field's text.
    pub fn typed(&self) -> &str {
        &self.typed
    }

    /// Whether the Hex field's text is not a colour, so the field says how one is written.
    pub fn typed_refused(&self) -> bool {
        Colour::parse(&self.typed).is_err()
    }

    /// Whether the picker shows its area, strip and Hex field below the swatch.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Whether it stands on the colour of one that picked none.
    pub fn is_fallback(&self) -> bool {
        self.picked() == self.fallback
    }

    /// What to save, when a change is kept: the pick, or none when it is the fallback again.
    pub fn update(&mut self, message: Message) -> Option<Option<Colour>> {
        match message {
            Message::Toggle if self.open => return self.done(),
            Message::Toggle => self.open = true,
            Message::Area { x, y } => {
                self.set(Hsv { saturation: x.clamp(0.0, 1.0), value: 1.0 - y.clamp(0.0, 1.0), ..self.hsv });
            }
            Message::Hue(along) => self.set(Hsv { hue: along.clamp(0.0, 1.0) * 360.0 % 360.0, ..self.hsv }),
            Message::Typed(typed) => {
                if let Ok(colour) = Colour::parse(&typed) {
                    let hsv = Hsv::of(colour);
                    // A grey or black has no hue of its own: the one the person was on stays.
                    let hue = if hsv.saturation == 0.0 || hsv.value == 0.0 { self.hsv.hue } else { hsv.hue };
                    self.hsv = Hsv { hue, ..hsv };
                }
                self.typed = typed;
            }
            Message::Reset => self.set(Hsv::of(self.fallback)),
            Message::Done => return self.done(),
        }
        None
    }

    /// The server or the account said the colour changed, here or on another device: a closed picker follows it.
    pub fn follow(&mut self, saved: Option<Colour>) {
        self.saved = saved;
        self.unanswered.clear();
        if !self.open {
            *self = Picker::new(saved, self.fallback);
        }
    }

    fn set(&mut self, hsv: Hsv) {
        self.hsv = hsv;
        self.typed = hsv.colour().to_string();
    }

    fn done(&mut self) -> Option<Option<Colour>> {
        self.open = false;
        let picked = self.picked();
        self.typed = picked.to_string();
        let kept = (picked != self.fallback).then_some(picked);
        (kept != self.saved).then(|| {
            self.unanswered.push(self.saved);
            self.saved = kept;
            kept
        })
    }

    /// The server's answer to the change last kept: a refusal puts the picker back, closed, on the colour it had before
    /// that change.
    pub(crate) fn answered(&mut self, answer: &Result<(), LinkError>) {
        if self.unanswered.is_empty() {
            return;
        }
        let before = self.unanswered.remove(0);
        if answer.is_err() {
            *self = Picker::new(before, self.fallback);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CORAL: Colour = Colour::new(0xFF, 0x8A, 0x5B);

    fn colour(text: &str) -> Colour {
        Colour::parse(text).expect("a colour")
    }

    #[test]
    fn every_colour_survives_the_trip_through_hue_saturation_and_brightness() {
        for value in (0..=0xFF_FF_FF_u32).step_by(0x01_03_05) {
            let [_, r, g, b] = value.to_be_bytes();
            let colour = Colour::new(r, g, b);
            assert_eq!(Hsv::of(colour).colour(), colour, "{colour}");
        }
    }

    #[test]
    fn a_pick_is_kept_on_done_and_the_fallback_again_is_none() {
        let mut picker = Picker::new(None, CORAL);
        assert_eq!(picker.update(Message::Toggle), None);
        assert!(picker.is_open());
        picker.update(Message::Typed("#3d7fd6".into()));
        assert_eq!(picker.picked(), colour("#3D7FD6"));
        assert_eq!(picker.update(Message::Done), Some(Some(colour("#3D7FD6"))));
        assert!(!picker.is_open());
        assert_eq!(picker.update(Message::Done), None, "nothing new to keep");
        picker.update(Message::Toggle);
        picker.update(Message::Reset);
        assert_eq!(picker.update(Message::Toggle), Some(None), "the fallback again is none picked");
    }

    #[test]
    fn a_refused_pick_puts_the_picker_back_on_the_colour_it_had_and_an_accepted_one_stays() {
        let refused = Err(LinkError::Refused(proto::chatkord_v1::ErrorCode::NotAuthorized));
        let mut picker = Picker::new(Some(colour("#FF0000")), CORAL);
        picker.update(Message::Toggle);
        picker.update(Message::Typed("#3d7fd6".into()));
        assert_eq!(picker.update(Message::Done), Some(Some(colour("#3D7FD6"))));
        picker.answered(&refused);
        assert_eq!((picker.picked(), picker.typed(), picker.is_open()), (colour("#FF0000"), "#FF0000", false));
        assert_eq!(picker.update(Message::Done), None, "the colour it is back on is the one the server holds");
        picker.update(Message::Toggle);
        picker.update(Message::Typed("#3d7fd6".into()));
        picker.update(Message::Done);
        picker.answered(&Ok(()));
        picker.answered(&refused);
        assert_eq!(picker.picked(), colour("#3D7FD6"), "an answer already taken is not taken back");
    }

    #[test]
    fn the_area_and_the_strip_move_the_colour_and_a_grey_keeps_the_hue() {
        let mut picker = Picker::new(Some(colour("#FF0000")), CORAL);
        picker.update(Message::Hue(1.0 / 3.0));
        assert_eq!(picker.picked(), colour("#00FF00"));
        picker.update(Message::Area { x: 0.0, y: 0.5 });
        assert_eq!(picker.typed(), "#808080");
        picker.update(Message::Area { x: 1.0, y: 0.0 });
        assert_eq!(picker.picked(), colour("#00FF00"), "the hue survived the grey");
        picker.update(Message::Typed("#000000".into()));
        picker.update(Message::Area { x: 1.0, y: 0.0 });
        assert_eq!(picker.picked(), colour("#00FF00"), "and black typed in");
        picker.update(Message::Area { x: 2.0, y: -1.0 });
        assert_eq!(picker.picked(), colour("#00FF00"), "a point past the edge is the edge");
    }

    #[test]
    fn a_hex_out_of_shape_is_said_and_moves_nothing() {
        let mut picker = Picker::new(None, CORAL);
        picker.update(Message::Typed("#3d7f".into()));
        assert!(picker.typed_refused());
        assert_eq!(picker.picked(), CORAL);
        assert_eq!(picker.update(Message::Done), None);
        assert_eq!(picker.typed(), "#FF8A5B", "closing shows what is kept");
    }

    #[test]
    fn a_closed_picker_follows_a_change_from_elsewhere_and_an_open_one_does_not() {
        let mut picker = Picker::new(None, CORAL);
        picker.follow(Some(colour("#3D7FD6")));
        assert_eq!(picker.picked(), colour("#3D7FD6"));
        picker.update(Message::Toggle);
        picker.update(Message::Hue(0.0));
        let held = picker.picked();
        picker.follow(Some(colour("#112233")));
        assert_eq!(picker.picked(), held);
    }
}
