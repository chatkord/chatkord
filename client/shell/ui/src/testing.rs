//! Testing without a window: [`render`] draws with the software renderer, deterministic and needing no GPU, and
//! [`Refs`] compares it with checked-in references. `iced_test`'s simulator hides its renderer, so this has its own.

pub use crate::focus::FindFocus;
use crate::{Theme, fonts};
use iced_core::keyboard::{self, Key, Modifiers, key};
use iced_core::renderer::{Headless, Style};
use iced_core::{
    Element as CoreElement, Event, InputMethod, Point, Size, Widget, clipboard, mouse, shell, time, window,
};
use iced_graphics::text::font_system;
use iced_runtime::UserInterface;
pub use iced_runtime::user_interface::Cache;
use iced_runtime::user_interface::State;
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Once;

/// The renderer the tests draw with.
pub type Renderer = iced_tiny_skia::Renderer;

/// An element of our theme on the software renderer.
pub type Element<'a, Message> = CoreElement<'a, Message, Theme, Renderer>;

/// How far one channel of a pixel may differ from its reference: antialiasing across CPUs.
pub const CHANNEL_TOLERANCE: u8 = 2;

/// The share of pixels that may differ by more than that: 0.1 %.
pub const PIXEL_TOLERANCE: f64 = 0.001;

/// The environment variable that, set, rewrites the references.
const UPDATE: &str = "UPDATE_REFS";

/// Pixels in RGBA order, as the renderer returns them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// In physical pixels.
    pub size: Size<u32>,
    /// Row by row from the top left, four bytes a pixel: `size.width * size.height * 4` long.
    pub rgba: Vec<u8>,
}

impl Image {
    /// The pixel at `x` across and `y` down, as `[r, g, b, a]`; panics outside the image, as a test fails.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * self.size.width + x) * 4) as usize;
        [self.rgba[at], self.rgba[at + 1], self.rgba[at + 2], self.rgba[at + 3]]
    }
}

/// Makes the font system ours: no face but the bundled ones, whichever the machine or the renderer
/// crate brought, so a picture is the same on every device. Idempotent.
pub fn hermetic_fonts() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let mut system = font_system().write().unwrap_or_else(|poisoned| poisoned.into_inner());
        let db = system.raw().db_mut();
        let ids: Vec<_> = db.faces().map(|face| face.id).collect();
        for id in ids {
            db.remove_face(id);
        }
        db.set_sans_serif_family(fonts::FAMILY);
        db.set_monospace_family(fonts::MONO_FAMILY);
        drop(system);
        fonts::install();
    });
}

/// Adds a font file to the font system, for a test that needs a face the shipped set does not have.
pub fn load_font(bytes: &'static [u8]) {
    font_system().write().unwrap_or_else(|poisoned| poisoned.into_inner()).load_font(Cow::Borrowed(bytes));
}

/// The family of every face the renderer can draw with, sorted and without repeats.
pub fn font_families() -> Vec<String> {
    let system = font_system().read().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut families: Vec<String> = system.families().map(str::to_string).collect();
    families.sort();
    families.dedup();
    families
}

/// What has happened to an interface before it is drawn: where the pointer is, and the events it
/// has seen, in order. The empty scene is an interface nobody has touched.
#[derive(Clone, Debug, Default)]
pub struct Scene {
    /// Where the pointer is, in logical pixels, or nowhere.
    pub cursor: Option<Point>,
    /// What reaches the interface, in order, before it is drawn.
    pub events: Vec<Event>,
    /// How long the interface then rests before it is drawn again: what a tooltip's delay waits for. It is
    /// slept, because the widgets read the real clock.
    pub settle: std::time::Duration,
}

impl Scene {
    /// The pointer over a point.
    pub fn hovering(at: Point) -> Scene {
        Scene { cursor: Some(at), ..Scene::default() }
    }

    /// The pointer over a point with the left button down.
    pub fn pressing(at: Point) -> Scene {
        Scene {
            cursor: Some(at),
            events: vec![Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))],
            ..Scene::default()
        }
    }

    /// The Tab key pressed (with Shift for backwards).
    pub fn tabbing(shift: bool) -> Scene {
        Scene { events: vec![key_press(Key::Named(key::Named::Tab), shift)], ..Scene::default() }
    }
}

