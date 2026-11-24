//! The D3D11 device every Windows capture draws on and the encoder adopts: one place makes it, and one lock guards its
//! immediate context.

use std::fmt;
use std::str::FromStr;
use windows::Win32::Foundation::{HMODULE, LUID};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_DEBUG, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION,
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread,
};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter, IDXGIDevice, IDXGIFactory4};
use windows::core::Interface;

use crate::validation::{Reported, Validation, d3d11_errors};

/// A graphics adapter, by the locally unique id Windows gives it until it goes or its driver restarts. It is read from
/// the window's device and written in a survey's devices, so the encoder, the capture and the card's ring are made on
/// the adapter that paints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AdapterId(i64);

impl AdapterId {
    /// The adapter `luid` names.
    pub fn of(luid: LUID) -> Self {
        Self(i64::from(luid.HighPart) << 32 | i64::from(luid.LowPart))
    }

    /// The id as Windows states it; none for a value that no adapter's id is.
    fn luid(self) -> Option<LUID> {
        Some(LUID { LowPart: u32::try_from(self.0 & 0xffff_ffff).ok()?, HighPart: i32::try_from(self.0 >> 32).ok()? })
    }
}

impl fmt::Display for AdapterId {
    /// Sixteen hex digits, which [`AdapterId::from_str`] reads back.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0.cast_unsigned())
    }
}

impl FromStr for AdapterId {
    type Err = std::num::ParseIntError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        u64::from_str_radix(text, 16).map(|bits| Self(bits.cast_signed()))
    }
}

/// A hardware D3D11 device on an adapter, multithread-protected: a share session's own, lent to its capture and
/// adopted by its encoder, or one of its own for the picker's stills. A clone is the same device.
#[derive(Clone)]
pub struct Gpu {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    multithread: ID3D11Multithread,
}

// SAFETY: the device is multithread-protected, so its methods may be called from any thread, and every use of its
// immediate context takes `lock`.
unsafe impl Send for Gpu {}
// SAFETY: as above.
unsafe impl Sync for Gpu {}

impl Gpu {
    /// A device of its own, on the default adapter with BGRA and video support, under the debug layer where
    /// `validation` says; none where there is no D3D11 device, or no debug layer to open it under.
    pub fn open(validation: Validation) -> Option<Self> {
        Self::open_on(None, validation)
    }

    /// As [`Gpu::open`], on `adapter` where one is named; none where it is gone, as after a driver's reset gave
    /// it another id.
    pub fn open_on(adapter: Option<AdapterId>, validation: Validation) -> Option<Self> {
        let named: Option<IDXGIAdapter> = match adapter {
            Some(adapter) => {
                // SAFETY: a factory made with no arguments, owned by the returned interface.
                let factory = unsafe { CreateDXGIFactory1::<IDXGIFactory4>() }.ok()?;
                // SAFETY: the factory is live; an id no adapter has is an error.
                Some(unsafe { factory.EnumAdapterByLuid::<IDXGIAdapter>(adapter.luid()?) }.ok()?)
            }
            None => None,
        };
        let driver = if named.is_some() { D3D_DRIVER_TYPE_UNKNOWN } else { D3D_DRIVER_TYPE_HARDWARE };
        let mut flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT;
        if validation == Validation::On {
            flags |= D3D11_CREATE_DEVICE_DEBUG;
        }
        let (mut device, mut context) = (None, None);
        // SAFETY: both out pointers are locals written on success; the adapter, when given, is live (the driver type
        // is then unknown, as the call requires), and no software module is given.
        unsafe {
            D3D11CreateDevice(
                named.as_ref(),
                driver,
                HMODULE::default(),
                flags,
                None,
                D3D11_SDK_VERSION,
                Some(&raw mut device),
                None,
                Some(&raw mut context),
            )
        }
        .ok()?;
        let (device, context) = (device?, context?);
        let multithread: ID3D11Multithread = device.cast().ok()?;
        // It answers whether the flag was set before, which a device just made never is.
        // SAFETY: a plain flag on a live device, set before the device is shared.
        let _ = unsafe { multithread.SetMultithreadProtected(true) };
        Some(Self { device, context, multithread })
    }

    /// The adapter the device is on; none where the device cannot say.
    pub fn adapter(&self) -> Option<AdapterId> {
        let dxgi: IDXGIDevice = self.device.cast().ok()?;
        // SAFETY: a live device; the call gives its adapter.
        let adapter = unsafe { dxgi.GetAdapter() }.ok()?;
        // SAFETY: a live adapter; the call reads its description.
        Some(AdapterId::of(unsafe { adapter.GetDesc() }.ok()?.AdapterLuid))
    }

    /// The D3D11 device, for what draws a picture on it.
    pub fn device(&self) -> &ID3D11Device {
        &self.device
    }

    /// The device's immediate context, to be used only under [`Gpu::lock`].
    pub fn context(&self) -> &ID3D11DeviceContext {
        &self.context
    }

    /// The device's multithread interface: its `Enter` and `Leave` are the lock, so whoever else uses the context
    /// takes this same one.
    pub fn multithread(&self) -> &ID3D11Multithread {
        &self.multithread
    }

    /// Takes the device's lock until the returned value is dropped.
    pub fn lock(&self) -> Locked<'_> {
        // SAFETY: a live interface; `Locked`'s drop leaves once.
        unsafe { self.multithread.Enter() };
        Locked(self)
    }

    /// The errors the debug layer found in what was asked of the device; none where it was not opened under it.
    pub fn reported(&self) -> Vec<Reported> {
        d3d11_errors(&self.device)
    }

    /// Whether the device was removed or reset (a driver update, a GPU hang): nothing drawn on it works again.
    pub fn is_lost(&self) -> bool {
        // SAFETY: a live device; the call reads its state.
        unsafe { self.device.GetDeviceRemovedReason() }.is_err()
    }
}

/// The device's lock, held; the next holder enters when this drops.
pub struct Locked<'a>(&'a Gpu);

impl Drop for Locked<'_> {
    fn drop(&mut self) {
        // SAFETY: the `Enter` of `Gpu::lock`, left once.
        unsafe { self.0.multithread.Leave() };
    }
}
