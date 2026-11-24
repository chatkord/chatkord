//! A screen picture in a DMA-BUF is converted on the encoder's GPU, encoded and decoded again in software to the
//! picture it was; the DMA-BUF is a VA-API surface exported as a compositor's buffer arrives, so no person is needed.

use crate::common;

use capture::{Layout, Pixels};
use common::dmabuf::Surface;
use common::{HEIGHT, LAYOUT, SETTINGS, WIDTH, colour_of, level_needed, picture, profile_and_level, psnr, skipped};
use media::{
    Backend, Codec, Convert, Decoder, EncodeError, Encoded, Encoder, Nv12, Path, Runs, Session, Settings, Sources,
    survey,
};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The encoders to try, each with a render node to make the buffer on: every H.264 encoder, and SVT-AV1 converting on
/// each device they run on, as a machine with no AV1 encoder would.
fn candidates() -> Vec<(Encoder, String)> {
    let report = survey();
    let vaapi = report.encoders.iter().filter_map(Encoder::device).find(|d| d.backend == Backend::VaApi);
    let Some(node) = vaapi.and_then(|d| d.node.clone()) else {
        skipped("no VA-API device to make a DMA-BUF on");
        return Vec::new();
    };
    let hardware: Vec<Encoder> = report.encoders.into_iter().filter(|e| e.codec() == Codec::H264).collect();
    let software = hardware.iter().filter_map(Encoder::device).map(|device| Encoder {
        name: media::Name::SvtAv1,
        runs: Runs::Cpu { convert: Some(device.clone()) },
        multiple: media::Multiple::EVEN,
    });
    let all: Vec<Encoder> = software.collect();
    hardware.into_iter().chain(all).map(|e| (e, node.clone())).collect()
}

/// Luma of picture `n` as the gray a screen would show, so the encoded luma should be the picture's own.
fn gray(n: usize) -> Vec<u8> {
    picture(n).luma().iter().map(|l| (((u32::from(*l) - 16) * 255 + 109) / 219) as u8).collect()
}

fn decoded(encoder: &Encoder, stream: &[Encoded]) -> Vec<Nv12> {
    let mut decoder = Decoder::open(encoder.codec(), &Sources { render_nodes: vec![], ..Sources::default() })
        .expect("software decoder");
    let mut got = Vec::new();
    for unit in stream {
        got.extend(decoder.decode(&unit.data, unit.pts).expect("decode"));
    }
    got.extend(decoder.finish().expect("finish"));
    got.into_iter().map(|d| d.picture).collect()
}

/// BT.709, limited range: what the stream is tagged as, in 8 bits.
fn expected(rgb: [u8; 3]) -> [f64; 3] {
    let [r, g, b] = rgb.map(|c| f64::from(c) / 255.0);
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    [16.0 + 219.0 * y, 128.0 + 224.0 * (b - y) / 1.8556, 128.0 + 224.0 * (r - y) / 1.5748]
}

/// A screen of `source` size painted `rgb` and sent at `target`, the last picture decoded; None where this encoder
/// or device cannot (said).
fn sent_scaled(
    encoder: &Encoder,
    node: &str,
    source: (usize, usize),
    target: (usize, usize),
    rgb: &[[u8; 3]],
) -> Option<(Nv12, Option<Convert>)> {
    let Some(mut surface) = Surface::new(node, source.0, source.1) else {
        skipped(format!("the VA-API device makes no {}x{} BGRx surface", source.0, source.1));
        return None;
    };
    let layout = Layout { width: source.0 as u32, height: source.1 as u32, ..LAYOUT };
    let settings = media::Settings { width: target.0, height: target.1, bits_per_second: 20_000_000, ..SETTINGS };
    let mut session = match Session::open_screen(encoder, settings, layout) {
        Ok(session) => session,
        Err(why) => {
            skipped(format!("{encoder} cannot scale a screen here: {why}"));
            return None;
        }
    };
    let convert = session.convert();
    surface.paint_rgb(rgb);
    let mut stream = Vec::new();
    // A second of the same picture, read at its end: what is measured is the scale, not how soon the encoder settles.
    for _ in 0..settings.frames_per_second {
        stream.extend(session.encode_frame(surface.frame(Pixels::Bgrx)).expect("encode a scaled picture"));
    }
    stream.extend(session.finish().expect("finish"));
    let last = decoded(encoder, &stream).pop().expect("a picture");
    assert_eq!((last.width(), last.height()), target, "{encoder}: the stream is the size asked for");
    Some((last, convert))
}

/// The mean of the `gray` picture `wide` pixels across under each pixel of a `width` by `height` stream, weighted by
/// how much of it is covered, as limited-range luma: what a stream that loses no line shows.
fn area_mean(gray: impl Fn(usize, usize) -> f64, wide: usize, high: usize, width: usize, height: usize) -> Vec<f64> {
    let span = |at: usize, of: usize, whole: usize| {
        let (low, high) = (at as f64 * whole as f64 / of as f64, (at + 1) as f64 * whole as f64 / of as f64);
        (low.floor() as usize..high.ceil() as usize)
            .map(move |i| (i, high.min((i + 1) as f64) - low.max(i as f64)))
            .collect::<Vec<_>>()
    };
    (0..width * height)
        .map(|i| {
            let (columns, rows) = (span(i % width, width, wide), span(i / width, height, high));
            let (mut sum, mut covered) = (0.0, 0.0);
            for (y, tall) in &rows {
                for (x, broad) in &columns {
                    sum += gray(*x, *y) * tall * broad;
                    covered += tall * broad;
                }
            }
            16.0 + 219.0 * sum / covered / 255.0
        })
        .collect()
}

