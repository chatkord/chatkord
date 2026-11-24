//! What the composer holds, judged by the rule the server keeps, so Send is offered only for a body it takes: its
//! text, and the marks a completion put in it, moved with each edit and dropped once their own text is changed.

use std::ops::Range;

use proto::rules::{BodyRefused, Carries, LinkTarget, MAX_MESSAGE_CHARS, message_shape};

use crate::body::Body;
use crate::link;
use crate::mark::{Mark, MarkKind, byte_ranges};
use crate::markdown::pieces;

/// The count is shown once a draft is this close to the most a message holds.
pub const COUNT_SHOWN_WITHIN: usize = 200;

/// How many characters after `:` start an emoji completion, as Discord's composer waits for.
pub const EMOJI_QUERY_FROM: usize = 2;

/// A message being written.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Draft {
    text: String,
    marks: Vec<Mark>,
}

/// What a completion looks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Trigger {
    /// `@`: a member, then a role, `@everyone` and `@here`.
    Member,
    /// `#`: a channel.
    Channel,
    /// `:`: an emoji, Unicode's or the server's.
    Emoji,
}

impl Trigger {
    /// The character that starts it.
    pub const fn symbol(self) -> char {
        match self {
            Trigger::Member => '@',
            Trigger::Channel => '#',
            Trigger::Emoji => ':',
        }
    }

    fn of(symbol: char) -> Option<Trigger> {
        [Trigger::Member, Trigger::Channel, Trigger::Emoji].into_iter().find(|trigger| trigger.symbol() == symbol)
    }
}

/// A completion being typed: its trigger at a word's start, up to the cursor.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Query {
    /// What it looks for.
    pub trigger: Trigger,
    /// What was typed after the trigger.
    pub text: String,
    /// Where the trigger and its text are, in Unicode scalar values of the draft.
    pub range: Range<usize>,
}

/// What a chosen completion puts in the draft.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Completion {
    /// The text written in place of the query: `@Ada`, `#general`, `:party:`, `🙂`.
    pub text: String,
    /// The mark the text carries; none for a Unicode emoji, which is only text.
    pub mark: Option<MarkKind>,
}

impl Draft {
    /// A draft holding `text` as typed, with no marks.
    pub fn new(text: impl Into<String>) -> Draft {
        Draft { text: text.into(), marks: Vec::new() }
    }

    /// A draft of a sent body, to edit it: its text and its marks.
    pub fn of(body: &Body) -> Draft {
        Draft { text: body.text().to_owned(), marks: body.marks().to_vec() }
    }

    /// The text as typed.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Its marks, sorted and apart.
    pub fn marks(&self) -> &[Mark] {
        &self.marks
    }

    /// How many Unicode scalar values it holds, as the server counts them.
    pub fn count(&self) -> usize {
        self.text.chars().count()
    }

    /// The count to show beside Send, once within [`COUNT_SHOWN_WITHIN`] of the most.
    pub fn shown_count(&self) -> Option<usize> {
        let count = self.count();
        (count + COUNT_SHOWN_WITHIN > MAX_MESSAGE_CHARS).then_some(count)
    }

    /// Whether it holds more than a message may.
    pub fn over(&self) -> bool {
        self.count() > MAX_MESSAGE_CHARS
    }

    /// The body's text to send, its white space at either end left off, or why it cannot be sent.
    pub fn body(&self, carries: Carries) -> Result<&str, BodyRefused> {
        message_shape(self.text.trim(), carries)
    }

    /// The text now `text`, as the field reports it whole: a mark after the change moves with it, one before it
    /// stays, one whose own text changed is dropped. Where an insertion or deletion could have been made at several
    /// places alike, it is taken as made where it leaves the most marks.
    pub fn edit(&mut self, text: impl Into<String>) {
        let text = text.into();
        if text == self.text {
            return;
        }
        let old: Vec<char> = self.text.chars().collect();
        let new: Vec<char> = text.chars().collect();
        let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
        let suffix = old.iter().rev().zip(new.iter().rev()).take_while(|(a, b)| a == b).count();
        let shortest = old.len().min(new.len());
        let (at, removed, inserted) = if old.len() != new.len() && prefix + suffix >= shortest {
            let changed = old.len().abs_diff(new.len());
            let (removed, inserted) = if new.len() > old.len() { (0, changed) } else { (changed, 0) };
            let (lo, hi) = (shortest - suffix.min(shortest), prefix.min(shortest));
            let at =
                (lo..=hi).rev().find(|&at| self.marks.iter().all(|mark| apart(mark, at, at + removed))).unwrap_or(hi);
            (at, removed, inserted)
        } else {
            let at = prefix.min(shortest);
            let same_end = suffix.min(shortest - at);
            (at, old.len() - at - same_end, new.len() - at - same_end)
        };
        self.marks = moved(&self.marks, at..at + removed, inserted);
        self.text = text;
    }

