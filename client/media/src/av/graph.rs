//! A filter graph that runs a picture through a VA-API device's own video processor.

use super::device::HwDevice;
use super::error::{AvError, check};
use super::format::{ColourRange, ColourSpace, PixelFormat};
use super::frame::Frame;
use super::frames::HwFrames;
use ffmpeg_sys_next as ff;
use std::ffi::{CStr, CString, c_int};
use std::ptr;

/// What a graph is fed: surfaces of `frames`, of this size and rate, in this colour.
pub struct GraphSource<'a> {
    /// The frames context the source's surfaces are drawn from.
    pub frames: &'a HwFrames,
    /// Pixels across.
    pub width: c_int,
    /// Pixels down.
    pub height: c_int,
    /// Pictures a second.
    pub frames_per_second: c_int,
    /// The matrix the surfaces are tagged with.
    pub space: ColourSpace,
    /// The range the surfaces are tagged with.
    pub range: ColourRange,
}

/// A configured graph: the source, its `scale_vaapi` steps, the sink. Freed when dropped, with the filters it holds.
pub struct Graph {
    graph: *mut ff::AVFilterGraph,
    source: *mut ff::AVFilterContext,
    sink: *mut ff::AVFilterContext,
}

impl Graph {
    /// A graph on `device` from `source` through one `scale_vaapi` per entry of `steps` (each its options, the last
    /// naming the output's format).
    pub fn scale_vaapi(device: &HwDevice, source: &GraphSource<'_>, steps: &[CString]) -> Result<Self, AvError> {
        // SAFETY: allocation has no preconditions; the graph is owned by the value from here.
        let graph = unsafe { ff::avfilter_graph_alloc() };
        if graph.is_null() {
            return Err(AvError::NoMemory);
        }
        let mut made = Self { graph, source: ptr::null_mut(), sink: ptr::null_mut() };
        // SAFETY: the graph is live.
        let input = unsafe { filter(graph, c"buffer", c"source", None, None) }?;
        // SAFETY: allocation has no preconditions; the parameters are freed below.
        let parameters = unsafe { ff::av_buffersrc_parameters_alloc() };
        if parameters.is_null() {
            return Err(AvError::NoMemory);
        }
        // SAFETY: the parameters were just allocated, and nothing else reaches them while this borrow lives.
        let asked = unsafe { &mut *parameters };
        asked.format = PixelFormat::Vaapi.to_c() as c_int;
        asked.width = source.width;
        asked.height = source.height;
        asked.time_base = ff::AVRational { num: 1, den: source.frames_per_second };
        asked.sample_aspect_ratio = ff::AVRational { num: 1, den: 1 };
        asked.hw_frames_ctx = source.frames.0;
        asked.color_space = source.space.to_c();
        asked.color_range = source.range.to_c();
        // SAFETY: `input` is this graph's buffer source, not yet initialised; it takes its own reference to the
        // frames context.
        let status = unsafe { ff::av_buffersrc_parameters_set(input, parameters) };
        // SAFETY: FFmpeg allocated the parameters above, and they are not used again.
        unsafe { ff::av_free(parameters.cast()) };
        check(status)?;
        // SAFETY: the source's parameters are set, which is all it is initialised with.
        check(unsafe { ff::avfilter_init_str(input, ptr::null()) })?;
        let mut chain = vec![input];
        for (i, step) in steps.iter().enumerate() {
            let name = CString::new(format!("scale{i}")).map_err(|_| AvError::Invalid)?;
            // SAFETY: the graph is live.
            chain.push(unsafe { filter(graph, c"scale_vaapi", &name, Some(device), Some(step.as_c_str())) }?);
        }
        let mut sink = ptr::null_mut();
        // SAFETY: the name is NUL terminated; a null result is handled.
        let sink_filter = unsafe { ff::avfilter_get_by_name(c"buffersink".as_ptr()) };
        if sink_filter.is_null() {
            return Err(AvError::Unsupported);
        }
        // SAFETY: the graph is live and owns the filter it makes; the names are NUL terminated.
        let created = unsafe {
            ff::avfilter_graph_create_filter(
                &raw mut sink,
                sink_filter,
                c"out".as_ptr(),
                ptr::null(),
                ptr::null_mut(),
                graph,
            )
        };
        check(created)?;
        chain.push(sink);
        for pair in chain.windows(2) {
            // SAFETY: each filter is one of this graph's, made above.
            check(unsafe { ff::avfilter_link(pair[0], 0, pair[1], 0) })?;
        }
        // SAFETY: the graph is live and its filters linked.
        check(unsafe { ff::avfilter_graph_config(graph, ptr::null_mut()) })?;
        (made.source, made.sink) = (input, sink);
        Ok(made)
    }

