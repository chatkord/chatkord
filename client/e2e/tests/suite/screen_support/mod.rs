//! What the screen end-to-end tests share: a picture painted in four colours where the compositor's would be (a
//! DMA-BUF on Linux, a texture on the encoder's D3D11 device on Windows), the encoder that takes it, and the feeding
//! of its units into the engine. Each test runs in a process of its own, as the decoder's choice is.
#![allow(dead_code, reason = "the fixture recorder takes only part of this module")]

#[cfg(windows)]
#[path = "../../../../media/tests/suite/common/d3d11.rs"]
mod d3d11;
#[cfg(target_os = "linux")]
#[path = "../../../../media/tests/suite/common/dmabuf.rs"]
mod dmabuf;

use crate::clock::frame_clock;
use capture::{Layout, Pixels};
#[cfg(windows)]
pub(crate) use d3d11::Surface;
#[cfg(target_os = "linux")]
pub(crate) use dmabuf::Surface;
use media::{Codec, Encoder, Name, Nv12, Runs, Session, Settings, survey};
use std::num::NonZeroU32;
use std::time::Duration;
use tokio::sync::watch;
use webrtc::Feeder;

pub(crate) const WIDTH: usize = 1280;
pub(crate) const HEIGHT: usize = 720;
pub(crate) const FPS: u32 = 30;
pub(crate) const KEY_EVERY: u32 = 2 * FPS;
pub(crate) const QUADRANTS: [[u8; 3]; 4] = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [200, 100, 50]];

/// A survey that is `report` for good, whatever the devices: the encoders a test chose for a share.
pub(crate) fn kept(report: media::Report) -> std::sync::Arc<media::Survey> {
    struct Chosen(media::Report);
    impl media::Surveyor for Chosen {
        fn devices(&self) -> media::Devices {
            media::Devices::default()
        }

        fn survey(&self) -> (media::Devices, media::Report) {
            (media::Devices::default(), self.0.clone())
        }
    }
    std::sync::Arc::new(media::Survey::with(Chosen(report)))
}

/// The screen as the compositor sends it.
pub(crate) fn layout(width: usize, height: usize) -> Layout {
    Layout { pixels: Pixels::Bgrx, width: width as u32, height: height as u32 }
}

pub(crate) fn settings(width: usize, height: usize) -> Settings {
    Settings {
        width,
        height,
        frames_per_second: FPS,
        bits_per_second: 4_000_000,
        key_frame_every: NonZeroU32::new(KEY_EVERY),
    }
}

/// Where a painted picture is made: the render node a DMA-BUF is made on.
#[cfg(target_os = "linux")]
pub(crate) struct Canvas(String);

/// Where a painted picture is made: the session's own D3D11 device, as the capture draws on it.
#[cfg(windows)]
pub(crate) struct Canvas;

impl Canvas {
    /// The render node, for a surface made before any session is.
    #[cfg(target_os = "linux")]
    pub(crate) fn render_node(&self) -> &str {
        &self.0
    }

    /// A `width`x`height` surface `session`'s encoder takes; none when the device makes none of that size.
    #[cfg(target_os = "linux")]
    pub(crate) fn surface(&self, _session: &Session, width: usize, height: usize) -> Option<Surface> {
        Surface::new(&self.0, width, height)
    }

    /// A `width`x`height` surface `session`'s encoder takes; none when the device makes none of that size.
    #[cfg(windows)]
    pub(crate) fn surface(&self, session: &Session, width: usize, height: usize) -> Option<Surface> {
        let gpu = session.gpu().expect("a D3D11 session lends its device");
        Surface::new(&gpu, width, height)
    }
}

/// The H.264 encoders the screen tries, best first, and where their picture is painted.
pub(crate) fn candidates() -> Vec<(Encoder, Canvas)> {
    candidates_of(Codec::H264)
}

/// `codec`'s encoders the screen tries, best first, and where their picture is painted.
#[cfg(target_os = "linux")]
pub(crate) fn candidates_of(codec: Codec) -> Vec<(Encoder, Canvas)> {
    tier::lab();
    let report = survey();
    let vaapi = report.encoders.iter().filter_map(media::Encoder::device).find(|d| d.backend == media::Backend::VaApi);
    let Some(node) = vaapi.and_then(|d| d.node.clone()) else {
        tier::skipped("no VA-API device to make a DMA-BUF on");
        return Vec::new();
    };
    screen_encoders(&report, codec).into_iter().map(|e| (e, Canvas(node.clone()))).collect()
}

/// `codec`'s encoders the screen tries, best first, and where their picture is painted.
#[cfg(windows)]
pub(crate) fn candidates_of(codec: Codec) -> Vec<(Encoder, Canvas)> {
    tier::lab();
    screen_encoders(&survey(), codec).into_iter().map(|e| (e, Canvas)).collect()
}