    /// The completion being typed with the cursor at the draft's end.
    pub fn query(&self) -> Option<Query> {
        self.query_at(self.count())
    }

    /// The completion being typed before `cursor`, in Unicode scalar values: a trigger starting a word, then no
    /// white space, outside every mark; an emoji's once [`EMOJI_QUERY_FROM`] characters follow its `:`.
    pub fn query_at(&self, cursor: usize) -> Option<Query> {
        let chars: Vec<char> = self.text.chars().collect();
        let before = chars.get(..cursor)?;
        let start = before.iter().rposition(|c| c.is_whitespace()).map_or(0, |at| at + 1);
        if start >= cursor {
            return None;
        }
        let trigger = Trigger::of(*chars.get(start)?)?;
        let typed = chars.get(start + 1..cursor)?;
        if trigger == Trigger::Emoji && (typed.len() < EMOJI_QUERY_FROM || typed.contains(&':')) {
            return None;
        }
        if self.marks.iter().any(|mark| mark.range.start < cursor && start < mark.range.end) {
            return None;
        }
        Some(Query { trigger, text: typed.iter().collect(), range: start..cursor })
    }

    /// Puts `choice` in place of `query` with a space after it unless one follows, its mark over its text. Answers
    /// where the cursor goes, or none when the draft no longer holds the query there.
    pub fn complete(&mut self, query: &Query, choice: Completion) -> Option<usize> {
        let chars: Vec<char> = self.text.chars().collect();
        let written: String = chars.get(query.range.clone())?.iter().collect();
        let expected: String = std::iter::once(query.trigger.symbol()).chain(query.text.chars()).collect();
        if written != expected || self.marks.iter().any(|mark| !apart(mark, query.range.start, query.range.end)) {
            return None;
        }
        let space = !chars.get(query.range.end).is_some_and(|c| c.is_whitespace());
        let length = choice.text.chars().count();
        let added = length + usize::from(space);
        let mut text: String = chars.get(..query.range.start)?.iter().collect();
        text.push_str(&choice.text);
        if space {
            text.push(' ');
        }
        text.extend(chars.get(query.range.end..)?);
        let mut marks = moved(&self.marks, query.range.clone(), added);
        if let Some(kind) = choice.mark
            && length > 0
        {
            let start = query.range.start;
            let at = marks.partition_point(|mark| mark.range.start < start);
            marks.insert(at, Mark { range: start..start + length, kind });
        }
        self.text = text;
        self.marks = marks;
        Some(query.range.start + added)
    }

    /// The link a preview is made of: the first link the draft sends as one, when it is an `https` address with no
    /// credentials; none when the first is any other, so a preview is never made of a later link.
    pub fn first_link(&self) -> Option<String> {
        let body = self.send(Carries::Files, |_| Vec::new()).ok()?;
        let first = body.links().next()?;
        let target = LinkTarget::parse(first)?;
        (target.is_https() && !target.has_userinfo()).then(|| first.to_owned())
    }

