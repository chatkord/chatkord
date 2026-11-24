//! How the call's connection is, in a popover over the call panel: the verdict in words and colour, what is sent and
//! what is heard, and what the call has carried. It is `Layered`, so Escape and a press outside close it.

use crate::layer::{Layered, Placement, on_panel};
use crate::panel_parts::{hairline, section_heading};
use crate::{GraphWords, Labelled, Renderer, Surfaced, TextClass, Theme, coloured, text};
use design::{
    Content, Label, MENU_RADIUS, ROUND_TRIP_TRACE, Radius, Role, STATS_POPOVER_WIDTH, SemanticRole, Semantics, Space,
    Status, StatusRole, TypeStyle,
};
use iced_core::{Alignment, Element, Length, Widget};
use iced_widget::{Column, Row, Space as Gap, container};
use proto::video::Video;
use screens::{Flow, Health, KEPT, OwnVideo, RoundTrips, Route};
use words::{Phrase, Sentence, Words};

/// The room between a heading and its rows, and between rows: the canvas's 6.
const TIGHT: f32 = Space::S.px() - Space::Xxs.px();

/// The popover `title`d for a screen reader: `health` as the call last looked, the `round_trips` of the looks before,
/// whether nobody else is in the room, and this account's `own` share or camera, with a tab each that `show`s one
/// while both are sent.
pub fn stats_popover<'a, Message: Clone + 'a, R: Renderer + 'a>(
    title: Label,
    health: Option<&Health>,
    round_trips: &RoundTrips,
    alone: bool,
    own: Option<OwnVideo<'_>>,
    show: impl Fn(Video) -> Message,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let health = health.copied().unwrap_or_default();
    let sending = Column::with_children([
        section_heading(&crate::labels::phrase(Phrase::StatsSending, words))._boxed(),
        figures(&health.sending, Some(health.round_trip.map(|trip| trip.took)), 0, words),
    ])
    .spacing(TIGHT)
    .width(Length::FillPortion(1));
    let receiving = Column::with_children([
        section_heading(&crate::labels::phrase(Phrase::StatsReceiving, words))._boxed(),
        if alone {
            coloured(words.phrase(Phrase::StatsAlone), TypeStyle::Small, TextClass::Content(Content::Secondary))
                ._boxed()
        } else {
            figures(&health.receiving, None, health.lost, words)
        },
    ])
    .spacing(TIGHT)
    .width(Length::FillPortion(1));
    let totals = Column::with_children([
        section_heading(&crate::labels::phrase(Phrase::StatsThisCall, words))._boxed(),
        Row::with_children([
            coloured(
                words.say(&Sentence::Up(words.bytes(health.up_bytes))),
                TypeStyle::Mono,
                TextClass::Content(Content::Secondary),
            )
            ._boxed(),
            coloured(
                words.say(&Sentence::Down(words.bytes(health.down_bytes))),
                TypeStyle::Mono,
                TextClass::Content(Content::Secondary),
            )
            ._boxed(),
        ])
        .spacing(Space::L.px() + Space::Xs.px())
        ._boxed(),
    ])
    .spacing(TIGHT);
    let mut sections: Vec<Element<'a, Message, Theme, R>> = vec![
        heading(
            health.quality.map(|quality| (quality.status(), crate::labels::phrase(quality.word(), words))),
            &crate::labels::phrase(Phrase::StatsTitle, words),
        ),
        Row::with_children([sending._boxed(), receiving._boxed()]).spacing(Space::L.px())._boxed(),
    ];
    // The graph waits for a first round trip, as the verdict does, and takes its colour.
    if let Some(quality) = health.quality {
        sections.push(round_trip_graph(round_trips, quality.status(), words));
    }
    // One section of this account's own video, only while one is sent: under a tab each when both are, so the
    // popover is no taller with both than with one.
    if let Some(own) = own {
        sections.push(hairline()._boxed());
        sections.push(own_video(own, show, words));
    }
    sections.push(hairline()._boxed());
    sections.push(totals._boxed());
    let column = Column::with_children(sections)
        .spacing(Space::M.px())
        .width(Length::Fill.max(STATS_POPOVER_WIDTH - 2.0 * Space::M.px()));
    let panel = on_panel(column._boxed(), Space::M, MENU_RADIUS);
    Labelled::new(panel, Semantics { role: SemanticRole::Dialog, label: title })._boxed()
}

