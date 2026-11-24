//! Which GPU paints the window, from what each one can do: the same choice on every system.

/// One GPU as the window weighs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Candidate {
    /// The API it is reached through.
    pub backend: wgpu::Backend,
    /// Discrete, integrated or other: a discrete one is preferred.
    pub kind: wgpu::DeviceType,
    /// Whether it can show the window at all.
    pub presents: bool,
    /// Whether it runs the window's shaders, which unpack half floats (`SHADER_F16_IN_F32`).
    pub paints: bool,
    /// Whether it takes a decoder's pictures as they are: it imports a surface and decodes in hardware.
    pub imports: bool,
}

impl Candidate {
    /// `adapter` as the window weighs it, given whether it shows the window and takes a decoder's pictures.
    pub(crate) fn of(adapter: &wgpu::Adapter, presents: bool, imports: bool) -> Self {
        let info = adapter.get_info();
        let paints = adapter.get_downlevel_capabilities().flags.contains(wgpu::DownlevelFlags::SHADER_F16_IN_F32);
        Self { backend: info.backend, kind: info.device_type, presents, paints, imports }
    }
}

/// Which of `candidates` paints: one that presents and runs the window's shaders, preferring one that also imports,
/// then the first backend in `order`, then a discrete GPU. `None` when no candidate does through a backend in `order`.
pub(crate) fn chosen(candidates: &[Candidate], order: &[wgpu::Backend]) -> Option<usize> {
    let backend = |backend: wgpu::Backend| order.iter().position(|&wanted| wanted == backend);
    let rank = |kind: wgpu::DeviceType| match kind {
        wgpu::DeviceType::DiscreteGpu => 0,
        wgpu::DeviceType::IntegratedGpu => 1,
        _ => 2,
    };
    candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.presents && candidate.paints && backend(candidate.backend).is_some())
        .min_by_key(|(_, candidate)| (!candidate.imports, backend(candidate.backend), rank(candidate.kind)))
        .map(|(at, _)| at)
}

#[cfg(test)]
mod tests {
    use super::{Candidate, chosen};
    use wgpu::Backend::{Dx12, Gl, Vulkan};
    use wgpu::DeviceType::{Cpu, DiscreteGpu, IntegratedGpu};

    const VULKAN: &[wgpu::Backend] = &[Vulkan];

    #[test]
    fn a_gpu_that_cannot_decode_still_paints_the_window() {
        // No VA-API here, or a GPU with no hardware H.264 decoder: the window still opens, its pictures painted from
        // memory.
        let only = Candidate { backend: Vulkan, kind: DiscreteGpu, presents: true, paints: true, imports: false };
        assert_eq!(chosen(&[only], VULKAN), Some(0));
    }

    #[test]
    fn the_gpu_that_takes_the_pictures_as_they_are_paints_before_a_faster_one() {
        let discrete = Candidate { backend: Vulkan, kind: DiscreteGpu, presents: true, paints: true, imports: false };
        let integrated =
            Candidate { backend: Vulkan, kind: IntegratedGpu, presents: true, paints: true, imports: true };
        assert_eq!(chosen(&[discrete, integrated], VULKAN), Some(1));
        let both = Candidate { backend: Vulkan, kind: DiscreteGpu, presents: true, paints: true, imports: true };
        assert_eq!(chosen(&[integrated, both], VULKAN), Some(1), "and among those that do, the discrete one");
    }

    #[test]
    fn a_gpu_that_cannot_show_the_window_never_paints_it() {
        let elsewhere = Candidate { backend: Vulkan, kind: DiscreteGpu, presents: false, paints: true, imports: true };
        assert_eq!(chosen(&[elsewhere], VULKAN), None);
    }

    #[test]
    fn a_vulkan_that_cannot_run_the_shaders_gives_way_to_gl_on_the_same_processor() {
        // llvmpipe, a machine with no GPU: its Vulkan has no half floats, its GL does.
        let order = &[Vulkan, Gl];
        let vulkan = Candidate { backend: Vulkan, kind: Cpu, presents: true, paints: false, imports: false };
        let gl = Candidate { backend: Gl, kind: Cpu, presents: true, paints: true, imports: false };
        assert_eq!(chosen(&[vulkan, gl], order), Some(1));
        assert_eq!(chosen(&[vulkan], order), None, "nothing paints rather than a window that draws nothing");
        let gpu = Candidate { backend: Vulkan, kind: IntegratedGpu, presents: true, paints: true, imports: false };
        assert_eq!(chosen(&[gl, gpu], order), Some(1), "Vulkan first where it paints");
    }

    #[test]
    fn dx12_paints_before_vulkan_and_vulkan_when_dx12_cannot() {
        let order = &[Dx12, Vulkan];
        let vulkan = Candidate { backend: Vulkan, kind: DiscreteGpu, presents: true, paints: true, imports: false };
        let dx12 = Candidate { backend: Dx12, kind: IntegratedGpu, presents: true, paints: true, imports: false };
        assert_eq!(chosen(&[vulkan, dx12], order), Some(1), "before a faster GPU on the other backend");
        let blind = Candidate { presents: false, ..dx12 };
        assert_eq!(chosen(&[blind, vulkan], order), Some(1));
        assert_eq!(chosen(&[dx12], VULKAN), None, "a backend the system does not use never paints");
    }
}
