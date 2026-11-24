//! A message's body as Discord draws its markdown: bold, italic, underline and strike in the line, inline code in mono
//! on its own ground, a block of code in one box, quote lines behind a bar, a spoiler hidden on a solid ground until
//! pressed, a mention, a role, @everyone, @here and a channel on a halo in the link colour, a link underlined, each
//! emoji Twemoji draws in the emoji font, and a server's emoji as its picture in the line. Selectable, as a body is.

use crate::body_text::{BodyText, FIRST_STAND_IN, Ground, Ink, Stretch};
use crate::{Renderer, Surfaced, Theme, fonts};
use design::{
    BLOCK_GAP, CODE_BLOCK_EM, CODE_INLINE_EM, CUSTOM_EMOJI_EM, Face, MARK_PAD, MessageSize, QUOTE_BAR, Radius, Rgb,
    Role, Space, Surface, TypeStyle, Weight,
};
use iced_core::{Alignment, Element, Font, Length, Padding, Widget, font, image};
use iced_widget::{Column, Row, Space as Gap, container};
use screens::chat::{Body, Code, EmojiId, MarkKind, Run, RunKind, SpoilerId, Style, link_opens};
use std::borrow::Cow;
use std::rc::Rc;
use terms::{ChannelId, MemberId, RoleId};
use words::{Phrase, Words};

/// The colour a role is shown in, if it has one.
type RoleColour<'a> = dyn Fn(&RoleId) -> Option<Rgb> + 'a;

/// A server emoji's picture, once the host holds it.
type EmojiPicture<'a> = dyn Fn(&EmojiId) -> Option<image::Handle> + 'a;

/// What a press on a body asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BodyPress {
    /// A mentioned member's popover.
    Person(MemberId),
    /// A mentioned role's members.
    Role(RoleId),
    /// A linked channel, opened.
    Channel(ChannelId),
    /// A link, opened outside the window: its text, which is its target.
    Link(String),
    /// A hidden spoiler, shown with every run of it.
    Spoiler(SpoilerId),
}

/// A message's body at a message size, with what it needs from outside: the roles' colours, the server's emoji
/// pictures, which spoilers are shown, and what a press sends.
pub struct RichBody<'a, Message> {
    body: &'a Body,
    size: MessageSize,
    words: &'a Words,
    roles: Box<RoleColour<'a>>,
    emoji: Box<EmojiPicture<'a>>,
    revealed: Box<dyn Fn(SpoilerId) -> bool + 'a>,
    channels: Box<dyn Fn(&ChannelId) -> bool + 'a>,
    on_press: Option<Rc<dyn Fn(BodyPress) -> Option<Message> + 'a>>,
}

/// Which block a run is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Block {
    Text,
    Quote,
    Code,
}

impl Block {
    fn of(style: &Style) -> Block {
        match (style.code, style.quote) {
            (Some(Code::Block), _) => Block::Code,
            (_, true) => Block::Quote,
            (_, false) => Block::Text,
        }
    }
}

