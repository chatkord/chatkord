//! Decode: the path taken is reported, hardware is preferred, software is the deliberate fallback, and a stream
//! nothing can read is an error, not a panic or a blank picture.

use crate::common;

use common::{FRAMES, encode_all, first_encoder, psnr, skipped};
use media::{Codec, Decoder, Path, Sources, survey};

fn software_only() -> Sources {
    Sources { render_nodes: vec![], ..Sources::default() }
}

/// A D3D12 device on the default adapter, as the window's would be.
#[cfg(windows)]
fn d3d12_device() -> media::D3d12Device {
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Direct3D12::{D3D12CreateDevice, D3D12GetDebugInterface, ID3D12Debug, ID3D12Device};
    if media::Validation::of_this_run() == media::Validation::On {
        let mut debug: Option<ID3D12Debug> = None;
        // SAFETY: the out-pointer is a live `Option`; the layer is on for the devices made after it.
        unsafe { D3D12GetDebugInterface(&raw mut debug) }.expect("the D3D12 debug layer");
        // SAFETY: as above.
        unsafe { debug.expect("made").EnableDebugLayer() };
    }
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a null adapter is the default one; the out-pointer is a live `Option`.
    unsafe { D3D12CreateDevice(None, D3D_FEATURE_LEVEL_11_0, &raw mut device) }.expect("a D3D12 device");
    media::D3d12Device(device.expect("made"))
}

mod lab {
    use super::*;

    #[test]
    fn every_codec_decodes_in_software_to_the_picture_it_was_given_av1_included() {
        let report = survey();
        if report.encoders.is_empty() {
            skipped("no hardware encoder opens on this machine, so there is no stream to decode");
            return;
        }
        for encoder in &report.encoders {
            let stream = encode_all(encoder);
            let mut decoder = Decoder::open(encoder.codec(), &software_only()).expect("software decoder");
            assert_eq!(decoder.path(), Path::Software);
            let mut worst = f64::MAX;
            let mut count = 0usize;
            for unit in &stream {
                for decoded in decoder.decode(&unit.data, unit.pts).expect("decode") {
                    worst = worst.min(psnr(&decoded.picture, count));
                    count += 1;
                }
            }
            for decoded in decoder.finish().expect("finish") {
                worst = worst.min(psnr(&decoded.picture, count));
                count += 1;
            }
            eprintln!("{encoder}: software decode {count} pictures, worst luma PSNR {worst:.1} dB");
            assert_eq!(count as i64, FRAMES, "{encoder}: every unit decodes to a picture");
            assert!(worst > 30.0, "{encoder}: not the picture that was encoded ({worst:.1} dB)");
        }
    }

    /// libdav1d works on two pictures at once, so a picture may come out two units late, never more, however many cores
    /// the machine has.
    #[test]
    fn software_av1_hands_each_picture_back_at_most_two_units_late() {
        let Some(encoder) = survey().encoders.into_iter().find(|encoder| encoder.codec() == Codec::Av1) else {
            skipped("no hardware AV1 encoder opens on this machine, so there is no stream to decode");
            return;
        };
        let stream = encode_all(&encoder);
        let mut decoder = Decoder::open(Codec::Av1, &software_only()).expect("libdav1d");
        let (mut out, mut latest) = (0usize, 0usize);
        for (fed, unit) in stream.iter().enumerate() {
            out += decoder.decode(&unit.data, unit.pts).expect("decode").len();
            latest = latest.max(fed + 1 - out);
        }
        out += decoder.finish().expect("finish").len();
        eprintln!("{encoder}: libdav1d held at most {latest} units back, {out} pictures in all");
        assert_eq!(out as i64, FRAMES, "every unit decodes to a picture");
        assert!(latest <= 2, "a picture came out {latest} units late");
    }

    #[test]
    fn each_codec_decodes_on_each_backend_whatever_path_it_ends_up_on_and_the_path_is_reported() {
        let report = survey();
        if report.encoders.is_empty() {
            skipped("no hardware encoder opens on this machine, so there is no stream to decode");
            return;
        }
        let system = Sources::system();
        let backends = [
            ("VA-API", Sources { render_nodes: system.render_nodes.clone(), ..Sources::default() }),
            ("Vulkan", Sources { render_nodes: vec![], vulkan: true, ..Sources::default() }),
            ("D3D11", Sources { d3d11: true, ..Sources::default() }),
        ];
        for encoder in &report.encoders {
            let stream = encode_all(encoder);
            for (label, sources) in &backends {
                let mut decoder = Decoder::open(encoder.codec(), sources).expect("a decoder");
                let mut count = 0usize;
                let mut worst = f64::MAX;
                let mut latest = 0usize;
                for (fed, unit) in stream.iter().enumerate() {
                    for decoded in decoder.decode(&unit.data, unit.pts).expect("decode") {
                        worst = worst.min(psnr(&decoded.picture, count));
                        count += 1;
                    }
                    latest = latest.max(fed + 1 - count);
                }
                for decoded in decoder.finish().expect("finish") {
                    worst = worst.min(psnr(&decoded.picture, count));
                    count += 1;
                }
                eprintln!(
                    "{encoder} stream, asked for {label}: decoded on {:?} (refused {:?}), {count} pictures, worst {worst:.1} dB, at most {latest} units held back",
                    decoder.path(),
                    decoder.refused()
                );
                assert_eq!(count as i64, FRAMES);
                assert!(worst > 30.0);
                assert!(latest <= 2, "{label}: a picture came out {latest} units late");
            }
        }
    }