mod lab {
    use super::*;

    /// A 4K screen of one-pixel lines, as text and window borders are, every third column and fifth row: a stream pixel
    /// covers 1.5, 2 or 3 of them on a side, and a read or two between pixels lands on the dark ones and drops a line.
    const THIN: (usize, usize) = (3840, 2160);

    /// The stream sizes the thin-line screen is sent at: 1.5x, 2x and 3x smaller.
    const THIN_SENT: [(usize, usize); 3] = [(2560, 1440), (1920, 1080), (1280, 720)];

    /// The least luma PSNR against the area mean at which the lines are kept.
    const LINES_KEPT_DB: f64 = 35.0;

    /// The gray of the thin-line screen at `x`, `y`.
    fn thin(x: usize, y: usize) -> f64 {
        if x.is_multiple_of(3) || y.is_multiple_of(5) { 230.0 } else { 20.0 }
    }

    /// The thin-line screen as `encoder` sends it at `size`: its luma PSNR against the area mean, and the convert it went
    /// through; none where it cannot (said).
    #[expect(clippy::print_stderr, reason = "a measuring test's output is what it prints")]
    fn thin_lines_at(
        encoder: &Encoder,
        node: &str,
        painted: &[[u8; 3]],
        size: (usize, usize),
    ) -> Option<(f64, Option<Convert>)> {
        let (last, convert) = sent_scaled(encoder, node, THIN, size, painted)?;
        let truth = area_mean(thin, THIN.0, THIN.1, size.0, size.1);
        let squared: f64 = last.luma().iter().zip(&truth).map(|(got, want)| (f64::from(*got) - want).powi(2)).sum();
        let db = 10.0 * (255.0f64 * 255.0 / (squared / truth.len() as f64).max(f64::EPSILON)).log10();
        eprintln!(
            "{encoder} through {convert:?}: 4K to {}x{} of one-pixel lines, luma PSNR {db:.1} dB against the mean",
            size.0, size.1
        );
        Some((db, convert))
    }

    /// The thin-line screen's pixels.
    fn thin_painted() -> Vec<[u8; 3]> {
        (0..THIN.0 * THIN.1).map(|i| [thin(i % THIN.0, i / THIN.0) as u8; 3]).collect()
    }

    #[test]
    fn a_screen_picture_is_converted_on_the_gpu_and_decodes_to_the_picture_it_was() {
        for (encoder, node) in candidates() {
            let Some(mut surface) = Surface::new(&node, WIDTH, HEIGHT) else {
                skipped("the VA-API device makes no BGRx surface");
                return;
            };
            let mut session = match Session::open_screen(&encoder, SETTINGS, LAYOUT) {
                Ok(session) => session,
                Err(why) => {
                    skipped(format!("{encoder} cannot take a screen picture here: {why}"));
                    continue;
                }
            };
            let mut stream = Vec::new();
            for n in 0..30 {
                surface.paint(&gray(n));
                stream.extend(session.encode_frame(surface.frame(Pixels::Bgrx)).expect("encode a screen picture"));
                // One buffer stands for the compositor's: it is drawn into again only once it is back.
                session.settle().expect("the convert finished");
            }
            stream.extend(session.finish().expect("finish"));
            assert_eq!(stream.len(), 30, "{encoder}: one unit for each picture");
            assert!(stream[0].key_frame, "{encoder}: starts on a key frame");
            // AV1's colour description is read back by the colours test, decoded.
            if encoder.codec() == Codec::H264 {
                let (profile, level) = profile_and_level(&stream[0].data).expect("an SPS");
                assert_eq!(profile, 100, "{encoder}: High");
                let needed = level_needed(WIDTH, HEIGHT, SETTINGS.frames_per_second as usize);
                assert!(level >= needed, "{encoder}: level {level} holds a stream that needs {needed}");
                assert_eq!(
                    colour_of(&stream[0].data),
                    Some((false, [1, 1, 1])),
                    "{encoder}: the stream says BT.709 in limited range"
                );
            }
            let worst = decoded(&encoder, &stream).iter().enumerate().map(|(n, p)| psnr(p, n)).fold(f64::MAX, f64::min);
            eprintln!("{encoder}: screen pictures decoded, worst luma PSNR {worst:.1} dB");
            assert!(worst > 30.0, "{encoder}: the decoded picture is not the one on the screen ({worst:.1} dB)");
        }
    }

    #[test]
    fn a_4k60_h264_screen_states_a_level_that_holds_it() {
        let (width, height) = (3840, 2160);
        let settings = Settings { width, height, frames_per_second: 60, bits_per_second: 20_000_000, ..SETTINGS };
        let layout = Layout { width: width as u32, height: height as u32, ..LAYOUT };
        for (encoder, node) in candidates().into_iter().filter(|(e, _)| e.codec() == Codec::H264) {
            let Some(mut surface) = Surface::new(&node, width, height) else {
                skipped("the VA-API device makes no 4K BGRx surface");
                return;
            };
            let Ok(mut session) = Session::open_screen(&encoder, settings, layout) else {
                skipped(format!("{encoder} cannot take a 4K screen here"));
                continue;
            };
            surface.paint(&vec![128; width * height]);
            let mut stream = session.encode_frame(surface.frame(Pixels::Bgrx)).expect("a picture");
            stream.extend(session.finish().expect("finish"));
            let (profile, level) = profile_and_level(&stream[0].data).expect("an SPS");
            eprintln!("{encoder}: 4K60 as profile_idc {profile}, level_idc {level}");
            assert_eq!(profile, 100, "{encoder}: High");
            assert!(level >= level_needed(width, height, 60), "{encoder}: level {level} holds 4K60");
        }
    }

