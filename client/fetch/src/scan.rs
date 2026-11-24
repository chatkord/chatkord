//! A bounded hand scan of a page's `<head>` for what a link preview shows: Open Graph title, description and
//! picture, then `<title>` and the description meta. Nothing here builds a document tree: the scan is fed the page a
//! part at a time, walks tags and stops at `</head>`, `<body` or the end of what was read. What it keeps of the page
//! is the one tag or title being read and the values it found, never the page: a script's body is passed over.

use std::borrow::Cow;

/// The most Unicode scalar values of a preview's title.
pub const MAX_TITLE: usize = 200;

/// The most Unicode scalar values of a preview's description.
pub const MAX_DESCRIPTION: usize = 500;

/// The longest picture address kept.
const MAX_IMAGE: usize = 2048;

/// The longest attribute value or title text looked at; a longer one is taken as absent.
const MAX_VALUE: usize = 8 * 1024;

/// The most bytes of one start tag held while its end is awaited; a longer tag is skipped to its next `>` and its
/// attributes are absent.
const MAX_TAG: usize = MAX_ATTRIBUTES * MAX_VALUE;

/// What ends a title's text, in lower case.
const TITLE_CLOSE: &[u8] = b"</title";

/// The most attributes of one tag looked at.
const MAX_ATTRIBUTES: usize = 64;

/// The longest entity name, `&` and `;` left out, that is decoded.
const MAX_ENTITY: usize = 12;

/// What the scan found, each cleaned and within its bound.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Head {
    /// `og:title`, else `<title>`.
    pub(crate) title: Option<String>,
    /// `og:description`, else the description meta.
    pub(crate) description: Option<String>,
    /// `og:image` as written, for the page's address to resolve.
    pub(crate) image: Option<String>,
}

/// Why a page is not scanned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScanRefused {
    /// The bytes are not UTF-8.
    NotUtf8,
    /// The page declares a charset other than UTF-8.
    OtherCharset,
}

/// Whether a scan wants more of the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flow {
    /// It reads on.
    More,
    /// It has what it will have: the page need not be read further.
    Done,
}

/// Where the scan is in the page.
#[derive(Clone, Copy, Debug, Default)]
enum Mode {
    /// Before the first character, which may be a byte order mark.
    #[default]
    Start,
    /// Between tags.
    Text,
    /// After a `<`.
    Open,
    /// After `<!`, with the dashes seen of a comment's opening.
    Bang { dashes: u8 },
    /// In a comment, with the dashes just seen, at most two.
    Comment { dashes: u8 },
    /// In what runs to the next `>` and says nothing: a declaration, an end tag, the rest of a closing tag.
    Skip,
    /// After `</`, reading the name.
    Closing,
    /// In a start tag, held until its `>` closes it, with where in it the last byte was.
    Tag { lex: Lex },
    /// In the body of a script or a style, passed over unkept, with how much of its closing tag was just seen.
    Raw { kind: RawText, matched: usize },
    /// In a title's text, with how much of its closing tag was just seen; `keeping` while the text is held.
    Title { matched: usize, keeping: bool },
    /// Stopped at the end of the head.
    Done,
    /// Stopped because the page is not scanned.
    Refused(ScanRefused),
}

/// Where a start tag being held is, as [`attributes`] reads it, so that only the `>` that closes it is acted on and a
/// tag is read once however many `>` its quoted values hold.
#[derive(Clone, Copy, Debug)]
enum Lex {
    /// In the tag's name.
    Name,
    /// Between attributes.
    Between,
    /// In an attribute's name.
    Attribute,
    /// After an attribute's name, where an `=` gives it a value.
    Named,
    /// After the `=`, before the value.
    Equals,
    /// In a value quoted by `quote`.
    Quoted { quote: u8 },
    /// In a value without quotes.
    Unquoted,
}

impl Lex {
    /// Where the tag is after `byte`; none when `byte` is the `>` that closes it.
    fn after(self, byte: u8) -> Option<Lex> {
        let space = byte.is_ascii_whitespace();
        Some(match self {
            Lex::Name if byte.is_ascii_alphanumeric() => Lex::Name,
            Lex::Name | Lex::Between => match byte {
                b'>' => return None,
                b'/' => Lex::Between,
                // A stray `=` is a name of its own, as `attributes` reads it.
                b'=' => Lex::Named,
                _ if space => Lex::Between,
                _ => Lex::Attribute,
            },
            Lex::Attribute | Lex::Named => match byte {
                b'>' => return None,
                b'/' => Lex::Between,
                b'=' => Lex::Equals,
                _ if space => Lex::Named,
                _ => Lex::Attribute,
            },
            Lex::Equals => match byte {
                b'>' => return None,
                b'"' | b'\'' => Lex::Quoted { quote: byte },
                _ if space => Lex::Equals,
                _ => Lex::Unquoted,
            },
            Lex::Quoted { quote } if byte == quote => Lex::Between,
            Lex::Quoted { .. } => self,
            Lex::Unquoted => match byte {
                b'>' => return None,
                _ if space => Lex::Between,
                _ => Lex::Unquoted,
            },
        })
    }
}

