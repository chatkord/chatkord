//! Encodes a synthetic picture on every encoder this machine opens and on each software encoder, then decodes the
//! stream in software and compares it to the source, so bytes alone prove nothing; what a machine cannot encode is
//! skipped by name.

use crate::common;

use common::{
    FRAMES, HEIGHT, SETTINGS, WIDTH, colour_of, encode_all, first_of_each_codec, level_needed, picture,
    profile_and_level, psnr, skipped,
};
use media::{
    Backend, Changes, Codec, Colours, Decoder, Device, EncodeError, Encoder, Family, FrameError, Matrix, Name, Nv12,
    Path, Range, Rate, Runs, Session, Settings, Shown, Sources, Validation, survey,
};
use std::num::NonZeroU32;
use std::time::{Duration, Instant};

const FPS: usize = SETTINGS.frames_per_second as usize;

/// Each codec's filler unit type: H.264's NAL, AV1's padding OBU.
const H264_FILLER: u8 = 12;
const AV1_PADDING: u8 = 15;
/// AV1's sequence header OBU, which a decoder starts from.
const AV1_SEQUENCE_HEADER: u8 = 1;

#[test]
fn bytes_that_are_not_a_frame_are_refused_where_they_are_made() {
    assert_eq!(Nv12::new(0, 4, vec![]), Err(FrameError::Size));
    assert_eq!(Nv12::new(3, 4, vec![0; 18]), Err(FrameError::Size));
    assert_eq!(Nv12::new(4, 4, vec![0; 23]), Err(FrameError::Length));
    assert_eq!(Nv12::new(usize::MAX - 1, 2, vec![]), Err(FrameError::Length));
    assert!(Nv12::new(4, 4, vec![0; 24]).is_ok());
}

#[test]
fn a_stream_says_the_colours_its_session_was_opened_for_and_a_decoder_reads_them_back() {
    let report = survey();
    let Some(x264) = report.memory_order(Codec::H264).find(|encoder| encoder.name == Name::X264) else {
        return skipped("x264 is not built here");
    };
    let sources = Sources { render_nodes: vec![], ..Sources::default() };
    let camera = Colours { matrix: Matrix::Bt601, range: Range::Limited };
    // The VUI's range flag, then its primaries, transfer and matrix codes: 5 is BT.601's (BT.470 BG).
    for (colours, said) in [(Colours::HD, (false, [1, 1, 1])), (camera, (false, [1, 1, 5]))] {
        let mut session =
            Session::open_in(x264, Settings { key_frame_every: None, ..SETTINGS }, colours).expect("open");
        assert_eq!(session.colours(), colours);
        let mut stream = run(&mut session, 0, 3);
        stream.extend(session.finish().expect("finish"));
        assert_eq!(colour_of(&stream[0].data), Some(said), "{colours:?}: what the stream says");
        let mut decoder = Decoder::open(Codec::H264, &sources).expect("a software decoder");
        let read = stream.iter().flat_map(|unit| decoder.decode_shown(&unit.data, unit.pts).expect("decode")).find_map(
            |shown| match shown {
                Shown::InMemory(picture) => Some(picture.colours()),
                _ => None,
            },
        );
        assert_eq!(read, Some(colours), "what the decoder's picture carries");
    }
}

/// Encodes `frames` grainy pictures from `from`, returning the units: busy enough that a lower rate shows, which noise
/// is not for SVT-AV1, whose coarsest quantizer still writes 6.6 Mbit/s of 720p30 noise.
fn run(session: &mut Session, from: usize, frames: usize) -> Vec<media::Encoded> {
    let colours = session.colours();
    (from..from + frames).flat_map(|n| session.encode(&grainy(n).in_colours(colours)).expect("encode")).collect()
}

#[test]
fn a_picture_in_other_colours_than_the_stream_is_refused() {
    let report = survey();
    let Some(x264) = report.memory_order(Codec::H264).find(|encoder| encoder.name == Name::X264) else {
        return skipped("x264 is not built here");
    };
    let camera = Colours { matrix: Matrix::Bt601, range: Range::Limited };
    let mut session = Session::open_in(x264, SETTINGS, camera).expect("open");
    assert_eq!(session.encode(&grainy(0)), Err(EncodeError::Input), "HD pictures under a BT.601 tag");
    assert!(session.encode(&grainy(0).in_colours(camera)).is_ok());
}

/// The bytes one session writes for `FRAMES` grainy pictures in one GOP opened at `bits_per_second`: the rate test's
/// shape, with its encoders opened fresh instead of changed mid-stream.
fn bytes_at(encoder: &Encoder, bits_per_second: u32) -> usize {
    let mut session =
        Session::open(encoder, Settings { bits_per_second, key_frame_every: None, ..SETTINGS }).expect("open");
    let mut stream = run(&mut session, 0, FRAMES as usize);
    stream.extend(session.finish().expect("finish"));
    stream.iter().map(|unit| unit.data.len()).sum::<usize>()
}

#[test]
fn a_rate_an_encoder_cannot_open_at_is_refused_where_it_is_made() {
    let thirty = NonZeroU32::new(30).unwrap();
    assert_eq!(Rate::new(2_500_000, thirty), Ok(Rate { bits_per_second: 2_500_000, frames_per_second: 30 }));
    for (bits, fps) in [(0, thirty), (u64::from(u32::MAX) + 1, thirty), (1_000_000, NonZeroU32::MAX)] {
        assert_eq!(Rate::new(bits, fps), Err(EncodeError::Settings), "{bits} bit/s at {fps} fps");
    }
}

fn noisy_at(width: usize, height: usize, n: usize) -> Nv12 {
    let mut state = 0x9E37_79B9u32.wrapping_add(n as u32);
    let mut data = vec![128u8; width * height * 3 / 2];
    for byte in &mut data[..width * height] {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *byte = (state >> 24) as u8;
    }
    Nv12::new(width, height, data).expect("noise is a valid frame")
}

fn decode_all(encoder: &media::Encoder, stream: &[media::Encoded]) -> Vec<(usize, usize)> {
    let mut decoder = Decoder::open(encoder.codec(), &Sources { render_nodes: vec![], ..Sources::default() })
        .expect("software decoder");
    let mut sizes = Vec::new();
    for unit in stream {
        sizes.extend(
            decoder
                .decode(&unit.data, unit.pts)
                .expect("decode")
                .iter()
                .map(|d| (d.picture.width(), d.picture.height())),
        );
    }
    sizes.extend(decoder.finish().expect("finish").iter().map(|d| (d.picture.width(), d.picture.height())));
    sizes
}

