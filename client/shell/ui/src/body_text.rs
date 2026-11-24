//! One paragraph of a message's styled text: its stretches shaped once, each in a colour named by what it is and read
//! from the theme when drawn, a mark's ground and a link's line drawn with it, a server's emoji painted as its picture
//! in the room its stretch keeps, and selected and copied with each such emoji's `:name:`.

use crate::Theme;
use crate::color::color;
use crate::rich_body::BodyPress;
use crate::theme::TextClass;
use design::{AccentRole, BORDER_WIDTH, Border as Edge, MARK_RADIUS, Palette, Rgb, Role, Surface};
use iced_core::text::{Alignment, Ellipsis, Fragment, LineHeight, Paragraph, Shaping, Span, Target, Wrapping};
use iced_core::widget::text::Catalog;
use iced_core::widget::tree::{self, Tree};
use iced_core::widget::{Meta, Operation, operation};
use iced_core::{
    Background, Border, Color, Event, Font, Layout, Length, Padding, Pixels, Point, Rectangle, Shell, Size, Vector,
    Widget, alignment, image, layout, mouse, renderer, window,
};
use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

/// The first of the private-use scalars a body's server emoji stand in its shaped text by, one each, in order.
pub(crate) const FIRST_STAND_IN: u32 = 0xE000;

/// What colour a stretch's letters are, named by what it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ink {
    /// The body's own.
    Text,
    /// A link's or a mention's: the accent's text.
    Link,
    /// A role's mention: its colour, where that reads on its ground.
    Picked(Rgb),
    /// A hidden spoiler's, and a server emoji's stand-in: none.
    Hidden,
}

/// What a stretch is drawn on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ground {
    /// A mention's or a channel's: the accent's tint.
    Halo,
    /// A role's mention: its colour laid thinly over the base.
    Picked(Rgb),
    /// Inline code: the raised surface inside a hairline.
    Code,
    /// A hidden spoiler: the strong edge's colour, which nothing shows through.
    Hidden,
    /// A spoiler shown: the hovered surface.
    Shown,
}

/// A stretch of a paragraph drawn one way.
pub(crate) struct Stretch<'a> {
    /// What is shaped: its words, or a server emoji's stand-in.
    pub(crate) text: Cow<'a, str>,
    /// What a reader is given in its place: a server emoji's `:name:`, a hidden spoiler's word; none for its text.
    pub(crate) said: Option<Cow<'a, str>>,
    /// Its font.
    pub(crate) font: Font,
    /// Its size, where it is not the paragraph's.
    pub(crate) size: Option<f32>,
    /// Its letters' colour.
    pub(crate) ink: Ink,
    /// What it is drawn on, if anything.
    pub(crate) ground: Option<Ground>,
    /// The room kept before and after it, which its ground covers.
    pub(crate) pad: f32,
    /// A line under it.
    pub(crate) underline: bool,
    /// A line through it.
    pub(crate) strike: bool,
    /// What a press on it asks for.
    pub(crate) press: Option<BodyPress>,
    /// The picture painted over it, a side wide: a server emoji's.
    pub(crate) picture: Option<(image::Handle, f32)>,
}

/// A paragraph of stretches at a size, selectable.
pub(crate) struct BodyText<'a, Message> {
    stretches: Vec<Stretch<'a>>,
    size: f32,
    line_height: f32,
    font: Font,
    on_press: Option<Rc<dyn Fn(BodyPress) -> Option<Message> + 'a>>,
    hovered: Option<usize>,
    over_glyph: bool,
}

impl<'a, Message> BodyText<'a, Message> {
    /// `stretches` set at `size` on lines `line_height` tall, in `font` where a stretch names none.
    pub(crate) fn new(stretches: Vec<Stretch<'a>>, size: f32, line_height: f32, font: Font) -> Self {
        BodyText { stretches, size, line_height, font, on_press: None, hovered: None, over_glyph: false }
    }