/// An element whose body is not markup.
#[derive(Clone, Copy, Debug)]
enum RawText {
    Script,
    Style,
}

impl RawText {
    /// What ends the body, in lower case.
    fn closing(self) -> &'static [u8] {
        match self {
            RawText::Script => b"</script",
            RawText::Style => b"</style",
        }
    }
}

/// The scan of a page fed to it a part at a time, in whatever parts: what it finds does not depend on where the parts
/// end.
#[derive(Default)]
pub(crate) struct Scan {
    mode: Mode,
    found: Raw,
    /// The start tag, the end tag's name or the title's text being read.
    held: Vec<u8>,
    /// The start of a character the last part ended in, at most three bytes.
    carry: Vec<u8>,
}

impl Scan {
    /// Reads the next part of the page. A byte that is not UTF-8 refuses the page when the scan reaches it.
    pub(crate) fn feed(&mut self, chunk: &[u8]) -> Flow {
        if self.stopped() {
            return Flow::Done;
        }
        let joined;
        let bytes = if self.carry.is_empty() {
            chunk
        } else {
            joined = [self.carry.as_slice(), chunk].concat();
            joined.as_slice()
        };
        let (valid, failure) = match std::str::from_utf8(bytes) {
            Ok(text) => (text, None),
            Err(failed) => match std::str::from_utf8(bytes.get(..failed.valid_up_to()).unwrap_or_default()) {
                Ok(text) => (text, Some(failed)),
                Err(_) => {
                    self.mode = Mode::Refused(ScanRefused::NotUtf8);
                    return Flow::Done;
                }
            },
        };
        self.carry.clear();
        self.walk(valid);
        if self.stopped() {
            return Flow::Done;
        }
        if let Some(failed) = failure {
            if failed.error_len().is_some() {
                self.mode = Mode::Refused(ScanRefused::NotUtf8);
                return Flow::Done;
            }
            self.carry.extend_from_slice(bytes.get(failed.valid_up_to()..).unwrap_or_default());
        }
        Flow::More
    }

    /// What the parts fed say. `cut` says the page was stopped short of its end, so a character split at the end is
    /// not an error.
    pub(crate) fn finish(self, cut: bool) -> Result<Head, ScanRefused> {
        if let Mode::Refused(refused) = self.mode {
            return Err(refused);
        }
        if !cut && !self.carry.is_empty() {
            return Err(ScanRefused::NotUtf8);
        }
        self.found.finish()
    }

    fn stopped(&self) -> bool {
        matches!(self.mode, Mode::Done | Mode::Refused(_))
    }

    fn walk(&mut self, text: &str) {
        let mut text = text;
        if matches!(self.mode, Mode::Start) && !text.is_empty() {
            text = text.strip_prefix('\u{feff}').unwrap_or(text);
            self.mode = Mode::Text;
        }
        let bytes = text.as_bytes();
        let mut at = 0;
        while let Some(rest) = bytes.get(at..).filter(|rest| !rest.is_empty()) {
            at += self.step(rest);
        }
    }