/// The NAL unit types of an H.264 access unit in Annex B.
fn nal_types(unit: &[u8]) -> Vec<u8> {
    unit.windows(4)
        .enumerate()
        .filter(|(_, w)| w[..3] == [0, 0, 1])
        .filter_map(|(at, _)| unit.get(at + 3).map(|header| header & 0x1f))
        .collect()
}

/// The moving picture under a light grain that differs every frame: busy enough that an encoder left to itself spends
/// more than it was asked to, yet well inside what every encoder can write at the rates below.
fn grainy(n: usize) -> Nv12 {
    let mut state = 0x9E37_79B9u32.wrapping_add(n as u32);
    let mut data = picture(n).luma().to_vec();
    for byte in &mut data {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *byte = (u32::from(*byte) + (state >> 24) % 25).saturating_sub(12).clamp(16, 235) as u8;
    }
    data.resize(WIDTH * HEIGHT * 3 / 2, 128);
    Nv12::new(WIDTH, HEIGHT, data).expect("grain is a valid frame")
}

/// The OBU types of an AV1 temporal unit of low-overhead OBUs: a header byte, an extension byte if flagged, a LEB128
/// size.
fn obu_types(unit: &[u8]) -> Vec<u8> {
    let (mut types, mut at) = (Vec::new(), 0);
    while let Some(header) = unit.get(at) {
        types.push((header >> 3) & 0x0f);
        at += 1 + usize::from((header >> 2) & 1);
        let (mut size, mut shift) = (0usize, 0);
        while let Some(byte) = unit.get(at) {
            size |= usize::from(byte & 0x7f) << shift;
            (at, shift) = (at + 1, shift + 7);
            if byte & 0x80 == 0 {
                break;
            }
        }
        at += size;
    }
    types
}

/// Whether a unit of `codec`'s stream, Annex B or AV1's OBUs, holds filler.
fn carries_filler(codec: Codec, unit: &[u8]) -> bool {
    match codec {
        Codec::H264 => unit.windows(4).any(|w| w[..3] == [0, 0, 1] && w[3] & 0x1f == H264_FILLER),
        Codec::Av1 => obu_types(unit).contains(&AV1_PADDING),
    }
}

/// This machine's encoders, or a recorded skip when none opens.
fn hardware() -> Vec<Encoder> {
    tier::lab();
    let report = survey();
    if report.encoders.is_empty() {
        skipped(format!("no encoder opens on this machine: {:?}", report.unavailable));
    }
    report.encoders
}

/// Every software encoder, taking pictures from memory.
fn software() -> Vec<Encoder> {
    let encoders = media::survey_on(&Sources::default()).encoders;
    assert!(!encoders.is_empty(), "this FFmpeg has its software encoders");
    assert!(encoders.iter().all(|encoder| encoder.path() == Path::Software));
    encoders
}

/// The first of `encoders` of each codec.
fn first_of_each(encoders: &[Encoder]) -> Vec<&Encoder> {
    let mut firsts: Vec<&Encoder> = encoders.iter().collect();
    firsts.dedup_by_key(|encoder| encoder.codec());
    firsts
}

/// Each claim about a session, made of the encoders it is given.
mod checks {
    use super::*;

    pub(super) fn every_encoder_that_opens_makes_a_stream_that_decodes_to_the_picture_it_was_given(
        encoders: &[Encoder],
    ) {
        for encoder in encoders {
            let stream = encode_all(encoder);
            let bytes: usize = stream.iter().map(|u| u.data.len()).sum();
            let kbps = bytes as f64 * 8.0 / (FRAMES as f64 / 30.0) / 1000.0;
            eprintln!("{encoder}: {} units, {bytes} bytes, {kbps:.0} kbit/s against a target of 4000", stream.len());

            assert_eq!(stream.len() as i64, FRAMES, "{encoder}: one unit for each frame, none held back");
            assert!(stream[0].key_frame, "{encoder}: a stream starts on a key frame");
            if encoder.codec() == Codec::H264 {
                // High, which the router offers, at a level that holds the stream.
                let (profile, level) = profile_and_level(&stream[0].data).expect("the first unit carries an SPS");
                assert_eq!(profile, 100, "{encoder}: profile_idc");
                let needed = level_needed(WIDTH, HEIGHT, FPS);
                assert!(level >= needed, "{encoder}: level {level} holds a stream that needs {needed}");
                assert_eq!(
                    colour_of(&stream[0].data),
                    Some((false, [1, 1, 1])),
                    "{encoder}: the stream says BT.709 in limited range"
                );
            }
            assert!(stream.windows(2).all(|w| w[0].pts < w[1].pts), "{encoder}: units leave in order");
            assert!(
                stream.iter().filter(|u| u.key_frame).count() >= 2,
                "{encoder}: a key frame at least every 30 frames"
            );

            let mut decoder = Decoder::open(encoder.codec(), &Sources { render_nodes: vec![], ..Sources::default() })
                .expect("software decoder");
            assert_eq!(decoder.path(), Path::Software);
            let mut scores = Vec::new();
            for unit in &stream {
                for decoded in decoder.decode(&unit.data, unit.pts).expect("decode") {
                    scores.push(psnr(&decoded.picture, scores.len()));
                }
            }
            for decoded in decoder.finish().expect("finish") {
                scores.push(psnr(&decoded.picture, scores.len()));
            }
            assert_eq!(scores.len() as i64, FRAMES, "{encoder}: every unit decodes to a picture");
            let worst = scores.iter().copied().fold(f64::MAX, f64::min);
            eprintln!("{encoder}: decoded, worst luma PSNR {worst:.1} dB");
            assert!(worst > 30.0, "{encoder}: the decoded picture is not the one encoded ({worst:.1} dB)");
        }
    }

    pub(super) fn a_frame_of_another_size_is_refused_not_stretched(encoders: &[Encoder]) {
        let Some(encoder) = encoders.first() else {
            return;
        };
        let mut session = Session::open(encoder, SETTINGS).expect("open");
        let small = Nv12::new(64, 64, vec![0; 64 * 64 * 3 / 2]).expect("frame");
        assert_eq!(session.encode(&small), Err(EncodeError::FrameSize));
    }