/// A key press, with Shift held or not.
pub fn key_press(key: Key, shift: bool) -> Event {
    Event::Keyboard(keyboard::Event::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: key::Physical::Unidentified(key::NativeCode::Unidentified),
        location: keyboard::Location::Standard,
        modifiers: if shift { Modifiers::SHIFT } else { Modifiers::empty() },
        text: None,
        repeat: false,
    })
}

/// Draws `element` at `size` logical pixels, scaled by `scale`, on the theme's base colour, with the
/// hermetic fonts: the same picture on every machine.
pub fn render<'a, Message: 'a>(
    theme: &Theme,
    element: impl Widget<Message, Theme, Renderer> + 'a,
    size: Size,
    scale: f32,
) -> Image {
    render_scene(theme, element, size, scale, &Scene::default())
}

/// [`render`] after the interface has seen a [`Scene`].
pub fn render_scene<'a, Message: 'a>(
    theme: &Theme,
    element: impl Widget<Message, Theme, Renderer> + 'a,
    size: Size,
    scale: f32,
    scene: &Scene,
) -> Image {
    hermetic_fonts();
    draw(theme, element, size, scale, scene)
}

/// What an interface asked of its surroundings while it handled events.
#[derive(Debug)]
pub struct Observed<Message> {
    /// The messages it published, in order.
    pub messages: Vec<Message>,
    /// Each write it asked the clipboard to make.
    pub clipboard: Vec<clipboard::Content>,
    /// The input method it was asking for once drawn.
    pub input_method: InputMethod,
}

/// Sends `events` to an interface one at a time with the pointer over `cursor`, then draws it once,
/// and says what it published and asked for: the messages, the clipboard writes, the input method.
pub fn interact<'a, Message: 'a>(
    element: impl Widget<Message, Theme, Renderer> + 'a,
    size: Size,
    cursor: Point,
    events: &[Event],
) -> Observed<Message> {
    hermetic_fonts();
    let mut renderer = Renderer::new(fonts::settings());
    let mut interface = UserInterface::build(element, size, Cache::default(), &mut renderer);
    let mut messages = shell::Bus::new();
    let cursor = mouse::Cursor::Available(cursor);
    let mut clipboard = Vec::new();
    let mut input_method = InputMethod::Disabled;
    let redraw = Event::Window(window::Event::RedrawRequested(time::Instant::now()));
    for event in events.iter().chain(std::iter::once(&redraw)) {
        let (state, _) = interface.update(
            &window::Headless,
            &shell::Waker::noop(),
            std::slice::from_ref(event),
            cursor,
            &mut renderer,
            &mut messages,
        );
        if let State::Updated { clipboard: requests, input_method: asked, .. } = state {
            clipboard.extend(requests.write);
            input_method = asked;
        }
    }
    Observed { messages: messages.drain().map(|(message, _)| message).collect(), clipboard, input_method }
}

/// As [`interact`], on an interface built from the widget tree `kept` holds, which it then holds in turn: the same
/// view built again keeps each widget's state (a field's focus), as a window's loop does between two frames.
pub fn interact_kept<'a, Message: 'a>(
    kept: &mut Option<Cache>,
    element: impl Widget<Message, Theme, Renderer> + 'a,
    size: Size,
    cursor: Point,
    events: &[Event],
) -> Vec<Message> {
    hermetic_fonts();
    let mut renderer = Renderer::new(fonts::settings());
    let mut interface = UserInterface::build(element, size, kept.take().unwrap_or_default(), &mut renderer);
    let mut messages = shell::Bus::new();
    let cursor = mouse::Cursor::Available(cursor);
    let redraw = Event::Window(window::Event::RedrawRequested(time::Instant::now()));
    for event in events.iter().chain(std::iter::once(&redraw)) {
        let _ = interface.update(
            &window::Headless,
            &shell::Waker::noop(),
            std::slice::from_ref(event),
            cursor,
            &mut renderer,
            &mut messages,
        );
    }
    *kept = Some(interface.into_cache());
    messages.drain().map(|(message, _)| message).collect()
}

