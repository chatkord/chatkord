//! Windows' screen path: the device's video processor reads the captured BGRA texture and writes NV12 of the
//! stream's size into the encoder's pool, in one pass on the GPU, as BT.709 in limited range.

use crate::av::{Frame, HwDevice, HwFrames};
use crate::encode::{EncodeError, Settings};
use crate::probe::{Backend, nv12_pool};
use capture::{Gpu, Layout, Output, Processor};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;

pub(crate) struct Path {
    gpu: Gpu,
    pool: HwFrames,
    processor: Processor,
}

// SAFETY: the COM objects belong to a device FFmpeg made multithread-protected, and every use of its immediate
// context is between the device's own lock and unlock; `&mut self` keeps one thread on a path at a time.
unsafe impl Send for Path {}

impl Path {
    /// The processor that reads the top-left `read` of BGRA textures laid out as `source` on `device` (FFmpeg's
    /// D3D11VA device, which the capture draws into) and leaves NV12 of the settings' size, from a pool the encoder is
    /// opened on.
    pub(super) fn open(
        device: &HwDevice,
        settings: Settings,
        source: Layout,
        read: (u32, u32),
    ) -> Result<Self, EncodeError> {
        let side = |value: usize| u32::try_from(value).ok().filter(|v| *v > 0).ok_or(EncodeError::Settings);
        let size = (side(settings.width)?, side(settings.height)?);
        let input = (side(source.width as usize)?, side(source.height as usize)?);
        let gpu = device.gpu().ok_or(EncodeError::Device)?;
        let processor = Processor::new(&gpu, input, read, size, Output::Nv12).ok_or(EncodeError::Device)?;
        let pool = nv12_pool(device, Backend::D3d11, size).ok_or(EncodeError::Frames)?;
        Ok(Self { gpu, pool, processor })
    }

    /// The frames the converted pictures are drawn from: the encoder is opened on these.
    pub(super) fn frames(&self) -> Result<HwFrames, EncodeError> {
        self.pool.again().ok_or(EncodeError::Frames)
    }