    pub(super) fn settings_that_cannot_work_are_refused_before_a_device_is_touched(encoders: &[Encoder]) {
        let Some(encoder) = encoders.first() else {
            return;
        };
        for bad in [
            Settings { width: 0, ..SETTINGS },
            Settings { frames_per_second: 0, ..SETTINGS },
            Settings { bits_per_second: 0, ..SETTINGS },
        ] {
            assert!(matches!(Session::open(encoder, bad), Err(EncodeError::Settings)));
        }
    }

    pub(super) fn a_reopen_names_each_thing_it_changed(encoders: &[Encoder]) {
        for encoder in first_of_each(encoders) {
            let mut session = Session::open(encoder, SETTINGS).expect("open");
            run(&mut session, 0, 2);
            let full = SETTINGS.bits_per_second;
            session.set_rate(Rate { bits_per_second: full / 2, frames_per_second: 15 }).expect("a rate");
            session.set_size(WIDTH / 2, HEIGHT / 2).expect("a size");
            let made = session.reopen().expect("reopen").changes;
            let expected = Changes {
                bits_per_second: Some((full, full / 2)),
                frames_per_second: Some((SETTINGS.frames_per_second, 15)),
                size: Some(((WIDTH, HEIGHT), (WIDTH / 2, HEIGHT / 2))),
                encoder: None,
            };
            assert_eq!(made, expected, "{encoder}: each change from what it was to what it became");
        }
    }

    pub(super) fn a_change_asked_is_made_by_a_reopen_before_the_encode_that_follows(encoders: &[Encoder]) {
        for encoder in first_of_each(encoders) {
            let mut session = Session::open(encoder, SETTINGS).expect("open");
            // A rate taken in place starts no new stream, so no key frame.
            let new_stream = !session.takes_rate_in_place();
            let mut before = run(&mut session, 0, 5);
            let half = Rate { bits_per_second: SETTINGS.bits_per_second / 2, ..session.rate() };
            session.set_rate(half).expect("a rate");
            assert_ne!(session.rate(), half, "{encoder}: asked, not yet made");
            before.extend(session.reopen().expect("reopen").held);
            assert_eq!(session.rate(), half, "{encoder}: made by the reopen");
            // A new stream hands the old one out whole; an encoder that takes the rate in place keeps what it holds.
            if new_stream {
                assert_eq!(before.len(), 5, "{encoder}: the old stream handed out whole");
            }
            let again = session.reopen().expect("again");
            assert!(again.held.is_empty() && again.changes.is_empty(), "{encoder}: nothing left to make");
            let mut after = run(&mut session, 5, 5);
            after.extend(session.finish().expect("finish"));
            let keys: Vec<bool> = before.iter().chain(&after).map(|unit| unit.key_frame).collect();
            let wanted: Vec<bool> = (0..10).map(|unit| unit == 0 || (new_stream && unit == 5)).collect();
            assert_eq!(keys, wanted, "{encoder}: one unit a picture, a key frame only where a stream starts");
        }
    }

    /// Whether `encoder` writes fewer bytes at a fifth of the rate, opened fresh at each: one that writes the same bytes
    /// either way cannot show a rate change, and says so with both figures.
    fn obeys_a_rate(encoder: &Encoder) -> bool {
        let at_fifth: usize = bytes_at(encoder, SETTINGS.bits_per_second / 5);
        let at_running: usize = bytes_at(encoder, SETTINGS.bits_per_second);
        eprintln!(
            "{encoder}: {FRAMES} grainy frames opened fresh wrote {at_fifth} bytes at a fifth, {at_running} at the rate"
        );
        if at_fifth * 5 >= at_running * 4 {
            skipped(format!(
                "{encoder}: {FRAMES} grainy frames wrote {at_fifth} bytes at {} kbit/s against {at_running} at {} kbit/s, so this machine's encoder takes no rate target and a rate change cannot be shown here",
                SETTINGS.bits_per_second / 5_000,
                SETTINGS.bits_per_second / 1_000
            ));
            return false;
        }
        true
    }

    pub(super) fn a_new_rate_alone_is_taken_in_place_with_no_key_frame_where_the_encoder_can(encoders: &[Encoder]) {
        // Every encoder that claims it, not the first of each codec: the claim is per encoder.
        for encoder in encoders {
            let settings = Settings { key_frame_every: None, ..SETTINGS };
            let mut session = Session::open(encoder, settings).expect("open");
            if !session.takes_rate_in_place() || !obeys_a_rate(encoder) {
                continue;
            }
            let mut stream = run(&mut session, 0, 60);
            let before: usize = stream.iter().map(|unit| unit.data.len()).sum();
            let fifth = Rate { bits_per_second: SETTINGS.bits_per_second / 5, ..session.rate() };
            session.set_rate(fifth).expect("a usable rate");
            let changed_at = stream.len();
            stream.extend(run(&mut session, 60, 60));
            assert_eq!(session.rate(), fifth, "{encoder}: taken");
            stream.extend(session.finish().expect("finish"));
            // The first pictures after a change still spend what the encoder's buffer held at the old rate.
            let after: usize = stream[changed_at + 10..].iter().map(|unit| unit.data.len()).sum::<usize>() * 60 / 50;
            eprintln!("{encoder}: 60 frames at the rate wrote {before} bytes, 60 at a fifth taken in place {after}");
            assert_eq!(stream.len(), 120, "{encoder}: one unit for each frame across the change");
            assert_eq!(
                stream.iter().filter(|unit| unit.key_frame).count(),
                1,
                "{encoder}: the stream's first unit is its only key frame"
            );
            assert!(after < before / 2, "{encoder}: a fifth of the rate wrote {after} bytes against {before}");
        }
    }