/// Draws `element` in `theme` for `frames` frames, each after the window's redraw, and says what each asked for next:
/// whether the interface keeps the window drawing.
pub fn redraws<'a, Message: 'a>(
    theme: &Theme,
    element: impl Widget<Message, Theme, Renderer> + 'a,
    size: Size,
    frames: usize,
) -> Vec<window::RedrawRequest> {
    use iced_core::theme::Base;
    hermetic_fonts();
    let mut renderer = Renderer::new(fonts::settings());
    let mut interface = UserInterface::build(element, size, Cache::default(), &mut renderer);
    let mut messages = shell::Bus::new();
    let style = Style { text_color: theme.base().text_color };
    (0..frames)
        .map(|_| {
            let redraw = [Event::Window(window::Event::RedrawRequested(time::Instant::now()))];
            let (state, _) = interface.update(
                &window::Headless,
                &shell::Waker::noop(),
                &redraw,
                mouse::Cursor::Unavailable,
                &mut renderer,
                &mut messages,
            );
            interface.draw(&mut renderer, theme, &style, mouse::Cursor::Unavailable);
            match state {
                State::Updated { redraw_request, .. } => redraw_request,
                State::Outdated => window::RedrawRequest::Wait,
            }
        })
        .collect()
}

/// The key presses of typing `text`, one per character, each carrying the character it produces.
pub fn typing(text: &str) -> Vec<Event> {
    text.chars()
        .map(|character| {
            let typed = character.to_string();
            let key = Key::Character(typed.clone().into());
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: key.clone(),
                modified_key: key,
                physical_key: key::Physical::Unidentified(key::NativeCode::Unidentified),
                location: keyboard::Location::Standard,
                modifiers: Modifiers::empty(),
                text: Some(typed.into()),
                repeat: false,
            })
        })
        .collect()
}

/// A key pressed with the command key (Ctrl, or Cmd on a Mac) held: `command('a')` is select all.
pub fn command(character: char) -> Event {
    let key = Key::Character(character.to_string().into());
    Event::Keyboard(keyboard::Event::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: key::Physical::Unidentified(key::NativeCode::Unidentified),
        location: keyboard::Location::Standard,
        modifiers: Modifiers::COMMAND,
        text: None,
        repeat: false,
    })
}

/// A left click.
pub fn click() -> [Event; 2] {
    [
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
    ]
}

/// A right click: what opens a context menu.
pub fn right_click() -> [Event; 2] {
    [
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right)),
    ]
}

/// Images of one height side by side, left to right: a row of states to be looked at together.
pub fn side_by_side(images: &[Image]) -> Image {
    let height = images.iter().map(|image| image.size.height).max().unwrap_or(0);
    let width: u32 = images.iter().map(|image| image.size.width).sum();
    let mut rgba = vec![0; (width * height * 4) as usize];
    let mut left = 0;
    for image in images {
        for row in 0..image.size.height {
            let from = (row * image.size.width * 4) as usize;
            let to = ((row * width + left) * 4) as usize;
            let length = (image.size.width * 4) as usize;
            rgba[to..to + length].copy_from_slice(&image.rgba[from..from + length]);
        }
        left += image.size.width;
    }
    Image { size: Size::new(width, height), rgba }
}

/// Images of one width one under another, top to bottom: a column of states to be looked at together.
pub fn stacked(images: &[Image]) -> Image {
    let width = images.iter().map(|image| image.size.width).max().unwrap_or(0);
    let height: u32 = images.iter().map(|image| image.size.height).sum();
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for image in images {
        rgba.extend_from_slice(&image.rgba);
    }
    Image { size: Size::new(width, height), rgba }
}

/// [`render`] with the bundled fonts installed over this machine's own, as the shell has them; no reference is made
/// from it. A test binary calls this or [`render`], never both: the font system is one per process.
pub fn render_on_this_machine<'a, Message: 'a>(
    theme: &Theme,
    element: impl Widget<Message, Theme, Renderer> + 'a,
    size: Size,
    scale: f32,
) -> Image {
    fonts::install();
    draw(theme, element, size, scale, &Scene::default())
}

