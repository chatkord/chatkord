//! Discord's markdown: `**bold**`, `*italic*` and `_italic_`, `__underline__`, `~~strike~~`, `` `code` ``, fenced
//! blocks, `> ` quote lines, `||spoiler||` and `\` escapes. Our own small parser, since CommonMark differs on `__` and
//! has no spoiler: code is found first and is verbatim, marks are atoms whose markers are literal, and every marker
//! used is left out of the pieces drawn.

use std::ops::Range;

/// How a run is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Style {
    /// `**bold**`.
    pub bold: bool,
    /// `*italic*` or `_italic_`.
    pub italic: bool,
    /// `__underline__`.
    pub underline: bool,
    /// `~~strike~~`.
    pub strike: bool,
    /// In code, inline or a fenced block; none outside code.
    pub code: Option<Code>,
    /// Hidden until revealed, with every run of the same spoiler; none outside a spoiler.
    pub spoiler: Option<SpoilerId>,
    /// On a `> ` quote line.
    pub quote: bool,
    /// One emoji, drawn in the emoji font.
    pub emoji: bool,
}

/// Which code a run is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Code {
    /// `` `inline` ``, in the line.
    Inline,
    /// A fenced block of its own lines; its language word is not shown.
    Block,
}

/// One spoiler of a body: every run that carries it is revealed together.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SpoilerId(usize);

/// A stretch of the text drawn one way: its bytes, its style, and the mark it is part of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Piece {
    pub(crate) bytes: Range<usize>,
    pub(crate) style: Style,
    pub(crate) mark: Option<usize>,
}

#[derive(Clone, Debug)]
enum Token {
    Text(Range<usize>),
    Delim { at: usize, ch: char },
    Code(Range<usize>),
    Block(Range<usize>),
    Mark(usize),
}

/// The pieces of `text` to draw in order, `marks` its marks in bytes, sorted and apart.
pub(crate) fn pieces(text: &str, marks: &[Range<usize>]) -> Vec<Piece> {
    let tokens = tokens(text, marks);
    let mut out = Out { text, marks, pieces: Vec::new(), spoilers: 0 };
    let mut from = 0;
    while from < tokens.len() {
        let quote = tokens.get(from).is_some_and(|(_, quote)| *quote);
        if let Some((Token::Block(range), _)) = tokens.get(from) {
            out.range(range.clone(), Style { code: Some(Code::Block), quote, ..Style::default() });
            from += 1;
            continue;
        }
        let to = tokens
            .iter()
            .enumerate()
            .skip(from)
            .find(|(_, (token, q))| *q != quote || matches!(token, Token::Block(_)))
            .map_or(tokens.len(), |(at, _)| at);
        let segment: Vec<Token> = tokens.get(from..to).unwrap_or_default().iter().map(|(t, _)| t.clone()).collect();
        out.inline(&segment, 0, segment.len(), Style { quote, ..Style::default() });
        from = to;
    }
    out.pieces
}

fn char_at(text: &str, at: usize) -> Option<char> {
    text.get(at..).and_then(|rest| rest.chars().next())
}

fn char_before(text: &str, at: usize) -> Option<char> {
    text.get(..at).and_then(|head| head.chars().next_back())
}

fn language(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-' | '.' | '#')
}

struct Scan<'a> {
    text: &'a str,
    marks: &'a [Range<usize>],
}

