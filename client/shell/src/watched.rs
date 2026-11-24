//! A picture in the window, drawn through a `shader` widget: a surface imported where it is, or a software decode's
//! planes copied up. Each screen in a call and one's own share is one [`picture_widget`].

use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant as Clock;

use choices::Mirror;
use design::Scheme;
use iced_wgpu::graphics::Viewport;
use iced_wgpu::primitive::{Pipeline, Primitive};
use iced_wgpu::{Renderer, wgpu};
use iced_widget::shader;
use iced_widget::shader::Program;
use iced_winit::core::{Element, Length, Rectangle, Size, Widget, mouse};
use picture::{Crop, Current, Framing, Painter, Shape, Slot};
use screens::{Lens, Seen, ShareCard, fit, whole_pixels};
use terms::MemberId;
use ui::picture::Painted;

use crate::themed::Themed;
use crate::window::{Input, Next, Redrawn, Scene, Wake};

/// What the window did, counted where it happens and read once a second: a picture taken onto the screen, and
/// the nanoseconds of each turn of the pipeline's prepare that took one.
static TAKEN: AtomicU64 = AtomicU64::new(0);
static PREPARE_NANOS: AtomicU64 = AtomicU64::new(0);
/// Whether a picture too large for this GPU has been reported; a stream sends many, and one line says it.
static TOO_LARGE: AtomicBool = AtomicBool::new(false);

/// Where the pictures come from, once there is somewhere: a file's decoder fills it at once, and a shared screen's
/// pipeline once the call shows it.
#[derive(Clone, Default)]
pub struct Feed(Arc<Mutex<Option<Slot>>>);

impl Feed {
    /// Where the pictures come from from now on, in place of any before.
    pub fn put(&self, slot: Slot) {
        if let Ok(mut place) = self.0.lock() {
            *place = Some(slot);
        }
    }

    fn slot(&self) -> Option<Slot> {
        self.0.lock().ok().and_then(|slot| slot.clone())
    }
}

impl fmt::Debug for Feed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Feed")
    }
}

/// A share's card with the name its sharer is shown by, which the place that owns the watch resolved.
type Sharer = (ShareCard, String);

/// What the receive loop says to the window: shown as a line of text, and the scene is told to drain it, which lays
/// the window out again.
#[derive(Clone)]
pub struct Said {
    text: Arc<Mutex<(String, String)>>,
    card: Arc<Mutex<Option<Sharer>>>,
    wake: Wake,
}

impl Said {
    /// `wake` is [`Started::told`](crate::window::Started::told): a line is laid out, not a picture. `first` is what
    /// the line says before the receive loop has said anything: a window watching a channel waits for a shared
    /// screen, a window showing this machine's own picture has no such wait.
    pub fn new(wake: Wake, first: String) -> Self {
        Self { text: Arc::new(Mutex::new((first, String::new()))), card: Arc::default(), wake }
    }

    /// How the receive loop has the scene drain what it said.
    pub fn wake(&self) -> Wake {
        Arc::clone(&self.wake)
    }

    fn card(&self) -> Option<Sharer> {
        self.card.lock().ok().and_then(|card| card.clone())
    }

    fn line(&self) -> String {
        self.text.lock().map(|said| format!("{}  {}", said.0, said.1)).unwrap_or_default()
    }
}

/// The picture `feed` holds, as the widget a call shows `painted` with: it takes the room it is given.
pub fn picture_widget<'a, Message: 'static>(feed: Feed, painted: Painted) -> Element<'a, Message, ui::Theme, Renderer> {
    let widget: Element<'a, Infallible, ui::Theme, Renderer> =
        Widget::_boxed(shader(Picture(feed, painted)).width(Length::Fill).height(Length::Fill));
    widget._map(|never| match never {})._boxed()
}

struct Picture(Feed, Painted);

impl Program<Infallible> for Picture {
    type State = ();
    type Primitive = Drawing;

    fn draw(&self, _: &(), _: mouse::Cursor, _: Rectangle) -> Drawing {
        Drawing(self.0.clone(), self.1.clone())
    }
}

#[derive(Debug)]
struct Drawing(Feed, Painted);

/// The painter and each picture on screen, kept between frames. iced keeps one of these for every widget of the type,
/// so each picture a call shows has its own here.
struct Held {
    painter: Painter,
    screens: HashMap<MemberId, Shown>,
    own: Shown,
    cameras: HashMap<MemberId, Shown>,
    own_camera: Shown,
}

/// One picture on screen, with where the widget is (in physical pixels) and the part of it that shows there, so it is
/// letterboxed and not stretched. `picture::Current` owns what the picture is (planes copied up, or a buffer imported
/// where it is) and draws it.
struct Shown {
    current: Current,
    area: [f32; 4],
    /// Where the picture is drawn in its area and which part of it: none while there is no picture to draw.
    seen: Option<Seen>,
    framing: Framing,
    /// Prepared since the last trim: a picture no widget drew is kept only until its watch ends.
    drawn: bool,
    /// The watch's slot, as last prepared: its end is read from it while no card draws it.
    slot: Option<Slot>,
}