    /// The body to send: the text as [`Draft::body`] gives it, its marks with it, but none inside code, each plain
    /// link outside code and marks made a link mark, and each `#name` typed at a word's start outside code and marks
    /// made a channel mark where `channels` names exactly one channel by that name; none or several leave it text.
    pub fn send(
        &self,
        carries: Carries,
        channels: impl Fn(&str) -> Vec<terms::ChannelId>,
    ) -> Result<Body, BodyRefused> {
        let text = self.body(carries)?;
        let lead = self.count() - self.text.trim_start().chars().count();
        let length = text.chars().count();
        let kept: Vec<Mark> = self
            .marks
            .iter()
            .filter_map(|mark| {
                let range = mark.range.start.checked_sub(lead)?..mark.range.end - lead;
                (range.end <= length).then(|| Mark { range, kind: mark.kind.clone() })
            })
            .collect();
        let Ok(bytes) = byte_ranges(text, &kept) else { return Ok(Body::plain(text)) };
        let drawn = pieces(text, &bytes);
        let in_code: Vec<Range<usize>> =
            drawn.iter().filter(|piece| piece.style.code.is_some()).map(|piece| piece.bytes.clone()).collect();
        let coded = |range: &Range<usize>| in_code.iter().any(|code| code.start < range.end && range.start < code.end);
        let mut marks: Vec<Mark> =
            kept.into_iter().zip(&bytes).filter(|(_, range)| !coded(range)).map(|(mark, _)| mark).collect();
        let scalars = |at: usize| text.get(..at).map_or(0, |head| head.chars().count());
        let links = link::find(text)
            .into_iter()
            .filter(|range| {
                !coded(range) && bytes.iter().all(|mark| mark.end <= range.start || range.end <= mark.start)
            })
            .map(|range| Mark { range: scalars(range.start)..scalars(range.end), kind: MarkKind::Link });
        marks.extend(links);
        let bytes_of = |range: &Range<usize>| {
            let at = |scalar: usize| text.char_indices().nth(scalar).map_or(text.len(), |(at, _)| at);
            at(range.start)..at(range.end)
        };
        let typed: Vec<Mark> = typed_channels(text)
            .into_iter()
            .filter(|(range, _)| {
                !coded(&bytes_of(range)) && marks.iter().all(|mark| apart(mark, range.start, range.end))
            })
            .filter_map(|(range, name)| match channels(name).as_slice() {
                [only] => Some(Mark { range, kind: MarkKind::Channel(only.clone()) }),
                [] | [_, _, ..] => None,
            })
            .collect();
        marks.extend(typed);
        marks.sort_by_key(|mark| mark.range.start);
        Ok(Body::new(text, marks).unwrap_or_else(|_| Body::plain(text)))
    }
}

/// Each `#name` of `text` at a word's start, by its range in Unicode scalar values with its `#`, and the name: the
/// word after the `#` up to white space, less a sentence's closing punctuation.
fn typed_channels(text: &str) -> Vec<(Range<usize>, &str)> {
    let word_character = |c: char| c.is_alphanumeric() || c == '_';
    let mut found = Vec::new();
    let mut before: Option<char> = None;
    for (scalar, (at, c)) in text.char_indices().enumerate() {
        let starts = c == Trigger::Channel.symbol() && !before.is_some_and(word_character);
        before = Some(c);
        if !starts {
            continue;
        }
        let rest = text.get(at + c.len_utf8()..).unwrap_or_default();
        let word = rest.split(char::is_whitespace).next().unwrap_or_default();
        let name = word.trim_end_matches(['.', ',', '!', '?', ';', ':', ')']);
        if !name.is_empty() {
            let end = scalar + 1 + name.chars().count();
            found.push((scalar..end, name));
        }
    }
    found
}

/// Whether `mark` is untouched by a change of `start..end`: wholly before it or wholly after.
fn apart(mark: &Mark, start: usize, end: usize) -> bool {
    mark.range.end <= start || mark.range.start >= end
}

/// The marks after `changed` was replaced by `inserted` scalars: those after it moved, those before kept, the rest
/// dropped.
fn moved(marks: &[Mark], changed: Range<usize>, inserted: usize) -> Vec<Mark> {
    marks
        .iter()
        .filter_map(|mark| {
            if mark.range.start >= changed.end {
                let start = mark.range.start - changed.end + changed.start + inserted;
                Some(Mark { range: start..start + mark.range.len(), kind: mark.kind.clone() })
            } else {
                (mark.range.end <= changed.start).then(|| mark.clone())
            }
        })
        .collect()
}

#[cfg(test)]
#[expect(clippy::single_range_in_vec_init, reason = "a run's one range is the fixture, not a range of numbers")]
mod tests {
    use super::*;
    use terms::{ChannelId, MemberId};

    const ADA: &str = "ada";
    const GENERAL: &str = "general";

    fn ada() -> MarkKind {
        MarkKind::Mention(MemberId::parse(ADA).unwrap())
    }

    fn marked(text: &str, range: Range<usize>) -> Draft {
        let mut draft = Draft::new(text);
        draft.marks = vec![Mark { range, kind: ada() }];
        draft
    }

    fn ranges(draft: &Draft) -> Vec<Range<usize>> {
        draft.marks().iter().map(|mark| mark.range.clone()).collect()
    }