    /// The stretch under `cursor` that a press asks for something by, read from the shaped text itself, so a press
    /// needs no move of the pointer before it.
    fn pressable_at<P: iced_core::text::Paragraph>(
        &self,
        paragraph: &P,
        layout: Layout,
        cursor: mouse::Cursor,
    ) -> Option<usize> {
        self.on_press.as_ref()?;
        let position = cursor.position_in(layout.bounds())?;
        paragraph
            .hit_span(position)
            .filter(|index| self.stretches.get(*index).is_some_and(|stretch| stretch.press.is_some()))
    }

    /// What a press on a stretch that asks for something sends.
    pub(crate) fn on_press(mut self, on_press: Option<Rc<dyn Fn(BodyPress) -> Option<Message> + 'a>>) -> Self {
        self.on_press = on_press;
        self
    }

    /// The spans shaped, coloured from `palette` once a theme has been drawn with.
    fn spans(&self, palette: Option<&Palette>) -> Vec<Span<'_, ()>> {
        self.stretches
            .iter()
            .map(|stretch| Span {
                text: Fragment::Borrowed(stretch.text.as_ref()),
                size: stretch.size.map(Pixels),
                line_height: None,
                font: Some(stretch.font),
                color: palette.and_then(|palette| ink(palette, stretch.ink)),
                link: None,
                highlight: None,
                padding: Padding { left: stretch.pad, right: stretch.pad, ..Padding::ZERO },
                underline: false,
                strikethrough: false,
            })
            .collect()
    }

    fn shape<P: Paragraph>(&self, spans: &[Span<'_, ()>], bounds: Size, hint_factor: Option<f32>) -> P {
        P::with_spans(iced_core::Text {
            content: spans,
            bounds,
            size: Pixels(self.size),
            line_height: LineHeight::Absolute(Pixels(self.line_height)),
            font: self.font,
            align_x: Alignment::Default,
            align_y: alignment::Vertical::Top,
            shaping: Shaping::Advanced,
            wrapping: Wrapping::default(),
            ellipsis: Ellipsis::default(),
            hint_factor,
        })
    }

    /// What a reader is given: each stretch's words, or what it says in their place.
    fn said(&self) -> String {
        self.stretches.iter().map(|stretch| stretch.said.as_deref().unwrap_or(stretch.text.as_ref())).collect()
    }

    /// The `:name:` each stand-in is, by its scalar.
    fn names(&self) -> Vec<(char, String)> {
        self.stretches
            .iter()
            .filter(|stretch| stretch.picture.is_some())
            .filter_map(|stretch| Some((stretch.text.chars().next()?, stretch.said.as_deref()?.to_string())))
            .collect()
    }
}

/// `ink`'s colour in `palette`; none for the paragraph's own.
fn ink(palette: &Palette, ink: Ink) -> Option<Color> {
    match ink {
        Ink::Text => None,
        Ink::Link => Some(color(palette.color(Role::Accent(AccentRole::Text)))),
        Ink::Picked(picked) => Some(color(palette.mark(picked).text)),
        Ink::Hidden => Some(Color::TRANSPARENT),
    }
}

/// `ground`'s fill and edge in `palette`.
fn ground(palette: &Palette, ground: Ground) -> (Color, Border) {
    let radius = MARK_RADIUS.into();
    let plain = |role: Role| (color(palette.color(role)), Border { radius, ..Border::default() });
    match ground {
        Ground::Halo => plain(Role::Accent(AccentRole::Tint)),
        Ground::Picked(picked) => (color(palette.mark(picked).ground), Border { radius, ..Border::default() }),
        Ground::Code => (
            color(palette.color(Role::Surface(Surface::Raised))),
            Border { radius, width: BORDER_WIDTH, color: color(palette.color(Role::Border(Edge::Subtle))) },
        ),
        Ground::Hidden => plain(Role::Border(Edge::Strong)),
        Ground::Shown => plain(Role::Surface(Surface::Hover)),
    }
}

struct Shaped<P> {
    palette: Option<Palette>,
    spans: Vec<Span<'static, ()>>,
    paragraph: P,
    pressed: Option<usize>,
}

/// Interior so a draw in a theme it was not shaped in can colour it again: colours never move a glyph, so the
/// layout it answers stays true.
struct State<P> {
    shaped: RefCell<Shaped<P>>,
}

impl<Message> Meta for BodyText<'_, Message> {}