impl Shown {
    fn new(painter: &Painter, device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self {
            current: Current::default(),
            area: [0.0; 4],
            seen: None,
            framing: painter.framing(device, queue),
            drawn: false,
            slot: None,
        }
    }
}

impl Held {
    fn of(&self, painted: &Painted) -> Option<&Shown> {
        match painted {
            Painted::Screen { user_id, .. } => self.screens.get(user_id),
            Painted::OwnScreen(_) => Some(&self.own),
            Painted::Camera { user_id } => self.cameras.get(user_id),
            Painted::OwnCamera(..) => Some(&self.own_camera),
            // A still and a held picture are images of their own, never a picture widget.
            Painted::Still(..) | Painted::Picture { .. } => None,
        }
    }
}

impl Pipeline for Held {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let painter = Painter::new(device, format);
        let own = Shown::new(&painter, device, queue);
        let own_camera = Shown::new(&painter, device, queue);
        Self { painter, screens: HashMap::new(), own, cameras: HashMap::new(), own_camera }
    }

    /// Once a frame, after it is drawn: a screen no card drew keeps its last picture, so a card shown again has it at
    /// once (its stream may be held meanwhile), and goes, with its texture, once its watch ended.
    fn trim(&mut self) {
        for pictures in [&mut self.screens, &mut self.cameras] {
            pictures.retain(|_, shown| {
                if std::mem::take(&mut shown.drawn) {
                    return true;
                }
                if let Some(slot) = &shown.slot {
                    shown.current.follow(slot);
                }
                shown.current.size().is_some()
            });
        }
        for own in [&mut self.own, &mut self.own_camera] {
            if !std::mem::take(&mut own.drawn) {
                own.current = Current::default();
            }
        }
    }
}

impl Primitive for Drawing {
    type Pipeline = Held;

    fn prepare(
        &self,
        held: &mut Held,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        let scale = viewport.scale_factor();
        let Held { painter, screens, own, cameras, own_camera } = held;
        let (shown, placing, corners) = match &self.1 {
            Painted::Screen { user_id, lens, corners } => {
                let shown = screens.entry(user_id.clone()).or_insert_with(|| Shown::new(painter, device, queue));
                (shown, Placing::Through(*lens), *corners)
            }
            Painted::OwnScreen(corners) => (own, Placing::Through(Lens::WHOLE), *corners),
            Painted::Camera { user_id } => {
                let shown = cameras.entry(user_id.clone()).or_insert_with(|| Shown::new(painter, device, queue));
                (shown, Placing::Covering, design::TILE_RADIUS)
            }
            Painted::OwnCamera(Mirror::On, corners) => (own_camera, Placing::Mirrored, *corners),
            Painted::OwnCamera(Mirror::Off, corners) => (own_camera, Placing::Covering, *corners),
            Painted::Still(..) | Painted::Picture { .. } => return,
        };
        shown.area = [bounds.x * scale, bounds.y * scale, bounds.width * scale, bounds.height * scale];
        let [left, top, width, height] = shown.area;
        shown.framing.shape(queue, Shape { left, top, width, height, radius: corners.px() * scale });
        shown.drawn = true;
        let area = (whole_pixels(shown.area[2]), whole_pixels(shown.area[3]));
        if let Some(slot) = self.0.slot() {
            take(shown, &slot, area, device, queue, painter);
            shown.slot = Some(slot);
        }
        shown.seen = shown.current.size().and_then(|size| match placing {
            Placing::Through(lens) => fit(size, area).and_then(|placed| lens.seen(placed, area)),
            Placing::Covering => Seen::filling(size, area),
            Placing::Mirrored => Seen::filling(size, area).map(Seen::mirrored),
        });
        if let Some(Seen { window: [left, top, width, height], .. }) = shown.seen {
            shown.framing.set(queue, Crop { left, top, width, height });
        }
    }

    fn draw(&self, held: &Held, pass: &mut wgpu::RenderPass<'_>) -> bool {
        let Some(shown) = held.of(&self.1) else { return false };
        let Some(Seen { viewport: [left, top, width, height], .. }) = shown.seen else { return false };
        pass.set_viewport(shown.area[0] + left, shown.area[1] + top, width, height, 0.0, 1.0);
        shown.current.draw(&held.painter, pass, &shown.framing);
        true
    }
}

/// How a picture sits in its widget: a screen whole through its lens, a camera covering its card, one's own mirrored.
#[derive(Clone, Copy)]
enum Placing {
    Through(Lens),
    Covering,
    Mirrored,
}