/// The round trips of the last minutes as a line in the verdict's colour, its scale and its minutes around it.
fn round_trip_graph<'a, Message: 'a, R: Renderer + 'a>(
    round_trips: &RoundTrips,
    status: Status,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let graph = round_trips.graph();
    let number = |time: std::time::Duration| words.count(u64::try_from(time.as_millis()).unwrap_or(u64::MAX));
    let marks = GraphWords {
        scale: [number(graph.top), number(graph.half()), number(std::time::Duration::ZERO)],
        minutes: graph
            .minutes
            .iter()
            .map(|minute| match minute.ago {
                0 => words.phrase(Phrase::StatsNow).to_string(),
                ago => words.say(&Sentence::MinutesAgo(ago)),
            })
            .collect(),
    };
    Column::with_children([
        coloured(
            words.say(&Sentence::RoundTripOver(KEPT.as_secs() / 60)),
            TypeStyle::Small,
            TextClass::Content(Content::Secondary),
        )
        ._boxed(),
        crate::line_graph(graph, marks, Role::Status(status, StatusRole::Text), ROUND_TRIP_TRACE),
    ])
    .spacing(TIGHT)
    ._boxed()
}

/// What this account's own share or camera is doing, under its heading or the tabs that `show` either: how it is
/// coded, encoded and sent, and what holds it back, if anything.
fn own_video<'a, Message: Clone + 'a, R: Renderer + 'a>(
    own: OwnVideo<'_>,
    show: impl Fn(Video) -> Message,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let heading = |video| match video {
        Video::Screen => crate::labels::phrase(Phrase::StatsYourShare, words),
        Video::Camera => crate::labels::phrase(Phrase::StatsYourCamera, words),
    };
    let (head, outgoing, watch) = match own {
        OwnVideo::Alone(video, outgoing, watch) => (section_heading(&heading(video))._boxed(), outgoing, watch),
        OwnVideo::Tabbed { shown, outgoing, watch } => {
            let picked = match shown {
                Video::Screen => 0,
                Video::Camera => 1,
            };
            let track = crate::choice_track(
                crate::labels::phrase(Phrase::StatsYourVideo, words),
                [Video::Screen, Video::Camera].into_iter().map(|video| (heading(video), show(video))).collect(),
                picked,
            );
            (track, outgoing, watch)
        }
    };
    let mut rows = vec![
        head,
        figure(Phrase::StatsCodec, outgoing.codec.clone(), None, words),
        encoder(&outgoing.encoder, words),
        figure(Phrase::StatsSize, words.dimensions(outgoing.width, outgoing.height), None, words),
    ];
    if let Some(frame_rate) = watch.frame_rate(outgoing, words) {
        rows.push(figure(Phrase::StatsFrameRate, frame_rate, None, words));
    }
    let sent = watch.rate_instead(words).unwrap_or_else(|| words.rate(outgoing.bits_per_second));
    rows.push(figure(Phrase::StatsRate, sent, None, words));
    rows.push(figure(Phrase::StatsFramesSent, words.count(outgoing.frames), None, words));
    let key_frames = Sentence::KeyFrames { made: outgoing.key_frames_made, asked: outgoing.key_frames_asked };
    rows.push(figure(Phrase::StatsKeyFrames, words.say(&key_frames), None, words));
    rows.push(figure(Phrase::StatsResent, words.bytes(outgoing.resent_bytes), None, words));
    rows.push(figure(Phrase::StatsRestarts, words.count(outgoing.restarts), None, words));
    if outgoing.limited {
        rows.push(figure(
            Phrase::StatsLimitedBy,
            words.phrase(Phrase::StatsBandwidth).to_string(),
            Some(Status::Warning),
            words,
        ));
    }
    Column::with_children(rows).spacing(TIGHT)._boxed()
}

