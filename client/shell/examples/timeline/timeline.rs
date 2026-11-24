//! The timeline's message list from public widgets only, each message in a `sensor` that reports its height. The scroll
//! position is a distance from the newest message, so one above the viewport taller than guessed moves nothing seen.

#![allow(
    clippy::disallowed_methods,
    reason = "a measuring spike on iced's own theme, not a screen of the product: it paints literal colours"
)]

use iced_wgpu::Renderer;
use iced_widget::container::Style;
use iced_widget::text::Rich;
use iced_widget::{Widget as _, column, container, rich_text, row, scrollable, sensor, space, span, text};
use iced_winit::core::{Background, Border, Color, Element, Font, Length, Padding, Pixels, Theme, font};

use crate::chat::{self, Piece};
use crate::heights::Heights;
use crate::rows::Rows;

/// Space under a message that starts a run, inside what is measured, so the spaces and the messages add up to the list.
const GAP: f32 = 14.0;
/// Space under a message that carries on a run.
const RUN_GAP: f32 = 3.0;
/// The avatar's diameter, and the width kept for it under a run's header.
const AVATAR: f32 = 40.0;
/// Pixels of messages built above and below the viewport, so a scroll shows them before they are measured.
const OVERSCAN: f32 = 600.0;
/// The height of the viewport until a scroll says what it is: tall, so the first screen is complete on any monitor.
const FIRST_VIEWPORT: f32 = 2400.0;
const FIRST_GUESS: f32 = 70.0;
/// The keys of the two blank spaces, beyond any message's number.
const BEFORE: usize = usize::MAX - 1;
const AFTER: usize = usize::MAX;

#[derive(Debug, Clone)]
pub(crate) enum Event {
    /// `from_end` pixels above the newest message, in a viewport `viewport` tall.
    Scrolled {
        from_end: f32,
        viewport: f32,
    },
    Measured {
        message: usize,
        height: f32,
    },
    Link(String),
}

pub(crate) struct Timeline {
    messages: Vec<chat::Message>,
    heights: Heights,
    from_end: f32,
    viewport: f32,
}

/// What a frame of the list was made of, for the readout.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Built {
    pub messages: usize,
    pub built: usize,
    pub from_end: f32,
    pub total: f32,
}

impl Timeline {
    pub(crate) fn new(count: usize, seed: u64) -> Self {
        let messages = chat::corpus(count, seed);
        Self { heights: Heights::new(messages.len(), FIRST_GUESS), messages, from_end: 0.0, viewport: FIRST_VIEWPORT }
    }

    /// True when the list has to be built again.
    pub(crate) fn update(&mut self, event: Event) -> bool {
        match event {
            Event::Scrolled { from_end, viewport } => {
                self.from_end = from_end;
                self.viewport = viewport;
                true
            }
            Event::Measured { message, height } => self.heights.measured(message, height),
            Event::Link(url) => {
                println!("a link was clicked: {url}");
                false
            }
        }
    }

    /// Where the viewport's top is, from the top of the whole list.
    fn offset(&self) -> f32 {
        (self.heights.total() - self.viewport - self.from_end).max(0.0)
    }

    /// Whether the viewport has reached the oldest message.
    pub(crate) fn at_oldest(&self) -> bool {
        self.from_end + self.viewport + 24.0 >= self.heights.total()
    }

    pub(crate) fn built(&self) -> Built {
        let window = self.heights.window(self.offset(), self.viewport, OVERSCAN);
        Built {
            messages: self.messages.len(),
            built: window.end - window.first,
            from_end: self.from_end,
            total: self.heights.total(),
        }
    }

