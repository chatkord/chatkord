//! A line over time on faint gridlines, its scale at the right and its minutes under it, as Discord draws a call's
//! ping: the connection popover's round trip. The points and marks come placed between 0 and 1; this only stretches
//! them over the room it is given and writes the words it is handed.

use crate::fonts::font;
use crate::{Renderer, Theme};
use design::{BORDER_WIDTH, Border, Content, Role, Space, Trace, TypeStyle};
use iced_core::alignment::Vertical;
use iced_core::{Color, Element, Length, Pixels, Point, Rectangle, Size, Widget, alignment, mouse};
use iced_widget::canvas::{Canvas, Frame, Geometry, LineCap, LineJoin, Path, Program, Stroke, Text};
use screens::Graph;

/// What the graph writes: its scale, top to bottom, and a word for each of its minute marks, in their order.
pub struct GraphWords {
    /// The scale's top, its middle and its bottom.
    pub scale: [String; 3],
    /// One word for each of [`Graph::minutes`].
    pub minutes: Vec<String>,
}

/// `graph`'s lines in `role`'s colour, with `words`, drawn as `trace` says.
struct Lines {
    graph: Graph,
    words: GraphWords,
    role: Role,
    trace: Trace,
}

/// `graph` across the width it is given, `trace`'s height, its line in `role`'s colour and `words` around it.
pub fn line_graph<'a, Message: 'a, R: Renderer + 'a>(
    graph: Graph,
    words: GraphWords,
    role: Role,
    trace: Trace,
) -> Element<'a, Message, Theme, R> {
    Canvas::<Lines, Message, Theme, R>::new(Lines { graph, words, role, trace })
        .width(Length::Fill)
        .height(Length::Fixed(trace.plot + trace.marks))
        ._boxed()
}

impl<Message, R: Renderer> Program<Message, Theme, R> for Lines {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &R,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry<R>> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (wide, high) = (bounds.width - self.trace.scale, self.trace.plot);
        let grid = theme.color(Role::Border(Border::Subtle));
        let quiet = theme.color(Role::Content(Content::Secondary));
        // The line's centre stays half its width inside the plot, so a round trip of nothing (loopback's) is whole.
        let inset = self.trace.stroke / 2.0;
        let across = |x: f32| inset + x * (wide - self.trace.stroke);
        let at =
            |point: &screens::Point| Point::new(across(point.x), inset + (1.0 - point.y) * (high - self.trace.stroke));
        for y in [0.0, (high - BORDER_WIDTH) / 2.0, high - BORDER_WIDTH] {
            frame.fill_rectangle(Point::new(0.0, y), Size::new(wide, BORDER_WIDTH), grid);
        }
        for minute in self.graph.minutes.iter().filter(|minute| minute.ago > 0) {
            let x = across(minute.x) - BORDER_WIDTH / 2.0;
            frame.fill_rectangle(Point::new(x, 0.0), Size::new(BORDER_WIDTH, high), grid);
        }
        let colour = theme.color(self.role);
        for line in &self.graph.lines {
            let Some(first) = line.first() else { continue };
            let trace = Path::new(|path| {
                path.move_to(at(first));
                line.iter().skip(1).for_each(|point| path.line_to(at(point)));
            });
            let stroke = Stroke::default()
                .with_color(colour)
                .with_width(self.trace.stroke)
                .with_line_cap(LineCap::Round)
                .with_line_join(LineJoin::Round);
            frame.stroke(&trace, stroke);
        }
        let number = TypeStyle::Mono;
        let reach = number.size() / 2.0;
        let left = wide + Space::S.px() - Space::Xxs.px();
        for (word, y) in self.words.scale.iter().zip([0.0, high / 2.0, high]) {
            let centre = y.clamp(reach, high - reach);
            frame.fill_text(words(word, Point::new(left, centre), number, quiet, alignment::Horizontal::Left));
        }
        let mark = TypeStyle::Caption;
        let row = high + self.trace.marks - mark.line_height() / 2.0;
        for (minute, word) in self.graph.minutes.iter().zip(&self.words.minutes) {
            // The latest look's word ends at the plot's edge; each other is centred on its mark.
            let (x, align) = if minute.ago == 0 {
                (wide, alignment::Horizontal::Right)
            } else {
                (across(minute.x), alignment::Horizontal::Center)
            };
            frame.fill_text(words(word, Point::new(x, row), mark, quiet, align));
        }
        vec![frame.into_geometry()]
    }
}

/// `content` in `style`, its middle at `at`'s height and placed across by `align`.
fn words(content: &str, at: Point, style: TypeStyle, color: Color, align: alignment::Horizontal) -> Text {
    Text {
        content: content.to_string(),
        position: at,
        color,
        size: Pixels(style.size()),
        font: font(style),
        align_x: align.into(),
        align_y: Vertical::Center,
        ..Text::default()
    }
}
