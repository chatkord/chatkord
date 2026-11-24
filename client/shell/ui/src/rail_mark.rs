//! The bar at the rail's edge: tall beside the open server, short while the pointer is on an item. Drawn inside the
//! item's button from the pointer it is given, so an item the keyboard focused shows the short bar too.

use crate::drawn::Drawn;
use crate::{Surfaced, Theme};
use design::{RAIL_BAR, RAIL_BAR_HOVER};
use iced_core::widget::{Tree, tree};
use iced_core::{Background, Event, Layout, Length, Rectangle, Shell, Size, Widget, layout, mouse, renderer};
use iced_widget::container;

/// The bar of one item, filling the item; `top` is where the tall bar starts below the item's top.
pub(crate) struct RailMark {
    pub open: bool,
    pub top: f32,
}

impl RailMark {
    /// How tall the bar is with the pointer as `cursor` says; none where no bar is drawn.
    fn height(&self, item: Rectangle, cursor: mouse::Cursor) -> Option<f32> {
        match (self.open, cursor.is_over(item)) {
            (true, _) => Some(RAIL_BAR.1),
            (false, true) => Some(RAIL_BAR_HOVER),
            (false, false) => None,
        }
    }
}

/// The bar's height as the last frame drew it.
type Shown = Drawn<Option<f32>>;

impl iced_core::widget::Meta for RailMark {}

impl<Message, R: renderer::Renderer> Widget<Message, Theme, R> for RailMark {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Shown>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Shown::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(&mut self, tree: &mut Tree, _renderer: &R, limits: &layout::Limits) {
        tree.size = limits.max;
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
        let now = self.height(layout.bounds(), cursor);
        tree.state.downcast_mut::<Shown>().follow(event, now, shell);
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut R,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let item = layout.bounds();
        let Some(height) = self.height(item, cursor) else { return };
        let look = <Theme as container::Catalog>::style(theme, &Surfaced::RailBar);
        let y = item.y + self.top + (RAIL_BAR.1 - height) / 2.0;
        let bounds = Rectangle { x: item.x, y, width: RAIL_BAR.0, height };
        let fill = look.background.unwrap_or(Background::Color(iced_core::Color::TRANSPARENT));
        renderer.fill_quad(renderer::Quad { bounds, border: look.border, ..renderer::Quad::default() }, fill);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawn::asked_after;
    use iced_core::Point;
    use iced_core::window::RedrawRequest;

    const ITEM: Size = Size::new(72.0, 48.0);
    const OUTSIDE: Point = Point::new(100.0, 24.0);
    const INSIDE: Point = Point::new(36.0, 24.0);

    fn moved(to: Point) -> [Event; 1] {
        [Event::Mouse(mouse::Event::CursorMoved { position: to })]
    }

    fn asked(open: bool, drawn: Point, now: Point) -> RedrawRequest {
        let mark = || RailMark { open, top: 0.0 }._boxed();
        asked_after::<()>(mark, ITEM, drawn, now, &moved(now))
    }

    #[test]
    fn the_pointer_coming_onto_an_item_or_leaving_it_asks_for_a_frame() {
        assert_eq!(asked(false, OUTSIDE, INSIDE), RedrawRequest::NextFrame, "the short bar shows");
        assert_eq!(asked(false, INSIDE, OUTSIDE), RedrawRequest::NextFrame, "and goes");
    }

    #[test]
    fn a_move_that_leaves_the_bar_as_drawn_asks_nothing() {
        assert_eq!(asked(false, INSIDE, Point::new(30.0, 20.0)), RedrawRequest::Wait, "still on the item");
        assert_eq!(asked(true, OUTSIDE, INSIDE), RedrawRequest::Wait, "the open server's tall bar either way");
    }
}