impl Scan<'_> {
    fn mark_at(&self, at: usize) -> Option<usize> {
        self.marks.binary_search_by_key(&at, |mark| mark.start).ok()
    }

    /// The next byte at or after `at` outside every mark.
    fn outside(&self, at: usize) -> usize {
        let after = self.marks.partition_point(|mark| mark.end <= at);
        self.marks.get(after).filter(|mark| mark.start <= at).map_or(at, |mark| mark.end)
    }

    /// How many backticks start at `at`, stopping at a mark.
    fn ticks(&self, at: usize) -> usize {
        let bytes = self.text.as_bytes();
        let mut n = 0;
        while bytes.get(at + n) == Some(&b'`') && (n == 0 || self.mark_at(at + n).is_none()) {
            n += 1;
        }
        n
    }

    /// The first run of exactly `n` backticks at or after `from`, outside marks.
    fn closing_ticks(&self, from: usize, n: usize) -> Option<usize> {
        let mut at = self.outside(from);
        while at < self.text.len() {
            if self.text.as_bytes().get(at) == Some(&b'`') {
                let run = self.ticks(at);
                if run == n {
                    return Some(at);
                }
                at = self.outside(at + run);
            } else {
                at = self.outside(at + char_at(self.text, at).map_or(1, char::len_utf8));
            }
        }
        None
    }

    /// A fenced block opened by the three backticks at `at`: its shown content and where it ends.
    fn block(&self, at: usize) -> Option<(Range<usize>, usize)> {
        let text = self.text;
        let mut start = at + 3;
        let word_end = text.get(start..)?.find(|c: char| !language(c)).map_or(text.len(), |end| start + end);
        if word_end > start && text.as_bytes().get(word_end) == Some(&b'\n') {
            start = word_end + 1;
        }
        while text.as_bytes().get(start) == Some(&b'\n') {
            start += 1;
        }
        let close = self.fence(start).filter(|&close| close > start)?;
        let mut end = close;
        while end > start && text.as_bytes().get(end - 1) == Some(&b'\n') {
            end -= 1;
        }
        (end > start).then_some((start..end, close + 3))
    }

    /// The first three backticks at or after `from`, outside marks, which close a block as Discord's do.
    fn fence(&self, from: usize) -> Option<usize> {
        let mut at = self.outside(from);
        while at < self.text.len() {
            if self.text.get(at..).is_some_and(|rest| rest.starts_with("```")) {
                return Some(at);
            }
            at = self.outside(at + char_at(self.text, at).map_or(1, char::len_utf8));
        }
        None
    }
}

/// The text's tokens, each with whether its line is a quote.
fn tokens(text: &str, marks: &[Range<usize>]) -> Vec<(Token, bool)> {
    let scan = Scan { text, marks };
    let mut out: Vec<(Token, bool)> = Vec::new();
    let push_text = |out: &mut Vec<(Token, bool)>, range: Range<usize>, quote: bool| {
        if let Some((Token::Text(last), q)) = out.last_mut()
            && *q == quote
            && last.end == range.start
        {
            last.end = range.end;
            return;
        }
        out.push((Token::Text(range), quote));
    };
    let (mut at, mut line_start, mut quote) = (0, true, false);
    while let Some(c) = char_at(text, at) {
        if line_start {
            line_start = false;
            quote = text.get(at..).is_some_and(|rest| rest.starts_with("> ")) && scan.mark_at(at).is_none();
            if quote {
                at += 2;
                continue;
            }
        }
        if let Some(mark) = scan.mark_at(at) {
            out.push((Token::Mark(mark), quote));
            at = marks.get(mark).map_or(text.len(), |range| range.end);
            continue;
        }
        let next = char_at(text, at + c.len_utf8());
        match c {
            '\n' => {
                push_text(&mut out, at..at + 1, quote);
                line_start = true;
                at += 1;
            }
            '\\' if next.is_some_and(|n| n.is_ascii_punctuation()) && scan.mark_at(at + 1).is_none() => {
                push_text(&mut out, at + 1..at + 2, quote);
                at += 2;
            }
            '`' => {
                let run = scan.ticks(at);
                if let Some((content, end)) = (run >= 3).then(|| scan.block(at)).flatten() {
                    out.push((Token::Block(content), quote));
                    at = end;
                } else if let Some(close) = scan.closing_ticks(at + run, run).filter(|&close| close > at + run) {
                    out.push((Token::Code(at + run..close), quote));
                    at = close + run;
                } else {
                    push_text(&mut out, at..at + run, quote);
                    at += run;
                }
            }
            '*' if next.is_some_and(|n| n == '\u{FE0F}' || n == '\u{20E3}') => {
                push_text(&mut out, at..at + 1, quote);
                at += 1;
            }
            '*' | '_' | '~' | '|' => {
                out.push((Token::Delim { at, ch: c }, quote));
                at += 1;
            }
            _ => {
                push_text(&mut out, at..at + c.len_utf8(), quote);
                at += c.len_utf8();
            }
        }
    }
    out
}