/// `codec`'s screen order in `report`, and for AV1 SVT-AV1 converting on each device the screen's H.264 encoders run
/// on, as a machine whose graphics card makes no AV1 would have it.
fn screen_encoders(report: &media::Report, codec: Codec) -> Vec<Encoder> {
    let mut encoders: Vec<Encoder> = report.screen_order(codec).cloned().collect();
    if codec == Codec::Av1 {
        for device in report.screen_order(Codec::H264).filter_map(Encoder::device) {
            let software = Encoder {
                name: Name::SvtAv1,
                runs: Runs::Cpu { convert: Some(device.clone()) },
                multiple: media::Multiple::EVEN,
            };
            if !encoders.contains(&software) {
                encoders.push(software);
            }
        }
    }
    encoders
}

pub(crate) fn quadrants(width: usize, height: usize) -> Vec<[u8; 3]> {
    (0..height)
        .flat_map(|y| {
            (0..width).map(move |x| QUADRANTS[usize::from(x >= width / 2) + 2 * usize::from(y >= height / 2)])
        })
        .collect()
}

/// BT.709 limited range, as the encoder tags it.
pub(crate) fn expected([r, g, b]: [u8; 3]) -> [f64; 3] {
    let (r, g, b) = (f64::from(r) / 255.0, f64::from(g) / 255.0, f64::from(b) / 255.0);
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    [16.0 + 219.0 * y, 128.0 + 224.0 * (b - y) / 1.8556, 128.0 + 224.0 * (r - y) / 1.5748]
}

/// Encodes the painted surface for `pictures` frames and pushes every unit to the engine, answering its key-frame
/// requests; at `resize_at` the screen takes the new size (a new surface). `fed` is told each picture's count.
pub(crate) fn feed(
    encoder: &Encoder,
    canvas: &Canvas,
    feeder: &Feeder,
    pictures: usize,
    resize_at: Option<(usize, (usize, usize))>,
    fed: &watch::Sender<usize>,
) -> usize {
    let mut session = Session::open_screen(encoder, settings(WIDTH, HEIGHT), layout(WIDTH, HEIGHT))
        .expect("the encoder takes the screen");
    let mut surface = canvas.surface(&session, WIDTH, HEIGHT).expect("a surface");
    surface.paint_rgb(&quadrants(WIDTH, HEIGHT));
    let (mut width, mut height) = (WIDTH, HEIGHT);
    let mut keys = 0;
    for n in 0..pictures {
        if let Some((_, (new_width, new_height))) = resize_at.filter(|(at, _)| *at == n) {
            (width, height) = (new_width, new_height);
            session.set_size(width, height).expect("a usable size");
            surface = canvas.surface(&session, width, height).expect("a surface");
            surface.paint_rgb(&quadrants(width, height));
        }
        if feeder.key_frame_wanted() {
            session.request_key_frame();
            keys += 1;
        }
        for unit in session.encode_frame(surface.frame(Pixels::Bgrx)).expect("encode the screen") {
            feeder.push(&unit.data, unit.key_frame, width as u32, height as u32, unit.captured);
        }
        fed.send_replace(n + 1);
        frame_clock(Duration::from_millis(1000 / u64::from(FPS)));
    }
    keys
}

/// The OBU types in an AV1 temporal unit of low-overhead OBUs: a header byte, an extension byte if flagged, a size.
pub(crate) fn obu_types(data: &[u8]) -> Vec<u8> {
    let (mut types, mut at) = (Vec::new(), 0);
    while let Some(header) = data.get(at) {
        types.push((header >> 3) & 0x0f);
        at += 1 + usize::from((header >> 2) & 1);
        let (mut size, mut shift) = (0usize, 0);
        while let Some(byte) = data.get(at) {
            size |= usize::from(byte & 0x7f) << shift;
            at += 1;
            shift += 7;
            if byte & 0x80 == 0 {
                break;
            }
        }
        at += size;
    }
    types
}

/// Whether the middle of each quadrant of a picture `media` decoded is the colour painted there.
pub(crate) fn decoded_is_the_painting(picture: &Nv12) -> bool {
    let (width, height) = (picture.width(), picture.height());
    QUADRANTS.iter().enumerate().all(|(i, rgb)| {
        let (x, y) = (width / 4 + (i % 2) * width / 2, height / 4 + (i / 2) * height / 2);
        let chroma = (y / 2) * width + (x / 2) * 2;
        let got = [
            f64::from(picture.luma()[y * width + x]),
            f64::from(picture.chroma()[chroma]),
            f64::from(picture.chroma()[chroma + 1]),
        ];
        got.iter().zip(expected(*rgb)).all(|(g, w)| (g - w).abs() <= 6.0)
    })
}
