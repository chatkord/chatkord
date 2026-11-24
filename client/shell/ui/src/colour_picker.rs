//! The colour picker: the colour as a swatch with its hex beside it; pressed, a panel under it with an area of
//! saturation and brightness, a hue strip, the Hex field, Reset and Done. It draws a `Picker` and decides nothing.

use crate::button::{ButtonClass, actionable};
use crate::door::named;
use crate::{Button, Icon, IconClass, Labelled, Renderer, Surfaced, TextInput, Theme, icon, text};
use design::{
    ButtonVariant, CARD_RADIUS, COLOUR_PICKER_AREA, COLOUR_PICKER_HANDLE, COLOUR_PICKER_HUE_HEIGHT,
    COLOUR_PICKER_WIDTH, COLOUR_SWATCH, Elevation, INPUT_RADIUS, IconSize, Label, Radius, Rgb, Role, SemanticRole,
    Semantics, Space, TypeStyle,
};
use iced_core::gradient::Linear;
use iced_core::widget::{Tree, tree};
use iced_core::{
    Alignment, Background, Border, Color, Element, Event, Layout, Length, Padding, Point, Radians, Rectangle, Shell,
    Size, Widget, layout, mouse, renderer,
};
use iced_widget::{Column, Row, button, container, hover};
use screens::colour::{Hsv, Message as PickerMessage, Picker, rgb};
use std::f32::consts::{FRAC_PI_2, PI};
use words::{Phrase, Words};

/// The picker of `picker`, called `name`. Each thing a person does is sent through `on`.
pub(crate) fn colour_picker<'a, Message: Clone + 'a, R: Renderer + 'a>(
    name: &Label,
    picker: &'a Picker,
    on: impl Fn(PickerMessage) -> Message + Clone + 'a,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    picker_helped(name, None, picker, on, words)
}

/// [`colour_picker`] with `help` under its swatch, over the open panel: where the colour is shown.
pub(crate) fn colour_picker_helped<'a, Message: Clone + 'a, R: Renderer + 'a>(
    name: &Label,
    help: String,
    picker: &'a Picker,
    on: impl Fn(PickerMessage) -> Message + Clone + 'a,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    picker_helped(name, Some(help), picker, on, words)
}

fn picker_helped<'a, Message: Clone + 'a, R: Renderer + 'a>(
    name: &Label,
    help: Option<String>,
    picker: &'a Picker,
    on: impl Fn(PickerMessage) -> Message + Clone + 'a,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let picked = picker.picked();
    let heading = text(name.as_str().to_string(), TypeStyle::Label).width(Length::Fill);
    let swatch = container(iced_widget::Space::new())
        .width(Length::Fixed(COLOUR_SWATCH.0))
        .height(Length::Fixed(COLOUR_SWATCH.1))
        .class(Surfaced::Swatch(rgb(picked)));
    // The pencil shows on the swatch under the pointer, on the scrim's round as the canvas draws it.
    let pencil = container(
        container(icon(Icon::Pencil, IconSize::S, IconClass::Role(Role::OnScrim)))
            .padding(Space::Xs.px())
            .class(Surfaced::Scrim),
    )
    .center(Length::Fill);
    let toggle = on(PickerMessage::Toggle);
    let inner = button::Button::new(hover(swatch, pencil))
        .class(ButtonClass::Swatch)
        .padding(Padding::ZERO)
        .on_press(toggle.clone());
    let trigger = actionable(inner, Semantics { role: SemanticRole::Button, label: name.clone() }, Some(toggle));
    let shown = Row::with_children([trigger, text(picked.to_string(), TypeStyle::Mono)._boxed()])
        .spacing(Space::M.px())
        .align_y(Alignment::Center);
    let mut parts = vec![heading._boxed(), shown._boxed()];
    parts.extend(help.map(|help| crate::field_message(screens::FieldTone::Help, help)));
    if picker.is_open() {
        parts.push(panel(picker, on, words));
    }
    Column::with_children(parts).spacing(Space::Xs.px())._boxed()
}