    pub(super) fn a_new_rate_starts_a_new_stream_on_a_key_frame_and_costs_no_unit(encoders: &[Encoder]) {
        for encoder in first_of_each(encoders) {
            let settings = Settings { key_frame_every: None, ..SETTINGS };
            let mut session = Session::open(encoder, settings).expect("open");
            // One that takes a rate in place opens no new stream: the check above is its.
            // The control: whether this machine's encoder obeys a target at all.
            if session.takes_rate_in_place() || !obeys_a_rate(encoder) {
                continue;
            }
            let mut stream = run(&mut session, 0, 60);
            let before: usize = stream.iter().map(|unit| unit.data.len()).sum::<usize>() * 60 / stream.len().max(1);

            session.set_rate(Rate { bits_per_second: 800_000, frames_per_second: 30 }).expect("a usable rate");
            assert_eq!(
                session.rate().bits_per_second,
                4_000_000,
                "{encoder}: the running encoder keeps its rate until the next frame"
            );
            let changed_at = stream.len();
            stream.extend(run(&mut session, 60, 60));
            assert_eq!(session.rate().bits_per_second, 800_000);
            stream.extend(session.finish().expect("finish"));

            let keys: Vec<usize> =
                stream.iter().enumerate().filter(|(_, unit)| unit.key_frame).map(|(at, _)| at).collect();
            let second = *keys.get(1).unwrap_or(&0);
            let after: usize = stream[second..].iter().map(|unit| unit.data.len()).sum();
            eprintln!(
                "{encoder}: 60 frames at 4000 kbit/s wrote {before} bytes, the next 60 at 800 kbit/s wrote {after}"
            );
            assert_eq!(
                stream.len(),
                120,
                "{encoder}: one unit for each frame across the change, none lost to the old encoder's flush"
            );
            assert_eq!(keys.len(), 2, "{encoder}: the first unit and the one after the change are the only key frames");
            assert!(
                second >= changed_at,
                "{encoder}: the old encoder's last units come before the new stream, in order"
            );
            assert!(stream.windows(2).all(|w| w[0].pts < w[1].pts), "{encoder}: units stay in order across the change");
            assert!(after < before / 2, "{encoder}: a fifth of the rate wrote {after} bytes against {before}");
        }
    }

    pub(super) fn the_rate_it_already_runs_at_changes_nothing(encoders: &[Encoder]) {
        let Some(encoder) = encoders.first() else {
            return;
        };
        let mut session = Session::open(encoder, Settings { key_frame_every: None, ..SETTINGS }).expect("open");
        let mut stream = run(&mut session, 0, 10);
        session.set_rate(session.rate()).expect("a usable rate");
        stream.extend(run(&mut session, 10, 10));
        assert_eq!(
            stream.iter().filter(|unit| unit.key_frame).count(),
            1,
            "{encoder}: only the first unit is a key frame"
        );
    }

    #[cfg(target_os = "linux")]
    pub(super) fn a_reopen_with_nothing_asked_waits_for_no_lock(encoders: &[Encoder]) {
        let Some(encoder) = encoders.first() else {
            return;
        };
        let mut session = Session::open(encoder, SETTINGS).expect("open");
        run(&mut session, 0, 2);
        let (held, release) = (std::sync::mpsc::channel(), std::sync::mpsc::channel::<()>());
        let holder = std::thread::spawn(move || {
            let _alone = media::alone();
            held.0.send(()).expect("the test waits");
            release.1.recv_timeout(Duration::from_secs(2)).ok();
        });
        held.1.recv().expect("the lock is held");
        let began = Instant::now();
        session.reopen().expect("nothing to change");
        let took = began.elapsed();
        release.0.send(()).ok();
        holder.join().expect("the holder ends");
        assert!(took < Duration::from_millis(500), "{encoder}: a reopen that changes nothing waited {took:?}");
    }

    pub(super) fn a_refused_reopen_keeps_the_running_stream_and_its_change_until_withdrawn(encoders: &[Encoder]) {
        for encoder in first_of_each(encoders) {
            let mut session = Session::open(encoder, SETTINGS).expect("open");
            for n in 0..5 {
                session.encode(&noisy_at(WIDTH, HEIGHT, n)).expect("encode");
            }
            // An encoder of the codec on a render node there is none of: refused on every system.
            let name = if encoder.codec() == Codec::H264 { Name::H264Vaapi } else { Name::Av1Vaapi };
            let nowhere = Device {
                backend: Backend::VaApi,
                node: Some("/dev/dri/renderD999".into()),
                vendor: None,
                validation: Validation::Off,
            };
            let missing = Encoder { name, runs: Runs::Gpu(nowhere), multiple: media::Multiple::EVEN };
            session.set_encoder(&missing).expect("same codec");
            assert!(session.reopen().is_err(), "{encoder}: no such encoder opens");
            assert_eq!(session.running().2, encoder, "{encoder}: the running stream stays");
            assert!(session.reopen().is_err(), "{encoder}: the change refused waits, for another encoder to take");
            session.withdraw();
            let withdrawn = session.reopen().expect("nothing to make");
            assert!(
                withdrawn.held.is_empty() && withdrawn.changes.is_empty(),
                "{encoder}: nothing asked once withdrawn"
            );
            for n in 5..10 {
                session.encode(&noisy_at(WIDTH, HEIGHT, n)).expect("the running stream goes on");
            }
        }
    }

    pub(super) fn a_rate_of_zero_is_refused_and_leaves_the_running_one(encoders: &[Encoder]) {
        let Some(encoder) = encoders.first() else {
            return;
        };
        let mut session = Session::open(encoder, SETTINGS).expect("open");
        assert_eq!(session.set_rate(Rate { bits_per_second: 0, frames_per_second: 30 }), Err(EncodeError::Settings));
        assert_eq!(
            session.set_rate(Rate { bits_per_second: 1_000_000, frames_per_second: 0 }),
            Err(EncodeError::Settings)
        );
        assert_eq!(session.rate(), Rate { bits_per_second: 4_000_000, frames_per_second: 30 });
    }

    pub(super) fn a_new_size_starts_a_new_stream_on_a_key_frame_and_the_receiver_follows_it(encoders: &[Encoder]) {
        for encoder in first_of_each(encoders) {
            let settings = Settings { key_frame_every: None, ..SETTINGS };
            let mut session = Session::open(encoder, settings).expect("open");
            let mut stream: Vec<media::Encoded> =
                (0..30).flat_map(|n| session.encode(&noisy_at(WIDTH, HEIGHT, n)).expect("encode")).collect();

            session.set_size(640, 360).expect("a usable size");
            assert_eq!(
                session.encode(&noisy_at(WIDTH, HEIGHT, 30)).map(|_| ()),
                Err(EncodeError::FrameSize),
                "{encoder}: a picture of the old size is refused, never scaled"
            );
            stream.extend((30..60).flat_map(|n| session.encode(&noisy_at(640, 360, n)).expect("encode")));
            assert_eq!((session.running().0, session.running().1), (640, 360));
            stream.extend(session.finish().expect("finish"));

            let keys: Vec<usize> =
                stream.iter().enumerate().filter(|(_, unit)| unit.key_frame).map(|(at, _)| at).collect();
            assert_eq!(stream.len(), 60, "{encoder}: one unit for each frame across the change");
            assert_eq!(keys.len(), 2, "{encoder}: the first unit and the one after the change are the only key frames");
            let sizes = decode_all(encoder, &stream);
            assert_eq!(sizes.len(), 60, "{encoder}: every unit decodes");
            assert!(
                sizes[..30].iter().all(|size| *size == (WIDTH, HEIGHT))
                    && sizes[30..].iter().all(|size| *size == (640, 360)),
                "{encoder}: the receiver's pictures follow the new size"
            );
        }
    }

