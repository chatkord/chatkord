//! A message's text as it is shown: the plain text, the marks beside it, and the styled runs the two make.

use std::ops::Range;

use crate::mark::{Mark, MarkKind, MarksRefused, byte_ranges};
use crate::markdown::{Piece, Style, pieces};

/// A message's text with its marks, its markdown read once when it is made, so a client that draws no style still
/// shows every word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Body {
    text: String,
    marks: Vec<Mark>,
    pieces: Vec<Piece>,
}

/// What a run is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunKind<'a> {
    /// Text, its markers already left out.
    Plain,
    /// Part or all of a mark's text: a mention, a role, everyone, a channel, a custom emoji's `:name:`, a link.
    Mark(&'a MarkKind),
}

/// A stretch of a body drawn one way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run<'a> {
    /// Its text.
    pub text: &'a str,
    /// How it is drawn.
    pub style: Style,
    /// What it is.
    pub kind: RunKind<'a>,
}

impl Body {
    /// A body of text with no marks; its markdown still styles it.
    pub fn plain(text: impl Into<String>) -> Body {
        Body::trusted(text.into(), Vec::new(), &[])
    }

    /// A body of text and its marks, which must lie in it sorted and apart.
    pub fn new(text: impl Into<String>, marks: Vec<Mark>) -> Result<Body, MarksRefused> {
        let text = text.into();
        let bytes = byte_ranges(&text, &marks)?;
        Ok(Body::trusted(text, marks, &bytes))
    }

    /// A body of marks already checked against the text, at `bytes`.
    pub(crate) fn trusted(text: String, marks: Vec<Mark>, bytes: &[Range<usize>]) -> Body {
        let pieces = pieces(&text, bytes);
        Body { text, marks, pieces }
    }

    /// The text as it is shown, markers left out, on one line: whitespace runs, line breaks among them, become one space.
    pub fn shown(&self) -> String {
        let flat: String = self.runs().map(|run| run.text).collect();
        flat.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// The first line of the text as it is shown, markers left out, trimmed; `None` where every line is blank.
    pub fn first_line_shown(&self) -> Option<String> {
        let flat: String = self.runs().map(|run| run.text).collect();
        flat.lines().map(str::trim).find(|line| !line.is_empty()).map(str::to_owned)
    }

    /// The text as written, every marker and every mark's text in it.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Its marks, sorted and apart.
    pub fn marks(&self) -> &[Mark] {
        &self.marks
    }

    /// The text of each link mark, in order.
    pub fn links(&self) -> impl Iterator<Item = &str> {
        self.runs().filter(|run| matches!(run.kind, RunKind::Mark(MarkKind::Link))).map(|run| run.text)
    }

    /// The stretches to draw, in order: markdown's markers left out, each emoji alone, a mark never split by style.
    pub fn runs(&self) -> impl Iterator<Item = Run<'_>> {
        self.pieces.iter().filter_map(|piece| {
            let kind = match piece.mark {
                Some(mark) => RunKind::Mark(&self.marks.get(mark)?.kind),
                None => RunKind::Plain,
            };
            Some(Run { text: self.text.get(piece.bytes.clone())?, style: piece.style, kind })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::Code;
    use terms::MemberId;

    const ADA: &str = "ada";

    #[test]
    fn runs_carry_a_mark_whole_inside_markdown() {
        let ada = MarkKind::Mention(MemberId::parse(ADA).unwrap());
        let body = Body::new("hi **@Ada_x_** `code` 🙂", vec![Mark { range: 5..12, kind: ada.clone() }]).unwrap();
        let runs: Vec<(&str, RunKind<'_>, bool, Option<Code>, bool)> =
            body.runs().map(|run| (run.text, run.kind, run.style.bold, run.style.code, run.style.emoji)).collect();
        assert_eq!(
            runs,
            vec![
                ("hi ", RunKind::Plain, false, None, false),
                ("@Ada_x_", RunKind::Mark(&ada), true, None, false),
                (" ", RunKind::Plain, false, None, false),
                ("code", RunKind::Plain, false, Some(Code::Inline), false),
                (" ", RunKind::Plain, false, None, false),
                ("🙂", RunKind::Plain, false, None, true),
            ]
        );
        assert_eq!(body.text(), "hi **@Ada_x_** `code` 🙂", "the text keeps every marker");
        assert_eq!(body.shown(), "hi @Ada_x_ code 🙂", "shown, the markers are gone");
    }

    #[test]
    fn shown_is_one_line() {
        assert_eq!(Body::plain("one\n\n  two   three").shown(), "one two three");
    }

    #[test]
    fn the_first_line_shown_leaves_the_markers_out_and_skips_blank_lines() {
        let body = Body::plain("\n  **bold** and `code` ||secret||\nsecond");
        assert_eq!(body.first_line_shown().as_deref(), Some("bold and code secret"));
        assert_eq!(Body::plain(" \n ").first_line_shown(), None);
    }

    #[test]
    fn marks_that_do_not_fit_the_text_are_refused() {
        let link = |range| Mark { range, kind: MarkKind::Link };
        assert_eq!(Body::new("abc", vec![link(0..9)]), Err(MarksRefused::OutOfRange));
        assert_eq!(Body::new("abc", vec![link(1..3), link(0..1)]), Err(MarksRefused::Overlapping));
    }
}