/// The open panel: the area and the strip, the Hex field with Reset, and Done. Escape is Done.
fn panel<'a, Message: Clone + 'a, R: Renderer + 'a>(
    picker: &'a Picker,
    on: impl Fn(PickerMessage) -> Message + Clone + 'a,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let hsv = picker.hsv();
    let (to_area, to_hue, to_typed) = (on.clone(), on.clone(), on.clone());
    let area = field(Kind::Area, hsv, Phrase::ColourArea, move |x, y| to_area(PickerMessage::Area { x, y }), words);
    let hue = field(Kind::Hue, hsv, Phrase::ColourHue, move |x, _| to_hue(PickerMessage::Hue(x)), words);
    let hex = TextInput::new(crate::labels::phrase(Phrase::ColourHex, words), picker.typed())
        .invalid(picker.typed_refused())
        .on_input(move |typed| to_typed(PickerMessage::Typed(typed)))
        .on_submit(on(PickerMessage::Done))
        .view::<R>();
    let reset =
        Button::new(crate::labels::phrase(Phrase::ZoomReset, words)).on_press(on(PickerMessage::Reset)).view::<R>();
    let done = Button::new(crate::labels::phrase(Phrase::Done, words))
        .variant(ButtonVariant::Primary)
        .compact()
        .on_press(on(PickerMessage::Done))
        .view::<R>();
    let mut parts = vec![
        area,
        hue,
        Row::with_children([named(Phrase::ColourHex, None, hex, words), reset])
            .spacing(Space::S.px())
            .align_y(Alignment::End)
            ._boxed(),
    ];
    if picker.typed_refused() {
        let hint = words.phrase(Phrase::ColourHexHint).to_string();
        parts.push(crate::refusal(hint));
    }
    parts.push(Row::with_children([iced_widget::Space::new().width(Length::Fill)._boxed(), done])._boxed());
    let body = container(Column::with_children(parts).spacing(Space::M.px()))
        .padding(Space::M.px())
        .width(Length::Fixed(COLOUR_PICKER_WIDTH))
        .class(Surfaced::Panel(Elevation::Overlay, CARD_RADIUS));
    crate::on_escape(body, on(PickerMessage::Done), None)._boxed()
}

/// The area or the strip, named for a screen reader.
fn field<'a, Message: 'a, R: Renderer + 'a>(
    kind: Kind,
    hsv: Hsv,
    name: Phrase,
    on_pick: impl Fn(f64, f64) -> Message + 'a,
    words: &'a Words,
) -> Element<'a, Message, Theme, R> {
    let field = Field { kind, hsv, on_pick: Box::new(on_pick) };
    Labelled::new(field, Semantics { role: SemanticRole::Image, label: crate::labels::phrase(name, words) })._boxed()
}

/// Which of the two a [`Field`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Saturation left to right, brightness top to bottom, in the hue the strip holds.
    Area,
    /// Hue left to right.
    Hue,
}

/// The area or the strip: drawn from gradients, and dragged as iced's slider is (a press inside starts it, a move
/// anywhere goes on with it, a release ends it), each point sent as fractions of its sides.
struct Field<'a, Message> {
    kind: Kind,
    hsv: Hsv,
    on_pick: Box<dyn Fn(f64, f64) -> Message + 'a>,
}

#[derive(Default)]
struct Dragging(bool);

impl<Message> Field<'_, Message> {
    fn side(&self) -> Size {
        match self.kind {
            Kind::Area => Size::new(COLOUR_PICKER_AREA.0, COLOUR_PICKER_AREA.1),
            Kind::Hue => Size::new(COLOUR_PICKER_AREA.0, COLOUR_PICKER_HUE_HEIGHT),
        }
    }

    fn pick(&self, bounds: Rectangle, at: Point) -> Message {
        let fraction = |offset: f32, side: f32| f64::from((offset / side).clamp(0.0, 1.0));
        (self.on_pick)(fraction(at.x - bounds.x, bounds.width), fraction(at.y - bounds.y, bounds.height))
    }
}

impl<Message> iced_core::widget::Meta for Field<'_, Message> {}