    #[test]
    fn send_is_offered_only_for_a_body_the_server_takes() {
        assert_eq!(Draft::new(" \n\t").body(Carries::Nothing), Err(BodyRefused::Empty));
        assert_eq!(Draft::new(" \n\t").body(Carries::Files), Ok(""), "files alone are a message");
        assert_eq!(Draft::new("  line one\nline two\n\n").body(Carries::Nothing), Ok("line one\nline two"));
        assert_eq!(
            Draft::new("x".repeat(MAX_MESSAGE_CHARS)).body(Carries::Nothing).map(str::len),
            Ok(MAX_MESSAGE_CHARS)
        );
        assert_eq!(Draft::new("x".repeat(MAX_MESSAGE_CHARS + 1)).body(Carries::Nothing), Err(BodyRefused::TooLong));
        assert_eq!(Draft::new("a\u{202E}b").body(Carries::Nothing), Err(BodyRefused::DirectionOverride));
    }

    #[test]
    fn the_count_shows_near_the_limit_and_counts_scalars_not_bytes() {
        assert_eq!(Draft::new("é".repeat(MAX_MESSAGE_CHARS - COUNT_SHOWN_WITHIN)).shown_count(), None);
        let near = Draft::new("é".repeat(MAX_MESSAGE_CHARS - COUNT_SHOWN_WITHIN + 1));
        assert_eq!(near.shown_count(), Some(MAX_MESSAGE_CHARS - COUNT_SHOWN_WITHIN + 1));
        assert!(!near.over());
        assert!(Draft::new("é".repeat(MAX_MESSAGE_CHARS + 1)).over());
    }

    #[test]
    fn a_mark_moves_with_typing_before_it_and_stays_with_typing_after() {
        let mut draft = marked("hi @Ada", 3..7);
        draft.edit("oh hi @Ada");
        assert_eq!(ranges(&draft), [6..10]);
        draft.edit("oh hi @Ada, you");
        assert_eq!(ranges(&draft), [6..10]);
        draft.edit("hi @Ada, you");
        assert_eq!(ranges(&draft), [3..7]);
        draft.edit("é hi @Ada, you");
        assert_eq!(ranges(&draft), [5..9], "counted in scalars");
    }

    #[test]
    fn an_edit_inside_a_mark_drops_it() {
        let mut draft = marked("hi @Ada", 3..7);
        draft.edit("hi @Adx");
        assert!(draft.marks().is_empty());
        let mut draft = marked("hi @Ada", 3..7);
        draft.edit("hi @A");
        assert!(draft.marks().is_empty());
        let mut draft = marked("hi @Ada", 3..7);
        draft.edit("hi @Aada");
        assert!(draft.marks().is_empty());
    }

    #[test]
    fn an_ambiguous_insertion_is_taken_where_it_keeps_the_mark() {
        let mut draft = marked("@Ada", 0..4);
        draft.edit("@@Ada");
        assert_eq!(ranges(&draft), [1..5]);
        let mut draft = marked("@@Ada", 1..5);
        draft.edit("@Ada");
        assert_eq!(ranges(&draft), [0..4]);
    }

    #[test]
    fn a_query_starts_a_word_and_runs_to_the_cursor() {
        let query = Draft::new("hi @ad").query().unwrap();
        assert_eq!(query, Query { trigger: Trigger::Member, text: "ad".into(), range: 3..6 });
        assert_eq!(Draft::new("#").query().map(|query| query.trigger), Some(Trigger::Channel));
        assert_eq!(Draft::new("hi @").query().map(|query| query.text), Some(String::new()));
        assert!(Draft::new("hey@").query().is_none(), "not at a word's start");
        assert!(Draft::new("hi @ad ").query().is_none(), "a space ends it");
        assert!(Draft::new("at 12:30").query().is_none());
        assert!(Draft::new(":sm").query().is_some());
        assert!(Draft::new(":s").query().is_none() && Draft::new(":smile:").query().is_none());
        assert!(marked("@Ada", 0..4).query().is_none(), "a mark is no query");
        let query = Draft::new("@ad and more").query_at(3).unwrap();
        assert_eq!(query.range, 0..3);
    }