    /// Reads from the start of `rest`, which is not empty; how many bytes it took.
    fn step(&mut self, rest: &[u8]) -> usize {
        match self.mode {
            Mode::Start | Mode::Text => match rest.iter().position(|byte| *byte == b'<') {
                Some(open) => {
                    self.mode = Mode::Open;
                    open + 1
                }
                None => rest.len(),
            },
            Mode::Open => {
                let Some(&byte) = rest.first() else { return 0 };
                self.mode = match byte {
                    b'!' => Mode::Bang { dashes: 0 },
                    b'?' => Mode::Skip,
                    b'/' => {
                        self.held.clear();
                        Mode::Closing
                    }
                    b'<' => Mode::Open,
                    letter if letter.is_ascii_alphabetic() => {
                        self.held.clear();
                        self.held.push(letter);
                        Mode::Tag { lex: Lex::Name }
                    }
                    _ => Mode::Text,
                };
                1
            }
            Mode::Bang { dashes } => {
                let Some(&byte) = rest.first() else { return 0 };
                self.mode = match byte {
                    b'-' if dashes == 1 => Mode::Comment { dashes: 0 },
                    b'-' => Mode::Bang { dashes: dashes + 1 },
                    b'>' => Mode::Text,
                    _ => Mode::Skip,
                };
                1
            }
            Mode::Comment { mut dashes } => {
                for (used, &byte) in (1..).zip(rest) {
                    match byte {
                        b'-' => dashes = (dashes + 1).min(2),
                        b'>' if dashes >= 2 => {
                            self.mode = Mode::Text;
                            return used;
                        }
                        _ => dashes = 0,
                    }
                }
                self.mode = Mode::Comment { dashes };
                rest.len()
            }
            Mode::Skip => match rest.iter().position(|byte| *byte == b'>') {
                Some(end) => {
                    self.mode = Mode::Text;
                    end + 1
                }
                None => rest.len(),
            },
            Mode::Closing => self.closing(rest),
            Mode::Tag { lex } => self.tag(rest, lex),
            Mode::Raw { kind, mut matched } => {
                let pattern = kind.closing();
                for (used, &byte) in (1..).zip(rest) {
                    matched = advance(matched, pattern, byte);
                    if matched == pattern.len() {
                        self.mode = Mode::Skip;
                        return used;
                    }
                }
                self.mode = Mode::Raw { kind, matched };
                rest.len()
            }
            Mode::Title { mut matched, mut keeping } => {
                for (used, &byte) in (1..).zip(rest) {
                    if keeping {
                        self.held.push(byte);
                        keeping = self.held.len() <= MAX_VALUE + TITLE_CLOSE.len();
                    }
                    matched = advance(matched, TITLE_CLOSE, byte);
                    if matched == TITLE_CLOSE.len() {
                        if keeping {
                            self.titled();
                        }
                        self.held.clear();
                        self.mode = Mode::Skip;
                        return used;
                    }
                }
                if !keeping {
                    self.held.clear();
                }
                self.mode = Mode::Title { matched, keeping };
                rest.len()
            }
            Mode::Done | Mode::Refused(_) => rest.len(),
        }
    }

    /// The name of an end tag: `</head` ends the scan, any other end tag is passed over.
    fn closing(&mut self, rest: &[u8]) -> usize {
        for (used, &byte) in (0..).zip(rest) {
            if byte.is_ascii_alphanumeric() {
                if self.held.len() < b"head".len() + 1 {
                    self.held.push(byte);
                }
                continue;
            }
            let ends_head = self.held.eq_ignore_ascii_case(b"head");
            self.held.clear();
            return if ends_head {
                self.mode = Mode::Done;
                used
            } else if byte == b'>' {
                self.mode = Mode::Text;
                used + 1
            } else {
                self.mode = Mode::Skip;
                used
            };
        }
        rest.len()
    }

    /// Holds the start tag up to the `>` that closes it, `lex` where the bytes before `rest` left it, and acts on it
    /// then; one past [`MAX_TAG`] is skipped to its next `>`.
    fn tag(&mut self, rest: &[u8], mut lex: Lex) -> usize {
        let room = MAX_TAG.saturating_sub(self.held.len());
        for (used, &byte) in (1..).zip(rest.iter().take(room)) {
            match lex.after(byte) {
                Some(next) => lex = next,
                None => {
                    self.held.extend_from_slice(rest.get(..used).unwrap_or(rest));
                    self.mode = acted_on(&self.held, &mut self.found);
                    self.held.clear();
                    return used;
                }
            }
        }
        let taken = room.min(rest.len());
        self.held.extend_from_slice(rest.get(..taken).unwrap_or(rest));
        self.mode = if self.held.len() >= MAX_TAG {
            self.held.clear();
            Mode::Skip
        } else {
            Mode::Tag { lex }
        };
        taken
    }

    /// The title's text, held with the start of its closing tag, is the page's title unless one was found already.
    fn titled(&mut self) {
        let text = self.held.get(..self.held.len().saturating_sub(TITLE_CLOSE.len())).unwrap_or_default();
        if let Ok(text) = std::str::from_utf8(text) {
            self.found.title_tag.get_or_insert_with(|| text.to_owned());
        }
    }
}

/// How much of `pattern`, a lower case string that starts with `<` and holds no other, has just been seen after
/// `byte`, given `matched` before it.
fn advance(matched: usize, pattern: &[u8], byte: u8) -> usize {
    if pattern.get(matched) == Some(&byte.to_ascii_lowercase()) { matched + 1 } else { usize::from(byte == b'<') }
}

