//! What the popover about a screen's picture says: a watched screen's, as this viewer gets it, or this account's own,
//! as it goes out. A person who opens it asked, so it names the interface the picture went through.

use crate::call::health::{Outgoing, OwnWatch, Reduced, Reduction};
use crate::call::share::{Route, ShareCard, ShareState, SoundSaid, size_and_rate};
use words::Sentence;
use words::{Phrase, Words};

/// The popover's lines, top to bottom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct About {
    /// Whose screen it is, as a person reads it: the sharer's name, or this account's own.
    pub title: String,
    /// "1920 × 1080 · 60 fps", or the size alone until the rate is known.
    pub size_and_rate: String,
    /// "Scaled from 3840 × 2160", for one's own screen sent smaller than it is.
    pub scaled_from: Option<String>,
    /// "Reduced from 60 fps: 4 Mbit/s limit", for one's own screen sent below what was chosen; it takes the place of
    /// the scaled line.
    pub reduced: Option<String>,
    /// How the picture is made or taken apart, in words.
    pub path: String,
    /// The route the path says.
    pub route: Route,
    /// The line under the rest; none when there is nothing more to say.
    pub note: Option<Note>,
    /// What one's own share's sound is, once the call has said.
    pub sound: Option<String>,
    /// Whether that sound is carried, which its line's glyph says.
    pub sound_kind: LineKind,
    /// How often a watched picture froze, and the longest, once it has.
    pub froze: Option<String>,
    /// How many of a watched screen's frames were not decoded, once one was not.
    pub refused: Option<String>,
}

/// What a line of the popover is about, which the front end draws a glyph for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    /// The picture's size and rate.
    Size,
    /// A picture made or taken apart on the graphics hardware.
    Hardware,
    /// A picture made or taken apart on the processor.
    Software,
    /// A sound that is carried.
    Sound,
    /// A sound that is not carried.
    NoSound,
    /// Something that is going wrong.
    Warning,
    /// Anything else worth saying.
    Info,
    /// Why the line above is as it is: no glyph of its own, set under that line's words.
    Detail,
}

/// One line of the popover under its title: what it says and what it is about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// What the line is about.
    pub kind: LineKind,
    /// The line, as a person reads it.
    pub said: String,
}

/// One more line, when there is something to say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// Pictures a newer one replaced before this viewer painted them.
    Unpainted(u64),
    /// The engine holds this account's picture back for the link's sake.
    HeldBack,
    /// This computer cannot decode the watched stream as fast as it comes.
    Behind,
    /// Nobody watches this account's picture, so it sends none.
    NotSending,
}

impl Note {
    /// The line in `words`, its count as a figure.
    pub fn said(self, words: &Words) -> String {
        match self {
            Note::Unpainted(1) => words.phrase(Phrase::OneUnpainted).into_owned(),
            Note::Unpainted(pictures) => {
                format!("{} {}", words.count(pictures), words.phrase(Phrase::ShareDropped))
            }
            Note::HeldBack => words.phrase(Phrase::HeldBack).into_owned(),
            Note::Behind => words.phrase(Phrase::CantKeepUpNote).into_owned(),
            Note::NotSending => words.phrase(Phrase::NotSendingNote).into_owned(),
        }
    }

    /// Whether the line is a warning rather than a plain fact.
    pub fn warns(self) -> bool {
        matches!(self, Note::HeldBack | Note::Behind)
    }
}

impl About {
    /// A path that fell back to the processor, which the popover marks.
    pub fn software(&self) -> bool {
        ShareCard::software(&self.route)
    }

    /// A watched screen of the person shown as `name`, as this viewer decodes and paints it.
    pub fn watched(card: &ShareCard, name: &str, words: &Words) -> Self {
        Self {
            title: words.say(&Sentence::ScreenOf(name.to_string())),
            size_and_rate: card.size_and_rate(words),
            scaled_from: None,
            reduced: None,
            path: path(&card.decoder, Phrase::HardwareDecoded, Phrase::SoftwareDecoded, words),
            route: card.decoder.clone(),
            note: if card.state == ShareState::Behind {
                Some(Note::Behind)
            } else {
                (card.dropped > 0).then_some(Note::Unpainted(card.dropped))
            },
            sound: None,
            sound_kind: LineKind::NoSound,
            froze: (card.freezes.0 > 0)
                .then(|| words.say(&Sentence::Froze { times: card.freezes.0, longest: card.freezes.1 })),
            refused: (card.refused > 0).then(|| words.say(&Sentence::NotDecoded(card.refused))),
        }
    }

