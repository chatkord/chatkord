//! Brings a window forward with the token its desktop handed over, where the windowing system takes one: on
//! Wayland `xdg-activation-v1`, which un-minimises and focuses it. Elsewhere a raise needs no token, so the window's
//! own toolkit does it.

#[cfg(target_os = "linux")]
mod wayland;

use crate::Unavailable;
use crate::activation::Activation;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

/// Asks the compositor to bring `window` forward with `token`. A window not on Wayland, or a compositor without
/// `xdg-activation-v1`, is [`Unavailable`], and the caller raises it as it would without a token.
pub fn raise(window: &(impl HasWindowHandle + HasDisplayHandle), token: &Activation) -> Result<(), Unavailable> {
    #[cfg(target_os = "linux")]
    {
        wayland::raise(window, token)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (window, token);
        Err(Unavailable)
    }
}