/// The start tag `held`, ended by the `>` [`Lex`] found closes it: its effect on `found`, and the mode the scan goes
/// on in.
fn acted_on(held: &[u8], found: &mut Raw) -> Mode {
    let Ok(text) = std::str::from_utf8(held) else { return Mode::Text };
    let name = name_at(held, 0);
    let attributes = attributes(text, name.len()).map(|(attributes, _)| attributes).unwrap_or_default();
    match name.as_str() {
        "body" => Mode::Done,
        "script" => Mode::Raw { kind: RawText::Script, matched: 0 },
        "style" => Mode::Raw { kind: RawText::Style, matched: 0 },
        "title" => Mode::Title { matched: 0, keeping: found.title_tag.is_none() },
        "meta" => {
            found.meta(&attributes);
            Mode::Text
        }
        _ => Mode::Text,
    }
}

/// What `bytes` of a page say, fed at once. `cut` says they were stopped short of the page's end.
#[cfg(any(test, fuzzing))]
pub(crate) fn page(bytes: &[u8], cut: bool) -> Result<Head, ScanRefused> {
    let mut scan = Scan::default();
    scan.feed(bytes);
    scan.finish(cut)
}

/// A start tag's attributes, each a lower case name and the value as written when it has one.
type Attributes<'a> = Vec<(String, Option<&'a str>)>;

/// A start tag's attributes, name lower case and value as written, and the offset after its `>`; none when the tag
/// is never closed. A value over [`MAX_VALUE`] is kept as no value.
fn attributes(text: &str, mut at: usize) -> Option<(Attributes<'_>, usize)> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    loop {
        while bytes.get(at).is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'/') {
            at += 1;
        }
        if *bytes.get(at)? == b'>' {
            return Some((found, at + 1));
        }
        let name_end = bytes.get(at..)?.iter().position(|b| b.is_ascii_whitespace() || matches!(b, b'=' | b'>' | b'/'));
        let name_end = name_end.map_or(bytes.len(), |end| at + end);
        // A stray `=` is part of the name, so the loop always advances.
        let name_end = name_end.max(at + 1);
        let name = text.get(at..name_end).unwrap_or_default().to_ascii_lowercase();
        at = name_end;
        let mut probe = at;
        while bytes.get(probe).is_some_and(u8::is_ascii_whitespace) {
            probe += 1;
        }
        let mut value = None;
        if bytes.get(probe) == Some(&b'=') {
            probe += 1;
            while bytes.get(probe).is_some_and(u8::is_ascii_whitespace) {
                probe += 1;
            }
            let (start, end, after) = match bytes.get(probe)? {
                quote @ (b'"' | b'\'') => {
                    let close = find(bytes, probe + 1, std::slice::from_ref(quote))?;
                    (probe + 1, close, close + 1)
                }
                _ => {
                    let end = bytes.get(probe..)?.iter().position(|b| b.is_ascii_whitespace() || *b == b'>');
                    let end = end.map_or(bytes.len(), |end| probe + end);
                    (probe, end, end)
                }
            };
            value = (end - start <= MAX_VALUE).then(|| text.get(start..end)).flatten();
            at = after;
        }
        if found.len() < MAX_ATTRIBUTES {
            found.push((name, value));
        }
    }
}

/// What the head said, before it is cleaned.
#[derive(Default)]
struct Raw {
    og_title: Option<String>,
    og_description: Option<String>,
    og_image: Option<String>,
    title_tag: Option<String>,
    description: Option<String>,
    other_charset: bool,
}

impl Raw {
    fn meta(&mut self, attributes: &[(String, Option<&str>)]) {
        let get = |wanted: &str| attributes.iter().find(|(name, _)| name == wanted).and_then(|(_, value)| *value);
        if get("charset").is_some_and(not_utf8) {
            self.other_charset = true;
        }
        let content = get("content");
        if get("http-equiv").is_some_and(|value| value.trim().eq_ignore_ascii_case("content-type"))
            && let Some(declared) = content.and_then(charset_of)
            && not_utf8(declared)
        {
            self.other_charset = true;
        }
        let Some(content) = content else { return };
        let key = get("property").or_else(|| get("name")).map(str::trim);
        let slot = match key {
            Some(key) if key.eq_ignore_ascii_case("og:title") => &mut self.og_title,
            Some(key) if key.eq_ignore_ascii_case("og:description") => &mut self.og_description,
            Some(key) if key.eq_ignore_ascii_case("og:image") => &mut self.og_image,
            Some(key) if key.eq_ignore_ascii_case("description") => &mut self.description,
            _ => return,
        };
        slot.get_or_insert_with(|| content.to_owned());
    }