    pub(super) fn a_size_of_zero_is_refused_and_the_same_size_changes_nothing(encoders: &[Encoder]) {
        let Some(encoder) = encoders.first() else {
            return;
        };
        let mut session = Session::open(encoder, SETTINGS).expect("open");
        assert_eq!(session.set_size(0, 360), Err(EncodeError::Settings));
        assert_eq!(session.set_size(640, 0), Err(EncodeError::Settings));
        session.set_size(WIDTH, HEIGHT).expect("the same size");
        let mut stream: Vec<media::Encoded> =
            (0..10).flat_map(|n| session.encode(&noisy_at(WIDTH, HEIGHT, n)).expect("encode")).collect();
        stream.extend(session.finish().expect("finish"));
        assert_eq!(
            stream.iter().filter(|unit| unit.key_frame).count(),
            1,
            "{encoder}: no new stream for the size it already has"
        );
    }

    pub(super) fn a_key_frame_asked_for_mid_stream_is_one_a_receiver_can_start_from(encoders: &[Encoder]) {
        for encoder in encoders {
            let mut session = Session::open(encoder, SETTINGS).expect("a session");
            for n in 0..20 {
                session.encode(&picture(n)).expect("encode");
            }
            session.request_key_frame();
            let mut after = Vec::new();
            for n in 20..30 {
                after.extend(session.encode(&picture(n)).expect("encode"));
            }
            after.extend(session.finish().expect("finish"));
            let Some(at) = after.iter().position(|unit| unit.key_frame) else {
                panic!("{encoder}: no key frame after one was asked for");
            };
            let unit = &after[at];
            match encoder.codec() {
                Codec::H264 => {
                    let types = nal_types(&unit.data);
                    eprintln!(
                        "{encoder}: unit {at} after the ask, frame {}, is a key frame of NAL types {types:?}",
                        unit.pts
                    );
                    assert!(types.contains(&5), "{encoder}: the key frame is an IDR");
                    assert!(types.contains(&7), "{encoder}: the IDR carries its SPS");
                }
                Codec::Av1 => {
                    let types = obu_types(&unit.data);
                    eprintln!(
                        "{encoder}: unit {at} after the ask, frame {}, is a key frame of OBU types {types:?}",
                        unit.pts
                    );
                    assert!(
                        types.contains(&AV1_SEQUENCE_HEADER),
                        "{encoder}: the key frame carries its sequence header"
                    );
                }
            }
            // A receiver that starts here, with nothing before it.
            let mut decoder = Decoder::open(encoder.codec(), &Sources { render_nodes: vec![], ..Sources::default() })
                .expect("software decoder");
            let mut decoded = decoder.decode(&unit.data, unit.pts).expect("the key frame alone decodes");
            decoded.extend(decoder.finish().expect("finish"));
            assert_eq!(decoded.len(), 1, "{encoder}: the key frame alone is one picture");
            let quality = psnr(&decoded[0].picture, unit.pts as usize);
            assert!(quality > 30.0, "{encoder}: the key frame alone is the picture encoded ({quality:.1} dB)");
        }
    }

    pub(super) fn a_busy_picture_keeps_to_its_rate_over_every_half_second_and_a_key_frame_to_its_budget(
        encoders: &[Encoder],
    ) {
        for encoder in encoders {
            for bits_per_second in [4_000_000u32, 16_000_000] {
                let settings = Settings { bits_per_second, key_frame_every: None, ..SETTINGS };
                let mut session = Session::open(encoder, settings).expect("open");
                let mut units: Vec<(usize, bool)> = Vec::new();
                let keep = |units: &mut Vec<(usize, bool)>, out: Vec<media::Encoded>| {
                    units.extend(out.iter().map(|unit| (unit.data.len(), unit.key_frame)));
                };
                for n in 0..4 * FPS {
                    // A key frame asked in the stream, as a viewer who joins asks.
                    if n == 2 * FPS {
                        session.request_key_frame();
                    }
                    keep(&mut units, session.encode(&grainy(n)).expect("encode"));
                }
                keep(&mut units, session.finish().expect("finish"));
                assert_eq!(units.len(), 4 * FPS, "{encoder}: one unit for each frame");
                let rate = u64::from(bits_per_second);
                let budget = rate * media::KEY_FRAME_BUDGET_MS.unsigned_abs() / 1000;
                let keys: Vec<usize> =
                    units.iter().enumerate().filter(|(_, (_, key))| *key).map(|(at, _)| at).collect();
                for (at, half) in units.windows(FPS / 2).enumerate() {
                    let bits = half.iter().map(|(bytes, _)| *bytes as u64 * 8).sum::<u64>();
                    // A constant rate pays a key frame's budget back over the second after it (the Renoir's sizes swing
                    // there), so a half second within it may carry the budget too.
                    let key = keys.iter().any(|key| (key.saturating_sub(FPS / 2)..key + FPS).contains(&at));
                    let most = rate * 5 / 8 + if key { budget } else { 0 };
                    assert!(
                        bits <= most,
                        "{encoder}: the half second from frame {at} wrote {} kbit against {} (a key frame in it: {key})",
                        bits / 1000,
                        most / 1000
                    );
                }
            }
        }
    }

