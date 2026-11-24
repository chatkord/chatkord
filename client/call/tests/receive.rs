#![expect(clippy::cast_sign_loss, reason = "a test's numbers are its own fixtures, bounded where it writes them")]
#![expect(clippy::print_stderr, reason = "a machine that cannot make the stream says why in the test's output")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test crate: a failed step is the test failing"
)]
//! The pipeline against a real encoder, a real decoder and the slot the window reads: where the pictures end up, and
//! what a stream does before it has a key frame. A machine with no hardware encoder says so and skips.

use call::receive::{Intake, Pipeline, Report};
use media::Codec;
use picture::{Frame, Painting, Slot};
use std::num::NonZeroU32;

use tier::skipped;

/// libwebrtc carries another FFmpeg under the host's names, built without hardware devices, and the linker picks one:
/// if it picks that one, every hardware path here (encode, decode, the window's import) silently reads unavailable.
#[cfg(any(target_os = "linux", windows))]
#[test]
fn the_ffmpeg_beside_the_engine_is_the_hosts_and_can_make_hardware_devices() {
    assert!(media::can_make_hardware_devices(), "the engine's FFmpeg won the link");
}

/// A short H.264 stream of one flat picture from this machine's encoder, or None where none opens.
fn stream() -> Option<Vec<media::Encoded>> {
    let report = media::survey();
    eprintln!("N {} {:?}", report.encoders.len(), report.unavailable.first());
    let encoder = report.encoders.into_iter().find(|e| e.codec() == Codec::H264)?;
    let settings = media::Settings {
        width: 1280,
        height: 720,
        frames_per_second: 30,
        bits_per_second: 4_000_000,
        key_frame_every: NonZeroU32::new(30),
    };
    let mut session = media::Session::open(&encoder, settings).map_err(|e| eprintln!("OPEN {e:?}")).ok()?;
    let picture = media::Nv12::new(1280, 720, vec![128; 1280 * 720 * 3 / 2]).ok()?;
    let mut all = Vec::new();
    for _ in 0..10 {
        all.extend(session.encode(&picture).map_err(|e| eprintln!("ENCODE {e:?}")).ok()?);
    }
    all.extend(session.finish().ok()?);
    Some(all)
}