    fn finish(self) -> Result<Head, ScanRefused> {
        if self.other_charset {
            return Err(ScanRefused::OtherCharset);
        }
        let first = |candidates: [Option<&str>; 2], most| {
            candidates.into_iter().flatten().map(|raw| clean(raw, most)).find(|cleaned| !cleaned.is_empty())
        };
        let image = self
            .og_image
            .as_deref()
            .map(|raw| decode(raw).trim().to_string())
            .filter(|image| !image.is_empty() && image.len() <= MAX_IMAGE);
        Ok(Head {
            title: first([self.og_title.as_deref(), self.title_tag.as_deref()], MAX_TITLE),
            description: first([self.og_description.as_deref(), self.description.as_deref()], MAX_DESCRIPTION),
            image,
        })
    }
}

/// The `charset=` a `Content-Type` value declares.
pub(crate) fn charset_of(content_type: &str) -> Option<&str> {
    content_type.split(';').skip(1).find_map(|parameter| {
        let (name, value) = parameter.split_once('=')?;
        name.trim().eq_ignore_ascii_case("charset").then(|| value.trim().trim_matches(['"', '\'']))
    })
}

/// Whether a declared charset names anything but UTF-8.
pub(crate) fn not_utf8(label: &str) -> bool {
    let label = label.trim();
    !(label.eq_ignore_ascii_case("utf-8") || label.eq_ignore_ascii_case("utf8"))
}

/// The tag name that starts at `from`, lower case.
fn name_at(bytes: &[u8], from: usize) -> String {
    let rest = bytes.get(from..).unwrap_or_default();
    let length = rest.iter().position(|b| !b.is_ascii_alphanumeric()).unwrap_or(rest.len());
    String::from_utf8_lossy(rest.get(..length).unwrap_or_default()).to_ascii_lowercase()
}

/// The first offset at or after `from` where `needle` starts.
fn find(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    bytes.get(from..)?.windows(needle.len()).position(|window| window == needle).map(|at| at + from)
}

/// `raw` as a preview's line: entities decoded, controls and direction overrides dropped, whitespace collapsed, within
/// `most` scalar values.
pub(crate) fn clean(raw: &str, most: usize) -> String {
    let kept: String = decode(raw)
        .chars()
        .filter(|c| {
            c.is_whitespace() || !(c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
        })
        .collect();
    shorten(&kept.split_whitespace().collect::<Vec<_>>().join(" "), most)
}

/// `text` within `most` scalar values: cut at the last word boundary that leaves most of it, and ended with an
/// ellipsis.
fn shorten(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_string();
    }
    let room = most.saturating_sub(1);
    let end = text.char_indices().nth(room).map_or(text.len(), |(at, _)| at);
    let kept = text.get(..end).unwrap_or_default();
    let at_word = kept.rfind(' ').filter(|at| kept.get(..*at).is_some_and(|before| before.chars().count() >= most / 2));
    let kept = at_word.and_then(|at| kept.get(..at)).unwrap_or(kept);
    format!("{}\u{2026}", kept.trim_end())
}