impl<Message, R: Renderer> Widget<Message, Theme, R> for Field<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Dragging>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Dragging::default())
    }

    fn size(&self) -> Size<Length> {
        let side = self.side();
        Size::new(Length::Fixed(side.width), Length::Fixed(side.height))
    }

    fn layout(&mut self, tree: &mut Tree, _renderer: &R, limits: &layout::Limits) {
        let side = self.side();
        tree.size = layout::atomic(limits, Length::Fixed(side.width), Length::Fixed(side.height));
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
        let bounds = layout.bounds();
        let dragging = tree.state.downcast_mut::<Dragging>();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(at) = cursor.position_over(bounds) {
                    dragging.0 = true;
                    shell.publish(self.pick(bounds, at));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if dragging.0 => {
                shell.publish(self.pick(bounds, *position));
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if dragging.0 => dragging.0 = false,
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &R,
    ) -> mouse::Interaction {
        if tree.state.downcast_ref::<Dragging>().0 || cursor.is_over(layout.bounds()) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut R,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let paint = |colour: Rgb, alpha: f32| Color { a: alpha, ..crate::color::color(colour) };
        let corners = |radius: f32| Border { radius: radius.into(), ..Border::default() };
        let quad = |bounds: Rectangle, border: Border| renderer::Quad { bounds, border, ..renderer::Quad::default() };
        let pure = rgb(Hsv { saturation: 1.0, value: 1.0, ..self.hsv }.colour());
        match self.kind {
            Kind::Area => {
                let round = corners(INPUT_RADIUS.px());
                renderer.fill_quad(quad(bounds, round), Background::Color(paint(pure, 1.0)));
                // Each gradient runs between one colour at full and at no coverage, never to clear black, so the
                // blend between them stays that colour and the middle does not grey.
                let (white, black) = (Rgb::WHITE, Rgb::BLACK);
                let lighter =
                    Linear::new(Radians(FRAC_PI_2)).add_stop(0.0, paint(white, 1.0)).add_stop(1.0, paint(white, 0.0));
                renderer.fill_quad(quad(bounds, round), Background::Gradient(lighter.into()));
                let darker = Linear::new(Radians(PI)).add_stop(0.0, paint(black, 0.0)).add_stop(1.0, paint(black, 1.0));
                renderer.fill_quad(quad(bounds, round), Background::Gradient(darker.into()));
            }
            Kind::Hue => {
                // Red, yellow, green, cyan, blue, magenta and red again: seven stops of the eight a gradient takes.
                let strip = [0.0_f32, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0].into_iter().fold(
                    Linear::new(Radians(FRAC_PI_2)),
                    |strip, step| {
                        let hue = f64::from(step) * 60.0 % 360.0;
                        let colour = rgb(Hsv { hue, saturation: 1.0, value: 1.0 }.colour());
                        strip.add_stop(step / 6.0, paint(colour, 1.0))
                    },
                );
                renderer.fill_quad(quad(bounds, corners(bounds.height / 2.0)), Background::Gradient(strip.into()));
            }
        }
        let (x, y, fill) = match self.kind {
            Kind::Area => (self.hsv.saturation, 1.0 - self.hsv.value, rgb(self.hsv.colour())),
            Kind::Hue => (self.hsv.hue / 360.0, 0.5, pure),
        };
        #[expect(clippy::cast_possible_truncation, reason = "a fraction of a side, well inside f32")]
        let centre = Point::new(bounds.x + x as f32 * bounds.width, bounds.y + y as f32 * bounds.height);
        let side = COLOUR_PICKER_HANDLE;
        let handle = Rectangle::new(Point::new(centre.x - side / 2.0, centre.y - side / 2.0), Size::new(side, side));
        let ring = Border {
            color: theme.color(Role::OnScrim),
            width: design::COLOUR_PICKER_HANDLE_RING,
            radius: Radius::Full.px().min(side).into(),
        };
        renderer.fill_quad(quad(handle, ring), Background::Color(paint(fill, 1.0)));
    }
}
