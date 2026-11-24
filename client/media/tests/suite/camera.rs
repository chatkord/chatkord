//! The machine's first camera, opened at 720p30, read, made ready for one's own card and encoded in hardware on the
//! route a call takes, with no person; nothing of the picture is kept or shown, only its size, its rate and the
//! stream checked.

#[path = "common/cpu.rs"]
mod cpu;

use std::num::NonZeroU32;
use std::time::Duration;

use capture::camera::{self, Camera, Mode};
use media::camera::{Decoder, Picture, Ready, Route};
use media::{Codec, Colours, Encoder, Session, Settings, survey};
#[cfg(target_os = "linux")]
use media::{Matrix, Range};
use tier::skipped;

const ASKED: Mode = Mode { width: 1280, height: 720, frames_per_second: 30 };
const PICTURES: usize = 30;

/// The camera's next picture, made of what it handed over, and when it came.
fn read(camera: &mut Camera, decoder: &mut Decoder) -> (Picture, std::time::Instant) {
    let (shot, at) = camera.read().expect("a picture");
    let decoded = decoder.decode(&shot).expect("a whole picture");
    camera.recycle(shot);
    (Picture::Decoded(decoded), at)
}

/// How a camera's pictures may go: lifted onto the encoder's device, or made NV12 in memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Way {
    Lifted,
    Memory,
}

/// The route `way` for pictures like `first` to `encoder` and its session, or why that way does not open here.
fn route(way: Way, encoder: &Encoder, settings: Settings, first: &Picture) -> Result<(Route, Session), String> {
    match (way, first) {
        #[cfg(target_os = "linux")]
        (Way::Lifted, Picture::Decoded(decoded)) => {
            Route::lifted(encoder, settings, decoded).map_err(|why| format!("{why:?}"))
        }
        (Way::Lifted, _) => Err("the system hands over NV12".to_string()),
        (Way::Memory, _) => Route::memory_session(encoder, settings)
            .map(|session| (Route::memory(first), session))
            .map_err(|why| format!("{why:?}")),
    }
}

/// Makes `picture` ready, gives one's own card what it would be given, and encodes it; how many units came out.
fn send(route: &mut Route, session: &mut Session, picture: &Picture) -> usize {
    let ready = route.ready(picture).expect("made ready");
    match &ready {
        // The card copies a picture in memory into its slot.
        Ready::Nv12(nv12) => drop(std::hint::black_box(nv12.luma().to_vec())),
        // The card is handed the surface's DMA-BUFs.
        #[cfg(target_os = "linux")]
        Ready::Lifted(lifted) => drop(std::hint::black_box(lifted.surface().expect("the surface exports"))),
    }
    ready.encode(session).expect("encoded").len()
}

/// The settings a camera's `mode` is encoded at.
fn settings(mode: Mode, bits_per_second: u32) -> Settings {
    Settings {
        width: mode.width as usize,
        height: mode.height as usize,
        frames_per_second: mode.frames_per_second,
        bits_per_second,
        key_frame_every: NonZeroU32::new(60),
    }
}

mod lab {
    use super::*;