    /// This account's own screen, as it is encoded and sent to viewers, with its sound.
    pub fn sent(outgoing: &Outgoing, sound: Option<&SoundSaid>, words: &Words) -> Self {
        Self::sent_while(OwnWatch::Watched, outgoing, sound, words)
    }

    /// This account's own screen as [`About::sent`] says it, or, while nobody `watch`es it, its size with no rate and the
    /// note that it sends nothing.
    pub fn sent_while(watch: OwnWatch, outgoing: &Outgoing, sound: Option<&SoundSaid>, words: &Words) -> Self {
        Self::own(Phrase::YourScreen, watch, outgoing, sound, words)
    }

    /// This account's own camera as it is encoded and sent, in the lines of [`About::sent_while`] with no sound.
    pub fn sent_camera(watch: OwnWatch, outgoing: &Outgoing, words: &Words) -> Self {
        Self::own(Phrase::StatsYourCamera, watch, outgoing, None, words)
    }

    fn own(title: Phrase, watch: OwnWatch, outgoing: &Outgoing, sound: Option<&SoundSaid>, words: &Words) -> Self {
        let rate = match watch {
            OwnWatch::Watched => outgoing.frame_rate,
            OwnWatch::Unwatched => None,
        };
        let note = match watch {
            OwnWatch::Watched => outgoing.limited.then_some(Note::HeldBack),
            OwnWatch::Unwatched => Some(Note::NotSending),
        };
        Self {
            title: words.phrase(title).into_owned(),
            size_and_rate: size_and_rate(outgoing.width, outgoing.height, rate, words),
            scaled_from: outgoing.scaled_from.map(|(width, height)| words.say(&Sentence::ScaledFrom { width, height })),
            reduced: outgoing.reduced.map(|reduced| reduced_said(reduced, words)),
            path: path(&outgoing.encoder, Phrase::HardwareEncoded, Phrase::SoftwareEncoded, words),
            route: outgoing.encoder.clone(),
            note,
            sound: sound.map(|sound| sound.line(words)),
            sound_kind: sound.map_or(LineKind::NoSound, SoundSaid::kind),
            froze: None,
            refused: None,
        }
    }

    /// The popover's lines under its title, top to bottom, each with what it is about.
    pub fn lines(&self, words: &Words) -> Vec<Line> {
        let line = |kind, said: String| Line { kind, said };
        let note = self.note.map(|note| {
            let kind = if note.warns() { LineKind::Warning } else { LineKind::Info };
            line(kind, note.said(words))
        });
        let route = if self.software() { LineKind::Software } else { LineKind::Hardware };
        [line(LineKind::Size, self.size_and_rate.clone())]
            .into_iter()
            .chain(self.reduced.clone().or_else(|| self.scaled_from.clone()).map(|said| line(LineKind::Detail, said)))
            .chain([line(route, self.path.clone())])
            .chain(self.sound.clone().map(|said| line(self.sound_kind, said)))
            .chain(self.froze.clone().map(|said| line(LineKind::Warning, said)))
            .chain(self.refused.clone().map(|said| line(LineKind::Warning, said)))
            .chain(note)
            .collect()
    }
}

/// Why the share sends less than it was chosen to, naming what it was chosen to send.
fn reduced_said(reduced: Reduced, words: &Words) -> String {
    let from = match reduced.from_size {
        Some((width, height)) => size_and_rate(width, height, Some(reduced.from_frame_rate), words),
        None => words.fps(reduced.from_frame_rate),
    };
    words.say(&match reduced.cause {
        Reduction::Ceiling { bits_per_second } => Sentence::ReducedByCeiling { from, bits_per_second },
        Reduction::Busy { ceiling: Some(bits_per_second) } => Sentence::ReducedByContent { from, bits_per_second },
        Reduction::Busy { ceiling: None } => Sentence::ReducedByConnection(from),
        Reduction::Encoder => Sentence::ReducedByEncoder(from),
    })
}

/// A protocol version as the About pages state it: "1.0".
pub fn protocol_said(version: proto::chatkord_v1::ProtoVersion) -> String {
    format!("{}.{}", version.major, version.minor)
}

/// The connection popover's Encoder value: "Hardware · VA-API". An encoder nobody tested is said so only where a
/// person asked about their screen ([`About`]).
pub fn encoder_row(route: &Route, words: &Words) -> String {
    let said = match route {
        Route::UntestedHardware { via } => Route::Hardware { via: via.clone() },
        Route::Hardware { .. } | Route::Software => route.clone(),
    };
    path(&said, Phrase::Hardware, Phrase::Software, words)
}

