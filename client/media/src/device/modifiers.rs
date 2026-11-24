//! Which DRM format modifiers the GPU can sample from, asked of Vulkan, so the compositor is offered only layouts
//! the encoder can import. Without this the only safe offer is linear, which the compositor converts every frame to.

use ash::{Entry, Instance, vk};
use capture::Modifier;

use crate::loader;

/// The modifiers every Vulkan GPU here can sample a captured picture from, tiled first, linear last, never empty.
/// Every GPU's, since the encoder is chosen after the capture is offered.
pub fn importable() -> Vec<Modifier> {
    let mut found = ask().unwrap_or_default();
    found.sort_by_key(|m| *m == Modifier::LINEAR);
    if found.is_empty() {
        found.push(Modifier::LINEAR);
    }
    found
}

fn ask() -> Option<Vec<Modifier>> {
    let _alone = loader::alone();
    // SAFETY: loading the system's Vulkan loader runs its initialisers, which is what asking for Vulkan means; a
    // failure is a machine without one.
    let entry = unsafe { Entry::load() }.ok()?;
    let app = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);
    let info = vk::InstanceCreateInfo::default().application_info(&app);
    // SAFETY: `info` and what it points at outlive the call.
    let instance = unsafe { entry.create_instance(&info, None) }.ok()?;
    let common = common_modifiers(&instance);
    // SAFETY: the instance is ours, made above, and nothing made from it outlives this call.
    unsafe { instance.destroy_instance(None) };
    common
}

fn common_modifiers(instance: &Instance) -> Option<Vec<Modifier>> {
    // SAFETY: the instance is valid for the whole function.
    let devices = unsafe { instance.enumerate_physical_devices() }.ok()?;
    let lists: Vec<Vec<Modifier>> = devices.into_iter().filter_map(|device| sampled(instance, device)).collect();
    (!lists.is_empty()).then(|| agreed(lists))
}

/// What every list holds, in the first list's order.
fn agreed(lists: Vec<Vec<Modifier>>) -> Vec<Modifier> {
    let mut lists = lists.into_iter();
    let first = lists.next().unwrap_or_default();
    lists.fold(first, |so_far, list| so_far.into_iter().filter(|m| list.contains(m)).collect())
}

/// What one GPU can sample a 8-bit BGRA picture from, in one plane; `None` when it cannot say.
fn sampled(instance: &Instance, device: vk::PhysicalDevice) -> Option<Vec<Modifier>> {
    // SAFETY: `device` came from this instance.
    let kind = unsafe { instance.get_physical_device_properties(device) }.device_type;
    // A software rasterizer has no say in what the GPU can import, and lists only linear.
    if kind == vk::PhysicalDeviceType::CPU {
        return None;
    }
    // SAFETY: as above.
    let extensions = unsafe { instance.enumerate_device_extension_properties(device) }.ok()?;
    let says = extensions
        .iter()
        .any(|e| e.extension_name_as_c_str().is_ok_and(|name| name == ash::ext::image_drm_format_modifier::NAME));
    if !says {
        return None;
    }
    let format = vk::Format::B8G8R8A8_UNORM;
    let mut count = vk::DrmFormatModifierPropertiesListEXT::default();
    {
        let mut properties = vk::FormatProperties2::default().push_next(&mut count);
        // SAFETY: both structures are valid and the chain is the one the extension defines.
        unsafe { instance.get_physical_device_format_properties2(device, format, &mut properties) };
    }
    let mut listed = vec![vk::DrmFormatModifierPropertiesEXT::default(); count.drm_format_modifier_count as usize];
    let mut list = vk::DrmFormatModifierPropertiesListEXT::default().drm_format_modifier_properties(&mut listed);
    {
        let mut properties = vk::FormatProperties2::default().push_next(&mut list);
        // SAFETY: `listed` has the room the first call asked for.
        unsafe { instance.get_physical_device_format_properties2(device, format, &mut properties) };
    }
    Some(
        listed
            .iter()
            // A layout with compression metadata beside the pixels is not imported.
            .filter(|p| {
                p.drm_format_modifier_plane_count == 1
                    && p.drm_format_modifier_tiling_features.contains(vk::FormatFeatureFlags::SAMPLED_IMAGE)
            })
            .map(|p| Modifier::from_raw(p.drm_format_modifier))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const TILED: Modifier = Modifier::from_raw(0x0200_0000_0000_0001);
    const OTHER: Modifier = Modifier::from_raw(0x0200_0000_0000_0002);

    #[test]
    fn only_what_every_gpu_can_read_is_offered() {
        assert_eq!(
            agreed(vec![vec![TILED, OTHER, Modifier::LINEAR], vec![OTHER, Modifier::LINEAR]]),
            vec![OTHER, Modifier::LINEAR]
        );
        assert_eq!(agreed(vec![vec![TILED, Modifier::LINEAR]]), vec![TILED, Modifier::LINEAR]);
        assert_eq!(agreed(vec![vec![TILED], vec![OTHER]]), Vec::<Modifier>::new());
        assert_eq!(agreed(Vec::new()), Vec::<Modifier>::new());
    }

    #[test]
    fn the_offer_is_never_empty_and_linear_is_last() {
        let offer = importable();
        assert!(!offer.is_empty());
        let (_, tiled) = offer.split_last().expect("not empty");
        assert!(tiled.iter().all(|m| *m != Modifier::LINEAR), "linear only ever comes last");
    }
}