fn draw<'a, Message: 'a>(
    theme: &Theme,
    element: impl Widget<Message, Theme, Renderer> + 'a,
    size: Size,
    scale: f32,
    scene: &Scene,
) -> Image {
    use iced_core::theme::Base;
    let mut renderer = Renderer::new(fonts::settings());
    let base = theme.base();
    let mut interface = UserInterface::build(element, size, Cache::default(), &mut renderer);
    let mut messages = shell::Bus::new();
    let cursor = scene.cursor.map_or(mouse::Cursor::Unavailable, mouse::Cursor::Available);
    // The scene's events first, then a redraw, which is when a widget settles its status; then, if the scene
    // rests, it waits and is drawn again (a tooltip shows once its delay has passed).
    let first = [Event::Window(window::Event::RedrawRequested(time::Instant::now()))];
    let mut input_method = InputMethod::Disabled;
    for events in [&scene.events[..], &first[..]] {
        let (state, _) =
            interface.update(&window::Headless, &shell::Waker::noop(), events, cursor, &mut renderer, &mut messages);
        if let State::Updated { input_method: asked, .. } = state {
            input_method = asked;
        }
    }
    if !scene.settle.is_zero() {
        std::thread::sleep(scene.settle);
        let again = [Event::Window(window::Event::RedrawRequested(time::Instant::now()))];
        let _ =
            interface.update(&window::Headless, &shell::Waker::noop(), &again, cursor, &mut renderer, &mut messages);
    }
    interface.draw(&mut renderer, theme, &Style { text_color: base.text_color }, cursor);
    // What an input method is composing, over the interface, as the window draws it. Kept until the screenshot:
    // the renderer holds a paragraph only as long as its owner does.
    let mut overlay = crate::Preedit::<Renderer>::default();
    if let InputMethod::Enabled { cursor: caret, preedit: Some(preedit), .. } = &input_method {
        overlay.update(*caret, preedit, &renderer);
        overlay.draw(&mut renderer, &crate::Composing::preedit(theme), iced_core::Rectangle::with_size(size));
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a test surface's size in pixels: positive and small"
    )]
    let physical = Size::new((size.width * scale).round() as u32, (size.height * scale).round() as u32);
    let rgba = renderer.screenshot(physical, scale, base.background_color);
    drop(overlay);
    Image { size: physical, rgba }
}

/// How two images of one size differ.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Difference {
    /// Pixels where some channel differs by more than [`CHANNEL_TOLERANCE`].
    pub beyond: usize,
    /// The largest difference of any channel anywhere.
    pub worst: u8,
    pub(crate) pixels: usize,
}

impl Difference {
    /// At most [`PIXEL_TOLERANCE`] of the pixels differ beyond [`CHANNEL_TOLERANCE`].
    pub fn within_tolerance(self) -> bool {
        (self.beyond as f64) <= PIXEL_TOLERANCE * self.pixels as f64
    }
}

/// The pixels of an image, one `[r, g, b, a]` each.
fn pixels(image: &Image) -> impl Iterator<Item = &[u8; 4]> {
    image.rgba.as_chunks::<4>().0.iter()
}

/// The largest difference of any channel between two pixels.
fn gap(left: &[u8; 4], right: &[u8; 4]) -> u8 {
    left.iter().zip(right).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0)
}

/// Compares two images of the same size; `None` when the sizes differ.
pub fn compare(a: &Image, b: &Image) -> Option<Difference> {
    if a.size != b.size {
        return None;
    }
    let mut beyond = 0;
    let mut worst = 0;
    for (left, right) in pixels(a).zip(pixels(b)) {
        let gap = gap(left, right);
        worst = worst.max(gap);
        if gap > CHANNEL_TOLERANCE {
            beyond += 1;
        }
    }
    Some(Difference { beyond, worst, pixels: a.rgba.len() / 4 })
}

/// Reference images in one directory, and where a failure leaves its evidence.
pub struct Refs {
    pub(crate) dir: PathBuf,
    pub(crate) failures: PathBuf,
}

/// Why an image is not its reference.
#[derive(Debug)]
pub enum Mismatch {
    /// There is no reference yet.
    Missing(PathBuf),
    /// The image is not the reference's size.
    Size {
        /// The reference's size, in physical pixels.
        reference: Size<u32>,
        /// The image's size, in physical pixels.
        actual: Size<u32>,
    },
    /// Too many pixels differ, by this much.
    Pixels(Difference),
    /// The reference could not be read or the evidence written: why, as the system said.
    Io(String),
}

impl Refs {
    /// References in `dir`; a failure leaves its actual, expected and diff images in `failures`.
    pub fn new(dir: impl Into<PathBuf>, failures: impl Into<PathBuf>) -> Refs {
        Refs { dir: dir.into(), failures: failures.into() }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.png"))
    }

    /// Makes `image` the reference called `name`: what `UPDATE_REFS` does, and what a person reviews as a PNG diff.
    pub fn accept(&self, name: &str, image: &Image) -> Result<(), Mismatch> {
        save(&self.path(name), image).map_err(|why| Mismatch::Io(why.to_string()))
    }

