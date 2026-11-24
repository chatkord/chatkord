//! Pictures in memory moved between layouts: a pixel format converted to another, at the same size or scaled.

use super::error::AvError;
use super::format::PixelFormat;
use super::frame::Frame;
use ffmpeg_sys_next as ff;
use std::ffi::c_int;
use std::marker::PhantomData;
use std::ptr;

/// A plane of a picture: where its first row starts and how many bytes a row takes.
pub(crate) type Plane = (*const u8, c_int);

/// A picture's planes in memory to read.
pub struct View<'a> {
    format: PixelFormat,
    size: (c_int, c_int),
    planes: [*const u8; 4],
    strides: [c_int; 4],
    from: PhantomData<&'a [u8]>,
}

/// A picture's planes in memory to write.
pub struct ViewMut<'a> {
    format: PixelFormat,
    size: (c_int, c_int),
    planes: [*mut u8; 4],
    strides: [c_int; 4],
    to: PhantomData<&'a mut [u8]>,
}

fn sides((width, height): (u32, u32)) -> Option<(c_int, c_int)> {
    Some((c_int::try_from(width).ok().filter(|v| *v > 0)?, c_int::try_from(height).ok().filter(|v| *v > 0)?))
}

impl<'a> View<'a> {
    /// The planes of `frame`, a whole picture in memory.
    pub fn of_frame(frame: &'a Frame) -> Option<Self> {
        let (format, size) = (frame.format()?, sides(frame.size())?);
        let mut view = Self { format, size, planes: [ptr::null(); 4], strides: [0; 4], from: PhantomData };
        for index in 0..4 {
            if let Some(plane) = frame.plane(index) {
                view.planes[index] = plane.bytes.as_ptr();
                view.strides[index] = c_int::try_from(plane.stride).ok()?;
            }
        }
        (!view.planes[0].is_null()).then_some(view)
    }

    /// A tightly packed NV12 picture: `luma` is `width * height` bytes and `chroma` half that.
    pub fn nv12(size: (u32, u32), luma: &'a [u8], chroma: &'a [u8]) -> Option<Self> {
        let size = sides(size)?;
        let pixels = usize::try_from(size.0).ok()?.checked_mul(usize::try_from(size.1).ok()?)?;
        (luma.len() == pixels && chroma.len() == pixels / 2).then_some(Self {
            format: PixelFormat::Nv12,
            size,
            planes: [luma.as_ptr(), chroma.as_ptr(), ptr::null(), ptr::null()],
            strides: [size.0, size.0, 0, 0],
            from: PhantomData,
        })
    }

    /// Its top-left `size`, read in place: an even size inside the picture of a format whose planes are at most
    /// halved; none for another.
    pub fn top_left(self, size: (u32, u32)) -> Option<Self> {
        let (width, height) = sides(size)?;
        let even = width % 2 == 0 && height % 2 == 0;
        (even && width <= self.size.0 && height <= self.size.1).then_some(Self { size: (width, height), ..self })
    }

    /// Its format, size, and its first two planes with their strides, for a copy that reads it.
    pub(crate) fn nv12_planes(&self) -> Option<((c_int, c_int), [Plane; 2])> {
        (self.format == PixelFormat::Nv12)
            .then_some((self.size, [(self.planes[0], self.strides[0]), (self.planes[1], self.strides[1])]))
    }
}

impl<'a> ViewMut<'a> {
    /// The planes of `frame`, a whole picture in memory.
    pub fn of_frame(frame: &'a mut Frame) -> Option<Self> {
        let (format, size) = (frame.format()?, sides(frame.size())?);
        let mut view = Self { format, size, planes: [ptr::null_mut(); 4], strides: [0; 4], to: PhantomData };
        for index in 0..4 {
            if let Some(plane) = frame.plane_mut(index) {
                view.planes[index] = plane.bytes.as_mut_ptr();
                view.strides[index] = c_int::try_from(plane.stride).ok()?;
            }
        }
        (!view.planes[0].is_null()).then_some(view)
    }

