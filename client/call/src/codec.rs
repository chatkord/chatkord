//! Where a codec crosses between the window's, `media`'s and the engine's names for it: the window names what a person
//! chooses, `media` what encodes and decodes it, the engine what it is sent and arrives as.

use choices::Codec;
use media::{Backend, Path, Rank, Runs};
use screens::Route;

/// What encodes a share in `codec`.
pub(crate) const fn encoded(codec: Codec) -> media::Codec {
    match codec {
        Codec::H264 => media::Codec::H264,
        Codec::Av1 => media::Codec::Av1,
    }
}

/// What a share in `codec` is sent as.
pub(crate) const fn sent(codec: Codec) -> webrtc::Codec {
    match codec {
        Codec::H264 => webrtc::Codec::H264,
        Codec::Av1 => webrtc::Codec::Av1,
    }
}

/// How a watched screen that arrived as `codec` is named.
pub(crate) const fn arrived(codec: webrtc::Codec) -> Codec {
    match codec {
        webrtc::Codec::H264 => Codec::H264,
        webrtc::Codec::Av1 => Codec::Av1,
    }
}

/// How a path reads on a card, decoding or encoding: the one place `media`'s paths become the card's routes.
pub(crate) fn route(path: Path) -> Route {
    match path {
        Path::Hardware(backend) => Route::Hardware { via: via(backend).to_string() },
        Path::Software => Route::Software,
    }
}

/// How an encoder codes, as a card reads it: said untested where nobody tested its family.
pub(crate) fn encoder_route(encoder: &media::Encoder) -> Route {
    match &encoder.runs {
        Runs::Gpu(device) if encoder.rank() == Rank::Untested => {
            Route::UntestedHardware { via: via(device.backend).to_string() }
        }
        Runs::Gpu(device) => Route::Hardware { via: via(device.backend).to_string() },
        Runs::Cpu { .. } => Route::Software,
    }
}

/// What an encoder of `codec` runs on, as a person is told it.
pub(crate) fn encoder_option(codec: Codec, encoder: &media::Encoder) -> choices::EncoderOption {
    let on = match encoder.rank() {
        Rank::Measured => choices::EncodesOn::GraphicsCard,
        Rank::Untested => choices::EncodesOn::UntestedCard,
        Rank::Software => choices::EncodesOn::Processor,
    };
    choices::EncoderOption { codec, on }
}

/// How a graphics backend is named on a card.
fn via(backend: Backend) -> &'static str {
    match backend {
        Backend::VaApi => "VA-API",
        Backend::Vulkan => "Vulkan",
        #[cfg(windows)]
        Backend::D3d11 => "D3D11",
        #[cfg(windows)]
        Backend::D3d12 => "D3D12",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_encoder_whose_family_nobody_tested_is_said_untested_on_its_option_and_its_route() {
        let device = media::Device {
            backend: media::SCREEN_ORDER[0],
            node: None,
            vendor: Some(media::PciVendor::AMD),
            validation: media::Validation::Off,
        };
        let on = |name| media::Encoder { name, runs: Runs::Gpu(device.clone()), multiple: media::Multiple::EVEN };
        let x264 = media::Encoder {
            name: media::Name::X264,
            runs: Runs::Cpu { convert: Some(device.clone()) },
            multiple: media::Multiple::EVEN,
        };
        let via = via(device.backend).to_string();
        assert_eq!(encoder_route(&on(media::Name::H264Nvenc)), Route::UntestedHardware { via: via.clone() });
        assert_eq!(encoder_route(&on(media::Name::H264Vaapi)), Route::Hardware { via });
        assert_eq!(encoder_route(&x264), Route::Software);
        let said = |encoder: &media::Encoder| encoder_option(Codec::H264, encoder).on;
        assert_eq!(said(&on(media::Name::H264Qsv)), choices::EncodesOn::UntestedCard);
        assert_eq!(said(&on(media::Name::H264Amf)), choices::EncodesOn::GraphicsCard);
        assert_eq!(said(&x264), choices::EncodesOn::Processor);
    }
}