struct Out<'a> {
    text: &'a str,
    marks: &'a [Range<usize>],
    pieces: Vec<Piece>,
    spoilers: usize,
}

/// What a rule that matched at a delimiter adds: its opener's length, its closer's index and length, and the style.
struct Matched {
    open: usize,
    close: usize,
    close_len: usize,
    style: Style,
}

impl Out<'_> {
    fn inline(&mut self, tokens: &[Token], lo: usize, hi: usize, style: Style) {
        let mut p = lo;
        while p < hi {
            match tokens.get(p) {
                Some(Token::Delim { at, .. }) => match self.rule(tokens, p, hi, style) {
                    Some(matched) => {
                        self.inline(tokens, p + matched.open, matched.close, matched.style);
                        p = matched.close + matched.close_len;
                    }
                    None => {
                        self.range(*at..at + 1, style);
                        p += 1;
                    }
                },
                Some(Token::Text(range)) => {
                    self.range(range.clone(), style);
                    p += 1;
                }
                Some(Token::Code(range)) => {
                    self.range(range.clone(), Style { code: Some(Code::Inline), ..style });
                    p += 1;
                }
                Some(Token::Block(range)) => {
                    self.range(range.clone(), Style { code: Some(Code::Block), ..style });
                    p += 1;
                }
                Some(Token::Mark(mark)) => {
                    if let Some(range) = self.marks.get(*mark) {
                        self.push(range.clone(), style, Some(*mark));
                    }
                    p += 1;
                }
                None => break,
            }
        }
    }

    fn rule(&mut self, tokens: &[Token], p: usize, hi: usize, style: Style) -> Option<Matched> {
        let Some(&Token::Delim { ch, .. }) = tokens.get(p) else { return None };
        let doubled = pair(tokens, p, ch, hi);
        let double = |close: usize, style: Style| Matched { open: 2, close, close_len: 2, style };
        match ch {
            '*' => doubled
                .then(|| double_close(tokens, p + 2, hi, '*', true))
                .flatten()
                .map(|close| double(close, Style { bold: true, ..style }))
                .or_else(|| self.single(tokens, p, hi, '*', style)),
            '_' => doubled
                .then(|| double_close(tokens, p + 2, hi, '_', true))
                .flatten()
                .map(|close| double(close, Style { underline: true, ..style }))
                .or_else(|| self.single(tokens, p, hi, '_', style)),
            '~' => doubled
                .then(|| double_close(tokens, p + 2, hi, '~', false))
                .flatten()
                .map(|close| double(close, Style { strike: true, ..style })),
            _ => {
                let close = doubled.then(|| double_close(tokens, p + 2, hi, '|', false)).flatten()?;
                self.spoilers += 1;
                Some(double(close, Style { spoiler: Some(SpoilerId(self.spoilers)), ..style }))
            }
        }
    }

    /// `*italic*` or `_italic_` opened at `p`: content may hold doubled markers whole, and the first lone marker
    /// decides, closing only when nothing blank is inside its edges (and, for `_`, no word goes on past it).
    fn single(&self, tokens: &[Token], p: usize, hi: usize, ch: char, style: Style) -> Option<Matched> {
        let Token::Delim { at, .. } = tokens.get(p)? else { return None };
        if self.blank_after(tokens, p) {
            return None;
        }
        if ch == '_' && char_before(self.text, *at).is_some_and(char::is_alphanumeric) {
            return None;
        }
        let mut q = p + 1;
        while q < hi {
            match tokens.get(q) {
                Some(Token::Delim { ch: c, at: close_at }) if *c == ch => {
                    if pair(tokens, q, ch, hi) {
                        q += 2;
                        continue;
                    }
                    let blank_before = self.blank_before(tokens, q);
                    let word_after = ch == '_' && char_at(self.text, close_at + 1).is_some_and(char::is_alphanumeric);
                    let ok = q > p + 1 && !blank_before && !word_after;
                    return ok.then_some(Matched {
                        open: 1,
                        close: q,
                        close_len: 1,
                        style: Style { italic: true, ..style },
                    });
                }
                _ => q += 1,
            }
        }
        None
    }

    fn blank_after(&self, tokens: &[Token], p: usize) -> bool {
        match tokens.get(p + 1) {
            Some(Token::Text(range)) => char_at(self.text, range.start).is_none_or(char::is_whitespace),
            Some(_) => false,
            None => true,
        }
    }

    fn blank_before(&self, tokens: &[Token], q: usize) -> bool {
        match q.checked_sub(1).and_then(|before| tokens.get(before)) {
            Some(Token::Text(range)) => char_before(self.text, range.end).is_none_or(char::is_whitespace),
            Some(_) => false,
            None => true,
        }
    }

    /// `range` in `style`, split around the marks inside it.
    fn range(&mut self, range: Range<usize>, style: Style) {
        let mut from = range.start;
        let inside: Vec<(usize, Range<usize>)> = self
            .marks
            .iter()
            .enumerate()
            .filter(|(_, mark)| range.start <= mark.start && mark.end <= range.end)
            .map(|(at, mark)| (at, mark.clone()))
            .collect();
        for (index, mark) in inside {
            if from < mark.start {
                self.push(from..mark.start, style, None);
            }
            from = mark.end;
            self.push(mark, style, Some(index));
        }
        if from < range.end {
            self.push(from..range.end, style, None);
        }
    }

    fn push(&mut self, bytes: Range<usize>, style: Style, mark: Option<usize>) {
        let Some(text) = self.text.get(bytes.clone()) else { return };
        for (part, emoji) in crate::emoji::clusters(text) {
            let bytes = bytes.start + part.start..bytes.start + part.end;
            let style = Style { emoji, ..style };
            if let Some(last) = self.pieces.last_mut()
                && !emoji
                && mark.is_none()
                && last.mark.is_none()
                && last.style == style
                && last.bytes.end == bytes.start
            {
                last.bytes.end = bytes.end;
                continue;
            }
            self.pieces.push(Piece { bytes, style, mark });
        }
    }
}