    /// A camera stepping down on the lifted route is scaled by the device's video processor (`scale_vaapi`'s `w` and
    /// `h`) as it is converted, and the new stream encodes at the new size; no camera is needed, only VA-API.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_lifted_camera_stepping_down_is_scaled_on_the_device_and_encoded_at_the_new_size() {
        let report = survey();
        let vaapi = |encoder: &&Encoder| matches!(&encoder.runs, media::Runs::Gpu(device) if device.backend == media::Backend::VaApi);
        let Some(encoder) = report.memory_order(Codec::H264).find(vaapi) else {
            return skipped("no VA-API H.264 encoder");
        };
        let (width, height) = (640_u32, 360_u32);
        let yuy2 = vec![128_u8; (width * height * 2) as usize];
        let shot = camera::Shot::new(&yuy2, None, camera::Pixel::Yuy2, (width, height), None);
        let decoded = Decoder::default().decode(&shot).expect("a whole picture");
        let mode = Mode { width, height, frames_per_second: 30 };
        let (mut route, mut session) = match Route::lifted(encoder, settings(mode, 1_000_000), &decoded) {
            Ok(opened) => opened,
            Err(why) => return skipped(format!("this device does not lift a camera's pictures: {why:?}")),
        };
        let picture = Picture::Decoded(decoded);
        send(&mut route, &mut session, &picture);
        let told = route.resize(&mut session, (320, 180)).expect("the device scales the pictures");
        assert!(told.is_some_and(|told| told.changes.size == Some(((640, 360), (320, 180)))), "a new stream");
        assert!(route.lifts(), "still lifted: the size is made on the device");
        let units: usize = (0..10).map(|_| send(&mut route, &mut session, &picture)).sum();
        assert_eq!((session.running().0, session.running().1), (320, 180), "the encoder runs the new size");
        assert!(units > 0, "the scaled pictures encode");
    }

    // Reading every picture as it comes is this machine's speed: the test runs with nothing beside it.
    mod timing {
        use super::*;

        #[test]
        fn the_first_camera_gives_pictures_of_its_mode_which_encode_in_hardware_on_each_way() {
            let _turn = tier::turn("camera");
            let Some(device) = camera::list().expect("listed").into_iter().next() else {
                return skipped("no camera plugged in");
            };
            let report = survey();
            let Some(encoder) =
                report.memory_order(Codec::H264).find(|encoder| encoder.path() != media::Path::Software)
            else {
                return skipped("no hardware H.264 encoder");
            };
            for way in [Way::Lifted, Way::Memory] {
                let mut camera = Camera::open(&device.id, ASKED).expect("opened");
                let mut decoder = Decoder::default();
                let mode = camera.mode();
                assert!(mode.width <= ASKED.width && mode.height <= ASKED.height, "never larger than asked: {mode:?}");
                let settings = settings(mode, 2_000_000);
                let (first, at) = read(&mut camera, &mut decoder);
                let (mut route, mut session) = match route(way, encoder, settings, &first) {
                    Ok(opened) => opened,
                    Err(why) => {
                        println!("{way:?}: does not open here: {why}");
                        continue;
                    }
                };
                // The stream says what the route makes: a device's conversion writes BT.709, the processor leaves a
                // camera's BT.601 and says so.
                let made = match way {
                    Way::Lifted => Colours::HD,
                    Way::Memory => media::camera::ToNv12::COLOURS,
                };
                assert_eq!(session.colours(), made, "{way:?}: the stream is tagged as its pictures are");
                #[cfg(target_os = "linux")]
                if way == Way::Memory {
                    assert_eq!(made, Colours { matrix: Matrix::Bt601, range: Range::Limited }, "a camera's, on Linux");
                }
                let (mut last, mut units, from) = (at, send(&mut route, &mut session, &first), camera.delivered());
                for _ in 1..PICTURES {
                    let (picture, captured) = read(&mut camera, &mut decoder);
                    assert_eq!(picture.size(), (settings.width, settings.height));
                    last = captured;
                    units += send(&mut route, &mut session, &picture);
                }
                // The camera sets its own rate, lower in dim light; what is ours is taking each picture it sends.
                let delivered = camera.delivered() - from;
                let period = (last - at) / u32::try_from(PICTURES - 1).unwrap();
                let expected = Duration::from_secs(1) / mode.frames_per_second;
                println!(
                    "{encoder}, {way:?}: {mode:?}, a picture every {period:?} against {expected:?}; read {} of the {delivered} \
                     sent meanwhile, {units} units",
                    PICTURES - 1
                );
                let read = u64::try_from(PICTURES - 1).unwrap();
                assert!(read * 10 >= delivered * 9, "every picture the camera sent is read: {read} of {delivered}");
                assert!(units > 0, "the encoder wrote the stream");
            }
        }
    }

    /// What a camera costs the sender's processor at each size a person picks, at 30 a second, on each way a call
    /// may send it: read and made ready for one's own card and the encoder, and encoded in hardware, every picture.
    /// A measurement whose numbers go in the commit that decides the camera's route.
    #[test]
    #[ignore = "a measurement, run on request"]
    fn what_a_camera_costs_the_processor_at_each_size() {
        let _turn = tier::turn("camera");
        let Some(device) = camera::list().expect("listed").into_iter().next() else {
            return skipped("no camera plugged in");
        };
        let report = survey();
        let Some(encoder) = report.memory_order(Codec::H264).find(|encoder| encoder.path() != media::Path::Software)
        else {
            return skipped("no hardware H.264 encoder");
        };
        const SPELL: Duration = Duration::from_secs(10);
        for asked in [
            Mode { width: 1280, height: 720, frames_per_second: 30 },
            Mode { width: 1920, height: 1080, frames_per_second: 30 },
        ] {
            for way in [Way::Memory, Way::Lifted] {
                let mut camera = Camera::open(&device.id, asked).expect("opened");
                let mut decoder = Decoder::default();
                let mode = camera.mode();
                let (first, _) = read(&mut camera, &mut decoder);
                let (mut route, mut session) = match route(way, encoder, settings(mode, 2_500_000), &first) {
                    Ok(opened) => opened,
                    Err(why) => {
                        println!("{way:?}: does not open here: {why}");
                        continue;
                    }
                };
                // The first pictures settle the camera's exposure and the encoder's first key frame.
                for _ in 0..30 {
                    let (picture, _) = read(&mut camera, &mut decoder);
                    send(&mut route, &mut session, &picture);
                }
                let (began, spent_before, mut pictures) = (std::time::Instant::now(), cpu::cpu_seconds(), 0_u32);
                while began.elapsed() < SPELL {
                    let (picture, _) = read(&mut camera, &mut decoder);
                    send(&mut route, &mut session, &picture);
                    pictures += 1;
                }
                let seconds = began.elapsed().as_secs_f64();
                let spent =
                    cpu::cpu_seconds().zip(spent_before).map(|(after, before)| after - before).expect("CPU time");
                println!(
                    "{encoder}, {way:?}: {}x{} at {} asked, {pictures} pictures in {seconds:.1} s ({:.1} a second): \
                     {:.1} % of a core",
                    mode.width,
                    mode.height,
                    mode.frames_per_second,
                    f64::from(pictures) / seconds,
                    spent / seconds * 100.0
                );
            }
        }
    }
}
