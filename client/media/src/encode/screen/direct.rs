//! Windows' screen as it was captured: the BGRA texture handed to an encoder that converts it itself, at the size it
//! was captured. AMF converts on its own engine, so no pass of ours waits on the 3D engine behind a game's drawing.

use crate::av::{Frame, HwDevice, HwFrames};
use crate::encode::EncodeError;
use crate::tuning::Name;
use capture::{Frame as Captured, Layout, Native};

/// Whether the encoder `name` takes BGRA pictures from a device, as AMF's do.
pub(super) fn takes_bgra(name: Name) -> bool {
    name.found().is_some_and(|codec| codec.takes_bgra())
}

pub(crate) struct Path {
    /// The frames the encoder is opened on: BGRA of the stream's size, of which each captured texture is taken as one.
    frames: HwFrames,
    /// The captured pictures' size.
    source: (u32, u32),
    /// What the encoder reads of each, from its top left: the stream.
    read: (u32, u32),
}

impl Path {
    /// The path for pictures laid out as `source` on `device`, the D3D11VA device the capture draws into, of which the
    /// encoder reads the top-left `read`: FFmpeg tells AMF a frame's own size as the part of its texture to code.
    pub(super) fn open(device: &HwDevice, source: Layout, read: (u32, u32)) -> Result<Self, EncodeError> {
        let source = (source.width, source.height);
        if read.0 == 0 || read.1 == 0 || read.0 > source.0 || read.1 > source.1 {
            return Err(EncodeError::Settings);
        }
        let frames = HwFrames::captured(device, read).ok_or(EncodeError::Frames)?;
        Ok(Self { frames, source, read })
    }

    /// The frames the encoder is opened on.
    pub(super) fn frames(&self) -> Result<HwFrames, EncodeError> {
        self.frames.again().ok_or(EncodeError::Frames)
    }