/// Whether the delimiter at `p` and the one after it, both before `hi`, are one doubled marker.
fn pair(tokens: &[Token], p: usize, ch: char, hi: usize) -> bool {
    if p + 1 >= hi {
        return false;
    }
    match (tokens.get(p), tokens.get(p + 1)) {
        (Some(Token::Delim { at, ch: a }), Some(Token::Delim { at: next, ch: b })) => {
            *a == ch && *b == ch && *next == at + 1
        }
        _ => false,
    }
}

/// The first doubled `ch` before `hi` closing content that starts at `from`, which may not be empty; with
/// `third_passes`, one followed by a third `ch` passes on.
fn double_close(tokens: &[Token], from: usize, hi: usize, ch: char, third_passes: bool) -> Option<usize> {
    (from + 1..hi).find(|&q| pair(tokens, q, ch, hi) && !(third_passes && pair(tokens, q + 1, ch, hi)))
}

#[cfg(test)]
#[expect(clippy::single_range_in_vec_init, reason = "a run's one range is the fixture, not a range of numbers")]
mod tests {
    use super::*;

    /// The text drawn, each piece as `[text|flags]` where flags are b i u s c(ode) k(block) q x(spoiler) e m(ark).
    fn drawn(text: &str, marks: &[Range<usize>]) -> String {
        pieces(text, marks)
            .iter()
            .map(|piece| {
                let style = piece.style;
                let flags: String = [
                    (style.bold, 'b'),
                    (style.italic, 'i'),
                    (style.underline, 'u'),
                    (style.strike, 's'),
                    (style.code == Some(Code::Inline), 'c'),
                    (style.code == Some(Code::Block), 'k'),
                    (style.quote, 'q'),
                    (style.spoiler.is_some(), 'x'),
                    (style.emoji, 'e'),
                    (piece.mark.is_some(), 'm'),
                ]
                .iter()
                .filter(|(on, _)| *on)
                .map(|(_, flag)| *flag)
                .collect();
                format!("[{}|{flags}]", text.get(piece.bytes.clone()).unwrap())
            })
            .collect()
    }

    #[test]
    fn each_marker_styles_its_text_and_is_hidden() {
        assert_eq!(drawn("a **b** c", &[]), "[a |][b|b][ c|]");
        assert_eq!(drawn("*i* _j_", &[]), "[i|i][ |][j|i]");
        assert_eq!(drawn("__u__ ~~s~~", &[]), "[u|u][ |][s|s]");
        assert_eq!(drawn("plain text", &[]), "[plain text|]");
    }