impl<'a, Message: 'a> RichBody<'a, Message> {
    /// `body` at `size`: no role has a colour, no server emoji a picture (each drawn as its `:name:`), every spoiler
    /// hidden, no channel opened (each mark of one drawn as its text alone), and a press sends nothing, until said
    /// otherwise.
    pub fn new(body: &'a Body, size: MessageSize, words: &'a Words) -> Self {
        RichBody {
            body,
            size,
            words,
            roles: Box::new(|_| None),
            emoji: Box::new(|_| None),
            revealed: Box::new(|_| false),
            channels: Box::new(|_| false),
            on_press: None,
        }
    }

    /// The channels the reader can open: a mark of one is a link; of any other, its text alone, as the rest of the
    /// line is drawn, with no press.
    pub fn channels(mut self, opens: impl Fn(&ChannelId) -> bool + 'a) -> Self {
        self.channels = Box::new(opens);
        self
    }

    /// The colour each role is shown in; none for a role with none, whose mention is drawn as a member's.
    pub fn roles(mut self, roles: impl Fn(&RoleId) -> Option<Rgb> + 'a) -> Self {
        self.roles = Box::new(roles);
        self
    }

    /// Each server emoji's picture, once the host holds it; none draws its `:name:`.
    pub fn emoji(mut self, emoji: impl Fn(&EmojiId) -> Option<image::Handle> + 'a) -> Self {
        self.emoji = Box::new(emoji);
        self
    }

    /// The spoilers shown.
    pub fn revealed(mut self, revealed: impl Fn(SpoilerId) -> bool + 'a) -> Self {
        self.revealed = Box::new(revealed);
        self
    }

    /// What a press on a mark, a link or a hidden spoiler sends, if anything.
    pub fn on_press(mut self, on_press: impl Fn(BodyPress) -> Option<Message> + 'a) -> Self {
        self.on_press = Some(Rc::new(on_press));
        self
    }

    /// The body, its blocks in order.
    pub fn view<R: Renderer + 'a>(self) -> Element<'a, Message, Theme, R> {
        let runs: Vec<Run<'a>> = self.body.runs().collect();
        let mut drawn = Drawn { stand_ins: 0, labelled: Vec::new() };
        let chunks: Vec<(Block, &[Run<'a>])> = runs
            .chunk_by(|a, b| Block::of(&a.style) == Block::of(&b.style))
            .map(|chunk| (block_of(chunk), chunk))
            .collect();
        let mut blocks: Vec<Element<'a, Message, Theme, R>> = Vec::with_capacity(chunks.len());
        for (at, (block, chunk)) in chunks.iter().enumerate() {
            let after_code = at > 0 && chunks.get(at - 1).is_some_and(|(before, _)| *before == Block::Code);
            let more = at + 1 < chunks.len();
            let trim = Trim { leading: after_code && *block != Block::Code, trailing: more && *block != Block::Code };
            let stretches = self.stretches(chunk, *block, trim, &mut drawn);
            if stretches.is_empty() {
                continue;
            }
            blocks.push(self.block(*block, stretches));
        }
        match blocks.len() {
            1 => blocks.pop().unwrap_or_else(|| Gap::new()._boxed()),
            _ => Column::with_children(blocks).spacing(BLOCK_GAP)._boxed(),
        }
    }

    fn block<R: Renderer + 'a>(&self, block: Block, stretches: Vec<Stretch<'a>>) -> Element<'a, Message, Theme, R> {
        let size = self.size.size(TypeStyle::Message);
        let line = self.size.line_height(TypeStyle::Message);
        let sans = fonts::font(TypeStyle::Message);
        match block {
            Block::Text => BodyText::new(stretches, size, line, sans).on_press(self.pressed())._boxed(),
            Block::Quote => {
                let bar =
                    container(Gap::new()).width(Length::Fixed(QUOTE_BAR)).height(Length::Fill).class(Surfaced::Fill {
                        fill: Role::Border(design::Border::Strong),
                        text: Role::Content(design::Content::Primary),
                        radius: Radius::None,
                    });
                let words = BodyText::new(stretches, size, line, sans).on_press(self.pressed());
                // A row takes Fill from its bar unless held to its words' height, which the bar then fills.
                Row::with_children([bar._boxed(), words._boxed()])
                    .spacing(Space::M.px())
                    .height(Length::Shrink)
                    .align_y(Alignment::Start)
                    ._boxed()
            }
            Block::Code => {
                let code = size * CODE_BLOCK_EM;
                let lines = (line / size * code).round();
                let words = BodyText::new(stretches, code, lines, fonts::weighted(Face::Mono, Weight::Regular))
                    .on_press(self.pressed());
                container(words)
                    .padding(Padding::from([Space::S.px(), Space::M.px()]))
                    .width(Length::Fill)
                    .class(Surfaced::Framed(Surface::Raised, Radius::S))
                    ._boxed()
            }
        }
    }

    /// What a paragraph's press sends: the body's, its none left unsent.
    fn pressed(&self) -> Option<Rc<dyn Fn(BodyPress) -> Option<Message> + 'a>> {
        self.on_press.clone()
    }

    fn stretches(&self, runs: &[Run<'a>], block: Block, trim: Trim, drawn: &mut Drawn) -> Vec<Stretch<'a>> {
        let last = runs.len().saturating_sub(1);
        runs.iter()
            .enumerate()
            .filter_map(|(at, run)| {
                let mut text = run.text;
                if trim.leading && at == 0 {
                    text = text.strip_prefix('\n').unwrap_or(text);
                }
                if trim.trailing && at == last {
                    text = text.strip_suffix('\n').unwrap_or(text);
                }
                (!text.is_empty()).then(|| self.stretch(Run { text, ..*run }, block, drawn))
            })
            .collect()
    }

    fn stretch(&self, run: Run<'a>, block: Block, drawn: &mut Drawn) -> Stretch<'a> {
        let style = run.style;
        let size = self.size.size(TypeStyle::Message);
        let hidden = style.spoiler.filter(|spoiler| !(self.revealed)(*spoiler));
        let marked = match run.kind {
            RunKind::Mark(MarkKind::Mention(_) | MarkKind::Role(_) | MarkKind::Everyone | MarkKind::Here) => true,
            RunKind::Mark(MarkKind::Channel(channel)) => (self.channels)(channel),
            RunKind::Plain | RunKind::Mark(MarkKind::CustomEmoji(_) | MarkKind::Link) => false,
        };
        let mut stretch = Stretch {
            text: Cow::Borrowed(run.text),
            said: None,
            font: font_of(&style, run.text, marked),
            size: (style.code == Some(Code::Inline) && block != Block::Code).then_some(size * CODE_INLINE_EM),
            ink: Ink::Text,
            ground: match (style.code, style.spoiler) {
                (Some(Code::Inline), _) if block != Block::Code => Some(Ground::Code),
                (_, Some(_)) => Some(Ground::Shown),
                _ => None,
            },
            pad: if style.code == Some(Code::Inline) && block != Block::Code { Space::Xs.px() } else { 0.0 },
            underline: style.underline,
            strike: style.strike,
            press: None,
            picture: None,
        };
        if marked {
            stretch.ink = Ink::Link;
            stretch.ground = Some(Ground::Halo);
            stretch.pad = MARK_PAD;
        }
        match run.kind {
            RunKind::Plain | RunKind::Mark(MarkKind::Everyone | MarkKind::Here) => {}
            RunKind::Mark(MarkKind::Mention(member)) => stretch.press = Some(BodyPress::Person(member.clone())),
            RunKind::Mark(MarkKind::Channel(channel)) if marked => {
                stretch.press = Some(BodyPress::Channel(channel.clone()));
            }
            RunKind::Mark(MarkKind::Channel(_)) => {}
            RunKind::Mark(MarkKind::Role(role)) => {
                if let Some(colour) = (self.roles)(role) {
                    stretch.ink = Ink::Picked(colour);
                    stretch.ground = Some(Ground::Picked(colour));
                }
                stretch.press = Some(BodyPress::Role(role.clone()));
            }
            RunKind::Mark(MarkKind::Link) if link_opens(run.text) => {
                stretch.ink = Ink::Link;
                stretch.underline = true;
                stretch.press = Some(BodyPress::Link(run.text.to_string()));
            }
            RunKind::Mark(MarkKind::Link) => {}
            RunKind::Mark(MarkKind::CustomEmoji(emoji)) => {
                if let Some(picture) = hidden.is_none().then(|| (self.emoji)(emoji)).flatten()
                    && let Some(stand_in) = char::from_u32(FIRST_STAND_IN + drawn.stand_ins)
                {
                    drawn.stand_ins += 1;
                    let side = size * CUSTOM_EMOJI_EM;
                    stretch.text = Cow::Owned(stand_in.to_string());
                    stretch.said = Some(Cow::Borrowed(run.text));
                    stretch.size = Some(1.0);
                    stretch.ink = Ink::Hidden;
                    stretch.ground = None;
                    stretch.pad = side / 2.0;
                    stretch.picture = Some((picture, side));
                }
            }
        }
        if let Some(spoiler) = hidden {
            stretch.ink = Ink::Hidden;
            stretch.ground = Some(Ground::Hidden);
            stretch.underline = false;
            stretch.strike = false;
            stretch.press = Some(BodyPress::Spoiler(spoiler));
            let first = !drawn.labelled.contains(&spoiler);
            drawn.labelled.push(spoiler);
            stretch.said = Some(if first {
                Cow::Owned(self.words.phrase(Phrase::SpoilerHidden).into_owned())
            } else {
                Cow::Borrowed("")
            });
        }
        stretch
    }
}

/// Which of a block's newlines are the edges it shares with its neighbours, not its own lines.
#[derive(Clone, Copy)]
struct Trim {
    leading: bool,
    trailing: bool,
}

/// What drawing a body so far has given out: the next stand-in, and the spoilers already named to a reader.
struct Drawn {
    stand_ins: u32,
    labelled: Vec<SpoilerId>,
}

fn block_of(chunk: &[Run<'_>]) -> Block {
    chunk.first().map_or(Block::Text, |run| Block::of(&run.style))
}

/// A run's font: mono in code, the emoji font for one emoji Twemoji draws, else the message's in its weight and slant.
fn font_of(style: &Style, text: &str, marked: bool) -> Font {
    if style.emoji && fonts::emoji_draws(text) {
        return fonts::emoji();
    }
    if style.code.is_some() {
        return fonts::weighted(Face::Mono, Weight::Regular);
    }
    let weight = match (style.bold, marked) {
        (true, _) => Weight::Bold,
        (false, true) => Weight::SemiBold,
        (false, false) => TypeStyle::Message.weight(),
    };
    let sans = fonts::weighted(Face::Sans, weight);
    if style.italic { Font { style: font::Style::Italic, ..sans } } else { sans }
}