    /// A screen that goes from still to busy and back, as a share of a game's menu does, at 60 a second: each half second
    /// of it keeps to the rate as a steady busy picture does, which a rate controller that learned the still picture's
    /// cheap frames and is then handed noise breaks.
    pub(super) fn a_screen_that_alternates_still_and_busy_keeps_to_its_rate_over_every_half_second(
        encoders: &[Encoder],
    ) {
        let Some(encoder) = encoders.iter().find(|encoder| encoder.name == Name::Av1Vaapi) else {
            return skipped("no av1_vaapi encoder opens on this machine");
        };
        let rate = 16_000_000u64;
        let settings =
            Settings { bits_per_second: rate as u32, frames_per_second: 60, key_frame_every: None, ..SETTINGS };
        let half = settings.frames_per_second as usize / 2;
        let still = picture(0);
        let mut session = Session::open(encoder, settings).expect("open");
        let mut sizes: Vec<usize> = Vec::new();
        for n in 0..8 * half {
            let shown = if (n / half).is_multiple_of(2) { still.clone() } else { grainy(n) };
            sizes.extend(session.encode(&shown).expect("encode").iter().map(|unit| unit.data.len()));
        }
        sizes.extend(session.finish().expect("finish").iter().map(|unit| unit.data.len()));
        assert_eq!(sizes.len(), 8 * half, "{encoder}: one unit for each frame");
        let budget = rate * media::KEY_FRAME_BUDGET_MS.unsigned_abs() / 1000;
        for (at, window) in sizes.windows(half).enumerate() {
            let bits = window.iter().map(|bytes| *bytes as u64 * 8).sum::<u64>();
            // The stream's one key frame is its first picture, whose budget the rate pays back over the second after.
            let key = at < 2 * half;
            let most = rate * 5 / 8 + if key { budget } else { 0 };
            assert!(
                bits <= most,
                "{encoder}: the half second from frame {at} wrote {} kbit against {} (still and busy alternate every {half} frames)",
                bits / 1000,
                most / 1000
            );
        }
    }

    /// No filler leaves a session, though a constant rate pads a still screen with it. What a still screen costs then
    /// is the driver's: next to nothing on the RX 9060 XT, about 1.2 Mbit/s at 30 a second on the Renoir.
    /// VA-API and Vulkan keep one picture in flight, so theirs come out of the next send, and the session says it
    /// holds a change until then.
    pub(super) fn each_picture_comes_out_of_its_send_or_the_next_and_is_held_as_a_change_until_then(
        encoders: &[Encoder],
    ) {
        for encoder in encoders {
            let behind = i64::from(matches!(encoder.name.family(), Family::Vaapi | Family::Vulkan));
            let mut session = Session::open(encoder, Settings { key_frame_every: None, ..SETTINGS }).expect("open");
            for n in 0..10 {
                let units = session.encode(&picture(n)).expect("encode");
                let out: Vec<i64> = units.iter().map(|unit| unit.pts).collect();
                let due: Vec<i64> = Some(n as i64 - behind).filter(|pts| *pts >= 0).into_iter().collect();
                assert_eq!(out, due, "{encoder}: picture {n}'s send hands out what is due, and only it");
                assert_eq!(session.holds_a_change(), behind > 0, "{encoder}: after picture {n}, holds a change");
            }
            eprintln!("{encoder}: each of 10 pictures out of its send, or the next ({behind} behind)");
        }
    }

    pub(super) fn a_still_screen_sends_no_filler(encoders: &[Encoder]) {
        let still = picture(0);
        for encoder in encoders {
            for bits_per_second in [4_000_000u32, 16_000_000] {
                let settings = Settings { bits_per_second, key_frame_every: None, ..SETTINGS };
                let mut session = Session::open(encoder, settings).expect("open");
                let mut units = Vec::new();
                for _ in 0..2 * FPS {
                    units.extend(session.encode(&still).expect("encode"));
                }
                units.extend(session.finish().expect("finish"));
                let after_the_key_frame = units.iter().skip(1).map(|unit| unit.data.len()).sum::<usize>() * 8;
                eprintln!("{encoder}: {after_the_key_frame} bits over two still seconds at {bits_per_second}");
                assert_eq!(units.len(), 2 * FPS, "{encoder}: one unit for each frame");
                let padded = units.iter().filter(|unit| carries_filler(encoder.codec(), &unit.data)).count();
                assert_eq!(padded, 0, "{encoder}: {padded} units at {bits_per_second} still carry filler");
            }
        }
    }

    /// The level an encoder states is the one its size and rate need, not its driver's default: a 4K60 stream past
    /// its level is one a decoder may refuse.
    pub(super) fn a_4k60_h264_stream_states_a_level_that_holds_it(encoders: &[Encoder]) {
        let (width, height, per_second) = (3840, 2160, 60);
        let settings = Settings {
            width,
            height,
            frames_per_second: per_second as u32,
            bits_per_second: 16_000_000,
            key_frame_every: None,
        };
        for encoder in encoders.iter().filter(|encoder| encoder.codec() == Codec::H264) {
            let mut session = Session::open(encoder, settings).expect("open");
            let mut units = session.encode(&noisy_at(width, height, 0)).expect("encode");
            units.extend(session.finish().expect("finish"));
            let (profile, level) = profile_and_level(&units[0].data).expect("an SPS");
            eprintln!("{encoder}: 4K60 profile_idc {profile}, level_idc {level}");
            assert_eq!(profile, 100, "{encoder}: profile_idc");
            let needed = level_needed(width, height, per_second);
            assert!(level >= needed, "{encoder}: level {level} holds a 4K60 stream, which needs {needed}");
        }
    }

    /// A share sends key frames only when one is asked for, so a still screen settles: a key frame on a timer, held to
    /// the rate, comes out soft and is still being sharpened when the next comes.
    pub(super) fn a_session_with_no_period_makes_a_key_frame_only_at_its_start_and_when_asked(encoders: &[Encoder]) {
        let still = picture(0);
        for encoder in encoders {
            let keys = |every: Option<NonZeroU32>, ask_at: Option<usize>| {
                let mut session =
                    Session::open(encoder, Settings { key_frame_every: every, ..SETTINGS }).expect("open");
                let mut units = Vec::new();
                for n in 0..12 * FPS {
                    if Some(n) == ask_at {
                        session.request_key_frame();
                    }
                    units.extend(session.encode(&still).expect("encode"));
                }
                units.extend(session.finish().expect("finish"));
                units.iter().enumerate().filter(|(_, unit)| unit.key_frame).map(|(n, _)| n).collect::<Vec<_>>()
            };
            assert_eq!(keys(None, None), [0], "{encoder}: none of its own");
            assert_eq!(keys(None, Some(200)), [0, 200], "{encoder}: one when asked");
            assert_eq!(keys(NonZeroU32::new(150), None), [0, 150, 300], "{encoder}: on its period when it has one");
        }
    }

