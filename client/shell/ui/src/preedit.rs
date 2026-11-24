//! The text an input method is composing, drawn over the focused field at its caret until it is committed. iced
//! draws this in its own runner only (private to `iced_winit`), so a window that hosts iced draws it here.

use crate::Theme;
use iced_core::text::{self, Paragraph as _};
use iced_core::{Color, Padding, Point, Rectangle, Size, Text, Vector, alignment, input_method, renderer};

/// What composing text is painted with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreeditColors {
    pub(crate) fill: Color,
    pub(crate) text: Color,
    pub(crate) selection: Color,
    pub(crate) underline_width: f32,
}

/// A theme a window can draw composing text in.
pub trait Composing {
    /// The colours and the underline composing text is drawn with in this theme.
    fn preedit(&self) -> PreeditColors;
}

impl Composing for Theme {
    fn preedit(&self) -> PreeditColors {
        let look = design::preedit();
        PreeditColors {
            fill: self.color(look.fill),
            text: self.color(look.text),
            selection: crate::color::translucent(self.palette().selection()),
            underline_width: look.underline_width,
        }
    }
}

/// iced's own theme, for a window drawn in it: its focused field's colours.
impl Composing for iced_core::Theme {
    fn preedit(&self) -> PreeditColors {
        let field =
            iced_widget::text_input::default(self, iced_widget::text_input::Status::Focused { is_hovered: false });
        let fill = match field.background {
            iced_core::Background::Color(color) => color,
            iced_core::Background::Gradient(_) => self.palette().background.base.color,
        };
        PreeditColors {
            fill,
            text: field.value,
            selection: field.selection,
            underline_width: design::preedit().underline_width,
        }
    }
}

/// The composing text, laid out once per change and drawn after the interface each frame.
pub struct Preedit<R: text::Renderer> {
    /// The bottom left of the caret it is drawn at.
    at: Point,
    content: R::Paragraph,
    spans: Vec<text::Span<'static>>,
    selected: bool,
}

impl<R: text::Renderer> Default for Preedit<R> {
    fn default() -> Self {
        Self { at: Point::ORIGIN, content: R::Paragraph::default(), spans: Vec::new(), selected: false }
    }
}

impl<R: text::Renderer> Preedit<R> {
    /// Lays out `preedit` at the field's caret `cursor`; the clause being converted is its own span.
    pub fn update(&mut self, cursor: Rectangle, preedit: &input_method::Preedit, renderer: &R) {
        self.at = cursor.position() + Vector::new(0.0, cursor.height);
        let content = preedit.content.as_str();
        let clauses = preedit.selection.clone().and_then(|selection| {
            Some((content.get(..selection.start)?, content.get(selection.clone())?, content.get(selection.end..)?))
        });
        let spans: Vec<text::Span<'_>> = match clauses {
            Some((before, clause, after)) => vec![
                text::Span::new(before),
                // An empty clause is still shown, as a hair's width.
                text::Span::new(if clause.is_empty() { "\u{200A}" } else { clause }),
                text::Span::new(after),
            ],
            None => vec![text::Span::new(content)],
        };
        self.selected = spans.len() == 3;
        if spans != self.spans.as_slice() {
            self.content = R::Paragraph::with_spans(Text {
                content: &spans,
                bounds: Size::INFINITE,
                size: preedit.text_size.unwrap_or_else(|| renderer.text_size()),
                line_height: renderer.line_height(),
                font: renderer.font(),
                align_x: text::Alignment::Default,
                align_y: alignment::Vertical::Top,
                shaping: text::Shaping::Advanced,
                wrapping: text::Wrapping::None,
                ellipsis: text::Ellipsis::None,
                hint_factor: renderer.hint_factor(),
            });
            self.spans = spans.into_iter().map(text::Span::to_static).collect();
        }
    }

    /// Draws it over the caret's line, kept inside `viewport`: the field's fill, the text, its underline, and the
    /// clause being converted in the selection colour.
    pub fn draw(&self, renderer: &mut R, colors: &PreeditColors, viewport: Rectangle) {
        if self.content.min_width() < 1.0 {
            return;
        }
        let mut bounds =
            Rectangle::new(self.at - Vector::new(0.0, self.content.min_height()), self.content.min_bounds());
        bounds.x = bounds.x.max(viewport.x).min(viewport.x + viewport.width - bounds.width);
        bounds.y = bounds.y.max(viewport.y).min(viewport.y + viewport.height - bounds.height);
        let quad = |bounds| renderer::Quad { bounds, ..renderer::Quad::default() };
        renderer.with_layer(bounds, |renderer| {
            renderer.fill_quad(quad(bounds), Color { a: 1.0, ..colors.fill });
            if self.selected {
                for clause in self.content.span_bounds(1) {
                    renderer.fill_quad(quad(clause + (bounds.position() - Point::ORIGIN)), colors.selection);
                }
            }
            renderer.fill_paragraph(&self.content, bounds.position(), colors.text, bounds);
            let underline = bounds.shrink(Padding { top: bounds.height - colors.underline_width, ..Padding::ZERO });
            renderer.fill_quad(quad(underline), colors.text);
        });
    }
}
