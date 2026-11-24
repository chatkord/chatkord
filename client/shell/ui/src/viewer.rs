//! A message's picture opened over the window: as large as the window holds it at its own shape, under a bar with its
//! name, its size, Save and the way out. Escape closes it, as it closes every layer, and so does a press beside the
//! picture.

use crate::typography::{coloured, line};
use crate::{Button, Icon, Labelled, Renderer, Surfaced, TextClass, Theme};
use design::{ButtonVariant, Content, Elevation, Radius, SemanticRole, Semantics, Space, TypeStyle, VIEWER_BAR_ROOM};
use iced_core::image::Handle;
use iced_core::{Alignment, Element, Length, Padding, Size, Widget};
use iced_widget::{Column, Row, Space as Gap, container, mouse_area, opaque, stack};
use words::{Phrase, Words};

/// The picture the viewer shows.
pub struct Viewed<Message> {
    /// The picture at full size, decoded, or its thumbnail until then; none while neither is held, which leaves its
    /// room empty.
    pub picture: Option<Handle>,
    /// Its thumbnail while the original is drawn over it: what shows in the frame the original is first painted in,
    /// which a painter leaves empty.
    pub under: Option<Handle>,
    /// The original's width and height in pixels where the message says them: the room the picture takes whichever
    /// of the two is drawn, so the original arriving moves nothing.
    pub shape: Option<(u32, u32)>,
    /// Its file's name.
    pub name: String,
    /// What it is, said under the name: its pixels and size ("1600 × 1200 · 2.4 MB").
    pub note: String,
    /// What Save sends.
    pub save: Message,
}

/// `screen` with `viewed` over it while a picture is open; Escape, the close button and a press beside the picture
/// send `close`.
pub fn viewer_over<'a, Message: Clone + 'a, R: Renderer + 'a>(
    screen: Element<'a, Message, Theme, R>,
    viewed: Option<Viewed<Message>>,
    close: Message,
    words: &Words,
) -> Element<'a, Message, Theme, R> {
    let layer = viewed.map(|viewed| viewer(viewed, close.clone(), words));
    crate::dialog(screen, layer, close)
}

/// The viewer filling the window: the picture centred below the bar's room, the bar over it at the top.
fn viewer<'a, Message: Clone + 'a, R: Renderer + 'a>(
    viewed: Viewed<Message>,
    close: Message,
    words: &Words,
) -> Element<'a, Message, Theme, R> {
    let name = crate::labels::label(&viewed.name);
    let pad = Padding { top: VIEWER_BAR_ROOM, bottom: Space::Xl.px(), left: Space::Xl.px(), right: Space::Xl.px() };
    let picture: Element<'a, Message, Theme, R> = match (viewed.picture, viewed.shape) {
        // Its own room, known before it is read: a press on it is its own, one beside it closes.
        (Some(handle), Some(shape)) => {
            let filling = |handle: Handle| {
                iced_widget::image(handle)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .content_fit(iced_core::ContentFit::Fill)
                    .border_radius(Radius::M.px())
                    ._boxed()
            };
            let layers = viewed.under.into_iter().chain([handle]).map(filling);
            Fitted { content: opaque(stack(layers))._boxed(), shape }._boxed()
        }
        // No shape said: the room's whole size, the picture fitted in it and never enlarged.
        (Some(handle), None) => iced_widget::image(handle)
            .width(Length::Fill)
            .height(Length::Fill)
            .content_fit(iced_core::ContentFit::ScaleDown)
            .border_radius(Radius::M.px())
            ._boxed(),
        (None, _) => Gap::new()._boxed(),
    };
    let room = container(picture).center(Length::Fill).padding(pad);
    let beside = mouse_area(Gap::new().width(Length::Fill).height(Length::Fill)).on_press(close.clone());
    let said = Column::with_children([
        line(viewed.name, TypeStyle::BodyStrong)._boxed(),
        coloured(viewed.note, TypeStyle::Stamp, TextClass::Content(Content::Secondary))._boxed(),
    ])
    .width(Length::Fill);
    let save = Button::new(crate::labels::phrase(Phrase::Save, words))
        .icon(Icon::Download)
        .variant(ButtonVariant::Secondary)
        .compact()
        .on_press(viewed.save)
        .view::<R>();
    let out = Button::new(crate::labels::phrase(Phrase::Close, words))
        .icon_only(Icon::Close)
        .variant(ButtonVariant::Quiet)
        .compact()
        .on_press(close)
        .view::<R>();
    let bar =
        container(Row::with_children([said._boxed(), save, out]).spacing(Space::S.px()).align_y(Alignment::Center))
            .padding(Padding {
                top: Space::S.px(),
                bottom: Space::S.px(),
                left: Space::M.px() + Space::Xxs.px(),
                right: Space::S.px(),
            })
            .width(Length::Fill)
            .class(Surfaced::Panel(Elevation::Overlay, Radius::M));
    let top = container(opaque(bar)._boxed()).padding(Space::M.px()).width(Length::Fill);
    let whole = stack([beside._boxed(), room._boxed(), top._boxed()]).width(Length::Fill).height(Length::Fill);
    Labelled::new(whole, Semantics { role: SemanticRole::Dialog, label: name })._boxed()
}

/// The room a picture of `shape` takes in `held`: its own pixels, made smaller at its own shape to fit, never larger.
fn fitted((width, height): (u32, u32), held: Size) -> Size {
    let (width, height) = (width.max(1) as f32, height.max(1) as f32);
    let scale = (held.width / width).min(held.height / height).min(1.0);
    Size::new(width * scale, height * scale)
}

/// `content` given the room a picture of `shape` takes in what it is offered, whatever `content` itself would ask
/// for: the room is the shape's, so it is the same before the picture is read and after.
struct Fitted<'a, Message, R> {
    content: Element<'a, Message, Theme, R>,
    shape: (u32, u32),
}

impl<Message, R> iced_core::widget::Meta for Fitted<'_, Message, R> {}

impl<Message, R: Renderer> Widget<Message, Theme, R> for Fitted<'_, Message, R> {
    crate::wrap::forward!(diff operate update mouse_interaction draw overlay);

    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn layout(&mut self, tree: &mut iced_core::widget::Tree, renderer: &R, limits: &iced_core::layout::Limits) {
        let room = fitted(self.shape, limits.max);
        self.content.layout(&mut tree.children[0], renderer, &iced_core::layout::Limits::new(room, room));
        tree.size = room;
    }
}