    /// Checks an image against its reference; with `UPDATE_REFS` set it writes the reference instead.
    pub fn check(&self, name: &str, actual: &Image) -> Result<(), Mismatch> {
        if std::env::var_os(UPDATE).is_some() {
            return self.accept(name, actual);
        }
        self.verdict(name, actual)
    }

    /// The verdict a reference earns whatever `UPDATE_REFS` says: what a test of the harness itself
    /// needs, since it is checking that a mismatch is reported, and the rewrite would hide it.
    pub fn verdict(&self, name: &str, actual: &Image) -> Result<(), Mismatch> {
        let path = self.path(name);
        if !path.exists() {
            return Err(Mismatch::Missing(path));
        }
        let reference = read(&path).map_err(|why| Mismatch::Io(why.to_string()))?;
        let Some(difference) = compare(&reference, actual) else {
            self.keep(name, &reference, actual);
            return Err(Mismatch::Size { reference: reference.size, actual: actual.size });
        };
        if difference.within_tolerance() {
            Ok(())
        } else {
            self.keep(name, &reference, actual);
            Err(Mismatch::Pixels(difference))
        }
    }

    /// Writes the actual image, the reference and a diff to the failures directory for a person
    /// (or the CI artifact) to look at. A failure to write is not the failure being reported.
    fn keep(&self, name: &str, reference: &Image, actual: &Image) {
        let _ = save(&self.failures.join(format!("{name}-actual.png")), actual);
        let _ = save(&self.failures.join(format!("{name}-expected.png")), reference);
        if reference.size == actual.size {
            let rgba = pixels(reference)
                .zip(pixels(actual))
                .flat_map(|(want, got)| {
                    if gap(want, got) > CHANNEL_TOLERANCE {
                        [255, 0, 0, 255]
                    } else {
                        [got[0] / 4, got[1] / 4, got[2] / 4, 255]
                    }
                })
                .collect();
            let _ = save(&self.failures.join(format!("{name}-diff.png")), &Image { size: actual.size, rgba });
        }
    }

    /// [`Refs::check`] that fails the test, saying what to do.
    #[expect(clippy::panic, reason = "a test helper: a mismatch is the test failing")]
    pub fn assert(&self, name: &str, actual: &Image) {
        if let Err(why) = self.check(name, actual) {
            panic!(
                "{name}: {why:?}\nthe images of a failure are in {}; if the new picture is right, rerun with {UPDATE}=1 and review the changed PNG",
                self.failures.display()
            );
        }
    }
}

/// Writes an image as a PNG, making its folder.
pub fn save(path: &Path, image: &Image) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, image.size.width, image.size.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&image.rgba)?;
    Ok(())
}

fn read(path: &Path) -> Result<Image, Box<dyn std::error::Error>> {
    let mut reader = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path)?)).read_info()?;
    let mut rgba = vec![0; reader.output_buffer_size().ok_or("a reference too large to hold")?];
    let info = reader.next_frame(&mut rgba)?;
    rgba.truncate(info.buffer_size());
    Ok(Image { size: Size::new(info.width, info.height), rgba })
}

/// The references of the crate whose tests call it, and where its failures go.
#[macro_export]
macro_rules! refs {
    () => {
        $crate::testing::Refs::new(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/refs"),
            concat!(env!("CARGO_TARGET_TMPDIR"), "/reftest"),
        )
    };
}

/// The round trips the canvas draws for `quality`'s connection popover: three waves around its figure (38, 172 and
/// 268 ms), the latest look at the figure, so a reference and the canvas show the same line.
pub fn canvas_round_trips(quality: screens::Quality) -> Vec<screens::RoundTrip> {
    let (figure, slow, slower, quick) = match quality {
        screens::Quality::Good => (38.0, 4.0, 3.0, 2.0),
        screens::Quality::Fair => (172.0, 16.0, 10.0, 8.0),
        screens::Quality::Poor => (268.0, 30.0, 20.0, 14.0),
    };
    let wave =
        |look: f64| slow * (look * 0.21).sin() + slower * (look * 0.057 + 1.3).sin() + quick * (look * 1.7).sin();
    let latest = (screens::KEPT_LOOKS - 1) as f64;
    (0..screens::KEPT_LOOKS)
        .map(|look| {
            let millis = (figure + wave(look as f64) - wave(latest)).round();
            screens::RoundTrip {
                took: std::time::Duration::from_secs_f64(millis / 1000.0),
                on: screens::MeasuredOn::Route,
            }
        })
        .collect()
}