    /// A software picture reaches the window as the decoder's own planes, never converted to NV12 first.
    #[cfg(any(target_os = "linux", windows))]
    #[test]
    fn a_software_decode_hands_the_window_its_own_planes() {
        let mut seen = std::collections::HashSet::new();
        for encoder in survey().encoders.into_iter().filter(|e| seen.insert(e.codec())) {
            let stream = encode_all(&encoder);
            let mut decoder = Decoder::open(encoder.codec(), &software_only()).expect("a software decoder");
            let mut planar = 0;
            for unit in &stream {
                for shown in decoder.decode_shown(&unit.data, unit.pts).expect("decode") {
                    let as_decoded =
                        matches!(&shown, media::Shown::InMemory(p) if p.layout() == media::PictureLayout::I420);
                    assert!(as_decoded, "{encoder}: a software picture was converted");
                    planar += 1;
                }
            }
            assert!(planar > 0, "{encoder}: no picture came out");
        }
    }

    /// A device whose driver lists no decoder for the codec is passed over at the open, so the stream's first unit
    /// never waits for a refusal and a replay.
    #[test]
    fn a_decoder_opens_only_where_its_driver_decodes_the_codec_and_stays_there() {
        let report = survey();
        if report.encoders.is_empty() {
            skipped("no hardware encoder opens on this machine, so there is no stream to decode");
            return;
        }
        let system = Sources::system();
        let backends = [
            ("VA-API", Sources { render_nodes: system.render_nodes.clone(), ..Sources::default() }),
            ("Vulkan", Sources { render_nodes: vec![], vulkan: true, ..Sources::default() }),
            ("D3D11", Sources { d3d11: true, ..Sources::default() }),
            #[cfg(windows)]
            ("D3D12", Sources { d3d12: Some(d3d12_device()), ..Sources::default() }),
        ];
        for encoder in &report.encoders {
            let stream = encode_all(encoder);
            for (label, sources) in &backends {
                let started = std::time::Instant::now();
                let mut decoder = Decoder::open(encoder.codec(), sources).expect("a decoder");
                let (opened, passed_over) = (decoder.path(), decoder.refused().to_vec());
                let mut first = None;
                for unit in &stream {
                    let pictures = decoder.decode(&unit.data, unit.pts).expect("decode");
                    if first.is_none() && !pictures.is_empty() {
                        first = Some(started.elapsed());
                    }
                }
                eprintln!(
                    "{encoder} stream, asked for {label}: opened on {opened:?}, passed over {passed_over:?}, first \
                     picture {:.1} ms after the open began",
                    first.unwrap_or_default().as_secs_f64() * 1000.0
                );
                assert_eq!(decoder.path(), opened, "{encoder} on {label}: the path it opened on refused the stream");
                assert_eq!(decoder.refused(), passed_over.as_slice());
            }
        }
    }

    #[test]
    fn a_device_that_does_not_open_is_reported_and_software_takes_over() {
        let Some(encoder) = first_encoder() else {
            return;
        };
        let stream = encode_all(&encoder);
        let sources = Sources { render_nodes: vec!["/dev/dri/renderD999".to_string()], ..Sources::default() };
        let mut decoder = Decoder::open(encoder.codec(), &sources).expect("software takes over");
        assert_eq!(decoder.path(), Path::Software);
        assert_eq!(decoder.refused(), &[Path::Hardware(media::Backend::VaApi)]);
        let pictures = stream.iter().map(|u| decoder.decode(&u.data, u.pts).expect("decode").len()).sum::<usize>();
        assert!(pictures > 0);
    }