    /// The frames the converted pictures are drawn from: an encoder is opened on these.
    pub fn output_frames(&self) -> Result<HwFrames, AvError> {
        // SAFETY: the sink is configured; it returns a reference it owns.
        let owned = unsafe { ff::av_buffersink_get_hw_frames_ctx(self.sink) };
        if owned.is_null() {
            return Err(AvError::Invalid);
        }
        // SAFETY: `owned` is the sink's live reference, referenced again here for the caller.
        let frames = unsafe { ff::av_buffer_ref(owned) };
        if frames.is_null() { Err(AvError::NoMemory) } else { Ok(HwFrames(frames)) }
    }

    /// Runs `picture`, a surface of the source's frames, through the device, whose references the graph takes, and
    /// returns the converted picture.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "the graph takes the frame's references, and the emptied frame is freed here"
    )]
    pub fn run(&mut self, picture: Frame) -> Result<Frame, AvError> {
        let converted = Frame::new().ok_or(AvError::NoMemory)?;
        // SAFETY: the source belongs to this configured graph and takes the picture's references (flags 0 moves them).
        check(unsafe { ff::av_buffersrc_add_frame_flags(self.source, picture.0, 0) })?;
        // SAFETY: the sink belongs to this configured graph and fills an empty frame.
        check(unsafe { ff::av_buffersink_get_frame(self.sink, converted.0) })?;
        Ok(converted)
    }
}

impl Drop for Graph {
    fn drop(&mut self) {
        // SAFETY: the pointer came from `avfilter_graph_alloc` and is freed once.
        unsafe { ff::avfilter_graph_free(&raw mut self.graph) };
    }
}

/// Makes the filter `name` in `graph`, on `device` if it needs one, initialised with `options`.
///
/// # Safety
/// `graph` is a live filter graph.
unsafe fn filter(
    graph: *mut ff::AVFilterGraph,
    name: &CStr,
    instance: &CStr,
    device: Option<&HwDevice>,
    options: Option<&CStr>,
) -> Result<*mut ff::AVFilterContext, AvError> {
    // SAFETY: the name is NUL terminated; a null result is handled.
    let found = unsafe { ff::avfilter_get_by_name(name.as_ptr()) };
    if found.is_null() {
        return Err(AvError::Unsupported);
    }
    // SAFETY: the caller's promise covers the graph, which owns the filter; the instance name is NUL terminated.
    let made = unsafe { ff::avfilter_graph_alloc_filter(graph, found, instance.as_ptr()) };
    if made.is_null() {
        return Err(AvError::NoMemory);
    }
    if let Some(device) = device {
        // SAFETY: the device is a live context; the new reference is the filter's, which frees it.
        let reference = unsafe { ff::av_buffer_ref(device.0) };
        // SAFETY: the filter was just made and is not initialised; nothing else writes it meanwhile.
        unsafe { (*made).hw_device_ctx = reference };
        if reference.is_null() {
            return Err(AvError::NoMemory);
        }
    }
    // The source is initialised by the caller, once its parameters are set.
    if name != c"buffer" {
        // SAFETY: the filter was just made and not yet initialised; the options are NUL terminated or null.
        check(unsafe { ff::avfilter_init_str(made, options.map_or(ptr::null(), CStr::as_ptr)) })?;
    }
    Ok(made)
}