    #[test]
    fn the_gpu_reads_the_buffer_in_place_so_a_repaint_of_it_is_what_the_next_picture_shows() {
        for (encoder, node) in candidates() {
            let Some(mut surface) = Surface::new(&node, WIDTH, HEIGHT) else {
                skipped("the VA-API device makes no BGRx surface");
                return;
            };
            let Ok(mut session) = Session::open_screen(&encoder, SETTINGS, LAYOUT) else {
                skipped(format!("{encoder} cannot take a screen picture here"));
                continue;
            };
            // One buffer painted with picture 0, read, painted with picture 20 and read through a new descriptor: a
            // copy taken at the first read would show picture 0 twice.
            surface.paint(&gray(0));
            let mut stream = session.encode_frame(surface.frame(Pixels::Bgrx)).expect("first");
            session.settle().expect("the first read finished");
            surface.paint(&gray(20));
            stream.extend(session.encode_frame(surface.frame(Pixels::Bgrx)).expect("second"));
            stream.extend(session.finish().expect("finish"));
            let pictures = decoded(&encoder, &stream);
            assert_eq!(pictures.len(), 2);
            let (first, second) = (psnr(&pictures[0], 0), psnr(&pictures[1], 20));
            let stale = psnr(&pictures[1], 0);
            eprintln!(
                "{encoder}: first {first:.1} dB against picture 0, second {second:.1} dB against picture 20 ({stale:.1} dB against 0)"
            );
            assert!(first > 30.0 && second > 30.0, "{encoder}: each read is the buffer as it was painted");
            assert!(stale < 30.0, "{encoder}: the second read is not the first picture again");
        }
    }

    #[test]
    fn a_picture_of_another_size_is_refused_and_a_memory_session_takes_no_screen() {
        for (encoder, node) in candidates() {
            let Some(small) = Surface::new(&node, 64, 64) else {
                skipped("the VA-API device makes no BGRx surface");
                return;
            };
            let Ok(mut session) = Session::open_screen(&encoder, SETTINGS, LAYOUT) else {
                skipped(format!("{encoder} cannot take a screen picture here"));
                continue;
            };
            assert_eq!(session.encode_frame(small.frame(Pixels::Bgrx)).err(), Some(EncodeError::FrameSize));
            assert_eq!(session.encode(&picture(0)).err(), Some(EncodeError::Input));
            let mut memory = Session::open(&encoder, SETTINGS).expect("open");
            assert_eq!(memory.encode_frame(small.frame(Pixels::Bgrx)).err(), Some(EncodeError::Input));
        }
    }

    #[test]
    fn a_screen_with_an_odd_side_is_taken_without_its_last_column_and_row() {
        for (encoder, node) in candidates() {
            let Some(mut surface) = Surface::new(&node, WIDTH + 1, HEIGHT + 1) else {
                skipped("the VA-API device makes no odd-sized BGRx surface");
                return;
            };
            let odd = Layout { width: LAYOUT.width + 1, height: LAYOUT.height + 1, ..LAYOUT };
            let Ok(mut session) = Session::open_screen(&encoder, SETTINGS, odd) else {
                skipped(format!("{encoder} cannot take a screen picture here"));
                continue;
            };
            // The last column and row are white and the rest dark: a rescue by scaling would bleed white into the edge.
            let (wide, high) = (WIDTH + 1, HEIGHT + 1);
            let painted: Vec<[u8; 3]> = (0..wide * high)
                .map(|i| if i % wide == wide - 1 || i / wide == high - 1 { [255; 3] } else { [40; 3] })
                .collect();
            surface.paint_rgb(&painted);
            let mut stream = Vec::new();
            for _ in 0..5 {
                stream.extend(session.encode_frame(surface.frame(Pixels::Bgrx)).expect("encode the cropped picture"));
            }
            stream.extend(session.finish().expect("finish"));
            let last = decoded(&encoder, &stream).pop().expect("a picture");
            assert_eq!((last.width(), last.height()), (WIDTH, HEIGHT), "{encoder}: the stream is of even size");
            let (right, bottom) =
                (last.luma()[(HEIGHT / 2) * WIDTH + WIDTH - 1], last.luma()[(HEIGHT - 1) * WIDTH + WIDTH / 2]);
            assert!(
                right < 80 && bottom < 80,
                "{encoder}: the left-out column and row show at the edge ({right}, {bottom})"
            );
        }
    }

    /// Each Vulkan H.264 encoder here with its render node, or none said skipped.
    fn vulkan_encoders() -> Vec<(Encoder, String)> {
        let vulkan: Vec<_> =
            candidates().into_iter().filter(|(e, _)| e.path() == Path::Hardware(Backend::Vulkan)).collect();
        if vulkan.is_empty() {
            skipped("no Vulkan H.264 encoder here");
        }
        vulkan
    }

