//! Decoded pictures left on the GPU: a VA-API decode hands out its surfaces as DMA-BUF planes, nothing copied on the
//! way out, each described whole, and a surface given back is the next picture's.

use crate::common;

use common::{FRAMES, HEIGHT, WIDTH, encode_all, skipped};
use media::{Backend, DecodeError, Decoder, Path, Sources, Surface, survey};
use std::collections::{HashSet, VecDeque};
use std::fs::File;
use std::os::unix::fs::MetadataExt;

const NV12: u32 = u32::from_le_bytes(*b"NV12");
const R8: u32 = u32::from_le_bytes(*b"R8  ");
const GR88: u32 = u32::from_le_bytes(*b"GR88");
/// What a window holds: the picture on screen and the few on their way.
const HELD: usize = 4;

fn va_api_only() -> Sources {
    Sources { vulkan: false, ..Sources::system() }
}

fn assert_send<T: Send + Sync>() {}

/// The DMA-BUF's identity: the file the descriptor names, whoever opened it.
fn identity(surface: &Surface) -> (u64, u64) {
    let object = surface.objects().next().expect("a surface has an object");
    let file = File::from(object.fd.try_clone_to_owned().expect("a duplicate descriptor"));
    let meta = file.metadata().expect("a DMA-BUF can be asked about");
    (meta.dev(), meta.ino())
}

mod lab {
    use super::*;

    #[test]
    fn a_surface_is_described_whole_and_leaves_the_decoder_without_a_copy() {
        assert_send::<Surface>();
        let report = survey();
        if report.encoders.is_empty() {
            skipped("no hardware encoder opens on this machine, so there is no stream to decode");
            return;
        }
        let mut ran = false;
        let mut seen = HashSet::new();
        for encoder in report.encoders.iter().filter(|e| seen.insert(e.codec())) {
            let stream = encode_all(encoder);
            let mut decoder = Decoder::open(encoder.codec(), &va_api_only()).expect("a decoder");
            if decoder.path() != Path::Hardware(Backend::VaApi) {
                skipped(format!(
                    "{:?} does not decode on VA-API here, so there is no surface to export",
                    encoder.codec()
                ));
                continue;
            }
            let mut identities = HashSet::new();
            let mut held: VecDeque<Surface> = VecDeque::new();
            let mut pictures = 0;
            let mut layouts = HashSet::new();
            for unit in &stream {
                let surfaces = decoder.decode_surfaces(&unit.data, unit.pts).expect("export");
                for surface in surfaces {
                    assert_eq!((surface.width() as usize, surface.height() as usize), (WIDTH, HEIGHT));
                    let objects: Vec<_> = surface.objects().collect();
                    let fourccs: Vec<u32> = surface.layers().iter().map(|l| l.fourcc).collect();
                    assert!(
                        fourccs == [NV12] || fourccs == [R8, GR88],
                        "NV12 as one layer or as luma and chroma layers, got {fourccs:x?}"
                    );
                    let planes: Vec<_> = surface.layers().iter().flat_map(|l| &l.planes).collect();
                    assert_eq!(planes.len(), 2, "luma and chroma");
                    for plane in &planes {
                        assert!(plane.pitch >= WIDTH as u64, "a row of the picture fits in the pitch");
                        let object = objects.get(plane.object).expect("the plane names an object that exists");
                        assert!(plane.offset < object.size, "the plane starts inside its buffer");
                    }
                    layouts.insert((objects.len(), objects[0].modifier.raw(), fourccs));
                    identities.insert(identity(&surface));
                    pictures += 1;
                    // Hold the most recent few, as a window with a picture on screen and one on its way would.
                    held.push_back(surface);
                    if held.len() > HELD {
                        held.pop_front();
                    }
                }
            }
            for surface in decoder.finish_surfaces().expect("finish") {
                drop(surface);
                pictures += 1;
            }
            eprintln!(
                "{encoder}: {pictures} pictures in {} distinct DMA-BUFs; layout (objects, modifier, layers) {layouts:x?}",
                identities.len()
            );
            assert_eq!(pictures as i64, FRAMES, "every unit gave a surface while {HELD} were held");
            // A copy per picture would be a new buffer each time; the decoder's pool is a handful that come round.
            assert!(
                identities.len() <= pictures / 2,
                "the pictures reuse the decoder's own buffers ({} distinct for {pictures})",
                identities.len()
            );
            ran = true;
        }
        if !ran {
            skipped("no codec this machine can encode also decodes on VA-API");
        }
    }

    #[test]
    fn only_a_va_api_decode_can_hand_out_surfaces() {
        let report = survey();
        let Some(encoder) = report.encoders.first() else {
            skipped("no hardware encoder opens on this machine, so there is no stream to decode");
            return;
        };
        let stream = encode_all(encoder);
        let mut decoder =
            Decoder::open(encoder.codec(), &Sources { render_nodes: vec![], ..Sources::default() }).expect("software");
        assert_eq!(decoder.path(), Path::Software);
        assert_eq!(decoder.decode_surfaces(&stream[0].data, stream[0].pts).err(), Some(DecodeError::Picture));
    }
}