    #[test]
    fn bytes_no_decoder_can_read_are_an_error_on_every_path() {
        let Some(encoder) = first_encoder() else {
            return;
        };
        let mut decoder = Decoder::open(encoder.codec(), &Sources::system()).expect("a decoder");
        let noise: Vec<u8> = (0..4096u32).map(|n| (n.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
        let result = decoder
            .decode(&media::Coded::copied(&noise).unwrap(), 0)
            .and_then(|first| if first.is_empty() { decoder.finish() } else { Ok(first) });
        assert!(result.map(|pictures| pictures.is_empty()).unwrap_or(true), "noise must not become a picture");
    }

    /// On Windows every codec this machine encodes is decoded on D3D11 too, not left to software.
    #[cfg(windows)]
    #[test]
    fn each_codec_decodes_on_d3d11_where_the_gpu_decodes_it() {
        let report = survey();
        if report.encoders.is_empty() {
            return skipped("no hardware encoder opens on this machine, so there is no stream to decode");
        }
        for encoder in &report.encoders {
            let stream = encode_all(encoder);
            let mut decoder =
                Decoder::open(encoder.codec(), &Sources { d3d11: true, ..Sources::default() }).expect("a decoder");
            let mut count = 0usize;
            for unit in &stream {
                count += decoder.decode(&unit.data, unit.pts).expect("decode").len();
            }
            count += decoder.finish().expect("finish").len();
            assert_eq!(count as i64, FRAMES, "{encoder}");
            assert_eq!(
                decoder.path(),
                Path::Hardware(media::Backend::D3d11),
                "{encoder}: refused {:?}",
                decoder.refused()
            );
        }
    }

    /// On Windows a path found broken hands over in order, the window's D3D12 device to D3D11 to software, and each
    /// decodes the stream from its key frame.
    #[cfg(windows)]
    #[test]
    fn a_broken_path_on_windows_hands_over_from_d3d12_to_d3d11_to_software_and_each_decodes() {
        let Some(encoder) = survey().encoders.into_iter().find(|encoder| encoder.codec() == Codec::H264) else {
            return skipped("no hardware H.264 encoder opens on this machine, so there is no stream to decode");
        };
        let stream = encode_all(&encoder);
        let sources = Sources { d3d12: Some(d3d12_device()), d3d11: true, ..Sources::default() };
        let mut decoder = Decoder::open(Codec::H264, &sources).expect("a decoder");
        let mut paths = vec![decoder.path()];
        for pass in 0_i64.. {
            let mut shown = 0;
            for unit in &stream {
                shown += decoder.decode(&unit.data, unit.pts + pass * FRAMES).expect("decode").len();
            }
            assert!(shown > 0, "{:?} decoded the stream", decoder.path());
            let Some(next) = decoder.hand_over() else { break };
            paths.push(next);
        }
        let d3d = |backend| Path::Hardware(backend);
        assert_eq!(paths, [d3d(media::Backend::D3d12), d3d(media::Backend::D3d11), Path::Software]);
    }

    /// On Windows every codec this machine encodes is decoded on the window's D3D12 device before D3D11, each picture
    /// left where it was written with its fence, and the same pictures copied out match the ones encoded.
    #[cfg(windows)]
    #[test]
    fn each_codec_decodes_on_the_windows_d3d12_device_and_leaves_its_pictures_there() {
        let report = survey();
        if report.encoders.is_empty() {
            return skipped("no hardware encoder opens on this machine, so there is no stream to decode");
        }
        let device = d3d12_device();
        let sources = Sources { d3d12: Some(device.clone()), d3d11: true, ..Sources::default() };
        for encoder in &report.encoders {
            let stream = encode_all(encoder);
            let mut decoder = Decoder::open(encoder.codec(), &sources).expect("a decoder");
            let mut left = 0usize;
            for unit in &stream {
                for surface in decoder.decode_d3d12(&unit.data, unit.pts).expect("decode") {
                    assert_eq!((surface.width(), surface.height()), (common::WIDTH as u32, common::HEIGHT as u32));
                    assert!(surface.resource().is_some(), "{encoder}: a picture holds the resource written");
                    let (fence, signal) = surface.written().expect("a fence");
                    let started = std::time::Instant::now();
                    // SAFETY: a live fence the surface holds a reference to.
                    while unsafe { fence.GetCompletedValue() } < signal {
                        assert!(
                            started.elapsed() < std::time::Duration::from_secs(2),
                            "{encoder}: the fence never reached {signal}"
                        );
                        std::thread::yield_now();
                    }
                    left += 1;
                }
            }
            left += decoder.finish().expect("finish").len();
            assert_eq!(left as i64, FRAMES, "{encoder}");
            assert_eq!(
                decoder.path(),
                Path::Hardware(media::Backend::D3d12),
                "{encoder}: refused {:?}",
                decoder.refused()
            );

            let mut decoder = Decoder::open(encoder.codec(), &sources).expect("a decoder");
            let mut scores = Vec::new();
            for unit in &stream {
                for decoded in decoder.decode(&unit.data, unit.pts).expect("decode") {
                    scores.push(psnr(&decoded.picture, scores.len()));
                }
            }
            for decoded in decoder.finish().expect("finish") {
                scores.push(psnr(&decoded.picture, scores.len()));
            }
            let worst = scores.iter().copied().fold(f64::MAX, f64::min);
            eprintln!("{encoder}: decoded on D3D12, copied out at a worst luma PSNR of {worst:.1} dB");
            assert_eq!(scores.len() as i64, FRAMES, "{encoder}");
            assert!(worst > 30.0, "{encoder}: {worst:.1} dB");
        }
        assert_eq!(capture::validation::unknown(device.reported()), Vec::<String>::new(), "the D3D12 debug layer");
    }
}