    /// A Vulkan encoder keeps a picture in flight; ended undrained, its bitstream buffer goes back before its device
    /// does, which the validation layer would report as a buffer alive at the device's end (`patches/ffmpeg` 0007).
    #[test]
    fn a_session_dropped_with_a_picture_in_flight_leaves_nothing_on_its_device() {
        if !tier::validated() {
            return;
        }
        for (encoder, node) in vulkan_encoders() {
            let Some(mut surface) = Surface::new(&node, WIDTH, HEIGHT) else {
                return skipped("the VA-API device makes no BGRx surface");
            };
            let mut session = Session::open_screen(&encoder, SETTINGS, LAYOUT).expect("a Vulkan screen session");
            for n in 0..2 {
                surface.paint(&gray(n));
                session.encode_frame(surface.frame(Pixels::Bgrx)).expect("encode");
                session.settle().expect("the convert finished");
            }
            assert!(session.holds_a_change(), "{encoder}: a picture still in flight");
            drop(session);
        }
    }

    /// A compositor's buffers, each drawn into again only once it came back: the Vulkan convert is not waited on
    /// before the encoder is given its picture, so a buffer stays lent while its run may still read it and comes back
    /// once the run is waited, one picture later; every picture still decodes to the one drawn, and every buffer is
    /// back once the session ends.
    #[test]
    fn a_buffer_read_by_a_convert_in_flight_comes_back_only_after_it_and_each_picture_is_the_one_drawn() {
        if !tier::validated() {
            return;
        }
        struct Back(Arc<AtomicUsize>);
        impl Drop for Back {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        for (encoder, node) in vulkan_encoders() {
            let Some(mut surfaces) = (0..3).map(|_| Surface::new(&node, WIDTH, HEIGHT)).collect::<Option<Vec<_>>>()
            else {
                return skipped("the VA-API device makes no BGRx surface");
            };
            let back: Vec<Arc<AtomicUsize>> = surfaces.iter().map(|_| Arc::default()).collect();
            let mut lent = vec![0; surfaces.len()];
            let mut session = Session::open_screen(&encoder, SETTINGS, LAYOUT).expect("a Vulkan screen session");
            let mut stream = Vec::new();
            for n in 0..30 {
                let at = n % surfaces.len();
                let home = back[at].load(Ordering::SeqCst);
                assert_eq!(home, lent[at], "{encoder}: buffer {at} is back before it is drawn into again");
                surfaces[at].paint(&gray(n));
                let frame = surfaces[at].frame_lent(Pixels::Bgrx, Back(Arc::clone(&back[at])));
                stream.extend(session.encode_frame(frame).expect("encode a screen picture"));
                lent[at] += 1;
            }
            stream.extend(session.finish().expect("finish"));
            let home: Vec<usize> = back.iter().map(|count| count.load(Ordering::SeqCst)).collect();
            assert_eq!(home, lent, "{encoder}: every buffer is back once the session ends");
            let worst = decoded(&encoder, &stream).iter().enumerate().map(|(n, p)| psnr(p, n)).fold(f64::MAX, f64::min);
            assert!(worst > 30.0, "{encoder}: a picture is not the one drawn ({worst:.1} dB)");
        }
    }

    /// A share handed to another encoder after two pictures keeps every unit the first made past that encoder's
    /// device: no unit's bytes live in an encoder's memory, which the layer would report at the device's end.
    #[test]
    fn a_hand_over_after_two_pictures_keeps_its_units_past_the_old_device() {
        if !tier::validated() {
            return;
        }
        for (encoder, node) in vulkan_encoders() {
            let others = candidates().into_iter().map(|(other, _)| other);
            let Some(next) =
                others.filter(|other| other.device() != encoder.device()).find(|o| o.codec() == encoder.codec())
            else {
                return skipped("no other H.264 encoder to hand a Vulkan encoder's share to");
            };
            let Some(mut surface) = Surface::new(&node, WIDTH, HEIGHT) else {
                return skipped("the VA-API device makes no BGRx surface");
            };
            let mut session = Session::open_screen(&encoder, SETTINGS, LAYOUT).expect("a Vulkan screen session");
            let mut kept = Vec::new();
            for n in 0..2 {
                surface.paint(&gray(n));
                kept.extend(session.encode_frame(surface.frame(Pixels::Bgrx)).expect("encode"));
                session.settle().expect("the convert finished");
            }
            session.set_encoder(&next).expect("the same codec");
            kept.extend(session.reopen().expect("the next encoder opens").held);
            for n in 2..4 {
                surface.paint(&gray(n));
                kept.extend(session.encode_frame(surface.frame(Pixels::Bgrx)).expect("encode on the next"));
                session.settle().expect("the convert finished");
            }
            kept.extend(session.finish().expect("finish"));
            assert!(kept.len() >= 4, "{encoder} then {next}: every picture's unit kept ({})", kept.len());
            drop(kept);
        }
    }

    /// Under GPU-assisted validation, which sees the convert shader's stores.
    mod gpu_validated {
        use super::*;

        /// 1080 rows are 67.5 of the 16-row groups a dispatch covers, so the last groups lie past the stream.
        const SCREEN: (usize, usize) = (1921, 1081);

        #[test]
        fn a_screen_whose_last_groups_lie_past_the_stream_stores_nothing_past_it() {
            if !tier::validated() {
                return;
            }
            let settings = Settings { width: SCREEN.0 - 1, height: SCREEN.1 - 1, ..SETTINGS };
            let layout = Layout { width: SCREEN.0 as u32, height: SCREEN.1 as u32, ..LAYOUT };
            for (encoder, node) in candidates() {
                let Some(mut surface) = Surface::new(&node, SCREEN.0, SCREEN.1) else {
                    skipped("the VA-API device makes no odd-sized BGRx surface");
                    return;
                };
                let Ok(mut session) = Session::open_screen(&encoder, settings, layout) else {
                    skipped(format!("{encoder} cannot take a screen picture here"));
                    continue;
                };
                surface.paint_rgb(&vec![[40; 3]; SCREEN.0 * SCREEN.1]);
                // A store past the stream is a validation error, which ends the test.
                for _ in 0..3 {
                    session.encode_frame(surface.frame(Pixels::Bgrx)).expect("encode the cropped picture");
                }
                session.finish().expect("finish");
            }
        }
    }

    #[test]
    fn a_still_screen_answers_a_key_frame_request_with_its_last_picture() {
        for (encoder, node) in candidates() {
            let Some(mut surface) = Surface::new(&node, WIDTH, HEIGHT) else {
                skipped("the VA-API device makes no BGRx surface");
                return;
            };
            let Ok(mut session) = Session::open_screen(&encoder, SETTINGS, LAYOUT) else {
                skipped(format!("{encoder} cannot take a screen picture here"));
                continue;
            };
            assert!(session.repeat_last().expect("nothing yet").is_empty(), "{encoder}: no picture, nothing to repeat");
            surface.paint(&gray(0));
            let mut stream = session.encode_frame(surface.frame(Pixels::Bgrx)).expect("the one picture");
            let held = session.holds_a_change();
            assert_eq!(held, stream.is_empty(), "{encoder}: a change not handed out is held, and only then");
            // The screen now sends nothing. It is sent again as it is asked to be, and a key frame asked for comes out;
            // the first repeat hands out a change the encoder held.
            let again = session.repeat_last().expect("the picture again");
            if held {
                assert!(again.iter().any(|unit| unit.pts == 0), "{encoder}: the repeat hands out the held picture");
            }
            assert!(!session.holds_a_change(), "{encoder}: a repeat is no change to hold");
            stream.extend(again);
            session.request_key_frame();
            let asked = session.repeat_last().expect("the picture as a key frame");
            assert!(asked.iter().any(|unit| unit.key_frame), "{encoder}: the ask is answered with a key frame");
            let key = asked.iter().find(|unit| unit.key_frame).expect("a key frame").clone();
            stream.extend(asked);
            stream.extend(session.finish().expect("finish"));
            let pictures = decoded(&encoder, std::slice::from_ref(&key));
            assert_eq!(pictures.len(), 1, "{encoder}: the key frame decodes on its own");
            let worst = psnr(&pictures[0], 0);
            assert!(worst > 30.0, "{encoder}: the repeat is the picture that was on the screen ({worst:.1} dB)");
        }
    }

    #[test]
    fn colours_arrive_as_bt709_limited_range_so_chroma_is_the_right_way_round() {
        // Four flat quadrants whose luma and both chroma values differ a long way from each other.
        const QUADRANTS: [[u8; 3]; 4] = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [200, 100, 50]];
        let picture: Vec<[u8; 3]> = (0..HEIGHT)
            .flat_map(|y| {
                (0..WIDTH).map(move |x| QUADRANTS[usize::from(x >= WIDTH / 2) + 2 * usize::from(y >= HEIGHT / 2)])
            })
            .collect();
        for (encoder, node) in candidates() {
            let Some(mut surface) = Surface::new(&node, WIDTH, HEIGHT) else {
                skipped("the VA-API device makes no BGRx surface");
                return;
            };
            let Ok(mut session) = Session::open_screen(&encoder, SETTINGS, LAYOUT) else {
                skipped(format!("{encoder} cannot take a screen picture here"));
                continue;
            };
            surface.paint_rgb(&picture);
            let mut stream = Vec::new();
            for _ in 0..10 {
                stream.extend(session.encode_frame(surface.frame(Pixels::Bgrx)).expect("encode"));
            }
            stream.extend(session.finish().expect("finish"));
            let last = decoded(&encoder, &stream).pop().expect("a picture");
            for (i, rgb) in QUADRANTS.iter().enumerate() {
                // The middle of the quadrant, away from the edges where chroma is shared with a neighbour.
                let (x, y) = (WIDTH / 4 + (i % 2) * WIDTH / 2, HEIGHT / 4 + (i / 2) * HEIGHT / 2);
                let got = [
                    f64::from(last.luma()[y * WIDTH + x]),
                    f64::from(last.chroma()[(y / 2) * WIDTH + (x / 2) * 2]),
                    f64::from(last.chroma()[(y / 2) * WIDTH + (x / 2) * 2 + 1]),
                ];
                let want = expected(*rgb);
                eprintln!("{encoder}: {rgb:?} -> Y/Cb/Cr {got:?}, wanted {want:.0?}");
                assert!(
                    got.iter().zip(want).all(|(g, w)| (g - w).abs() <= 3.0),
                    "{encoder}: {rgb:?} decoded as {got:?}, not {want:.0?}"
                );
            }
        }
    }

