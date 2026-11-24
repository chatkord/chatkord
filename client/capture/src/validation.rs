//! A device opened under its API's debug layer, which checks every call made on it and keeps what it finds in the
//! device's info queue: a test's choice, never a person's.

/// Whether a hardware device is opened under its API's debug layer (D3D11's, through `D3D11_CREATE_DEVICE_DEBUG`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Validation {
    /// As a person's machine runs it.
    #[default]
    Off,
    /// Under the debug layer, which Windows' Graphics Tools install.
    On,
}

/// An error or corruption the debug layer reported: its message id, which names its kind, and its description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reported {
    /// The API's id of this kind of message (`D3D11_MESSAGE_ID`, `D3D12_MESSAGE_ID`).
    pub id: i32,
    /// What the layer said.
    pub description: String,
}

/// The errors and corruptions an info queue (D3D11's or D3D12's, which differ only by type) kept.
#[cfg(windows)]
macro_rules! errors {
    ($queue:expr, $message:ty, $corruption:expr, $error:expr) => {{
        let queue = $queue;
        // SAFETY: a live info queue; counting reads nothing else.
        let stored = unsafe { queue.GetNumStoredMessages() };
        (0..stored)
            .filter_map(|index| {
                let mut length = 0;
                // SAFETY: asked with no buffer, the queue writes the message's length alone.
                unsafe { queue.GetMessage(index, None, &raw mut length) }.ok()?;
                let mut buffer = vec![0u64; length.div_ceil(8)];
                let message = buffer.as_mut_ptr().cast::<$message>();
                // SAFETY: the buffer holds `length` bytes, aligned for the message, which the queue fills.
                unsafe { queue.GetMessage(index, Some(message), &raw mut length) }.ok()?;
                // SAFETY: filled above; the description points into the same buffer, NUL-terminated.
                let message = unsafe { &*message };
                [$corruption, $error].contains(&message.Severity).then(|| Reported {
                    id: message.ID.0,
                    // SAFETY: as above.
                    description: unsafe { std::ffi::CStr::from_ptr(message.pDescription.cast()) }
                        .to_string_lossy()
                        .into_owned(),
                })
            })
            .collect()
    }};
}

/// The errors the debug layer of `device` kept; none where it was not opened under it.
#[cfg(windows)]
pub fn d3d11_errors(device: &windows::Win32::Graphics::Direct3D11::ID3D11Device) -> Vec<Reported> {
    use windows::Win32::Graphics::Direct3D11 as d3d11;
    use windows::core::Interface;
    let Ok(queue) = device.cast::<d3d11::ID3D11InfoQueue>() else { return Vec::new() };
    errors!(&queue, d3d11::D3D11_MESSAGE, d3d11::D3D11_MESSAGE_SEVERITY_CORRUPTION, d3d11::D3D11_MESSAGE_SEVERITY_ERROR)
}

/// The same for a D3D12 device.
#[cfg(windows)]
pub fn d3d12_errors(device: &windows::Win32::Graphics::Direct3D12::ID3D12Device) -> Vec<Reported> {
    use windows::Win32::Graphics::Direct3D12 as d3d12;
    use windows::core::Interface;
    let Ok(queue) = device.cast::<d3d12::ID3D12InfoQueue>() else { return Vec::new() };
    errors!(&queue, d3d12::D3D12_MESSAGE, d3d12::D3D12_MESSAGE_SEVERITY_CORRUPTION, d3d12::D3D12_MESSAGE_SEVERITY_ERROR)
}

impl Validation {
    /// Under the debug layer where this test run asks for it (`tier::d3d_validated`).
    #[cfg(all(windows, feature = "testing"))]
    pub fn of_this_run() -> Self {
        if tier::d3d_validated() { Self::On } else { Self::Off }
    }
}

/// What the debug layer reported beyond the known issues this test run's setup names, each as it was said.
#[cfg(all(windows, feature = "testing"))]
pub fn unknown(reported: Vec<Reported>) -> Vec<String> {
    let known = tier::d3d_known();
    reported.into_iter().filter(|r| !known.contains(&r.id)).map(|r| format!("{}: {}", r.id, r.description)).collect()
}
