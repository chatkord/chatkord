//! The primary monitor captured by Windows Graphics Capture into the encoder's own D3D11 device, converted there and
//! encoded, with no person; nothing of the picture is kept or shown, only its size and the stream checked.

use crate::common;

use std::num::NonZeroU32;
use std::time::{Duration, Instant};

use common::skipped;
use media::{Capture, Codec, D3d11, Frame, Gpu, Layout, Native, Pixels, Session, Settings, Sources, survey_on};
use windows::Graphics::Capture::GraphicsCaptureItem;
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::core::{IUnknown, Interface};

fn primary_monitor() -> GraphicsCaptureItem {
    // SAFETY: plain values in; the primary monitor always exists while a desktop does.
    let monitor = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().unwrap();
    // SAFETY: a live monitor handle.
    unsafe { interop.CreateForMonitor(monitor) }.unwrap()
}

/// A blank BGRA texture of `width` by `height` on `gpu`, as a capture fills one.
fn blank(gpu: &Gpu, (width, height): (u32, u32)) -> ID3D11Texture2D {
    let described = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
        ..Default::default()
    };
    let mut texture = None;
    // SAFETY: the description outlives the call, and no data is given.
    unsafe { gpu.device().CreateTexture2D(&raw const described, None, Some(&raw mut texture)) }.expect("a texture");
    texture.expect("made")
}

/// How many references `texture` has, besides the one this asks through.
fn references(texture: &ID3D11Texture2D) -> u32 {
    let unknown: IUnknown = texture.cast().expect("every interface is one");
    // SAFETY: a reference added to a live interface, released at once below.
    unsafe { (unknown.vtable().AddRef)(unknown.as_raw()) };
    // SAFETY: the reference added above; what is left is the count.
    let left = unsafe { (unknown.vtable().Release)(unknown.as_raw()) };
    left - 1
}

mod lab {
    use super::*;

    #[test]
    fn a_session_whose_stream_rests_holds_none_of_the_pictures_it_was_given() {
        let report = survey_on(&Sources { validation: media::Validation::of_this_run(), ..Sources::system() });
        let encoders: Vec<_> = [Codec::H264, Codec::Av1]
            .into_iter()
            .filter_map(|codec| report.screen_order(codec).next().cloned())
            .collect();
        if encoders.is_empty() {
            return skipped("no hardware screen encoder");
        }
        for encoder in encoders {
            rests_holding_nothing(&encoder);
        }
    }

    fn rests_holding_nothing(encoder: &media::Encoder) {
        let source = Layout { pixels: Pixels::Bgra, width: 1280, height: 720 };
        let settings = Settings {
            width: 1280,
            height: 720,
            frames_per_second: 30,
            bits_per_second: 4_000_000,
            key_frame_every: NonZeroU32::new(60),
        };
        let mut session = Session::open_screen(encoder, settings, source).expect("a screen session");
        let gpu = session.gpu().expect("a D3D11 session lends its device");
        let texture = blank(&gpu, (source.width, source.height));
        for _ in 0..10 {
            let frame = Frame::new(source, Native::D3d11(D3d11::new(texture.clone())), Instant::now());
            drop(session.encode_frame(frame).expect("encoded"));
        }
        // Nobody takes the stream: what the encoder finishes is collected, and a newer picture stands for the last.
        let until = Instant::now() + Duration::from_secs(1);
        while references(&texture) > 2 && Instant::now() < until {
            drop(session.finished().expect("collected"));
            std::thread::sleep(Duration::from_millis(5));
        }
        session.superseded();
        assert_eq!(references(&texture), 1, "{encoder}: the session and its encoder let go of every picture given");
    }

    #[test]
    fn the_primary_monitor_is_captured_into_the_encoders_device_and_encoded() {
        let report = survey_on(&Sources { validation: media::Validation::of_this_run(), ..Sources::system() });
        let Some(encoder) = report.screen_order(Codec::H264).next().cloned() else {
            return skipped("no hardware H.264 encoder");
        };
        let item = primary_monitor();
        let size = item.Size().unwrap();
        let source = Layout {
            pixels: Pixels::Bgra,
            width: u32::try_from(size.Width).unwrap(),
            height: u32::try_from(size.Height).unwrap(),
        };
        let settings = Settings {
            width: (source.width as usize).min(1920) & !1,
            height: (source.height as usize * (source.width as usize).min(1920) / source.width as usize) & !1,
            frames_per_second: 30,
            bits_per_second: 4_000_000,
            key_frame_every: NonZeroU32::new(60),
        };
        let mut session = Session::open_screen(&encoder, settings, source).expect("a screen session");
        let gpu = session.gpu().expect("a D3D11 session lends its device");
        let capture = Capture::open(&item, &gpu, true, None).expect("a capture");
        let mut units = Vec::new();
        let mut pictures = 0;
        let (mut ages, mut captured) = (Vec::new(), Vec::new());
        for _ in 0..60 {
            let Some(frame) = capture.next(Duration::from_millis(200)).expect("the capture runs") else {
                // A screen that does not change sends nothing: the last picture is sent again, as a call does.
                units.extend(session.repeat_last().expect("repeated"));
                continue;
            };
            assert_eq!((frame.layout().width, frame.layout().height), (source.width, source.height));
            // Windows' time for a picture is its composition's, near its arrival on either side.
            let (now, at) = (Instant::now(), frame.captured());
            let ahead_ms = at.saturating_duration_since(now).as_secs_f64() * 1000.0
                - now.saturating_duration_since(at).as_secs_f64() * 1000.0;
            assert!(ahead_ms.abs() < 250.0, "Windows' time is {ahead_ms:.1} ms from its arrival");
            ages.push(ahead_ms);
            pictures += 1;
            captured.push(frame.captured());
            let encoded = session.encode_frame(frame).expect("encoded");
            assert!(encoded.iter().all(|unit| captured.contains(&unit.captured)), "a unit carries its picture's time");
            units.extend(encoded);
        }
        units.extend(session.finish().expect("finished"));
        ages.sort_by(f64::total_cmp);
        eprintln!(
            "{encoder}: {pictures} pictures of {}x{} captured, {} units; Windows' time {:?} ms after taking, the median",
            source.width,
            source.height,
            units.len(),
            ages.get(ages.len() / 2)
        );
        assert!(pictures > 0, "the capture delivered a picture");
        assert!(units.first().is_some_and(|unit| unit.key_frame), "the stream starts on a key frame");
        let first = &units.first().unwrap().data;
        assert_eq!(common::colour_of(first), Some((false, [1, 1, 1])), "BT.709 in limited range, said in the stream");
        drop(capture);
        assert_eq!(capture::validation::unknown(gpu.reported()), Vec::<String>::new(), "the D3D11 debug layer");
    }
}