    /// A tightly packed NV12 picture: `luma` is `width * height` bytes and `chroma` half that.
    pub fn nv12(size: (u32, u32), luma: &'a mut [u8], chroma: &'a mut [u8]) -> Option<Self> {
        let size = sides(size)?;
        let pixels = usize::try_from(size.0).ok()?.checked_mul(usize::try_from(size.1).ok()?)?;
        (luma.len() == pixels && chroma.len() == pixels / 2).then_some(Self {
            format: PixelFormat::Nv12,
            size,
            planes: [luma.as_mut_ptr(), chroma.as_mut_ptr(), ptr::null_mut(), ptr::null_mut()],
            strides: [size.0, size.0, 0, 0],
            to: PhantomData,
        })
    }
}

/// What a conversion keeps of the samples' values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conversion {
    /// FFmpeg's own, which maps a full-range format to limited range and leaves any other's range as it is.
    Colour,
    /// Only the samples' order changes, their full range kept, as a device that maps the range itself takes them.
    Repack,
}

/// An FFmpeg scaler context (a pixel-format converter, scaling where asked), made again only when the size or a format
/// changes, and freed when dropped.
pub struct Scaler {
    raw: *mut ff::SwsContext,
    /// How many converters were made.
    #[cfg(test)]
    pub made: usize,
}

impl Default for Scaler {
    fn default() -> Self {
        Self {
            raw: ptr::null_mut(),
            #[cfg(test)]
            made: 0,
        }
    }
}

impl Scaler {
    /// Converts `from` to `to`, both of one size, as `how` says.
    pub fn convert(&mut self, from: &View<'_>, to: &mut ViewMut<'_>, how: Conversion) -> Result<(), AvError> {
        if from.size != to.size {
            return Err(AvError::Invalid);
        }
        self.scale(from, to, how)
    }

    /// Converts `from` to `to`, as `how` says, scaled to `to`'s size where it is another (bilinear).
    pub fn scale(&mut self, from: &View<'_>, to: &mut ViewMut<'_>, how: Conversion) -> Result<(), AvError> {
        let (width, height) = from.size;
        let (out_width, out_height) = to.size;
        let held = std::mem::replace(&mut self.raw, ptr::null_mut());
        // The same converter comes back while the size and formats are unchanged; another is made (and the old one
        // freed) when they change.
        // SAFETY: `held` is null or the converter this value owned, whose ownership passes to the call.
        let made = unsafe {
            ff::sws_getCachedContext(
                held,
                width,
                height,
                from.format.to_c(),
                out_width,
                out_height,
                to.format.to_c(),
                ff::SwsFlags::SWS_BILINEAR as c_int,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        if made.is_null() {
            return Err(AvError::Invalid);
        }
        self.raw = made;
        if how == Conversion::Repack && made != held {
            // SAFETY: a table of FFmpeg's own, for the converter's default (BT.601); it is only read.
            let table = unsafe { ff::sws_getCoefficients(ff::SWS_CS_ITU601 as c_int) };
            let unit = 1 << 16;
            // SAFETY: the converter was just made; both ranges full leaves the samples' values as they are.
            if unsafe { ff::sws_setColorspaceDetails(made, table, 1, table, 1, 0, unit, unit) } < 0 {
                return Err(AvError::Invalid);
            }
        }
        #[cfg(test)]
        if made != held {
            self.made += 1;
        }
        // SAFETY: the converter was made for this size and these formats, and the views hold planes of that size
        // and format, `to`'s borrowed for writing.
        let rows = unsafe {
            ff::sws_scale(
                made,
                from.planes.as_ptr(),
                from.strides.as_ptr(),
                0,
                height,
                to.planes.as_ptr(),
                to.strides.as_ptr(),
            )
        };
        if rows == out_height { Ok(()) } else { Err(AvError::Invalid) }
    }
}

impl Drop for Scaler {
    fn drop(&mut self) {
        // SAFETY: the pointer came from `sws_getCachedContext` (or is null) and is freed once.
        unsafe { ff::sws_freeContext(self.raw) };
    }
}