    /// Converts `texture`, a BGRA texture of the source's size on this device, into a new NV12 frame of the pool,
    /// `pts` stamped. The GPU is only told to: the encoder, on the same context, is queued behind it.
    pub(super) fn blit(&mut self, texture: &ID3D11Texture2D, pts: i64) -> Result<Frame, EncodeError> {
        let mut converted = self.pool.take().map_err(|_| EncodeError::Frames)?;
        let output = converted.d3d11_texture().ok_or(EncodeError::Frames)?;
        if !self.processor.blit(&self.gpu, texture, &output) {
            return Err(EncodeError::Convert);
        }
        converted.set_pts(pts);
        converted.tag_bt709_limited();
        Ok(converted)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use capture::Pixels;
    use std::ffi::c_int;
    use std::num::NonZeroU32;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_BIND_RENDER_TARGET, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    };
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};

    pub(in crate::encode::screen) fn device() -> Option<HwDevice> {
        let validation = if tier::d3d_validated() { capture::Validation::On } else { capture::Validation::Off };
        HwDevice::on_d3d11(&Gpu::open(validation)?)
    }

    /// A BGRA texture of one colour on `device`: the test's stand-in for a captured picture.
    pub(in crate::encode::screen) fn texture(device: &HwDevice, size: (u32, u32), rgb: [u8; 3]) -> ID3D11Texture2D {
        painted(device, size, |_, _| rgb)
    }

    /// A BGRA texture on `device` whose pixel at each column and row is the colour `paint` gives.
    pub(in crate::encode::screen) fn painted(
        device: &HwDevice,
        (width, height): (u32, u32),
        paint: impl Fn(u32, u32) -> [u8; 3],
    ) -> ID3D11Texture2D {
        let pixels: Vec<u8> = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .flat_map(|(x, y)| {
                let [r, g, b] = paint(x, y);
                [b, g, r, 255]
            })
            .collect();
        let described = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            ..Default::default()
        };
        let data =
            D3D11_SUBRESOURCE_DATA { pSysMem: pixels.as_ptr().cast(), SysMemPitch: width * 4, SysMemSlicePitch: 0 };
        let mut made = None;
        // SAFETY: the description and the pixels outlive the call.
        unsafe {
            device.gpu().unwrap().device().CreateTexture2D(
                &raw const described,
                Some(&raw const data),
                Some(&raw mut made),
            )
        }
        .unwrap();
        made.unwrap()
    }

    /// The Y, Cb and Cr at the middle of an NV12 device frame, and its size.
    pub(in crate::encode::screen) fn middle(frame: &Frame) -> ([u8; 3], (c_int, c_int)) {
        let mut memory = Frame::new().unwrap();
        memory.fetch(frame).expect("downloaded");
        let (width, height) = memory.size();
        let half = |side: u32| usize::try_from(side / 2).unwrap() & !1;
        let (x, y) = (half(width), half(height));
        let (luma, chroma) = (memory.plane(0).unwrap(), memory.plane(1).unwrap());
        let at = y / 2 * chroma.stride + x;
        (
            [luma.bytes[y * luma.stride + x], chroma.bytes[at], chroma.bytes[at + 1]],
            (c_int::try_from(width).unwrap(), c_int::try_from(height).unwrap()),
        )
    }

    /// Y, Cb, Cr in limited range for an RGB colour, with the matrix's luma weights for red and blue.
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "limited range is 16..=240")]
    pub(in crate::encode::screen) fn limited([r, g, b]: [u8; 3], (kr, kb): (f64, f64)) -> [u8; 3] {
        let [r, g, b] = [r, g, b].map(|v| f64::from(v) / 255.0);
        let y = kr * r + (1.0 - kr - kb) * g + kb * b;
        let cb = (b - y) / (2.0 * (1.0 - kb));
        let cr = (r - y) / (2.0 * (1.0 - kr));
        [16.0 + 219.0 * y, 128.0 + 224.0 * cb, 128.0 + 224.0 * cr].map(|v| v.round() as u8)
    }

    pub(in crate::encode::screen) fn settings((width, height): (usize, usize)) -> Settings {
        Settings {
            width,
            height,
            frames_per_second: 30,
            bits_per_second: 1_000_000,
            key_frame_every: NonZeroU32::new(60),
        }
    }

    pub(in crate::encode::screen) fn layout((width, height): (u32, u32)) -> Layout {
        Layout { pixels: Pixels::Bgra, width, height }
    }

    mod lab {
        use super::*;

        #[test]
        fn a_bgra_texture_comes_out_as_bt709_limited_nv12() {
            let Some(device) = device() else { return tier::skipped("no D3D11 device") };
            let size = (1280, 720);
            let mut path = Path::open(&device, settings((1280, 720)), layout(size), size).expect("a processor");
            for rgb in [[255, 0, 0], [0, 255, 0], [0, 0, 255], [128, 128, 128], [255, 255, 255], [0, 0, 0]] {
                let (got, _) = middle(&path.blit(&texture(&device, size, rgb), 0).expect("converted"));
                let (bt709, bt601) = (limited(rgb, (0.2126, 0.0722)), limited(rgb, (0.299, 0.114)));
                eprintln!("rgb {rgb:?}: got {got:?}, BT.709 {bt709:?}, BT.601 {bt601:?}");
                assert!(
                    got.iter().zip(bt709).all(|(g, e)| g.abs_diff(e) <= 2),
                    "rgb {rgb:?}: {got:?} is not BT.709 limited {bt709:?} (BT.601 would be {bt601:?})"
                );
            }
        }

        #[test]
        fn a_picture_larger_than_the_stream_is_scaled_to_the_stream() {
            let Some(device) = device() else { return tier::skipped("no D3D11 device") };
            let source = (3840, 2160);
            let mut path = Path::open(&device, settings((1280, 720)), layout(source), source).expect("a processor");
            let (got, size) = middle(&path.blit(&texture(&device, source, [0, 255, 0]), 0).expect("converted"));
            assert_eq!(size, (1280, 720));
            let green = limited([0, 255, 0], (0.2126, 0.0722));
            assert!(got.iter().zip(green).all(|(g, e)| g.abs_diff(e) <= 2), "{got:?} is not {green:?}");
        }

        #[test]
        fn a_texture_of_another_size_is_refused() {
            let Some(device) = device() else { return tier::skipped("no D3D11 device") };
            let mut path =
                Path::open(&device, settings((1280, 720)), layout((1280, 720)), (1280, 720)).expect("a processor");
            assert!(matches!(path.blit(&texture(&device, (1920, 1080), [0, 0, 0]), 0), Err(EncodeError::Convert)));
        }
    }
}