    /// AV1 has no crop in its stream, so what an encoder pads a picture to is what a viewer decodes: a session runs at
    /// the largest of its encoder's multiple inside the size asked, and its stream decodes to that, the right and
    /// bottom edge the picture's own, neither padding nor squeezed in.
    pub(super) fn a_stream_decodes_to_the_size_it_runs_at_whatever_size_is_asked(encoders: &[Encoder]) {
        const KEPT: u8 = 180;
        const LEFT_OUT: u8 = 40;
        let sources = Sources { render_nodes: vec![], ..Sources::default() };
        for encoder in encoders {
            for (width, height) in [(1902, 1102), (1918, 1078), (1366, 768), (1920, 1080)] {
                let sent = encoder.multiple.inside(width as u32, height as u32);
                let sent = (sent.0 as usize, sent.1 as usize);
                let settings = Settings { width, height, key_frame_every: None, ..SETTINGS };
                let mut session = Session::open(encoder, settings).expect("open");
                assert_eq!((session.running().0, session.running().1), sent, "{encoder} asked {width}x{height}");
                let mut data: Vec<u8> = (0..height)
                    .flat_map(|y| (0..width).map(move |x| if x < sent.0 && y < sent.1 { KEPT } else { LEFT_OUT }))
                    .collect();
                data.resize(width * height * 3 / 2, 128);
                let frame = Nv12::new(width, height, data).expect("a frame");
                let mut stream: Vec<_> = (0..3).flat_map(|_| session.encode(&frame).expect("encode")).collect();
                stream.extend(session.finish().expect("finish"));
                let mut decoder = Decoder::open(encoder.codec(), &sources).expect("a software decoder");
                let mut last = None;
                for unit in &stream {
                    last = decoder.decode(&unit.data, unit.pts).expect("decode").pop().or(last);
                }
                let picture = decoder.finish().expect("finish").pop().or(last).expect("a picture").picture;
                let decoded = (picture.width(), picture.height());
                let luma = |x: usize, y: usize| picture.luma()[y * decoded.0 + x];
                let edges = [luma(decoded.0 - 1, decoded.1 / 2), luma(decoded.0 / 2, decoded.1 - 1)];
                eprintln!(
                    "{encoder}: {width}x{height} sent as {sent:?}, decoded {decoded:?}, right and bottom {edges:?}"
                );
                assert_eq!(decoded, sent, "{encoder}: {width}x{height} decodes to the size sent, no padding");
                assert!(
                    edges.iter().all(|edge| edge.abs_diff(KEPT) < 24),
                    "{encoder}: {width}x{height}: the edges {edges:?} are not the picture's ({KEPT})"
                );
            }
        }
    }
}

mod lab {
    use super::*;

    #[test]
    fn a_stream_decodes_to_the_size_it_runs_at_whatever_size_is_asked() {
        checks::a_stream_decodes_to_the_size_it_runs_at_whatever_size_is_asked(&hardware());
    }

    #[test]
    fn every_encoder_that_opens_makes_a_stream_that_decodes_to_the_picture_it_was_given() {
        checks::every_encoder_that_opens_makes_a_stream_that_decodes_to_the_picture_it_was_given(&hardware());
    }

    #[test]
    fn a_key_frame_asked_for_mid_stream_is_one_a_receiver_can_start_from() {
        checks::a_key_frame_asked_for_mid_stream_is_one_a_receiver_can_start_from(&hardware());
    }

    #[test]
    fn a_busy_picture_keeps_to_its_rate_over_every_half_second_and_a_key_frame_to_its_budget() {
        checks::a_busy_picture_keeps_to_its_rate_over_every_half_second_and_a_key_frame_to_its_budget(&hardware());
    }

    #[test]
    fn a_screen_that_alternates_still_and_busy_keeps_to_its_rate_over_every_half_second() {
        checks::a_screen_that_alternates_still_and_busy_keeps_to_its_rate_over_every_half_second(&hardware());
    }

    #[test]
    fn a_still_screen_sends_no_filler() {
        checks::a_still_screen_sends_no_filler(&hardware());
    }

    #[test]
    fn a_4k60_h264_stream_states_a_level_that_holds_it() {
        checks::a_4k60_h264_stream_states_a_level_that_holds_it(&hardware());
    }

    #[test]
    fn each_picture_comes_out_of_its_send_or_the_next_and_is_held_as_a_change_until_then() {
        checks::each_picture_comes_out_of_its_send_or_the_next_and_is_held_as_a_change_until_then(&hardware());
    }

    #[test]
    fn a_session_with_no_period_makes_a_key_frame_only_at_its_start_and_when_asked() {
        checks::a_session_with_no_period_makes_a_key_frame_only_at_its_start_and_when_asked(&hardware());
    }

    #[test]
    fn a_frame_of_another_size_is_refused_not_stretched() {
        checks::a_frame_of_another_size_is_refused_not_stretched(&hardware());
    }

    #[test]
    fn settings_that_cannot_work_are_refused_before_a_device_is_touched() {
        checks::settings_that_cannot_work_are_refused_before_a_device_is_touched(&hardware());
    }