    /// `captured` as a frame of the encoder's, its own texture, `pts` stamped. The capture has the texture back when
    /// the encoder lets the frame go.
    pub(super) fn wrap(&self, captured: Captured, pts: i64) -> Result<Frame, EncodeError> {
        let layout = captured.layout();
        if (layout.width, layout.height) != self.source {
            return Err(EncodeError::Convert);
        }
        let Native::D3d11(texture) = captured.native();
        let texture = texture.texture().clone();
        let mut frame = Frame::d3d11(&self.frames, &texture, captured, self.read).map_err(|_| EncodeError::Frames)?;
        frame.set_pts(pts);
        frame.tag_bt709_limited();
        Ok(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::super::d3d11::tests::{device, layout, limited, painted, settings, texture};
    use super::*;
    use crate::decode::Decoder;
    use crate::encode::{Feed, Session};
    use crate::probe::{Sources, survey};
    use capture::D3d11;
    use std::ffi::c_void;
    use std::time::Instant;
    use windows::core::Interface;

    fn captured(device: &HwDevice, size: (u32, u32), rgb: [u8; 3]) -> (Captured, *mut c_void) {
        let texture = texture(device, size, rgb);
        let raw = texture.as_raw();
        (Captured::new(layout(size), Native::D3d11(D3d11::new(texture)), Instant::now()), raw)
    }

    mod lab {
        use super::*;

        #[test]
        fn the_encoder_is_given_the_captured_texture_itself_not_a_copy() {
            let Some(device) = device() else { return tier::skipped("no D3D11 device") };
            let size = (1280, 720);
            let path = Path::open(&device, layout(size), size).expect("a path");
            let (frame, raw) = captured(&device, size, [255, 0, 0]);
            let wrapped = path.wrap(frame, 7).expect("wrapped");
            let made = wrapped.d3d11_texture().expect("a D3D11 frame");
            assert_eq!(made.as_raw(), raw, "the frame is the captured texture");
            assert_eq!(wrapped.size(), (1280, 720));
            assert_eq!(wrapped.pts(), 7);
        }

        /// A share at a window's own size is sent at the largest of its encoder's multiple inside, every encoder's
        /// convert reading the window's top left one pixel to one: a viewer decodes that size, its right and bottom
        /// edge the window's own, neither the encoder's padding nor the left-out edge squeezed in.
        #[test]
        fn a_window_shared_at_its_own_size_is_cropped_to_its_encoder_s_multiple_not_padded() {
            if device().is_none() {
                return tier::skipped("no D3D11 device");
            }
            const KEPT: [u8; 3] = [200, 200, 200];
            const LEFT_OUT: [u8; 3] = [20, 20, 20];
            let window = (1902, 1102);
            for encoder in survey().encoders {
                let stream = encoder.multiple.inside(window.0, window.1);
                let settings = settings((window.0 as usize, window.1 as usize));
                let mut session = Session::open_screen(&encoder, settings, layout(window)).expect("a session");
                assert_eq!(session.running().0, stream.0 as usize, "{encoder}");
                let device = session.parts.feed.device().and_then(HwDevice::again).expect("the session's device");
                let paint = |x, y| if x < stream.0 && y < stream.1 { KEPT } else { LEFT_OUT };
                let mut units = Vec::new();
                for _ in 0..3 {
                    let frame = Captured::new(
                        layout(window),
                        Native::D3d11(D3d11::new(painted(&device, window, paint))),
                        Instant::now(),
                    );
                    units.extend(session.encode_frame(frame).expect("encoded"));
                }
                units.extend(session.finish().expect("finished"));
                let software = Sources { render_nodes: Vec::new(), ..Sources::default() };
                let mut decoder = Decoder::open(encoder.codec(), &software).expect("a decoder");
                let mut last = None;
                for unit in &units {
                    last = decoder.decode(&unit.data, unit.pts).expect("decoded").pop().or(last);
                }
                let picture = last.expect("a picture").picture;
                let decoded = (picture.width(), picture.height());
                let luma = |x: usize, y: usize| picture.luma()[y * decoded.0 + x];
                let edges = [luma(decoded.0 - 1, decoded.1 / 2), luma(decoded.0 / 2, decoded.1 - 1)];
                let kept = limited(KEPT, (0.2126, 0.0722))[0];
                eprintln!("{encoder}: {window:?} sent as {stream:?}, decoded {decoded:?}, right and bottom {edges:?}");
                assert_eq!(decoded, (stream.0 as usize, stream.1 as usize), "{encoder}: no padding");
                assert!(edges.iter().all(|edge| edge.abs_diff(kept) < 24), "{encoder}: edges {edges:?}, not {kept}");
            }
        }

        /// A share at its source's size on an encoder that takes BGRA goes the direct way, and its colours arrive as
        /// BT.709 in limited range.
        #[test]
        fn a_source_size_share_reaches_a_bgra_encoder_as_captured_and_keeps_its_colours() {
            if device().is_none() {
                return tier::skipped("no D3D11 device");
            }
            let encoders: Vec<_> = survey().encoders.into_iter().filter(|e| takes_bgra(e.name)).collect();
            if encoders.is_empty() {
                return tier::skipped("no encoder here takes BGRA");
            }
            let size = (1280, 720);
            for encoder in &encoders {
                for rgb in [[255, 0, 0], [0, 255, 0], [0, 0, 255], [200, 100, 50]] {
                    let mut session =
                        Session::open_screen(encoder, settings((1280, 720)), layout(size)).expect("a session");
                    assert!(
                        matches!(&session.parts.feed, Feed::Screen { path: super::super::super::Path::Direct(_), .. }),
                        "{encoder}: not the direct way"
                    );
                    let device = session.parts.feed.device().and_then(HwDevice::again).expect("the session's device");
                    let mut units = Vec::new();
                    for _ in 0..5 {
                        units.extend(session.encode_frame(captured(&device, size, rgb).0).expect("encoded"));
                    }
                    units.extend(session.finish().expect("finished"));
                    let software = Sources { render_nodes: Vec::new(), ..Sources::default() };
                    let mut decoder = Decoder::open(encoder.codec(), &software).expect("a decoder");
                    let mut last = None;
                    for unit in &units {
                        last = decoder.decode(&unit.data, unit.pts).expect("decoded").pop().or(last);
                    }
                    let picture = last.expect("a picture").picture;
                    let (x, y) = (640, 360);
                    let got = [
                        picture.luma()[y * 1280 + x],
                        picture.chroma()[(y / 2) * 1280 + x],
                        picture.chroma()[(y / 2) * 1280 + x + 1],
                    ];
                    let want = limited(rgb, (0.2126, 0.0722));
                    eprintln!("{encoder}: rgb {rgb:?} -> Y/Cb/Cr {got:?}, BT.709 limited {want:?}");
                    assert!(
                        got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 3),
                        "{encoder}: rgb {rgb:?} decoded as {got:?}, not {want:?}"
                    );
                }
            }
        }
    }
}
