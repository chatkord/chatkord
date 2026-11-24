//! A list kept where its reader is: the scrollable inside, anchored at its end, is moved to the distance from its end
//! the window says each time the window moves it, so rows that change below the one in view never move what is read.
//! A move the person makes is the scrollable's own, never fought.

use crate::Theme;
use iced_core::renderer;
use iced_core::widget::operation::{Animation, scrollable::scroll_to};
use iced_core::widget::{Id, Tree, tree};
use iced_core::{Event, Layout, Rectangle, Shell, Widget, mouse, window};
use iced_widget::scrollable::AbsoluteOffset;

/// The window's last move put on the scrollable; none before the first.
#[derive(Default)]
struct Put(Option<u64>);

/// `content`, holding the scrollable `id`, put `from_end` pixels above its end by the window's move `moved`.
pub(crate) struct KeepAt<W> {
    content: W,
    id: Id,
    from_end: f32,
    moved: u64,
}

impl<W> KeepAt<W> {
    /// `content` whose scrollable `id` the window's move `moved` puts `from_end` pixels above its end.
    pub(crate) fn new(content: W, id: Id, from_end: f32, moved: u64) -> Self {
        KeepAt { content, id, from_end, moved }
    }
}

impl<W> iced_core::widget::Meta for KeepAt<W> {}

impl<Message, W, R> Widget<Message, Theme, R> for KeepAt<W>
where
    R: renderer::Renderer,
    W: Widget<Message, Theme, R>,
{
    crate::wrap::forward!(size diff layout operate mouse_interaction draw overlay);

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Put>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Put::default())
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        renderer: &R,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        // A list rebuilt (the pane folding moves it in the tree) starts at its end: it is put back before it sees an
        // event, so the first distance it tells is the window's, never its fresh state's.
        let put: &mut Put = tree.state.downcast_mut();
        let redraw = matches!(event, Event::Window(window::Event::RedrawRequested(_)));
        if put.0 != Some(self.moved) && (put.0.is_none() || redraw) {
            put.0 = Some(self.moved);
            let offset = AbsoluteOffset { x: None, y: Some(self.from_end) };
            let mut moved = scroll_to(self.id.clone(), offset, Animation::Instant);
            self.content.operate(&mut tree.children[0], layout, viewport, renderer, &mut moved);
            shell.request_redraw();
        }
        self.content.update(&mut tree.children[0], event, layout, cursor, renderer, shell, viewport);
    }
}