impl<Message, R> Widget<Message, Theme, R> for BodyText<'_, Message>
where
    R: iced_core::text::Renderer + image::Renderer<Handle = image::Handle>,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State<R::Paragraph>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::<R::Paragraph> {
            shaped: RefCell::new(Shaped {
                palette: None,
                spans: Vec::new(),
                paragraph: R::Paragraph::default(),
                pressed: None,
            }),
        })
    }

    fn size(&self) -> Size<Length> {
        Size { width: Length::Shrink, height: Length::Shrink }
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &R, limits: &layout::Limits) {
        let shaped = tree.state.downcast_mut::<State<R::Paragraph>>().shaped.get_mut();
        tree.size = layout::sized(limits, Length::Shrink, Length::Shrink, |limits| {
            let bounds = limits.bounds();
            let spans = self.spans(shaped.palette.as_ref());
            if shaped.spans != spans {
                shaped.paragraph = self.shape(&spans, bounds, renderer.hint_factor());
                shaped.spans = spans.into_iter().map(Span::to_static).collect();
            } else {
                match shaped.paragraph.compare(iced_core::Text {
                    content: (),
                    bounds,
                    size: Pixels(self.size),
                    line_height: LineHeight::Absolute(Pixels(self.line_height)),
                    font: self.font,
                    align_x: Alignment::Default,
                    align_y: alignment::Vertical::Top,
                    shaping: Shaping::Advanced,
                    wrapping: Wrapping::default(),
                    ellipsis: Ellipsis::default(),
                    hint_factor: renderer.hint_factor(),
                }) {
                    iced_core::text::Difference::None => {}
                    iced_core::text::Difference::Bounds => shaped.paragraph.resize(bounds),
                    iced_core::text::Difference::Shape => {
                        shaped.paragraph = self.shape(&spans, bounds, renderer.hint_factor());
                    }
                }
            }
            shaped.paragraph.min_bounds()
        });
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut R,
        theme: &Theme,
        defaults: &renderer::Style,
        layout: Layout,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if !layout.bounds().intersects(viewport) {
            return;
        }
        let mut shaped = tree.state.downcast_ref::<State<R::Paragraph>>().shaped.borrow_mut();
        let palette = theme.palette();
        if shaped.palette.as_ref() != Some(palette) {
            let spans = self.spans(Some(palette));
            shaped.paragraph = self.shape(&spans, shaped.paragraph.bounds(), renderer.hint_factor());
            shaped.spans = spans.into_iter().map(Span::to_static).collect();
            shaped.palette = Some(palette.clone());
        }
        let translation = layout.position() - Point::ORIGIN;
        for (index, stretch) in self.stretches.iter().enumerate() {
            let Some(drawn_on) = stretch.ground else { continue };
            let (fill, edge) = ground(palette, drawn_on);
            let regions = shaped.paragraph.span_bounds(index);
            let last = regions.len().saturating_sub(1);
            for (at, region) in regions.iter().enumerate() {
                let left = if at == 0 { stretch.pad } else { 0.0 };
                let right = if at == last { stretch.pad } else { 0.0 };
                let bounds = Rectangle::new(
                    region.position() - Vector::new(left, 0.0),
                    region.size() + Size::new(left + right, 0.0),
                );
                renderer.fill_quad(
                    renderer::Quad { bounds: bounds + translation, border: edge, ..renderer::Quad::default() },
                    Background::Color(fill),
                );
            }
        }
        let style = <Theme as Catalog>::style(theme, &TextClass::Inherit);
        iced_core::widget::text::draw(
            renderer,
            defaults,
            layout.bounds(),
            &shaped.paragraph,
            style,
            <Theme as Catalog>::selection(theme),
            viewport,
        );
        for (index, stretch) in self.stretches.iter().enumerate() {
            let regions = shaped.paragraph.span_bounds(index);
            if let Some((picture, side)) = &stretch.picture {
                // The stand-in's ink is a pixel tall; its line box is a whole line, every line being one height.
                if let Some(region) = regions.first() {
                    let line_top = (region.center_y() / self.line_height).floor() * self.line_height;
                    let at = Point::new(region.center_x() - side / 2.0, line_top + (self.line_height - side) / 2.0);
                    let bounds = Rectangle::new(at, Size::new(*side, *side)) + translation;
                    renderer.draw_image(image::Image::new(picture.clone()), bounds, *viewport);
                }
            }
            if !(stretch.underline || stretch.strike) {
                continue;
            }
            let size = stretch.size.unwrap_or(self.size);
            let colour = ink(palette, stretch.ink).or(style.color).unwrap_or(defaults.text_color);
            let baseline = translation + Vector::new(0.0, size + (self.line_height - size) / 2.0);
            for region in &regions {
                let mut lines = Vec::with_capacity(2);
                if stretch.underline {
                    lines.push(region.position() + baseline);
                }
                if stretch.strike {
                    lines.push(region.position() + baseline - Vector::new(0.0, size / 2.0));
                }
                for start in lines {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle::new(start, Size::new(region.width, BORDER_WIDTH)),
                            ..renderer::Quad::default()
                        },
                        Background::Color(colour),
                    );
                }
            }
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        _renderer: &R,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let shaped = tree.state.downcast_mut::<State<R::Paragraph>>().shaped.get_mut();
        match event {
            Event::Mouse(mouse::Event::CursorMoved { .. }) | Event::Window(window::Event::RedrawRequested(_)) => {
                let Some(position) = cursor.position_in(layout.bounds()) else {
                    self.over_glyph = false;
                    self.hovered = None;
                    return;
                };
                self.over_glyph = shaped.paragraph.hit_glyph(position);
                if self.on_press.is_none() {
                    return;
                }
                let was = self.hovered.is_some();
                self.hovered = shaped
                    .paragraph
                    .hit_span(position)
                    .filter(|index| self.stretches.get(*index).is_some_and(|stretch| stretch.press.is_some()));
                if was != self.hovered.is_some() && !matches!(event, Event::Window(_)) {
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(under) = self.pressable_at(&shaped.paragraph, layout, cursor) else { return };
                shaped.pressed = Some(under);
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let pressed = shaped.pressed.take();
                let Some(on_press) = &self.on_press else { return };
                let under = self.pressable_at(&shaped.paragraph, layout, cursor);
                if let Some(index) = pressed.filter(|index| Some(*index) == under)
                    && let Some(press) = self.stretches.get(index).and_then(|stretch| stretch.press.clone())
                    && let Some(message) = on_press(press)
                {
                    shell.publish(message);
                }
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        _layout: Layout,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &R,
    ) -> mouse::Interaction {
        if self.hovered.is_some() {
            mouse::Interaction::Pointer
        } else if self.over_glyph {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::None
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        _viewport: &Rectangle,
        _renderer: &R,
        operation: &mut dyn Operation,
    ) {
        let shaped = tree.state.downcast_mut::<State<R::Paragraph>>().shaped.get_mut();
        let mut operand = Operand { paragraph: &mut shaped.paragraph, layout, said: self.said(), names: self.names() };
        operation.text(None, layout.bounds(), &mut operand);
    }
}

struct Operand<'a, P> {
    paragraph: &'a mut P,
    layout: Layout,
    said: String,
    names: Vec<(char, String)>,
}

impl<P: Paragraph> operation::Text for Operand<'_, P> {
    fn text(&self) -> Fragment<'_> {
        Fragment::Borrowed(&self.said)
    }

    fn select(&mut self, start: Point, end: Point, target: Target) {
        let translation = self.layout.position() - Point::ORIGIN;
        self.paragraph.select(start - translation, end - translation, target);
    }

    fn select_all(&mut self) {
        self.paragraph.select_all();
    }

    fn deselect(&mut self) {
        self.paragraph.deselect();
    }

    fn copy(&mut self) -> Option<String> {
        let copied = self.paragraph.copy()?;
        Some(copied.chars().fold(String::with_capacity(copied.len()), |mut out, scalar| {
            match self.names.iter().find(|(stand_in, _)| *stand_in == scalar) {
                Some((_, name)) => out.push_str(name),
                None => out.push(scalar),
            }
            out
        }))
    }
}