/// `hardware` with the interface after it ("Hardware decoded · VA-API"), said untested where nobody tested the
/// encoder ("Hardware encoded, untested · D3D11"), or `software` alone.
fn path(route: &Route, hardware: Phrase, software: Phrase, words: &Words) -> String {
    match route {
        Route::Hardware { via } => format!("{} \u{b7} {via}", words.phrase(hardware)),
        Route::UntestedHardware { via } => {
            format!("{}, {} \u{b7} {via}", words.phrase(hardware), words.phrase(Phrase::Untested))
        }
        Route::Software => words.phrase(software).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call::health::OwnWatch;
    use crate::call::share::ShareState;
    use crate::harness::member;

    /// Ada's user id, which is not her name.
    const ADA: &str = "2";

    fn card(dropped: u64, decoder: Route) -> ShareCard {
        ShareCard {
            who: member(ADA),
            width: 1920,
            height: 1080,
            frame_rate: Some(60),
            dropped,
            codec: "H.264".into(),
            decoder,
            refused: 0,
            freezes: (0, std::time::Duration::ZERO),
            state: ShareState::Live,
        }
    }

    fn outgoing(limited: bool, frame_rate: Option<u32>) -> Outgoing {
        Outgoing {
            codec: "H.264".into(),
            encoder: Route::Hardware { via: "VA-API".into() },
            width: 1920,
            height: 1080,
            scaled_from: None,
            frame_rate,
            bits_per_second: 8_000_000,
            frames: 212_604,
            key_frames_made: 0,
            key_frames_asked: 0,
            resent_bytes: 0,
            restarts: 0,
            limited,
            reduced: None,
        }
    }

    fn plain(said: Option<&str>) -> Option<String> {
        said.map(words::without_isolation)
    }

    #[test]
    fn a_watched_screen_says_its_freezes_and_frames_not_decoded_only_when_there_are_some() {
        let words = Words::english();
        let calm = About::watched(&card(0, Route::Software), "Ada", &words);
        assert_eq!((calm.froze, calm.refused), (None, None));
        let rough =
            ShareCard { refused: 3, freezes: (2, std::time::Duration::from_millis(900)), ..card(0, Route::Software) };
        let rough = About::watched(&rough, "Ada", &words);
        assert_eq!(plain(rough.froze.as_deref()).as_deref(), Some("Froze 2 times, the longest 0.9 s"));
        assert_eq!(plain(rough.refused.as_deref()).as_deref(), Some("3 frames not decoded"));
        let once =
            ShareCard { refused: 1, freezes: (1, std::time::Duration::from_secs(2)), ..card(0, Route::Software) };
        let once = About::watched(&once, "Ada", &words);
        assert_eq!(
            (plain(once.froze.as_deref()).as_deref(), plain(once.refused.as_deref()).as_deref()),
            (Some("Froze once, the longest 2.0 s"), Some("1 frame not decoded"))
        );
    }

    #[test]
    fn a_watched_screen_says_how_it_is_drawn_and_the_pictures_it_did_not_paint_only_when_there_are_some() {
        let about = About::watched(&card(0, Route::Hardware { via: "VA-API".into() }), "Ada", &Words::english());
        assert_eq!(
            (
                words::without_isolation(&about.title).as_str(),
                about.size_and_rate.as_str(),
                about.path.as_str(),
                about.note
            ),
            ("Ada’s stream", "1920 \u{d7} 1080 \u{b7} 60 fps", "Hardware decoded \u{b7} VA-API", None)
        );
        let dropped = About::watched(&card(1_234, Route::Software), "Ada", &Words::english());
        assert_eq!(
            dropped.note.map(|note| note.said(&Words::english())).as_deref(),
            Some("1,234 pictures not painted")
        );
        assert!(dropped.software() && !Note::Unpainted(1).warns());
        assert_eq!(dropped.path, "Software decoded", "a fallback names no interface");
        let one = About::watched(&card(1, Route::Software), "Ada", &Words::english());
        assert_eq!(one.note.map(|note| note.said(&Words::english())).as_deref(), Some("1 picture not painted"));
    }

    #[test]
    fn a_watched_stream_this_computer_cannot_keep_up_with_warns_before_its_unpainted_pictures() {
        let behind = ShareCard { state: ShareState::Behind, ..card(37, Route::Software) };
        let about = About::watched(&behind, "Ada", &Words::english());
        let note = about.note.expect("a note");
        assert_eq!(note.said(&Words::english()), "This computer can’t keep up with this stream");
        assert!(note.warns(), "said as a warning, as a held-back share is");
    }

    #[test]
    fn ones_own_screen_names_its_encoder_and_warns_while_held_back() {
        let about = About::sent(&outgoing(false, Some(60)), None, &Words::english());
        assert_eq!(
            (about.title.as_str(), about.size_and_rate.as_str(), about.path.as_str(), about.note),
            ("Your stream", "1920 \u{d7} 1080 \u{b7} 60 fps", "Hardware encoded \u{b7} VA-API", None)
        );
        assert_eq!(about.sound, None, "no sound line before the call said what it is");
        let brave = SoundSaid::App("Brave".into());
        assert_eq!(
            plain(About::sent(&outgoing(false, Some(60)), Some(&brave), &Words::english()).sound.as_deref()).as_deref(),
            Some("Sound: Brave")
        );
        let held = About::sent(&outgoing(true, None), None, &Words::english());
        assert_eq!(held.size_and_rate, "1920 \u{d7} 1080", "no rate until one was measured");
        assert_eq!(held.note, Some(Note::HeldBack));
        assert!(Note::HeldBack.warns());
        assert_eq!(Note::HeldBack.said(&Words::english()), "Held back by your connection");
    }

    #[test]
    fn ones_own_camera_names_its_encoder_and_warns_while_held_back() {
        let words = Words::english();
        let outgoing = Outgoing { width: 1280, height: 720, ..outgoing(false, Some(30)) };
        let about = About::sent_camera(OwnWatch::Watched, &outgoing, &words);
        assert_eq!(
            (about.title.as_str(), about.size_and_rate.as_str(), about.path.as_str(), about.note),
            ("Your camera", "1280 \u{d7} 720 \u{b7} 30 fps", "Hardware encoded \u{b7} VA-API", None)
        );
        assert_eq!((about.sound, about.scaled_from), (None, None), "a camera has no sound line nor a scaled screen");
        let held = About::sent_camera(OwnWatch::Watched, &Outgoing { limited: true, ..outgoing }, &words);
        let lines = held.lines(&words).into_iter().map(|line| (line.kind, line.said)).collect::<Vec<_>>();
        assert_eq!(lines.last(), Some(&(LineKind::Warning, "Held back by your connection".to_string())));
    }

    #[test]
    fn ones_own_screen_sent_smaller_says_the_size_it_was_scaled_from() {
        assert_eq!(
            About::sent(&outgoing(false, Some(60)), None, &Words::english()).scaled_from,
            None,
            "a screen sent as it is says nothing more"
        );
        let scaled = About::sent(
            &Outgoing { scaled_from: Some((3840, 2160)), ..outgoing(false, Some(60)) },
            None,
            &Words::english(),
        );
        assert_eq!(scaled.size_and_rate, "1920 \u{d7} 1080 \u{b7} 60 fps", "the size sent comes first");
        assert_eq!(plain(scaled.scaled_from.as_deref()).as_deref(), Some("Scaled from 3840 \u{d7} 2160"));
    }

    #[test]
    fn ones_own_screen_sent_below_what_was_chosen_says_why_in_one_muted_line_under_its_size() {
        let words = Words::english();
        let said = |reduced: Option<Reduced>, scaled_from| {
            let sent = Outgoing { reduced, scaled_from, width: 3840, height: 2160, ..outgoing(false, Some(15)) };
            let lines = About::sent(&sent, None, &words).lines(&words);
            (lines[0].kind, lines[1].kind, lines[1].said.clone())
        };
        let reduced = |from_size, cause| Some(Reduced { from_size, from_frame_rate: 60, cause });
        let ceiling = reduced(None, Reduction::Ceiling { bits_per_second: 4_000_000 });
        let (size, kind, line) = said(ceiling, None);
        assert_eq!((size, kind), (LineKind::Size, LineKind::Detail), "muted like the scaled line, never amber");
        assert_eq!(words::without_isolation(&line), "Reduced from 60 fps: 4 Mbit/s limit");
        let with_size = reduced(Some((3840, 2160)), Reduction::Ceiling { bits_per_second: 4_000_000 });
        assert_eq!(
            words::without_isolation(&said(with_size, None).2),
            "Reduced from 3840 \u{d7} 2160 \u{b7} 60 fps: 4 Mbit/s limit"
        );
        let busy = reduced(None, Reduction::Busy { ceiling: Some(8_000_000) });
        assert_eq!(words::without_isolation(&said(busy, None).2), "Reduced from 60 fps: content exceeds 8 Mbit/s");
        let automatic = reduced(None, Reduction::Busy { ceiling: None });
        assert_eq!(
            words::without_isolation(&said(automatic, None).2),
            "Reduced from 60 fps: content exceeds the connection"
        );
        let slow = reduced(None, Reduction::Encoder);
        assert_eq!(words::without_isolation(&said(slow, None).2), "Reduced from 60 fps: encoder overloaded");
        let both = said(slow, Some((3840, 2160)));
        assert_eq!(
            words::without_isolation(&both.2),
            "Reduced from 60 fps: encoder overloaded",
            "it replaces the scaled line"
        );
        let none = About::sent(&outgoing(false, Some(60)), None, &words);
        assert_eq!(none.reduced, None, "a share sending what was chosen says nothing more");
    }

    #[test]
    fn the_encoder_row_names_the_interface() {
        assert_eq!(encoder_row(&Route::Hardware { via: "Vulkan".into() }, &Words::english()), "Hardware \u{b7} Vulkan");
    }

    #[test]
    fn an_encoder_nobody_tested_is_said_untested_only_on_about_your_screen() {
        let untested = Route::UntestedHardware { via: "D3D11".into() };
        assert_eq!(
            encoder_row(&untested, &Words::english()),
            "Hardware \u{b7} D3D11",
            "the popover's has no word of it"
        );
        let words = &Words::english();
        let said = path(&untested, Phrase::HardwareEncoded, Phrase::SoftwareEncoded, words);
        assert_eq!(said, "Hardware encoded, untested \u{b7} D3D11", "as the canvas's voice.share.info.untested");
    }

    #[test]
    fn ones_own_screen_nobody_watches_keeps_its_lines_with_no_rate_and_a_note_that_it_sends_nothing() {
        let words = Words::english();
        let sound = SoundSaid::Everything;
        let rested = About::sent_while(OwnWatch::Unwatched, &outgoing(true, Some(60)), Some(&sound), &words);
        let lines: Vec<_> = rested.lines(&words).into_iter().map(|line| (line.kind, line.said)).collect();
        assert_eq!(
            lines,
            [
                (LineKind::Size, "1920 \u{d7} 1080".to_string()),
                (LineKind::Hardware, rested.path.clone()),
                (sound.kind(), sound.line(&words)),
                (LineKind::Info, "Not sending: no viewers.".to_string())
            ]
        );
        let watched = About::sent_while(OwnWatch::Watched, &outgoing(false, Some(60)), None, &words);
        assert_eq!(watched.note, None);
        assert_eq!(watched.size_and_rate, "1920 \u{d7} 1080 \u{b7} 60 fps");
    }

    #[test]
    fn each_line_of_the_popover_says_what_it_is_about_for_the_front_end_to_draw_a_glyph() {
        let words = Words::english();
        let kinds = |about: &About| about.lines(&words).into_iter().map(|line| line.kind).collect::<Vec<_>>();
        let sent = About::sent(&outgoing(true, Some(60)), Some(&SoundSaid::App("Brave".into())), &words);
        assert_eq!(kinds(&sent), [LineKind::Size, LineKind::Hardware, LineKind::Sound, LineKind::Warning]);
        let silent = About::sent(&outgoing(false, Some(60)), Some(&SoundSaid::Own), &words);
        assert_eq!(kinds(&silent), [LineKind::Size, LineKind::Hardware, LineKind::NoSound]);
        let behind = ShareCard { state: ShareState::Behind, refused: 2, ..card(5, Route::Software) };
        let watched = About::watched(&behind, "Ada", &words);
        assert_eq!(kinds(&watched), [LineKind::Size, LineKind::Software, LineKind::Warning, LineKind::Warning]);
        let plain = About::watched(&card(5, Route::Software), "Ada", &words);
        assert_eq!(kinds(&plain), [LineKind::Size, LineKind::Software, LineKind::Info]);
    }

    #[test]
    fn a_protocol_version_is_said_as_major_dot_minor() {
        assert_eq!(protocol_said(proto::chatkord_v1::ProtoVersion { major: 1, minor: 12 }), "1.12");
        let this = proto::envelope::PROTO;
        assert_eq!(protocol_said(this), format!("{}.{}", this.major, this.minor));
    }
}