/// Feed `stream` through a pipeline and wait for all of it to be decoded.
fn run(slot: &Slot, stream: &[media::Encoded]) -> Report {
    let intake = Intake::new();
    let pipeline =
        Pipeline::start(&intake, choices::Codec::H264, &Painting::found(), slot.clone()).expect("a decoder opens");
    for unit in stream {
        intake.feed(&unit.data, unit.pts, unit.key_frame);
        // The queue is short on purpose: a producer faster than the decoder loses frames, which this test is not about.
        while pipeline.report().decoded + pipeline.report().failed < unit.pts as u64 + 1 {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    pipeline.report()
}

mod lab {
    use super::*;

    #[test]
    fn a_watched_screen_opens_the_decoder_of_the_codec_it_arrived_in() {
        let painting = Painting::found();
        for (codec, theirs) in [(choices::Codec::H264, Codec::H264), (choices::Codec::Av1, Codec::Av1)] {
            let pipeline = Pipeline::start(&Intake::new(), codec, &painting, Slot::new()).expect("a decoder opens");
            let alone = media::Decoder::open(theirs, &painting.sources()).expect("media opens one");
            let report = pipeline.report();
            eprintln!("{codec:?} decodes on {:?}", report.path);
            assert_eq!((report.codec, report.path), (codec, alone.path()), "the codec's own decoder, on its path");
        }
    }

    #[test]
    fn a_stream_is_decoded_into_the_slot_on_the_gpu_and_then_as_copies_once_the_window_refuses_surfaces() {
        let Some(stream) = stream() else { return skipped("no hardware encoder opens on this machine") };
        let slot = Slot::new();
        let report = run(&slot, &stream);
        assert_eq!((report.failed, report.dropped), (0, 0));
        assert_eq!(slot.stats().received, report.decoded);
        assert!(report.decoded >= 10);
        let first = slot.take().expect("a picture waits");
        #[cfg(target_os = "linux")]
        let on_gpu = matches!(first, Frame::Surface(_));
        // No decoder's surface is imported elsewhere yet.
        #[cfg(not(target_os = "linux"))]
        let on_gpu = false;
        assert_eq!(on_gpu, report.on_gpu, "the report says where the pictures went");
        drop(first);

        slot.refuse_imports();
        let before = slot.stats().received;
        let report = run(&slot, &stream);
        assert!(!report.on_gpu);
        assert_eq!(slot.stats().received, before + report.decoded);
        assert!(matches!(slot.take(), Some(Frame::InMemory(_))), "pictures in memory, by reference");
    }

    #[test]
    fn a_stream_that_starts_between_key_frames_waits_for_one_and_asks_for_it() {
        let Some(stream) = stream() else { return skipped("no hardware encoder opens on this machine") };
        let intake = Intake::new();
        let pipeline =
            Pipeline::start(&intake, choices::Codec::H264, &Painting::found(), Slot::new()).expect("a decoder opens");
        intake.feed(&stream[1].data, 1, false);
        assert_eq!(pipeline.report().dropped, 1, "a delta frame with no key frame before it is not decodable");
        // The first thing a stream needs is a key frame, and it was asked for when the pipeline started.
        let runtime = tokio::runtime::Builder::new_current_thread().enable_time().build().expect("a runtime");
        runtime.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(1), pipeline.wants_key_frame()).await.expect("asked")
        });
    }
    /// Waits until what `pipeline` reports passes `done`, for at most 10 s, and returns it.
    fn until(pipeline: &Pipeline, done: impl Fn(&Report) -> bool) -> Report {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let report = pipeline.report();
            if done(&report) || std::time::Instant::now() > deadline {
                return report;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    /// A unit no decoder takes, sent as a key frame: a path that refuses [`KEYS`] in a row after its pictures is broken.
    const REFUSED: [u8; 64] = [0xff; 64];
    const KEYS: u64 = 2;

    #[test]
    fn a_decoder_that_refuses_key_frames_after_its_pictures_hands_over_once_and_the_card_names_the_path_in_use() {
        // The last hand-over lands on software H.264, FFmpeg's own, which needs nothing loaded.
        let Some(stream) = stream() else { return skipped("no hardware encoder opens on this machine") };
        let intake = Intake::new();
        let pipeline =
            Pipeline::start(&intake, choices::Codec::H264, &Painting::found(), Slot::new()).expect("a decoder opens");
        let opened = pipeline.report().path;
        if opened == media::Path::Software {
            return skipped("no hardware decoder to hand over from");
        }
        for unit in &stream {
            intake.feed(&unit.data, unit.pts, unit.key_frame);
            until(&pipeline, |report| report.decoded >= unit.pts as u64);
        }
        let shown = until(&pipeline, |report| report.decoded >= stream.len() as u64 - 1);
        assert!(shown.decoded > 0 && shown.failed == 0, "the opened path showed pictures: {shown:?}");

        intake.feed(&REFUSED, 99, true);
        let once = until(&pipeline, |report| report.failed >= 1);
        assert_eq!(
            (once.failed, once.path),
            (1, opened),
            "one refused key frame is a damaged frame, not a broken path"
        );
        assert_eq!(pipeline.handed_over(), None);

        // The window's device may be followed by the system's own, the same backend: each broken one hands over.
        let (mut refused, mut pts) = (1_u64, 100);
        let mut handed = Vec::new();
        let now = loop {
            // The one refused above counts toward the first hand-over.
            let more = if handed.is_empty() { KEYS - 1 } else { KEYS };
            for _ in 0..more {
                intake.feed(&REFUSED, pts, true);
                (refused, pts) = (refused + 1, pts + 1);
                until(&pipeline, |report| report.failed >= refused);
            }
            let now = pipeline.report();
            let Some(to) = pipeline.handed_over() else { panic!("two refused key frames hand over: {now:?}") };
            assert_eq!(pipeline.handed_over(), None, "said once");
            assert_eq!(now.path, to, "the card names the path in use");
            handed.push(to);
            if to != opened || handed.len() > 4 {
                break now;
            }
        };
        eprintln!("opened on {opened:?}, handed over to {handed:?}");
        assert_ne!(now.path, opened, "it moved off the broken backend");
        let twice = now;
        let before = twice.decoded;
        // The key frame alone: the whole stream at once overflows the queue behind a slower path, which drops it.
        let key = &stream[0];
        assert!(key.key_frame, "the stream starts on a key frame");
        intake.feed(&key.data, key.pts + 200, key.key_frame);
        let after = until(&pipeline, |report| report.decoded > before);
        assert!(after.decoded > before, "the next key frame decodes on the new path: {after:?}");
    }

    #[test]
    fn an_h264_screen_broken_on_every_graphics_path_hands_over_to_software_and_decodes_there() {
        let Some(stream) = stream() else { return skipped("no hardware encoder opens on this machine") };
        let intake = Intake::new();
        let pipeline =
            Pipeline::start(&intake, choices::Codec::H264, &Painting::found(), Slot::new()).expect("a decoder opens");
        if pipeline.report().path == media::Path::Software {
            return skipped("no hardware decoder to hand over from");
        }
        for unit in &stream {
            intake.feed(&unit.data, unit.pts, unit.key_frame);
            until(&pipeline, |report| report.decoded >= unit.pts as u64);
        }
        let shown = until(&pipeline, |report| report.decoded >= stream.len() as u64 - 1);
        assert!(shown.decoded > 0 && shown.failed == 0, "the opened path showed pictures: {shown:?}");
        let (mut refused, mut pts, mut handed) = (0_u64, 100, Vec::new());
        // Each graphics path, the window's device and the system's, then software: a few tries are plenty.
        for _ in 0..6 {
            if pipeline.report().path == media::Path::Software {
                break;
            }
            for _ in 0..KEYS {
                intake.feed(&REFUSED, pts, true);
                (refused, pts) = (refused + 1, pts + 1);
                until(&pipeline, |report| report.failed >= refused);
            }
            handed.extend(pipeline.handed_over());
        }
        eprintln!("handed over to {handed:?}");
        assert_eq!(pipeline.report().path, media::Path::Software, "software is the last path, after {handed:?}");
        let before = pipeline.report().decoded;
        let key = &stream[0];
        intake.feed(&key.data, pts + 100, key.key_frame);
        let after = until(&pipeline, |report| report.decoded > before);
        assert!(after.decoded > before, "the next key frame decodes in software: {after:?}");
    }

    /// The units of `stream` that are key frames, to feed as often as a test needs.
    fn keys(stream: &[media::Encoded]) -> Vec<&media::Encoded> {
        stream.iter().filter(|unit| unit.key_frame).collect()
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_stream_whose_graphics_open_is_refused_decodes_in_software_and_moves_no_other_stream() {
        let painting = Painting::found();
        let refused = media::Sources { render_nodes: vec!["/dev/dri/renderD0-none".into()], ..Default::default() };
        let (moved, unmoved) = (
            Pipeline::start_with(&Intake::new(), choices::Codec::H264, refused, Slot::new()).expect("software opens"),
            Pipeline::start(&Intake::new(), choices::Codec::H264, &painting, Slot::new()).expect("a decoder opens"),
        );
        assert_eq!(moved.report().path, media::Path::Software, "the card says Software decoded");
        let alone = media::Decoder::open(Codec::H264, &painting.sources()).expect("media opens one");
        assert_eq!(unmoved.report().path, alone.path(), "another stream is where it would have been");
        moved.hold();
        moved.resume();
        assert_eq!(moved.report().path, media::Path::Software, "and a stream taken again is as it was");
    }

    #[test]
    fn a_graphics_decoder_behind_past_its_time_stays_where_a_software_trial_lags_too_and_no_other_stream_moves() {
        let Some(stream) = stream() else { return skipped("no hardware encoder opens on this machine") };
        let (slow_in, calm_in) = (Intake::new(), Intake::new());
        let open = |intake: &Intake| {
            Pipeline::start(intake, choices::Codec::H264, &Painting::found(), Slot::new()).expect("a decoder opens")
        };
        let (slow, calm) = (open(&slow_in), open(&calm_in));
        let opened = slow.report().path;
        if opened == media::Path::Software {
            return skipped("no graphics decoder to move from");
        }
        let keys = keys(&stream);
        // The slow one's pictures come a microsecond apart on the stream's own clock: no decoder keeps up with that,
        // a software trial beside it neither. The calm one's come a tenth of a second apart. Behind for twice the 5 s
        // a graphics decoder waits before a trial, so one is started, judged and dropped.
        let (mut pts, mut decoded, mut was_behind) = (0_i64, (0_u64, 0_u64), false);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            for (intake, pipeline, step, seen) in
                [(&slow_in, &slow, 1, &mut decoded.0), (&calm_in, &calm, 100_000, &mut decoded.1)]
            {
                let key = keys[usize::try_from(pts).unwrap() % keys.len()];
                intake.feed(&key.data, pts * step, true);
                *seen += 1;
                until(pipeline, |report| report.decoded + report.failed >= *seen);
            }
            was_behind |= slow.report().behind;
            assert_eq!(slow.report().path, opened, "a trial that lags never takes over: {:?}", slow.report());
            pts += 1;
        }
        assert!(was_behind, "the graphics decoder was behind its stream: {:?}", slow.report());
        assert_eq!(slow.handed_over(), None, "nothing moved, so nothing is told");
        assert_eq!(calm.report().path, opened, "the stream that kept up stayed where it was");
        assert_eq!(calm.handed_over(), None);
        let before = slow.report().decoded;
        slow_in.feed(&keys[0].data, pts * 1_000_000, true);
        let after = until(&slow, |report| report.decoded > before);
        assert!(after.decoded > before, "the graphics decoder decodes on: {after:?}");
    }

    #[test]
    fn a_held_stream_frees_its_graphics_decoder_and_decodes_again_from_the_next_key_frame() {
        let Some(stream) = stream() else { return skipped("no hardware encoder opens on this machine") };
        let intake = Intake::new();
        let pipeline =
            Pipeline::start(&intake, choices::Codec::H264, &Painting::found(), Slot::new()).expect("a decoder opens");
        let keys = keys(&stream);
        intake.feed(&keys[0].data, 0, true);
        until(&pipeline, |report| report.decoded >= 1);
        pipeline.hold();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !pipeline.is_freed() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(pipeline.is_freed(), "the decoder is closed while the stream is held");
        let held = pipeline.report().decoded;
        intake.feed(&keys[0].data, 1, true);
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert_eq!(pipeline.report().decoded, held, "nothing decodes while held");
        pipeline.resume();
        intake.feed(&keys[0].data, 2, true);
        let after = until(&pipeline, |report| report.decoded > held);
        assert!(after.decoded > held, "a decoder opened again decodes the key frame: {after:?}");
    }

    /// How many streams the idle test holds: a load for the measurement, not a limit of the product.
    const LOAD: usize = 8;

    /// The most the process's resident memory may grow by from opening [`LOAD`] watched streams and holding every one.
    /// Measured: 19.6 MB on Machine A (Windows) and 65.4 MB on Machine B (Linux), and 206 MB on A with every decoder
    /// left open.
    const IDLE_BOUND_BYTES: u64 = 96 << 20;

    /// The resident memory of this process, as the system reports it.
    #[cfg(target_os = "linux")]
    fn resident() -> u64 {
        let status = std::fs::read_to_string("/proc/self/status").expect("the process's status");
        let kilobytes = status
            .lines()
            .find_map(|line| line.strip_prefix("VmRSS:"))
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|number| number.parse::<u64>().ok())
            .expect("a resident size");
        kilobytes * 1024
    }

    /// The resident memory of this process, as the system reports it.
    #[cfg(windows)]
    fn resident() -> u64 {
        use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
        use windows::Win32::System::Threading::GetCurrentProcess;
        let mut counters = PROCESS_MEMORY_COUNTERS::default();
        let size = u32::try_from(size_of::<PROCESS_MEMORY_COUNTERS>()).unwrap();
        // SAFETY: the call takes nothing and gives this process's pseudo handle, which is always valid.
        let process = unsafe { GetCurrentProcess() };
        // SAFETY: `counters` is live for the call and `size` its size.
        unsafe { GetProcessMemoryInfo(process, &raw mut counters, size) }.expect("the process's memory");
        counters.WorkingSetSize as u64
    }

    #[test]
    fn a_window_whose_watched_streams_are_all_held_has_no_decoder_open_and_grows_by_no_more_than_its_bound() {
        let before = resident();
        let open = |_| {
            let intake = Intake::new();
            let pipeline = Pipeline::start(&intake, choices::Codec::H264, &Painting::found(), Slot::new())
                .expect("a decoder opens");
            (intake, pipeline)
        };
        let streams: Vec<_> = (0..LOAD).map(open).collect();
        for (_, pipeline) in &streams {
            pipeline.hold();
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !streams.iter().all(|(_, pipeline)| pipeline.is_freed()) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(streams.iter().all(|(_, pipeline)| pipeline.is_freed()), "every decoder is closed");
        let grown = resident().saturating_sub(before);
        eprintln!("held {LOAD} streams: resident memory grew by {grown} bytes");
        assert!(grown <= IDLE_BOUND_BYTES, "grew by {grown} bytes");
    }
}
