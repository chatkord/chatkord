//! What the server answers to a request to consume a stream.

use proto::mediasoup_types::rtp_parameters::RtpParameters;
use proto::sdp::{SfuJsonRefused, sfu_json};
use serde::Deserialize;

/// The part of a consumer the engine needs.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Consumed {
    /// What the stream is made of.
    pub rtp_parameters: RtpParameters,
}

/// Reads `VoiceConsumed.consumer`, the server's JSON for one consumer.
pub fn consumed(bytes: &[u8]) -> Result<Consumed, SfuJsonRefused> {
    sfu_json(bytes)
}
