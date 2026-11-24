//! The keys a step answers when nothing inside took them: Escape goes back, Enter presses the main button.

use crate::Theme;
use iced_core::keyboard::{self, Key, key::Named};
use iced_core::widget::{Meta, Tree};
use iced_core::{Event, Layout, Rectangle, Shell, Widget, mouse, renderer};

/// `content`, where an Escape press none of its widgets captured sends `back`, and an Enter press sends `submit` when
/// there is one (the main button works).
pub(crate) fn on_escape<W, Message>(content: W, back: Message, submit: Option<Message>) -> OnEscape<W, Message> {
    OnEscape { content, back, submit }
}

pub(crate) struct OnEscape<W, Message> {
    content: W,
    back: Message,
    submit: Option<Message>,
}

impl<W, Message> Meta for OnEscape<W, Message> {}

impl<Message, W, R> Widget<Message, Theme, R> for OnEscape<W, Message>
where
    Message: Clone,
    R: renderer::Renderer,
    W: Widget<Message, Theme, R>,
{
    crate::wrap::forward!(size diff layout operate mouse_interaction draw overlay);

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
        self.content.update(&mut tree.children[0], event, layout, cursor, renderer, shell, viewport);
        if shell.is_event_captured() {
            return;
        }
        let said = match event {
            Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(Named::Escape), .. }) => Some(&self.back),
            Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(Named::Enter), .. }) => self.submit.as_ref(),
            _ => None,
        };
        if let Some(message) = said {
            shell.publish(message.clone());
            shell.capture_event();
        }
    }
}