    pub(crate) fn view(&self) -> Element<'_, Event, Theme, Renderer> {
        let window = self.heights.window(self.offset(), self.viewport, OVERSCAN);
        // Keyed by the message, so it keeps its selection and hover state as the window slides.
        let mut rows: Vec<(usize, Element<'_, Event, Theme, Renderer>)> =
            Vec::with_capacity(window.end - window.first + 2);
        rows.push((BEFORE, space().height(window.before).boxed()));
        rows.extend(
            (window.first..window.end)
                .map(|n| (n, message(n, &self.messages[n], n.checked_sub(1).map(|before| &self.messages[before])))),
        );
        rows.push((AFTER, space().height(window.after).boxed()));
        scrollable::<Event, _, Theme>(Rows::new(rows).width(Length::Fill))
            .anchor_bottom()
            .on_scroll(|scroll| Event::Scrolled {
                from_end: scroll.viewport.absolute_offset().y,
                viewport: scroll.viewport.bounds.height,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .boxed()
    }
}

const BOLD: Font = Font { weight: font::Weight::Bold, ..Font::DEFAULT };
const ITALIC: Font = Font { style: font::Style::Italic, ..Font::DEFAULT };
const DIM: Color = Color::from_rgb(0.62, 0.65, 0.70);
const BLUE: Color = Color::from_rgb(0.48, 0.64, 0.97);
const CODE: Color = Color::from_rgb(0.95, 0.72, 0.45);
const PILL: Color = Color::from_rgb(0.20, 0.22, 0.26);
const HUES: [Color; 5] = [
    Color::from_rgb(0.35, 0.40, 0.95),
    Color::from_rgb(0.23, 0.65, 0.40),
    Color::from_rgb(0.85, 0.40, 0.45),
    Color::from_rgb(0.80, 0.60, 0.20),
    Color::from_rgb(0.55, 0.38, 0.80),
];

fn rounded(color: Color, radius: f32) -> Style {
    Style {
        background: Some(Background::Color(color)),
        border: Border { radius: radius.into(), ..Border::default() },
        ..Style::default()
    }
}

fn avatar<'a>(author: &str) -> Element<'a, Event, Theme, Renderer> {
    let hue = HUES[author.bytes().map(usize::from).sum::<usize>() % HUES.len()];
    let initial = author.chars().next().map(String::from).unwrap_or_default();
    container::<_, Theme>(text(initial).size(18).color(Color::WHITE))
        .center(AVATAR)
        .style(move |_| rounded(hue, AVATAR / 2.0))
        .boxed()
}

fn message<'a>(
    number: usize,
    chat: &'a chat::Message,
    before: Option<&chat::Message>,
) -> Element<'a, Event, Theme, Renderer> {
    let run = chat.continues(before);
    let mut body: Vec<Element<'a, Event, Theme, Renderer>> = Vec::new();
    if let Some(to) = chat.reply_to {
        let reply: Rich<'_, String, Event, Theme> = rich_text([
            span("↪ ").color(DIM),
            span(format!("@{to}")).color(BLUE),
            span(" an earlier message…").color(DIM),
        ])
        .size(Pixels(13.0))
        .on_link_click(Event::Link);
        body.push(reply.boxed());
    }
    if !run {
        let when = format!("  {:02}:{:02}", chat.minute / 60 % 24, chat.minute % 60);
        let header: Rich<'_, String, Event, Theme> =
            rich_text([span(chat.author).font(BOLD).color(Color::WHITE), span(when).size(Pixels(12.0)).color(DIM)])
                .selectable(true)
                .on_link_click(Event::Link);
        body.push(header.boxed());
    }
    body.extend(chat.lines.iter().map(|pieces| line(pieces).boxed()));
    if !chat.reactions.is_empty() {
        let pills = chat.reactions.iter().map(|(emoji, count)| {
            container::<_, Theme>(text(format!("{emoji} {count}")).size(13))
                .padding([2.0, 8.0])
                .style(|_| rounded(PILL, 8.0))
                .boxed()
        });
        body.push(row(pills).spacing(6).boxed());
    }
    let left = if run { space().width(AVATAR).boxed() } else { avatar(chat.author) };
    let bottom = if run { RUN_GAP } else { GAP };
    let top = if run { 0.0 } else { 2.0 };
    sensor(
        container::<_, Theme>(row([left, column(body).spacing(2).width(Length::Fill).boxed()]).spacing(12))
            .padding(Padding { top, bottom, ..Padding::ZERO })
            .width(Length::Fill),
    )
    .on_resize(move |size| Event::Measured { message: number, height: size.height })
    .boxed()
}

fn line(pieces: &[Piece]) -> Rich<'_, String, Event, Theme> {
    let spans: Vec<_> = pieces
        .iter()
        .map(|piece| match piece {
            Piece::Plain(t) => span(t.as_str()),
            Piece::Bold(t) => span(t.as_str()).font(BOLD),
            Piece::Italic(t) => span(t.as_str()).font(ITALIC),
            Piece::Code(t) => span(t.as_str()).font(Font::MONOSPACE).color(CODE),
            Piece::Mention(t) => span(t.as_str()).font(BOLD).color(BLUE),
            Piece::Link(t) => span(t.as_str()).color(BLUE).underline(true).link(t.clone()),
        })
        .collect();
    rich_text(spans).selectable(true).on_link_click(Event::Link)
}