/// Takes the slot's newest picture onto the screen, if one waits, after telling the slot the size it is drawn at.
fn take(
    shown: &mut Shown,
    slot: &Slot,
    area: (u32, u32),
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    painter: &Painter,
) {
    slot.want(area);
    let started = Clock::now();
    shown.current.follow(slot);
    let Some(frame) = slot.take() else { return };
    let first = shown.current.size().is_none();
    if shown.current.show(device, queue, painter, slot, frame).is_err() && !TOO_LARGE.swap(true, Ordering::Relaxed) {
        report::note!(warn, "a shared picture is larger than this GPU can show, so it is not drawn");
    }
    if let Some((width, height)) = shown.current.size().filter(|_| first) {
        report::note!(info, "watched screen: first picture painted", width = width, height = height);
    }
    if TAKEN.fetch_add(1, Ordering::Relaxed) == 0 {
        // The one fact this window is here to show: a decoded picture becomes a picture on its own device,
        // whatever shape it arrives in. A surface this device cannot import leaves it saying `none`.
        let size = shown.current.size();
        report::note!(
            info,
            "picture on iced's device",
            width = size.map(|(width, _)| width),
            height = size.map(|(_, height)| height)
        );
    }
    PREPARE_NANOS.fetch_add(u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX), Ordering::Relaxed);
}

/// The picture scene: the slot's picture under its line, beside the share's card, and once a second what it cost.
pub struct Watched {
    feed: Feed,
    said: Said,
    line: String,
    card: Option<Sharer>,
    words: words::Words,
    themed: Themed,
    /// Draw again and again, and end after this many redraws: a picture held up long enough to be photographed.
    finite: Option<u32>,
    /// Pictures decoded so far, where the decoder counts them for this window.
    decoded: Option<&'static AtomicU64>,
    /// Since when a second is counted, the redraws and the laid-out ones in it, and the totals it started at.
    counted: (Clock, u32, u32, [u64; 3]),
}

impl Watched {
    /// The scene over `feed`, its line and card read from `said` and said in `words`, in the window's scheme and the
    /// default accent.
    pub fn new(feed: Feed, said: Said, words: words::Words) -> Self {
        Self {
            feed,
            line: said.line(),
            card: said.card(),
            said,
            words,
            themed: Themed::new(Scheme::Dark, design::Accent::DEFAULT, design::Contrast::Standard),
            finite: None,
            decoded: None,
            counted: (Clock::now(), 0, 0, [0; 3]),
        }
    }

    /// Draws again and again, and ends the run after `redraws` redraws.
    pub fn finite(mut self, redraws: u32) -> Self {
        self.finite = Some(redraws);
        self
    }

    /// Reports `decoded`, the decoder's count of pictures for this window, in the line it writes each second.
    pub fn counting(mut self, decoded: &'static AtomicU64) -> Self {
        self.decoded = Some(decoded);
        self
    }

    /// Once a second: decoded, painted and redrawn pictures, how many redraws laid the window out, and what a prepare
    /// that took a picture cost.
    fn report(&mut self, laid_out: bool) {
        self.counted.1 += 1;
        self.counted.2 += u32::from(laid_out);
        let seconds = self.counted.0.elapsed().as_secs_f64();
        if seconds < 1.0 {
            return;
        }
        let now = [
            self.decoded.map_or(0, |decoded| decoded.load(Ordering::Relaxed)),
            TAKEN.load(Ordering::Relaxed),
            PREPARE_NANOS.load(Ordering::Relaxed),
        ];
        let d = |i: usize| now[i] - self.counted.3[i];
        if d(1) > 0 {
            report::note!(
                info,
                "watched screen counted",
                over_ms = report::millis(self.counted.0.elapsed()),
                decoded = d(0),
                pictures = d(1),
                redraws = self.counted.1,
                laid_out = self.counted.2,
                prepare_ns_each = d(2) / d(1)
            );
        }
        self.counted = (Clock::now(), 0, 0, now);
    }
}

impl Scene for Watched {
    type Message = Infallible;
    type Theme = ui::Theme;

    fn title(&self) -> String {
        self.words.phrase(words::Phrase::WindowTitle).into_owned()
    }

    fn prepare(&mut self, scheme: Scheme, motion: design::MotionPreference) {
        self.themed.follow(scheme, design::Accent::DEFAULT, design::Contrast::Standard, motion);
    }

    fn theme(&self) -> &ui::Theme {
        self.themed.theme()
    }

    fn view(&self, _: Size) -> Element<'_, Infallible, ui::Theme, Renderer> {
        let painted = Painted::OwnScreen(design::Radius::None);
        let picture = shader(Picture(self.feed.clone(), painted)).width(Length::Fill).height(Length::Fill);
        let card = self.card.as_ref().map(|(card, name)| (card, name.as_str()));
        ui::watch::view(self.line.clone(), card, Widget::_boxed(picture), &self.words)
    }

    fn update(&mut self, _: Vec<Infallible>) -> bool {
        false
    }

    fn drain(&mut self) -> bool {
        let (line, card) = (self.said.line(), self.said.card());
        let changed = line != self.line || card != self.card;
        (self.line, self.card) = (line, card);
        changed
    }

    fn redrawn(&mut self, redrawn: Redrawn, _: &mut Input<'_>) -> Next {
        self.report(redrawn.laid_out);
        match self.finite {
            Some(redraws) if redrawn.frames >= redraws => {
                report::note!(info, "redraws done", redraws = redrawn.frames);
                Next::Exit
            }
            Some(_) => Next::Again,
            None => Next::Wait,
        }
    }
}
