//! What the video devices need beyond FFmpeg's: VA-API and Vulkan handles used directly, a decoded surface as DMA-BUFs,
//! and the DRM format modifiers the GPU imports. FFmpeg itself is reached only through [`crate::av`].

#[cfg(windows)]
pub(crate) mod d3d12;
#[cfg(target_os = "linux")]
mod modifiers;
#[cfg(target_os = "linux")]
pub(crate) mod surface;
#[cfg(target_os = "linux")]
pub(crate) mod vaapi;
#[cfg(target_os = "linux")]
pub mod vulkan_image;

#[cfg(target_os = "linux")]
pub use modifiers::importable;
