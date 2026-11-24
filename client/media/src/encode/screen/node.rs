//! FFmpeg's Vulkan device on the GPU of a render node, found by the node itself: the Vulkan GPU whose
//! `VK_EXT_physical_device_drm` render node is that device number, opened by its `deviceUUID`, which FFmpeg matches
//! exactly. Deriving one from a VA-API device would match by PCI id, vendor or index where the node is not known.

use std::os::unix::fs::{FileTypeExt, MetadataExt};

use ash::vk;

use crate::av::HwDevice;
use crate::encode::EncodeError;
use crate::probe::Backend;
use capture::Validation;

/// A Vulkan device on `node`'s GPU, under the debug layer as `validation` says; refused (`Screen`) where `node` is
/// no render node or no Vulkan GPU here says it is that node's.
pub(crate) fn vulkan_on(node: &str, validation: Validation) -> Result<HwDevice, EncodeError> {
    let uuid = uuid_of(node)?;
    HwDevice::open(Backend::Vulkan, Some(&uuid), validation).ok_or(EncodeError::Screen)
}

/// The device number of `node`, refused unless it is a character device.
fn number_of(node: &str) -> Result<(u32, u32), EncodeError> {
    let found = std::fs::metadata(node).map_err(|_| EncodeError::Screen)?;
    if !found.file_type().is_char_device() {
        return Err(EncodeError::Screen);
    }
    Ok((rustix::fs::major(found.rdev()), rustix::fs::minor(found.rdev())))
}

/// The `deviceUUID` of the Vulkan GPU whose render node is `node`, as FFmpeg reads a device string: 32 hex digits in
/// the groups 8-4-4-4-12.
fn uuid_of(node: &str) -> Result<String, EncodeError> {
    let number = number_of(node)?;
    let _alone = crate::loader::alone();
    // SAFETY: loading the system's Vulkan loader runs its initialisers, which is what asking for Vulkan means; a
    // failure is a machine without one.
    let entry = unsafe { ash::Entry::load() }.map_err(|_| EncodeError::Screen)?;
    let app = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);
    // SAFETY: the create info and what it points at outlive the call.
    let instance = unsafe { entry.create_instance(&vk::InstanceCreateInfo::default().application_info(&app), None) }
        .map_err(|_| EncodeError::Screen)?;
    let found = rendering(&instance, number);
    // SAFETY: the instance is this function's, and nothing made from it outlives this call.
    unsafe { instance.destroy_instance(None) };
    found.map(|uuid| written(&uuid)).ok_or(EncodeError::Screen)
}

/// The `deviceUUID` of the first GPU of `instance` whose render node is `number`.
fn rendering(instance: &ash::Instance, (major, minor): (u32, u32)) -> Option<[u8; vk::UUID_SIZE]> {
    // SAFETY: the instance is live for the whole function.
    let gpus = unsafe { instance.enumerate_physical_devices() }.ok()?;
    gpus.into_iter().find_map(|gpu| {
        // SAFETY: `gpu` came from this instance.
        let extensions = unsafe { instance.enumerate_device_extension_properties(gpu) }.ok()?;
        let says = extensions
            .iter()
            .any(|e| e.extension_name_as_c_str().is_ok_and(|name| name == ash::ext::physical_device_drm::NAME));
        if !says {
            return None;
        }
        let mut drm = vk::PhysicalDeviceDrmPropertiesEXT::default();
        let mut id = vk::PhysicalDeviceIDProperties::default();
        let mut properties = vk::PhysicalDeviceProperties2::default().push_next(&mut drm).push_next(&mut id);
        // SAFETY: `gpu` came from this instance, which is 1.1, and lists the extension whose structure is chained.
        unsafe { instance.get_physical_device_properties2(gpu, &mut properties) };
        let renders = drm.has_render == vk::TRUE
            && u32::try_from(drm.render_major) == Ok(major)
            && u32::try_from(drm.render_minor) == Ok(minor);
        renders.then_some(id.device_uuid)
    })
}

/// `uuid` as FFmpeg's `av_uuid_parse` reads it.
fn written(uuid: &[u8; vk::UUID_SIZE]) -> String {
    let hex = |bytes: &[u8]| bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    [0..4, 4..6, 6..8, 8..10, 10..16].map(|group| hex(&uuid[group])).join("-")
}

/// What a test reads of the GPU a Vulkan device context is on: its render node's number and its name.
#[cfg(test)]
fn node_of(device: &HwDevice) -> Option<((u32, u32), String)> {
    let loaded = super::pass::Loaded::of(device).ok()?;
    let mut drm = vk::PhysicalDeviceDrmPropertiesEXT::default();
    let mut properties = vk::PhysicalDeviceProperties2::default().push_next(&mut drm);
    // SAFETY: the GPU is the device's, of its live instance; a GPU without the extension leaves the chain unwritten.
    unsafe { loaded.instance.get_physical_device_properties2(loaded.physical, &mut properties) };
    let name = properties.properties.device_name_as_c_str().ok()?.to_string_lossy().into_owned();
    let number = (u32::try_from(drm.render_major).ok()?, u32::try_from(drm.render_minor).ok()?);
    (drm.has_render == vk::TRUE).then_some((number, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_uuid_is_written_as_ffmpeg_reads_one() {
        let uuid = std::array::from_fn(|at| u8::try_from(at * 17).unwrap_or(0));
        assert_eq!(written(&uuid), "00112233-4455-6677-8899-aabbccddeeff");
    }

    #[test]
    fn a_node_that_is_not_there_is_refused() {
        let refused = vulkan_on("/dev/dri/renderD999", Validation::Off).err();
        assert_eq!(refused, Some(EncodeError::Screen));
    }

    mod lab {
        use super::super::*;
        use crate::probe::survey;

        /// On a machine of two GPUs the device is the node's, not the first Vulkan lists.
        #[test]
        fn the_vulkan_device_opened_for_a_node_is_on_that_node() {
            tier::lab();
            let report = survey();
            let nodes: Vec<String> = report
                .encoders
                .iter()
                .filter_map(|e| e.device())
                .filter(|d| d.backend == Backend::VaApi)
                .filter_map(|d| d.node.clone())
                .collect();
            if nodes.is_empty() {
                return tier::skipped("no VA-API encoder's render node here");
            }
            for node in nodes {
                let device = vulkan_on(&node, Validation::Off);
                let Ok(device) = device else {
                    tier::skipped(format!("no Vulkan GPU here says it is {node}'s"));
                    continue;
                };
                let (number, name) = node_of(&device).expect("the device's GPU names its render node");
                eprintln!("{node}: Vulkan's {name}, render node {}:{}", number.0, number.1);
                assert_eq!(Ok(number), number_of(&node), "{node}: the device is on another GPU ({name})");
            }
        }
    }
}