    #[test]
    fn the_rate_it_already_runs_at_changes_nothing() {
        checks::the_rate_it_already_runs_at_changes_nothing(&hardware());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_reopen_with_nothing_asked_waits_for_no_lock() {
        checks::a_reopen_with_nothing_asked_waits_for_no_lock(&hardware());
    }

    #[test]
    fn a_refused_reopen_keeps_the_running_stream_and_its_change_until_withdrawn() {
        checks::a_refused_reopen_keeps_the_running_stream_and_its_change_until_withdrawn(&hardware());
    }

    #[test]
    fn a_rate_of_zero_is_refused_and_leaves_the_running_one() {
        checks::a_rate_of_zero_is_refused_and_leaves_the_running_one(&hardware());
    }

    #[test]
    fn a_size_of_zero_is_refused_and_the_same_size_changes_nothing() {
        checks::a_size_of_zero_is_refused_and_the_same_size_changes_nothing(&hardware());
    }

    #[test]
    fn a_new_rate_starts_a_new_stream_on_a_key_frame_and_costs_no_unit() {
        checks::a_new_rate_starts_a_new_stream_on_a_key_frame_and_costs_no_unit(&hardware());
    }

    #[test]
    fn a_new_rate_alone_is_taken_in_place_with_no_key_frame_where_the_encoder_can() {
        checks::a_new_rate_alone_is_taken_in_place_with_no_key_frame_where_the_encoder_can(&hardware());
    }

    #[test]
    fn a_change_asked_is_made_by_a_reopen_before_the_encode_that_follows() {
        checks::a_change_asked_is_made_by_a_reopen_before_the_encode_that_follows(&hardware());
    }

    #[test]
    fn a_reopen_names_each_thing_it_changed() {
        checks::a_reopen_names_each_thing_it_changed(&hardware());
    }

    #[test]
    fn a_new_size_starts_a_new_stream_on_a_key_frame_and_the_receiver_follows_it() {
        checks::a_new_size_starts_a_new_stream_on_a_key_frame_and_the_receiver_follows_it(&hardware());
    }

    #[test]
    fn another_encoder_of_the_same_codec_takes_over_on_a_key_frame_and_another_codec_is_refused() {
        let report = survey();
        let firsts = first_of_each_codec();
        for first in &firsts {
            let mut session = Session::open(first, SETTINGS).expect("open");
            match firsts.iter().find(|encoder| encoder.codec() != first.codec()) {
                Some(other) => assert_eq!(session.set_encoder(other), Err(EncodeError::OtherCodec)),
                None => skipped("only one codec opens here, so a refused codec is not exercised"),
            }
            let Some(second) =
                report.encoders.iter().find(|encoder| encoder.codec() == first.codec() && *encoder != first)
            else {
                skipped(format!(
                    "only one encoder of {:?} opens here, so its handover is not exercised",
                    first.codec()
                ));
                continue;
            };
            let mut stream: Vec<media::Encoded> =
                (0..30).flat_map(|n| session.encode(&noisy_at(WIDTH, HEIGHT, n)).expect("encode")).collect();
            session.set_encoder(second).expect("same codec");
            stream.extend((30..60).flat_map(|n| session.encode(&noisy_at(WIDTH, HEIGHT, n)).expect("encode")));
            assert_eq!(session.running().2, second);
            stream.extend(session.finish().expect("finish"));
            assert_eq!(stream.len(), 60, "{first} then {second}: one unit for each frame");
            assert_eq!(
                stream.iter().filter(|unit| unit.key_frame).count(),
                2,
                "{first} then {second}: a key frame at the handover"
            );
            assert_eq!(
                decode_all(first, &stream).len(),
                60,
                "{first} then {second}: the receiver decodes across the handover"
            );
            eprintln!("{first} then {second}: 60 units, a key frame at the handover, all decoded");
        }
    }
}

/// The same claims of each software encoder, fed from memory: no device is needed, so they run everywhere.
mod software {
    use super::*;

    #[test]
    fn a_stream_decodes_to_the_size_it_runs_at_whatever_size_is_asked() {
        checks::a_stream_decodes_to_the_size_it_runs_at_whatever_size_is_asked(&software());
    }

    #[test]
    fn every_encoder_that_opens_makes_a_stream_that_decodes_to_the_picture_it_was_given() {
        checks::every_encoder_that_opens_makes_a_stream_that_decodes_to_the_picture_it_was_given(&software());
    }

    #[test]
    fn a_key_frame_asked_for_mid_stream_is_one_a_receiver_can_start_from() {
        checks::a_key_frame_asked_for_mid_stream_is_one_a_receiver_can_start_from(&software());
    }

    #[test]
    fn a_busy_picture_keeps_to_its_rate_over_every_half_second_and_a_key_frame_to_its_budget() {
        checks::a_busy_picture_keeps_to_its_rate_over_every_half_second_and_a_key_frame_to_its_budget(&software());
    }

    #[test]
    fn a_still_screen_sends_no_filler() {
        checks::a_still_screen_sends_no_filler(&software());
    }

    #[test]
    fn a_4k60_h264_stream_states_a_level_that_holds_it() {
        checks::a_4k60_h264_stream_states_a_level_that_holds_it(&software());
    }

    #[test]
    fn each_picture_comes_out_of_its_send_or_the_next_and_is_held_as_a_change_until_then() {
        checks::each_picture_comes_out_of_its_send_or_the_next_and_is_held_as_a_change_until_then(&software());
    }

    #[test]
    fn a_session_with_no_period_makes_a_key_frame_only_at_its_start_and_when_asked() {
        checks::a_session_with_no_period_makes_a_key_frame_only_at_its_start_and_when_asked(&software());
    }

    #[test]
    fn a_frame_of_another_size_is_refused_not_stretched() {
        checks::a_frame_of_another_size_is_refused_not_stretched(&software());
    }

    #[test]
    fn settings_that_cannot_work_are_refused_before_a_device_is_touched() {
        checks::settings_that_cannot_work_are_refused_before_a_device_is_touched(&software());
    }

    #[test]
    fn the_rate_it_already_runs_at_changes_nothing() {
        checks::the_rate_it_already_runs_at_changes_nothing(&software());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_reopen_with_nothing_asked_waits_for_no_lock() {
        checks::a_reopen_with_nothing_asked_waits_for_no_lock(&software());
    }

    #[test]
    fn a_refused_reopen_keeps_the_running_stream_and_its_change_until_withdrawn() {
        checks::a_refused_reopen_keeps_the_running_stream_and_its_change_until_withdrawn(&software());
    }

    #[test]
    fn a_rate_of_zero_is_refused_and_leaves_the_running_one() {
        checks::a_rate_of_zero_is_refused_and_leaves_the_running_one(&software());
    }

    #[test]
    fn a_size_of_zero_is_refused_and_the_same_size_changes_nothing() {
        checks::a_size_of_zero_is_refused_and_the_same_size_changes_nothing(&software());
    }

    #[test]
    fn a_new_rate_starts_a_new_stream_on_a_key_frame_and_costs_no_unit() {
        checks::a_new_rate_starts_a_new_stream_on_a_key_frame_and_costs_no_unit(&software());
    }

    #[test]
    fn a_new_rate_alone_is_taken_in_place_with_no_key_frame_where_the_encoder_can() {
        checks::a_new_rate_alone_is_taken_in_place_with_no_key_frame_where_the_encoder_can(&software());
    }

    #[test]
    fn a_change_asked_is_made_by_a_reopen_before_the_encode_that_follows() {
        checks::a_change_asked_is_made_by_a_reopen_before_the_encode_that_follows(&software());
    }

    #[test]
    fn a_reopen_names_each_thing_it_changed() {
        checks::a_reopen_names_each_thing_it_changed(&software());
    }

    #[test]
    fn a_new_size_starts_a_new_stream_on_a_key_frame_and_the_receiver_follows_it() {
        checks::a_new_size_starts_a_new_stream_on_a_key_frame_and_the_receiver_follows_it(&software());
    }
}