/// `screen`, with `popover` over it while it is `Some`, its bottom edge at `bottom` and its left edge at `left`.
pub fn stats_popover_over<'a, Message: Clone + 'a, R: Renderer + 'a>(
    screen: Element<'a, Message, Theme, R>,
    popover: Option<Element<'a, Message, Theme, R>>,
    (left, bottom): (f32, f32),
    on_close: Message,
) -> Element<'a, Message, Theme, R> {
    Layered::new(screen, popover, Placement::Rising { left, bottom }, on_close, false)._boxed()
}

/// "Connection", and at its end the verdict: a dot and the word, in the status's colour.
fn heading<'a, Message: 'a, R: Renderer + 'a>(
    verdict: Option<(Status, Label)>,
    title: &Label,
) -> Element<'a, Message, Theme, R> {
    let mut row =
        Row::with_children([text(title.as_str().to_string(), TypeStyle::BodyStrong).width(Length::Fill)._boxed()])
            .align_y(Alignment::Center)
            .spacing(TIGHT);
    if let Some((status, word)) = verdict {
        let colour = Role::Status(status, StatusRole::Text);
        let dot = container(Gap::new())
            .width(Length::Fixed(Space::S.px()))
            .height(Length::Fixed(Space::S.px()))
            .class(Surfaced::Fill { fill: colour, text: colour, radius: Radius::Full });
        let word = coloured(word.as_str().to_string(), TypeStyle::CaptionStrong, TextClass::Status(status));
        row = row.push(dot._boxed()).push(word._boxed());
    }
    row._boxed()
}

/// A direction's rows: its rate and packets, then its round trip when it is shown, then what was lost when any was.
fn figures<'a, Message: 'a, R: Renderer + 'a>(
    flow: &Flow,
    round_trip: Option<Option<std::time::Duration>>,
    lost: u64,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let mut rows = vec![
        figure(Phrase::StatsRate, words.rate(flow.bits_per_second), None, words),
        figure(Phrase::StatsPackets, words.count(flow.packets), None, words),
    ];
    if let Some(round_trip) = round_trip {
        rows.push(figure(Phrase::StatsRoundTrip, words.millis(round_trip), None, words));
    }
    if lost > 0 {
        rows.push(figure(Phrase::StatsLost, words.count(lost), Some(Status::Danger), words));
    }
    Column::with_children(rows).spacing(TIGHT)._boxed()
}

/// A name, and its `value` under it, so a long value never takes the name's room.
fn named<'a, Message: 'a, R: Renderer + 'a>(
    name: Phrase,
    value: Element<'a, Message, Theme, R>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    Column::with_children([
        coloured(words.phrase(name), TypeStyle::Small, TextClass::Content(Content::Secondary))
            .width(Length::Fill)
            ._boxed(),
        value,
    ])
    ._boxed()
}

/// A name and its figure under it; the figure in a status's colour when it has one.
fn figure<'a, Message: 'a, R: Renderer + 'a>(
    name: Phrase,
    value: String,
    status: Option<Status>,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let class = status.map_or(TextClass::Content(Content::Primary), TextClass::Status);
    named(name, coloured(value, TypeStyle::Mono, class)._boxed(), words)
}

/// The Encoder row: hardware in the ok colour after the ok dot, as the design draws it, software in plain text.
fn encoder<'a, Message: 'a, R: Renderer + 'a>(route: &Route, words: &'a Words) -> Element<'a, Message, Theme, R> {
    let said = screens::encoder_row(route, words);
    let value = match route {
        Route::Hardware { .. } | Route::UntestedHardware { .. } => Row::with_children([
            crate::status_dot::status_dot::<Message, R>(Status::Success),
            coloured(said, TypeStyle::Small, TextClass::Status(Status::Success))._boxed(),
        ])
        .spacing(TIGHT)
        .align_y(Alignment::Center)
        ._boxed(),
        Route::Software => coloured(said, TypeStyle::Small, TextClass::Content(Content::Primary))._boxed(),
    };
    named(Phrase::StatsEncoder, value, words)
}