    #[test]
    fn a_screen_is_scaled_down_on_the_gpu_and_keeps_its_colours() {
        const QUADRANTS: [[u8; 3]; 4] = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [200, 100, 50]];
        let (wide, high) = (2560, 1440);
        let painted: Vec<[u8; 3]> = (0..wide * high)
            .map(|i| QUADRANTS[usize::from(i % wide >= wide / 2) + 2 * usize::from(i / wide >= high / 2)])
            .collect();
        for (encoder, node) in candidates() {
            for (width, height) in [(1920, 1080), (1280, 720)] {
                let Some((last, _)) = sent_scaled(&encoder, &node, (wide, high), (width, height), &painted) else {
                    continue;
                };
                for (i, rgb) in QUADRANTS.iter().enumerate() {
                    let (x, y) = (width / 4 + (i % 2) * width / 2, height / 4 + (i / 2) * height / 2);
                    let got = [
                        f64::from(last.luma()[y * width + x]),
                        f64::from(last.chroma()[(y / 2) * width + (x / 2) * 2]),
                        f64::from(last.chroma()[(y / 2) * width + (x / 2) * 2 + 1]),
                    ];
                    let want = expected(*rgb);
                    assert!(
                        got.iter().zip(want).all(|(g, w)| (g - w).abs() <= 3.0),
                        "{encoder} at {width}x{height}: {rgb:?} decoded as {got:?}, not {want:.0?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_scaled_screen_is_the_mean_of_what_each_pixel_covers_so_thin_lines_do_not_vanish() {
        let painted = thin_painted();
        let mut measured = Vec::new();
        for (width, height) in THIN_SENT {
            for (encoder, node) in candidates() {
                let Some((db, convert)) = thin_lines_at(&encoder, &node, &painted, (width, height)) else {
                    continue;
                };
                // Our area pass averages by area at any ratio; VA-API's scaler is the driver's and only printed.
                if convert == Some(Convert::Area) {
                    measured.push((format!("{encoder} at {width}x{height}"), db));
                }
            }
        }
        for (what, db) in measured {
            assert!(db > LINES_KEPT_DB, "{what}: the scaled lines are not the mean of what they cover ({db:.1} dB)");
        }
    }

    /// A VA-API encoder's share is scaled by our area pass, writing into the encoder's own surfaces, not by VA-API's
    /// scaler, which drops the lines.
    #[test]
    fn a_share_scaled_on_vaapi_through_the_area_pass_keeps_thin_lines() {
        let vaapi: Vec<_> =
            candidates().into_iter().filter(|(e, _)| e.path() == Path::Hardware(Backend::VaApi)).collect();
        if vaapi.is_empty() {
            return skipped("no VA-API H.264 encoder here");
        }
        let painted = thin_painted();
        for (width, height) in THIN_SENT {
            for (encoder, node) in &vaapi {
                let (db, convert) =
                    thin_lines_at(encoder, node, &painted, (width, height)).expect("a VA-API encoder scales a screen");
                assert_eq!(convert, Some(Convert::Area), "{encoder} at {width}x{height}: through our area pass");
                assert!(
                    db > LINES_KEPT_DB,
                    "{encoder} at {width}x{height}: the scaled lines are not the mean ({db:.1} dB)"
                );
            }
        }
    }

    #[test]
    fn a_scaled_screen_with_an_odd_side_leaves_its_last_column_and_row_out() {
        // 1.5x, where a read at the stream's last pixel reaches past the even edge unless it is kept inside.
        let (wide, high) = (1921, 1081);
        let painted: Vec<[u8; 3]> = (0..wide * high)
            .map(|i| if i % wide == wide - 1 || i / wide == high - 1 { [255; 3] } else { [40; 3] })
            .collect();
        let dark = expected([40; 3])[0];
        for (encoder, node) in candidates() {
            let Some((last, _)) = sent_scaled(&encoder, &node, (wide, high), (WIDTH, HEIGHT), &painted) else {
                continue;
            };
            let (right, bottom) =
                (last.luma()[(HEIGHT / 2) * WIDTH + WIDTH - 1], last.luma()[(HEIGHT - 1) * WIDTH + WIDTH / 2]);
            eprintln!("{encoder}: the scaled edge reads {right} and {bottom}, the picture {dark:.0}");
            assert!(
                f64::from(right.max(bottom)) - dark <= 4.0,
                "{encoder}: the left-out column and row bleed into the scaled edge ({right}, {bottom}; the picture is {dark:.0})"
            );
        }
    }

    #[test]
    fn a_stream_larger_than_the_screen_or_of_an_odd_size_is_refused() {
        for (encoder, _) in candidates() {
            let at = |width, height| media::Settings { width, height, ..SETTINGS };
            let odd = Layout { width: LAYOUT.width + 1, height: LAYOUT.height + 1, ..LAYOUT };
            for (settings, source) in
                [(at(WIDTH + 2, HEIGHT), LAYOUT), (at(WIDTH, HEIGHT + 2), odd), (at(641, 360), LAYOUT)]
            {
                assert_eq!(
                    Session::open_screen(&encoder, settings, source).err(),
                    Some(EncodeError::Settings),
                    "{encoder}: {}x{} from {}x{}",
                    settings.width,
                    settings.height,
                    source.width,
                    source.height
                );
            }
        }
    }

    #[test]
    fn a_screen_that_changes_size_goes_on_as_a_new_stream_at_the_size_asked_for_it() {
        for (encoder, node) in candidates() {
            let (Some(mut first), Some(mut grown)) =
                (Surface::new(&node, WIDTH, HEIGHT), Surface::new(&node, 1920, 1080))
            else {
                skipped("the VA-API device makes no BGRx surfaces of these sizes");
                return;
            };
            let Ok(mut session) = Session::open_screen(&encoder, SETTINGS, LAYOUT) else {
                skipped(format!("{encoder} cannot take a screen picture here"));
                continue;
            };
            first.paint_rgb(&vec![[20, 20, 20]; WIDTH * HEIGHT]);
            grown.paint_rgb(&vec![[230, 230, 230]; 1920 * 1080]);
            let mut stream = Vec::new();
            for _ in 0..10 {
                stream.extend(session.encode_frame(first.frame(Pixels::Bgrx)).expect("encode"));
            }
            let bigger = Layout { width: 1920, height: 1080, ..LAYOUT };
            assert_eq!(
                session.set_source(bigger, (1922, 1080)),
                Err(EncodeError::Settings),
                "never wider than the screen"
            );
            assert_eq!(session.set_source(bigger, (1281, 720)), Err(EncodeError::Settings), "never an odd side");
            // The share keeps its chosen size: the larger screen is scaled to it.
            session.set_source(bigger, (WIDTH, HEIGHT)).expect("a screen that grew");
            assert_eq!(
                session.encode_frame(first.frame(Pixels::Bgrx)).map(|_| ()),
                Err(EncodeError::FrameSize),
                "{encoder}: a picture of the old size is refused once the new one is asked"
            );
            for _ in 0..10 {
                stream.extend(session.encode_frame(grown.frame(Pixels::Bgrx)).expect("encode the grown screen"));
            }
            stream.extend(session.finish().expect("finish"));
            assert_eq!(
                stream.iter().filter(|unit| unit.key_frame).count(),
                2,
                "{encoder}: the new stream starts on a key frame"
            );
            let pictures = decoded(&encoder, &stream);
            assert!(
                pictures.iter().all(|p| (p.width(), p.height()) == (WIDTH, HEIGHT)),
                "{encoder}: the size asked for"
            );
            let luma = pictures.last().expect("a picture").luma();
            let mean = luma.iter().map(|&l| f64::from(l)).sum::<f64>() / luma.len() as f64;
            assert!(mean > 150.0, "{encoder}: the last picture is the grown screen's, scaled: mean luma {mean}");
        }
    }

    #[test]
    fn a_session_never_given_a_picture_ends_with_nothing_to_hand_out() {
        for (encoder, _) in candidates() {
            let Ok(session) = Session::open_screen(&encoder, SETTINGS, LAYOUT) else {
                skipped(format!("{encoder} cannot take a screen picture here"));
                continue;
            };
            // A share nobody watched before it stopped: FFmpeg's Vulkan encoders crashed when ended so.
            assert!(session.finish().expect("finish").is_empty(), "{encoder}: nothing was encoded");
        }
    }

    #[test]
    fn a_memory_session_takes_no_new_screen() {
        for (encoder, _) in candidates() {
            let mut session = Session::open(&encoder, SETTINGS).expect("open");
            assert_eq!(session.set_source(LAYOUT, (WIDTH, HEIGHT)), Err(EncodeError::Input));
        }
    }

    mod timing {
        use super::*;

        /// What the call that converts a 4K screen picture and hands it on takes at each size sent, printed and not
        /// asserted: time is measured on a machine, never a verdict of the gate.
        #[test]
        #[ignore = "a measurement, run on request"]
        fn a_4k_screen_picture_s_convert_and_hand_on_is_printed_for_the_record() {
            const WIDTH_4K: usize = 3840;
            const HEIGHT_4K: usize = 2160;
            const PICTURES: usize = 120;
            // Sent as it is and scaled down, in one test so the sizes are timed one after another and never against
            // each other on the same GPU.
            const SIZES: [(usize, usize); 4] = [(WIDTH_4K, HEIGHT_4K), (2560, 1440), (1920, 1080), (1280, 720)];
            // Software's own measurement is its limit's, below.
            for (encoder, node) in candidates().into_iter().filter(|(e, _)| e.path() != Path::Software) {
                let Some(mut surface) = Surface::new(&node, WIDTH_4K, HEIGHT_4K) else {
                    skipped("the VA-API device makes no 4K BGRx surface");
                    return;
                };
                surface.paint(&(0..WIDTH_4K * HEIGHT_4K).map(|i| (i / 7 % 200 + 16) as u8).collect::<Vec<_>>());
                for (width, height) in SIZES {
                    let settings = media::Settings {
                        width,
                        height,
                        frames_per_second: 60,
                        bits_per_second: 20_000_000,
                        key_frame_every: NonZeroU32::new(60),
                    };
                    let opened = Session::open_screen(
                        &encoder,
                        settings,
                        Layout { width: WIDTH_4K as u32, height: HEIGHT_4K as u32, ..LAYOUT },
                    );
                    let mut session = match opened {
                        Ok(session) => session,
                        Err(why) => {
                            skipped(format!(
                                "{encoder} cannot take a 4K screen picture at {width}x{height} here: {why}"
                            ));
                            continue;
                        }
                    };
                    let mut each = Vec::new();
                    let (busy, cpu) = (Busy::of(&node), processor_seconds());
                    for _ in 0..PICTURES {
                        let frame = surface.frame(Pixels::Bgrx);
                        let started = std::time::Instant::now();
                        session.encode_frame(frame).expect("encode");
                        each.push(started.elapsed());
                    }
                    let (cpu, busy) = (processor_seconds() - cpu, busy.map(Busy::mean));
                    each.sort();
                    let (median, slowest) = (each[PICTURES / 2], each[PICTURES - 1]);
                    eprintln!(
                        "{encoder}: 4K sent at {width}x{height}, {PICTURES} pictures, median {median:?}, slowest {slowest:?} for the call that converts and hands on to the encoder; {:.2} ms of processor a picture, the GPU busy {}",
                        cpu / PICTURES as f64 * 1000.0,
                        busy.map_or_else(|| "unread".to_string(), |busy| format!("{busy:.0}% of the time"))
                    );
                }
            }
        }

        /// Each size and rate a software share may be sent at, measured as a share sends it: the screen converted on
        /// the GPU, brought to memory and encoded, every unit out before the clock stops. Eight pictures in turn, a block
        /// moving over fine lines, and the process's processor time beside the wall clock's.
        #[test]
        fn software_encode_at_each_size_and_rate_is_printed_for_the_record() {
            const PICTURES: usize = 240;
            const SIZES: [(usize, usize); 4] = [(3840, 2160), (2560, 1440), (1920, 1080), (1280, 720)];
            let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get) as f64;
            for (encoder, node) in candidates().into_iter().filter(|(e, _)| e.path() == Path::Software) {
                for (width, height) in SIZES {
                    let surfaces: Option<Vec<Surface>> = (0..8)
                        .map(|n| {
                            let mut surface = Surface::new(&node, width, height)?;
                            let block = |x: usize, y: usize| (x + n * width / 8) % width < width / 6 && y < height / 3;
                            let lines = |x: usize, y: usize| if (x / 2 + y) % 9 < 2 { 40 } else { 200 };
                            let gray: Vec<u8> = (0..width * height)
                                .map(|i| if block(i % width, i / width) { 235 } else { lines(i % width, i / width) })
                                .collect();
                            surface.paint(&gray);
                            Some(surface)
                        })
                        .collect();
                    let Some(surfaces) = surfaces else {
                        skipped(format!("the VA-API device makes no {width}x{height} BGRx surface"));
                        continue;
                    };
                    for fps in [60u32, 30] {
                        let settings = media::Settings {
                            width,
                            height,
                            frames_per_second: fps,
                            bits_per_second: 8_000_000,
                            key_frame_every: None,
                        };
                        let layout = Layout { width: width as u32, height: height as u32, ..LAYOUT };
                        let mut session = Session::open_screen(&encoder, settings, layout).expect("open");
                        let (started, cpu) = (std::time::Instant::now(), processor_seconds());
                        for n in 0..PICTURES {
                            session.encode_frame(surfaces[n % 8].frame(Pixels::Bgrx)).expect("encode");
                        }
                        session.finish().expect("finish");
                        let (wall, cpu) = (started.elapsed().as_secs_f64(), processor_seconds() - cpu);
                        let share = cpu / PICTURES as f64 * f64::from(fps) / threads * 100.0;
                        eprintln!(
                            "{encoder}: {width}x{height} at {fps}: {:.1} pictures a second at most, {:.1} ms of processor a picture, {share:.0}% of this machine's {threads} threads at {fps} a second",
                            PICTURES as f64 / wall,
                            cpu / PICTURES as f64 * 1000.0
                        );
                    }
                }
            }
        }

        /// The GPU's own busy figure (`gpu_busy_percent`, amdgpu's), sampled every 5 ms on a thread of its own while it
        /// is held; none on a driver that has none.
        struct Busy {
            stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
            samples: std::thread::JoinHandle<Vec<u32>>,
        }

        impl Busy {
            fn of(node: &str) -> Option<Self> {
                let name = std::path::Path::new(node).file_name()?.to_str()?;
                let file = format!("/sys/class/drm/{name}/device/gpu_busy_percent");
                std::fs::read_to_string(&file).ok()?;
                let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let stopped = stop.clone();
                let samples = std::thread::spawn(move || {
                    let mut samples = Vec::new();
                    while !stopped.load(std::sync::atomic::Ordering::Relaxed) {
                        if let Some(busy) = std::fs::read_to_string(&file).ok().and_then(|s| s.trim().parse().ok()) {
                            samples.push(busy);
                        }
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    samples
                });
                Some(Self { stop, samples })
            }

            fn mean(self) -> f64 {
                self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
                let samples = self.samples.join().expect("the sampler");
                f64::from(samples.iter().sum::<u32>()) / samples.len().max(1) as f64
            }
        }

        /// This process's user and system time, from `/proc/self/stat`, in seconds.
        fn processor_seconds() -> f64 {
            let stat = std::fs::read_to_string("/proc/self/stat").expect("this process's stat");
            let (_, after_name) = stat.rsplit_once(") ").expect("a name");
            let fields: Vec<&str> = after_name.split(' ').collect();
            let ticks: f64 = fields[11].parse::<f64>().expect("utime") + fields[12].parse::<f64>().expect("stime");
            ticks / 100.0
        }
    }
}
