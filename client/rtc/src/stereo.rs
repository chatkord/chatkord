//! A stream sent in stereo heard in stereo. The engine sets up a receiver's Opus decoder from its own answer, not the
//! offer, and its answer never asks for two channels; so, as mediasoup's own client does, the answer says `stereo=1`
//! for each Opus stream whose sender said `sprop-stereo=1`, before the engine is given it.

use crate::remote::Media;
use proto::mediasoup_types::rtp_parameters::{MimeTypeAudio, RtpCodecParameters, RtpCodecParametersParametersValue};

/// `answer`, the engine's own to an offer of `offered`, asking for stereo where a sender sends it.
pub fn heard_in_stereo<'a>(answer: &str, offered: impl IntoIterator<Item = &'a Media>) -> String {
    let stereo: Vec<(&str, u8)> =
        offered.into_iter().filter_map(|media| Some((media.mid.as_str(), sent_in_stereo(media)?))).collect();
    let mut out = String::with_capacity(answer.len() + stereo.len() * 10);
    let mut payload = None;
    for line in answer.split_inclusive('\n') {
        let text = line.trim_end_matches(['\r', '\n']);
        if text.starts_with("m=") {
            payload = None;
        } else if let Some(mid) = text.strip_prefix("a=mid:") {
            payload = stereo.iter().find(|(stereo_mid, _)| *stereo_mid == mid).map(|(_, payload)| *payload);
        } else if let Some(payload) = payload
            && let Some(settings) = text.strip_prefix(&format!("a=fmtp:{payload} "))
            && !settings.split(';').any(|setting| setting.starts_with("stereo="))
        {
            out.push_str(text);
            out.push_str(";stereo=1");
            out.push_str(line.strip_prefix(text).unwrap_or_default());
            continue;
        }
        out.push_str(line);
    }
    out
}

/// The payload type of the Opus codec `media` is sent in, where its sender says that is stereo.
fn sent_in_stereo(media: &Media) -> Option<u8> {
    media.rtp_parameters.codecs.iter().find_map(|codec| match codec {
        RtpCodecParameters::Audio { mime_type: MimeTypeAudio::Opus, payload_type, parameters, .. } => {
            matches!(parameters.get("sprop-stereo"), Some(RtpCodecParametersParametersValue::Number(1)))
                .then_some(*payload_type)
        }
        RtpCodecParameters::Audio { .. } | RtpCodecParameters::Video { .. } => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opus(mid: &str, sprop_stereo: bool) -> Media {
        let extra = if sprop_stereo { r#", "sprop-stereo": 1"# } else { "" };
        Media {
            mid: mid.into(),
            rtp_parameters: serde_json::from_str(&format!(
                r#"{{"codecs": [{{"mimeType": "audio/opus", "payloadType": 100, "clockRate": 48000, "channels": 2,
                    "parameters": {{"useinbandfec": 1{extra}}}, "rtcpFeedback": []}}],
                    "headerExtensions": [], "encodings": [{{"ssrc": 1}}], "rtcp": {{"cname": "c", "reducedSize": true}}}}"#
            ))
            .expect("parameters"),
            stream_id: "s".into(),
            track_id: "t".into(),
        }
    }

    const ANSWER: &str = "v=0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 100\r\na=mid:0\r\na=rtpmap:100 opus/48000/2\r\n\
        a=fmtp:100 minptime=10;useinbandfec=1\r\nm=audio 9 UDP/TLS/RTP/SAVPF 100\r\na=mid:1\r\n\
        a=rtpmap:100 opus/48000/2\r\na=fmtp:100 minptime=10;useinbandfec=1\r\n";

    #[test]
    fn only_a_stream_sent_in_stereo_is_asked_for_in_stereo() {
        let (microphone, share) = (opus("0", false), opus("1", true));
        let answer = heard_in_stereo(ANSWER, [&microphone, &share]);
        let fmtp: Vec<&str> = answer.lines().filter(|line| line.starts_with("a=fmtp")).collect();
        assert_eq!(fmtp, ["a=fmtp:100 minptime=10;useinbandfec=1", "a=fmtp:100 minptime=10;useinbandfec=1;stereo=1"]);
        assert!(answer.ends_with("\r\n") && !answer.contains("\r\r"), "the line ends are kept");
        assert_eq!(heard_in_stereo(&answer, [&share]), answer, "said once");
        assert_eq!(heard_in_stereo(ANSWER, [&microphone]), ANSWER, "nothing in stereo, nothing changed");
    }
}
