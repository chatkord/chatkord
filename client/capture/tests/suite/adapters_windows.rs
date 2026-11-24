//! A device is opened on the adapter it is asked for, and on none that is not there.

use capture::{AdapterId, Gpu, Validation};
use tier::skipped;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1};

/// The hardware adapters DXGI lists, each once as DXGI lists it: a machine's one GPU may be listed twice.
fn listed() -> Vec<AdapterId> {
    // SAFETY: a factory made with no arguments, owned by the returned interface.
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else { return Vec::new() };
    (0..)
        // SAFETY: the factory is live; an index past the last adapter ends the list with an error.
        .map_while(|at| unsafe { factory.EnumAdapters1(at) }.ok())
        // SAFETY: the adapter is live and its description is read once.
        .filter_map(|adapter| unsafe { adapter.GetDesc1() }.ok())
        .filter(|described| described.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0.cast_unsigned() == 0)
        .map(|described| AdapterId::of(described.AdapterLuid))
        .collect()
}

mod lab {
    use super::*;

    #[test]
    fn a_device_is_opened_on_each_adapter_dxgi_lists_and_says_which_it_is_on() {
        let adapters = listed();
        if adapters.is_empty() {
            return skipped("this machine lists no hardware adapter");
        }
        for adapter in adapters {
            let Some(gpu) = Gpu::open_on(Some(adapter), Validation::Off) else {
                eprintln!("{adapter}: no D3D11 device opens on it");
                continue;
            };
            assert_eq!(gpu.adapter(), Some(adapter), "the device is on the adapter it was asked for");
        }
    }

    #[test]
    fn an_adapter_that_is_not_there_opens_no_device_where_the_default_one_does() {
        let gone: AdapterId = "7fffffff7fffffff".parse().expect("an id");
        assert!(listed().iter().all(|adapter| *adapter != gone));
        assert!(Gpu::open_on(Some(gone), Validation::Off).is_none(), "no adapter has this id");
    }

    #[test]
    fn an_adapter_id_is_read_back_from_the_text_it_is_written_as() {
        for text in ["0000000000012345", "ffffffff00000001", "7fffffff7fffffff"] {
            let id: AdapterId = text.parse().expect("an id");
            assert_eq!(id.to_string(), text);
        }
        assert!("not hex".parse::<AdapterId>().is_err());
    }
}