    #[test]
    fn markers_nest() {
        assert_eq!(drawn("***both***", &[]), "[both|bi]");
        assert_eq!(drawn("___both___", &[]), "[both|iu]");
        assert_eq!(drawn("*a **b** c*", &[]), "[a |i][b|bi][ c|i]");
        assert_eq!(drawn("**a *b* c**", &[]), "[a |b][b|bi][ c|b]");
        assert_eq!(drawn("~~**x**~~", &[]), "[x|bs]");
    }

    #[test]
    fn code_wins_and_is_verbatim() {
        assert_eq!(drawn("`**not bold**`", &[]), "[**not bold**|c]");
        assert_eq!(drawn("**a `b**` c**", &[]), "[a |b][b**|bc][ c|b]");
        assert_eq!(drawn("``a`b``", &[]), "[a`b|c]");
        assert_eq!(drawn("`\\*`", &[]), "[\\*|c]");
        assert_eq!(drawn("```rust\nfn main() {}\n```", &[]), "[fn main() {}|k]");
        assert_eq!(drawn("```\n**x**\n```after", &[]), "[**x**|k][after|]");
        assert_eq!(drawn("```one line```", &[]), "[one line|k]");
    }

    #[test]
    fn an_escape_makes_a_marker_literal_and_hides_its_backslash() {
        assert_eq!(drawn("\\*not\\*", &[]), "[*not|][*|]");
        assert_eq!(drawn("\\`x\\`", &[]), "[`x|][`|]");
        assert_eq!(drawn("a\\b", &[]), "[a\\b|]");
    }

    #[test]
    fn unclosed_or_blank_edged_markers_are_literal() {
        assert_eq!(drawn("**open", &[]), "[**open|]");
        assert_eq!(drawn("2 * 3 * 4", &[]), "[2 * 3 * 4|]");
        assert_eq!(drawn("snake_case_name", &[]), "[snake_case_name|]");
        assert_eq!(drawn("`open", &[]), "[`open|]");
        assert_eq!(drawn("~single~", &[]), "[~single~|]");
        assert_eq!(drawn("|one|", &[]), "[|one||]");
        assert_eq!(drawn("**a*", &[]), "[*|][a|i]");
    }

    #[test]
    fn a_spoiler_is_one_id_for_all_its_runs() {
        let found = pieces("||a **b**|| ||c||", &[]);
        let ids: Vec<Option<SpoilerId>> = found.iter().map(|piece| piece.style.spoiler).collect();
        assert_eq!(ids.len(), 4, "{found:?}");
        assert_eq!(ids.first(), ids.get(1));
        assert!(ids.get(2).is_some_and(Option::is_none), "the space between is no spoiler");
        assert_ne!(ids.first(), ids.get(3));
    }

    #[test]
    fn a_quote_line_is_quoted_without_its_prefix() {
        assert_eq!(drawn("> quoted\nnot", &[]), "[quoted\n|q][not|]");
        assert_eq!(drawn("> a\n> **b**", &[]), "[a\n|q][b|bq]");
        assert_eq!(drawn("a > b", &[]), "[a > b|]");
        assert_eq!(drawn(">no space", &[]), "[>no space|]");
    }

    #[test]
    fn a_mark_is_atomic_and_its_markers_literal() {
        assert_eq!(drawn("@a*b c* d", &[0..4]), "[@a*b|m][ c* d|]");
        assert_eq!(drawn("**@ada**", &[2..6]), "[@ada|bm]");
        assert_eq!(drawn("`@ada`", &[1..5]), "[@ada|cm]");
        assert_eq!(drawn("*a @b_c* d_", &[3..7]), "[a |i][@b_c|im][ d_|]");
    }

    #[test]
    fn an_emoji_is_its_own_piece_everywhere() {
        assert_eq!(drawn("hi 🙂!", &[]), "[hi |][🙂|e][!|]");
        assert_eq!(drawn("**🙂**", &[]), "[🙂|be]");
        assert_eq!(drawn("`🙂`", &[]), "[🙂|ce]");
        assert_eq!(drawn("@a🙂", &[0..6]), "[@a|m][🙂|em]");
    }
}