/// `raw` with its character references replaced: the common named ones, and decimal and hexadecimal numbers. One
/// that names nothing is left as written, and a number that is no scalar value is dropped.
fn decode(raw: &str) -> Cow<'_, str> {
    if !raw.contains('&') {
        return Cow::Borrowed(raw);
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(at) = rest.find('&') {
        out.push_str(rest.get(..at).unwrap_or_default());
        let after = rest.get(at + 1..).unwrap_or_default();
        let reference =
            after.find(';').filter(|end| *end <= MAX_ENTITY).and_then(|end| after.get(..end).map(|name| (name, end)));
        match reference.and_then(|(name, end)| reference_of(name).map(|replacement| (replacement, end))) {
            Some((replacement, end)) => {
                if let Some(c) = replacement {
                    out.push(c);
                }
                rest = after.get(end + 1..).unwrap_or_default();
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// What `&name;` stands for: the character, or none for a number that is no scalar value; `None` when it names
/// nothing known.
fn reference_of(name: &str) -> Option<Option<char>> {
    if let Some(number) = name.strip_prefix('#') {
        let value = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16),
            None => number.parse::<u32>(),
        };
        return value.ok().map(char::from_u32);
    }
    let named = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "copy" => '\u{a9}',
        "reg" => '\u{ae}',
        "trade" => '\u{2122}',
        "hellip" => '\u{2026}',
        "mdash" => '\u{2014}',
        "ndash" => '\u{2013}',
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "ldquo" => '\u{201c}',
        "rdquo" => '\u{201d}',
        "laquo" => '\u{ab}',
        "raquo" => '\u{bb}',
        "middot" => '\u{b7}',
        "bull" => '\u{2022}',
        "euro" => '\u{20ac}',
        "pound" => '\u{a3}',
        "yen" => '\u{a5}',
        "cent" => '\u{a2}',
        "deg" => '\u{b0}',
        "times" => '\u{d7}',
        _ => return None,
    };
    Some(Some(named))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(html: &str) -> Head {
        page(html.as_bytes(), false).unwrap()
    }

    #[test]
    fn open_graph_is_read_in_either_attribute_order_and_any_quoting() {
        let html = r#"<html><head>
            <meta property="og:title" content="Title one">
            <meta content='A description' property='og:description'>
            <META PROPERTY=og:image CONTENT=/pic.png>
            </head><body><meta property="og:title" content="late"></body>"#;
        let found = scan(html);
        assert_eq!(found.title.as_deref(), Some("Title one"));
        assert_eq!(found.description.as_deref(), Some("A description"));
        assert_eq!(found.image.as_deref(), Some("/pic.png"));
        let named = scan(
            r#"<head><meta name="og:title" content="By name"/><meta  content = "x y"  property = " OG:Description " /></head>"#,
        );
        assert_eq!((named.title.as_deref(), named.description.as_deref()), (Some("By name"), Some("x y")));
    }

    #[test]
    fn the_title_tag_and_the_description_meta_are_the_fallbacks() {
        let found = scan(
            "<head><title>\n  Plain \t title &amp; more </title><meta name=description content=\"Plain description\"></head>",
        );
        assert_eq!(found.title.as_deref(), Some("Plain title & more"));
        assert_eq!(found.description.as_deref(), Some("Plain description"));
        assert_eq!(found.image, None);
        let both = scan(r#"<title>Tag</title><meta property="og:title" content="Graph">"#);
        assert_eq!(both.title.as_deref(), Some("Graph"));
        let empty = scan(r#"<title>Tag</title><meta property="og:title" content="  ">"#);
        assert_eq!(empty.title.as_deref(), Some("Tag"));
    }

    #[test]
    fn the_scan_stops_at_the_end_of_the_head_or_the_body() {
        assert_eq!(scan("<head></head><title>Late</title>").title, None);
        assert_eq!(scan("<body><title>Late</title>").title, None);
        assert_eq!(
            scan("<head><script>var a = '</head><title>Hidden</title>';</script><title>Real</title></head>")
                .title
                .as_deref(),
            Some("Real")
        );
        assert_eq!(scan("<head><!-- <title>Commented</title> --><title>Real</title>").title.as_deref(), Some("Real"));
        assert_eq!(scan("<!DOCTYPE html><style>a{}</style><title>T</title>").title.as_deref(), Some("T"));
    }

    #[test]
    fn entities_are_decoded_and_the_unknown_ones_left_alone() {
        let found = scan(
            r#"<meta property="og:title" content="Tom &amp; Jerry &#8212; &#x41;&#X42; &nbsp;&bogus; &amp &#1114112; &#xD800; &lt;b&gt;">"#,
        );
        assert_eq!(found.title.as_deref(), Some("Tom & Jerry \u{2014} AB &bogus; &amp <b>"));
    }

    #[test]
    fn controls_and_direction_overrides_are_dropped_and_whitespace_collapsed() {
        let found = scan(
            "<title>a\u{0}b\u{7}\u{202a}\u{202b}\u{202c}\u{202d}\u{202e}c\u{2066}\u{2067}\u{2068}\u{2069}d\u{85}e\u{2029}\r\n\tf&#0;g&#8238;h&#x1b;</title>",
        );
        assert_eq!(found.title.as_deref(), Some("abcd e fgh"));
    }

    #[test]
    fn a_page_that_is_not_utf8_has_no_preview() {
        assert_eq!(page(b"<title>caf\xe9</title>", false), Err(ScanRefused::NotUtf8));
        assert_eq!(page(b"<title>caf\xe9</title>", true), Err(ScanRefused::NotUtf8));
    }

    #[test]
    fn a_character_split_by_the_cap_is_not_an_error() {
        let bytes = "<meta property=\"og:title\" content=\"T\"><i>\u{e9}".as_bytes();
        let split = bytes.get(..bytes.len() - 1).unwrap();
        assert_eq!(page(split, true).unwrap().title.as_deref(), Some("T"));
        assert_eq!(page(split, false), Err(ScanRefused::NotUtf8));
        let with_bom = [b"\xef\xbb\xbf".as_slice(), b"<title>T</title>"].concat();
        assert_eq!(page(&with_bom, false).unwrap().title.as_deref(), Some("T"));
    }

    #[test]
    fn a_declared_charset_other_than_utf8_has_no_preview() {
        for html in [
            r#"<meta charset="iso-8859-1"><title>T</title>"#,
            "<meta charset=windows-1253><title>T</title>",
            r#"<meta http-equiv="Content-Type" content="text/html; charset=ISO-8859-7"><title>T</title>"#,
        ] {
            assert_eq!(page(html.as_bytes(), false), Err(ScanRefused::OtherCharset), "{html}");
        }
        for html in [
            r#"<meta charset="UTF-8"><title>T</title>"#,
            r#"<meta http-equiv="content-type" content="text/html; charset=utf-8"><title>T</title>"#,
            r#"<meta name="charset" content="latin1"><title>T</title>"#,
        ] {
            assert_eq!(page(html.as_bytes(), false).unwrap().title.as_deref(), Some("T"), "{html}");
        }
    }

    #[test]
    fn a_huge_attribute_is_no_value_and_costs_nothing_more() {
        let huge = "x".repeat(MAX_VALUE + 1);
        let found = scan(&format!(r#"<meta property="og:title" content="{huge}"><title>Short</title>"#));
        assert_eq!(found.title.as_deref(), Some("Short"));
        let long_title = format!("<title>{huge}</title><meta property=\"og:description\" content=\"D\">");
        let found = scan(&long_title);
        assert_eq!((found.title, found.description.as_deref()), (None, Some("D")));
    }

    #[test]
    fn an_unterminated_tag_ends_the_scan_with_what_it_had() {
        for html in [
            r#"<title>Kept</title><meta property="og:description" content="never closed"#,
            r#"<title>Kept</title><meta property="og:description" content="x" "#,
            "<title>Kept</title><meta property=",
            "<title>Kept</title><!-- never closed <title>Other</title>",
            "<title>Kept</title><",
            "<title>Kept</title><meta",
        ] {
            let found = scan(html);
            assert_eq!((found.title.as_deref(), found.description), (Some("Kept"), None), "{html}");
        }
        assert_eq!(scan("<title>never closed").title, None);
        assert_eq!(scan("").title, None);
    }

    #[test]
    fn a_long_line_is_shortened_at_a_word_with_an_ellipsis() {
        let words = "word ".repeat(100);
        let title = clean(&words, MAX_TITLE);
        assert!(title.chars().count() <= MAX_TITLE);
        assert!(title.ends_with("word\u{2026}"), "{title}");
        let short = clean("A short one", MAX_TITLE);
        assert_eq!(short, "A short one");
        let exact = "a".repeat(MAX_TITLE);
        assert_eq!(clean(&exact, MAX_TITLE), exact);
        let unbroken = clean(&"\u{1f600}".repeat(MAX_TITLE + 40), MAX_TITLE);
        assert_eq!(unbroken.chars().count(), MAX_TITLE);
        assert!(unbroken.ends_with('\u{2026}'));
        let description = clean(&"ab cd ".repeat(400), MAX_DESCRIPTION);
        assert!(description.chars().count() <= MAX_DESCRIPTION);
    }

    fn in_parts(page: &[u8], cuts: &[usize], cut: bool) -> Result<Head, ScanRefused> {
        let mut scan = Scan::default();
        let mut from = 0;
        for end in cuts.iter().copied().chain([page.len()]) {
            scan.feed(page.get(from..end).unwrap());
            from = end;
        }
        scan.finish(cut)
    }

    #[test]
    fn a_page_fed_in_parts_says_what_it_says_whole_wherever_the_parts_end() {
        let pages: [&str; 9] = [
            r#"<html><head><meta property="og:title" content="A > B"><title>t</title></head><title>late</title>"#,
            "<!-- <title>no</title> --><title>Real &amp; true</title><!--> still a comment --><title>x</title>",
            "<script>var s = '</scr' + 'ipt>'; </SCRIPT ><title>After</title></head>",
            "<style>a{}</styl</STYLE\n><title>After</title><scrip</script",
            "<title>caf\u{e9} \u{1f600} done</title><meta name=description content='d\u{e9}\u{1f600}'>",
            "\u{feff}<head><title>Bom</title></head><title>late</title>",
            "<!DOCTYPE html><?x?><!-><!-->< <<meta charset=latin1><title>T</title>",
            "<head><META PROPERTY=og:image CONTENT=/a.png><meta name=\"og:title\"content=\"x\"><b <i> </ head>",
            "<title></titl</TITLE ><title>second</title><title>third</title></hea",
        ];
        for page in pages {
            let bytes = page.as_bytes();
            let whole = in_parts(bytes, &[], false);
            let singly: Vec<usize> = (1..bytes.len()).collect();
            assert_eq!(in_parts(bytes, &singly, false), whole, "a byte at a time: {page}");
            for cut in 0..=bytes.len() {
                assert_eq!(in_parts(bytes, &[cut], false), whole, "in two at {cut}: {page}");
            }
            let short = bytes.get(..bytes.len() - 1).unwrap();
            let singly: Vec<usize> = (1..short.len()).collect();
            assert_eq!(in_parts(short, &singly, true), in_parts(short, &[], true), "a prefix: {page}");
        }
    }

    #[test]
    fn a_head_behind_a_page_of_script_is_read_without_holding_it() {
        let script = "x".repeat(2 * 1024 * 1024);
        let found = scan(&format!(
            r#"<head><script>{script}</script><style>{script}</STYLE><meta property="og:title" content="Deep"></head>"#
        ));
        assert_eq!(found.title.as_deref(), Some("Deep"));
        let mut scan = Scan::default();
        assert_eq!(scan.feed(b"<head><script>"), Flow::More);
        let part = vec![b'x'; 64 * 1024];
        for _ in 0..64 {
            scan.feed(&part);
        }
        assert!(scan.held.is_empty() && scan.carry.is_empty(), "nothing of the script is kept");
        assert_eq!(scan.feed(b"</script></head>"), Flow::Done);
        assert_eq!(scan.feed(b"\xff anything"), Flow::Done, "nothing past the end of the head is read");
    }

    #[test]
    fn a_tag_past_the_hold_bound_is_skipped_and_the_metas_after_it_still_read() {
        let padding = "a ".repeat(MAX_TAG);
        let found = scan(&format!(
            r#"<div {padding}><meta property="og:title" content="After"><meta property="og:image" {padding} content="/lost.png"><title>Tag</title>"#
        ));
        assert_eq!(found.title.as_deref(), Some("After"));
        assert_eq!(found.image, None, "a tag too long to hold says nothing");
        let long = format!(r#"<meta property="og:title" content="Lost" {padding}><title>Tag</title>"#);
        assert_eq!(scan(&long).title.as_deref(), Some("Tag"));
    }

    #[test]
    fn a_byte_past_the_head_is_never_read_and_one_before_it_refuses_the_page() {
        assert_eq!(page(b"<title>T</title></head>\xff", false).unwrap().title.as_deref(), Some("T"));
        assert_eq!(page(b"<title>T</title>\xff</head>", false), Err(ScanRefused::NotUtf8));
        let mut scan = Scan::default();
        assert_eq!(scan.feed(b"<title>T</title>\xff"), Flow::Done);
        assert_eq!(scan.finish(true), Err(ScanRefused::NotUtf8));
    }

    #[test]
    fn a_tag_whose_quoted_value_holds_many_closes_is_read_once() {
        let closes = ">".repeat(MAX_TAG - 64);
        let found = scan(&format!(r#"<meta property="og:description" content="{closes}"><title>After</title>"#));
        assert_eq!((found.title.as_deref(), found.description), (Some("After"), None), "too long a value is absent");
        let mut scan = Scan::default();
        scan.feed(b"<meta content='");
        for _ in 0..(MAX_TAG - 64) / 1024 {
            scan.feed(&[b'>'; 1024]);
        }
        assert!(matches!(scan.mode, Mode::Tag { lex: Lex::Quoted { quote: b'\'' } }), "still in the value");
    }

    #[test]
    fn a_picture_address_is_kept_as_written_within_its_bound() {
        let found = scan(r#"<meta property="og:image" content=" https://cdn.example.invalid/a.png?x=1&amp;y=2 ">"#);
        assert_eq!(found.image.as_deref(), Some("https://cdn.example.invalid/a.png?x=1&y=2"));
        let long = format!(r#"<meta property="og:image" content="/{}">"#, "a".repeat(MAX_IMAGE));
        assert_eq!(scan(&long).image, None);
    }
}
