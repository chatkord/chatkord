//! Reads the clipboard once and publishes the text it held, or writes it once and publishes whether that worked.
//! Neither takes room or draws: a widget, so the window's loop carries the read and the write like any other.

use crate::Theme;
use iced_core::widget::{Meta, Tree, tree};
use iced_core::{Event, Layout, Length, Rectangle, Shell, Size, Widget, clipboard, layout, mouse, renderer};

/// Asks for the clipboard's text as soon as it is in the interface, and publishes `on_read` with it (empty when
/// the clipboard held no text or could not be read).
pub fn clipboard_reader<'a, Message: 'a>(on_read: impl Fn(String) -> Message + 'a) -> ClipboardReader<'a, Message> {
    ClipboardReader { on_read: Box::new(on_read) }
}

/// The widget [`clipboard_reader`] makes: it takes no room and draws nothing.
pub struct ClipboardReader<'a, Message> {
    on_read: Box<dyn Fn(String) -> Message + 'a>,
}

#[derive(Default)]
struct Asked(bool);

impl<Message> Meta for ClipboardReader<'_, Message> {}

impl<Message, R: renderer::Renderer> Widget<Message, Theme, R> for ClipboardReader<'_, Message> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(0.0), Length::Fixed(0.0))
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Asked>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Asked::default())
    }

    fn layout(&mut self, tree: &mut Tree, _renderer: &R, limits: &layout::Limits) {
        tree.size = layout::atomic(limits, Length::Fixed(0.0), Length::Fixed(0.0));
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        _layout: Layout,
        _cursor: mouse::Cursor,
        _renderer: &R,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let asked = tree.state.downcast_mut::<Asked>();
        match event {
            Event::Clipboard(clipboard::Event::Read(read)) if asked.0 => {
                let text = match read.as_deref() {
                    Ok(clipboard::Content::Text(text)) => text.clone(),
                    _ => String::new(),
                };
                shell.publish((self.on_read)(text));
            }
            _ if !asked.0 => {
                asked.0 = true;
                shell.read_clipboard(clipboard::Kind::Text);
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        _tree: &Tree,
        _renderer: &mut R,
        _theme: &Theme,
        _style: &renderer::Style,
        _layout: Layout,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
    }
}

/// Puts `text` on the clipboard as soon as it is in the interface, and publishes `on_written` with whether it took.
pub fn clipboard_writer<'a, Message: 'a>(
    text: impl Into<String>,
    on_written: impl Fn(bool) -> Message + 'a,
) -> ClipboardWriter<'a, Message> {
    ClipboardWriter { text: text.into(), on_written: Box::new(on_written) }
}

/// The widget [`clipboard_writer`] makes: it takes no room and draws nothing.
pub struct ClipboardWriter<'a, Message> {
    text: String,
    on_written: Box<dyn Fn(bool) -> Message + 'a>,
}

impl<Message> Meta for ClipboardWriter<'_, Message> {}

impl<Message, R: renderer::Renderer> Widget<Message, Theme, R> for ClipboardWriter<'_, Message> {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(0.0), Length::Fixed(0.0))
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Asked>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Asked::default())
    }

    fn layout(&mut self, tree: &mut Tree, _renderer: &R, limits: &layout::Limits) {
        tree.size = layout::atomic(limits, Length::Fixed(0.0), Length::Fixed(0.0));
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        _layout: Layout,
        _cursor: mouse::Cursor,
        _renderer: &R,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let asked = tree.state.downcast_mut::<Asked>();
        match event {
            Event::Clipboard(clipboard::Event::Written(written)) if asked.0 => {
                shell.publish((self.on_written)(written.is_ok()));
            }
            _ if !asked.0 => {
                asked.0 = true;
                shell.write_clipboard(self.text.clone());
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        _tree: &Tree,
        _renderer: &mut R,
        _theme: &Theme,
        _style: &renderer::Style,
        _layout: Layout,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
    }
}