    #[test]
    fn a_completion_writes_its_text_and_mark_and_moves_the_rest() {
        let mut draft = marked("hey @ad and @Ada", 12..16);
        let query = draft.query_at(7).unwrap();
        let cursor = draft.complete(&query, Completion { text: "@Ada".into(), mark: Some(ada()) });
        assert_eq!(draft.text(), "hey @Ada and @Ada");
        assert_eq!(cursor, Some(8), "the cursor after the space that was there");
        assert_eq!(ranges(&draft), [4..8, 13..17]);
        let mut draft = Draft::new("go #gen");
        let query = draft.query().unwrap();
        let channel = MarkKind::Channel(ChannelId::parse(GENERAL).unwrap());
        assert_eq!(draft.complete(&query, Completion { text: "#general".into(), mark: Some(channel) }), Some(12));
        assert_eq!(draft.text(), "go #general ");
        let mut draft = Draft::new(":smi");
        let query = draft.query().unwrap();
        assert_eq!(draft.complete(&query, Completion { text: "😄".into(), mark: None }), Some(2));
        assert!(draft.marks().is_empty());
        assert_eq!(draft.complete(&query, Completion { text: "😄".into(), mark: None }), None, "the query is gone");
    }

    #[test]
    fn links_are_found_at_send_outside_code_and_marks() {
        let draft = marked("  @Ada see https://example.invalid/a. `http://example.invalid/b`  ", 2..6);
        let body = draft.send(Carries::Nothing, |_| Vec::new()).unwrap();
        assert_eq!(body.text(), "@Ada see https://example.invalid/a. `http://example.invalid/b`");
        let kinds: Vec<(Range<usize>, MarkKind)> =
            body.marks().iter().map(|mark| (mark.range.clone(), mark.kind.clone())).collect();
        assert_eq!(kinds, [(0..4, ada()), (9..34, MarkKind::Link)]);
    }

    #[test]
    fn the_link_a_preview_is_made_of_is_the_first_one_and_an_https_one() {
        let first = |text: &str| Draft::new(text).first_link();
        assert_eq!(
            first("see https://example.invalid/a and https://example.invalid/b"),
            Some("https://example.invalid/a".into())
        );
        assert_eq!(first("see http://example.invalid/a and https://example.invalid/b"), None, "never a later one");
        assert_eq!(first("`https://example.invalid/a` then text"), None, "code is not a link");
        assert_eq!(first("https://user@example.invalid/a"), None, "no credentials");
        assert_eq!(first("no link"), None);
    }

    #[test]
    fn a_typed_channel_name_is_a_mark_only_where_exactly_one_channel_has_it() {
        let general = ChannelId::parse("c-general").unwrap();
        let channels = |name: &str| match name {
            "general" => vec![general.clone()],
            "games" => vec![ChannelId::parse("c-games").unwrap(), ChannelId::parse("c-games-2").unwrap()],
            _ => Vec::new(),
        };
        let sent = |text: &str| -> Vec<(Range<usize>, MarkKind)> {
            let body = Draft::new(text).send(Carries::Nothing, channels).unwrap();
            body.marks().iter().map(|mark| (mark.range.clone(), mark.kind.clone())).collect()
        };
        assert_eq!(sent("see #general."), [(4..12, MarkKind::Channel(general.clone()))], "one match");
        assert_eq!(sent("#general"), [(0..8, MarkKind::Channel(general.clone()))], "at the start");
        assert_eq!(sent("see #nowhere"), [], "none stays text");
        assert_eq!(sent("see #games"), [], "two stay text");
        assert_eq!(sent("see `#general`"), [], "inside code");
        assert_eq!(sent("see a#general"), [], "after a word character");
        assert_eq!(sent("see https://example.invalid/#general").len(), 1, "a link's own, never a channel's");
        let completed = marked("#general", 0..8);
        let lounge = ChannelId::parse("c-lounge").unwrap();
        let completed =
            Draft { marks: vec![Mark { range: 0..8, kind: MarkKind::Channel(lounge.clone()) }], ..completed };
        let body = completed.send(Carries::Nothing, channels).unwrap();
        assert_eq!(body.marks().iter().map(|mark| mark.kind.clone()).collect::<Vec<_>>(), [MarkKind::Channel(lounge)]);
    }

    #[test]
    fn a_mark_inside_code_is_not_sent() {
        let draft = marked("`@Ada`", 1..5);
        assert!(draft.send(Carries::Nothing, |_| Vec::new()).unwrap().marks().is_empty());
        let draft = marked("@Ada `x`", 0..4);
        assert_eq!(draft.send(Carries::Nothing, |_| Vec::new()).unwrap().marks().len(), 1);
    }
}
