//! The video codecs a screen is sent and watched in, as the router names them.

use proto::mediasoup_types::rtp_parameters::{
    MimeTypeVideo, RtpCodecParameters, RtpCodecParametersParametersValue as Value, RtpParameters,
};

/// A codec a screen goes out in and arrives in; the router offers each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Codec {
    /// H.264 High, its units in Annex B.
    H264,
    /// AV1, its units as OBUs.
    Av1,
}

impl Codec {
    /// The router's name for this codec.
    pub const fn mime(self) -> MimeTypeVideo {
        match self {
            Self::H264 => MimeTypeVideo::H264,
            Self::Av1 => MimeTypeVideo::AV1,
        }
    }

    /// The codec `parameters` carry the picture in; none for any other.
    pub fn carried_by(parameters: &RtpParameters) -> Option<Self> {
        parameters.codecs.iter().find_map(|codec| match codec {
            RtpCodecParameters::Video { mime_type: MimeTypeVideo::H264, .. } => Some(Self::H264),
            RtpCodecParameters::Video { mime_type: MimeTypeVideo::AV1, .. } => Some(Self::Av1),
            _ => None,
        })
    }

    /// Keeps only this codec in `parameters` and the retransmission that repairs it, so the engine and the router
    /// agree on the one codec the units are in.
    pub fn alone_in(self, parameters: &mut RtpParameters) {
        let kept: Vec<u8> = parameters
            .codecs
            .iter()
            .filter_map(|codec| match codec {
                RtpCodecParameters::Video { mime_type, payload_type, .. } if *mime_type == self.mime() => {
                    Some(*payload_type)
                }
                _ => None,
            })
            .collect();
        parameters.codecs.retain(|codec| match codec {
            RtpCodecParameters::Video { mime_type: MimeTypeVideo::Rtx, parameters, .. } => {
                parameters.get("apt").is_some_and(|apt| kept.iter().any(|kept| apt_is(apt, *kept)))
            }
            RtpCodecParameters::Video { mime_type, .. } => *mime_type == self.mime(),
            RtpCodecParameters::Audio { .. } => false,
        });
        if !parameters
            .codecs
            .iter()
            .any(|codec| matches!(codec, RtpCodecParameters::Video { mime_type: MimeTypeVideo::Rtx, .. }))
        {
            for encoding in &mut parameters.encodings {
                encoding.rtx = None;
            }
        }
    }
}

pub(crate) fn apt_is(apt: &Value, payload_type: u8) -> bool {
    match apt {
        Value::Number(number) => *number == u32::from(payload_type),
        Value::String(text) => text.parse::<u8>() == Ok(payload_type),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the engine offers a screen, as the router keeps it: every codec the two share, each with its repair.
    fn offered() -> RtpParameters {
        serde_json::from_str(
            r#"{"mid": "1", "codecs": [
                {"mimeType": "video/AV1", "payloadType": 45, "clockRate": 90000, "parameters": {}, "rtcpFeedback": []},
                {"mimeType": "video/rtx", "payloadType": 46, "clockRate": 90000, "parameters": {"apt": 45}, "rtcpFeedback": []},
                {"mimeType": "video/H264", "payloadType": 102, "clockRate": 90000,
                 "parameters": {"packetization-mode": 1, "profile-level-id": "42e01f"}, "rtcpFeedback": []},
                {"mimeType": "video/rtx", "payloadType": 103, "clockRate": 90000, "parameters": {"apt": 102}, "rtcpFeedback": []}
            ], "headerExtensions": [], "encodings": [{"ssrc": 1, "rtx": {"ssrc": 2}}], "rtcp": {"reducedSize": true}}"#,
        )
        .expect("parameters")
    }

    fn payload_types(parameters: &RtpParameters) -> Vec<u8> {
        parameters
            .codecs
            .iter()
            .map(|codec| match codec {
                RtpCodecParameters::Video { payload_type, .. } | RtpCodecParameters::Audio { payload_type, .. } => {
                    *payload_type
                }
            })
            .collect()
    }

    #[test]
    fn a_share_is_offered_in_its_one_codec_with_that_codecs_repair() {
        for (codec, kept) in [(Codec::Av1, [45, 46]), (Codec::H264, [102, 103])] {
            let mut parameters = offered();
            codec.alone_in(&mut parameters);
            assert_eq!(payload_types(&parameters), kept, "{codec:?}");
            assert_eq!(Codec::carried_by(&parameters), Some(codec));
            assert!(parameters.encodings[0].rtx.is_some(), "its repair stream stays");
        }
    }

    #[test]
    fn a_codec_with_no_repair_leaves_no_repair_stream() {
        let mut parameters = offered();
        parameters.codecs.retain(|codec| !matches!(codec, RtpCodecParameters::Video { payload_type: 46, .. }));
        Codec::Av1.alone_in(&mut parameters);
        assert_eq!(payload_types(&parameters), [45]);
        assert!(parameters.encodings[0].rtx.is_none(), "a repair stream with no codec is refused by the router");
    }
}
