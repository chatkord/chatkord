//! The fuzz targets' entries for what a server's JSON becomes in the SDP the engine is given.

use crate::{
    Codec, Consumed, LocalSend, Media, Reached, RemoteOffer, Resolved, Scope, Transport, consumed, dtls_from_sdp,
};
use proto::mediasoup_types::data_structures::IceParameters;
use proto::mediasoup_types::rtp_parameters::{
    MimeTypeVideo, RtpCapabilitiesFinalized, RtpCodecCapabilityFinalized, RtpCodecParameters, RtpParameters,
};
use proto::sdp::sfu_json;

const SEED_TRANSPORT: &str = r#"{"iceParameters":{"usernameFragment":"u","password":"p","iceLite":true},
    "iceCandidates":[{"foundation":"f","priority":1,"address":"127.0.0.1","port":4000,"protocol":"udp",
    "type":"host"}],"dtlsParameters":{"role":"auto","fingerprints":[{"algorithm":"sha-256","value":
    "AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89"}]}}"#;

const SEED_STREAM: &str = r#"{"mid":"0","codecs":[{"mimeType":"audio/opus","payloadType":100,"clockRate":48000,
    "channels":2,"parameters":{},"rtcpFeedback":[]}],"headerExtensions":[],"encodings":[{"ssrc":1}],
    "rtcp":{"cname":"c","reducedSize":true}}"#;

/// An engine's offer with a voice section and a screen section: what the router's capabilities are judged against.
const SEED_OFFER: &str = "v=0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=mid:0\r\n\
    a=extmap:1 urn:ietf:params:rtp-hdrext:sdes:mid\r\na=rtpmap:111 opus/48000/2\r\n\
    a=fmtp:111 minptime=10;useinbandfec=1\r\na=ssrc:1111 cname:voice\r\n\
    m=video 9 UDP/TLS/RTP/SAVPF 102 103\r\na=mid:1\r\na=rtpmap:102 H264/90000\r\n\
    a=fmtp:102 packetization-mode=1;profile-level-id=42e01f\r\na=rtpmap:103 rtx/90000\r\na=fmtp:103 apt=102\r\n\
    a=ssrc-group:FID 2222 3333\r\na=ssrc:2222 cname:screen\r\n";

/// `transport` as a server on this machine is reached: every address it lists is within scope.
fn reached(transport: &Transport) -> Option<Reached> {
    Reached::of(transport, &Resolved::new(), Scope::Loopback).ok()
}

fn seed_transport() -> Option<Transport> {
    Transport::parse(SEED_TRANSPORT.as_bytes()).ok()
}

fn seed_media(rtp_parameters: RtpParameters) -> Media {
    Media { mid: "0".into(), rtp_parameters, stream_id: "stream".into(), track_id: "track".into() }
}

fn seed_stream() -> Option<RtpParameters> {
    sfu_json(SEED_STREAM.as_bytes()).ok()
}

/// Whether `sdp` is whole lines: each a type letter and `=`, none holding a bare line break, the text ended by CRLF.
fn whole_lines(sdp: &str) -> bool {
    sdp.strip_suffix("\r\n").map(|body| body.split("\r\n")).is_some_and(|mut lines| {
        lines.all(|line| {
            let mut chars = line.chars();
            chars.next().is_some_and(|kind| kind.is_ascii_lowercase())
                && chars.next() == Some('=')
                && !chars.any(|c| c == '\r' || c == '\n')
        })
    })
}

/// A server's transport and one stream's parameters, as JSON split at the first zero byte: whatever the server says,
/// the offer written from it is whole SDP lines, each a type letter and `=`, each ended by CRLF, or it is refused.
/// The text read back as the engine's own SDP never panics.
pub fn sdp_from_server(data: &[u8]) {
    let _ = dtls_from_sdp(&String::from_utf8_lossy(data));
    let (transport, parameters) = data.split_at(data.iter().position(|byte| *byte == 0).unwrap_or(data.len()));
    let Ok(transport) = Transport::parse(transport) else { return };
    let parameters = parameters.get(1..).unwrap_or_default();
    let Ok(rtp_parameters) = sfu_json::<RtpParameters>(parameters) else { return };
    let Some(reached) = reached(&transport) else { return };
    let Ok(sdp) = RemoteOffer::new(reached).with(seed_media(rtp_parameters)).sdp() else { return };
    assert!(whole_lines(&sdp), "an offer that is not whole SDP lines: {sdp:?}");
}

/// A server's answer to a consume request: a stream carried in a codec is offered in that codec alone, with only the
/// repair that names a kept payload; keeping it again changes nothing; and the offer written from it is whole lines.
pub fn consumer(data: &[u8]) {
    let Ok(Consumed { rtp_parameters }) = consumed(data) else { return };
    let Some(codec) = Codec::carried_by(&rtp_parameters) else { return };
    let mut alone = rtp_parameters;
    codec.alone_in(&mut alone);
    let kept: Vec<u8> = alone
        .codecs
        .iter()
        .filter_map(|kept| match kept {
            RtpCodecParameters::Video { mime_type, payload_type, .. } if *mime_type == codec.mime() => {
                Some(*payload_type)
            }
            _ => None,
        })
        .collect();
    for each in &alone.codecs {
        match each {
            RtpCodecParameters::Video { mime_type: MimeTypeVideo::Rtx, parameters, .. } => assert!(
                parameters.get("apt").is_some_and(|apt| kept.iter().any(|kept| crate::codec::apt_is(apt, *kept))),
                "a repair that names no kept payload"
            ),
            RtpCodecParameters::Video { mime_type, .. } => assert_eq!(*mime_type, codec.mime(), "another codec kept"),
            RtpCodecParameters::Audio { .. } => assert!(kept.is_empty() && alone.codecs.is_empty(), "audio kept"),
        }
    }
    let mut again = alone.clone();
    codec.alone_in(&mut again);
    assert_eq!(again, alone, "keeping one codec is not idempotent");
    let Some(transport) = seed_transport().as_ref().and_then(reached) else { return };
    if let Ok(sdp) = RemoteOffer::new(transport).with(seed_media(alone)).sdp() {
        assert!(whole_lines(&sdp), "an offer that is not whole SDP lines: {sdp:?}");
    }
}

/// A server's ICE parameters as JSON, put into a transport: the offer written from it is whole lines, and the
/// username fragment and password it states are the parsed ones, or it is refused.
pub fn ice(data: &[u8]) {
    let Ok(parsed) = sfu_json::<IceParameters>(data) else { return };
    let (Some(mut transport), Some(stream)) = (seed_transport(), seed_stream()) else { return };
    transport.ice_parameters = parsed.clone();
    let Some(reached) = reached(&transport) else { return };
    let Ok(sdp) = RemoteOffer::new(reached).with(seed_media(stream)).sdp() else { return };
    assert!(whole_lines(&sdp), "an offer that is not whole SDP lines: {sdp:?}");
    let states = |line: String| sdp.split("\r\n").any(|stated| stated == line);
    assert!(states(format!("a=ice-ufrag:{}", parsed.username_fragment)), "the fragment changed on its way");
    assert!(states(format!("a=ice-pwd:{}", parsed.password)), "the password changed on its way");
}

/// A router's capabilities as JSON, against an engine's offer: every codec a section is sent as, and every header
/// extension, is one the router has.
pub fn router(data: &[u8]) {
    let Ok(router) = sfu_json::<RtpCapabilitiesFinalized>(data) else { return };
    let Ok(sections) = LocalSend::parse_all(SEED_OFFER) else { return };
    for section in &sections {
        let Ok(sent) = section.rtp_parameters(&router) else { continue };
        for codec in &sent.codecs {
            assert!(router.codecs.iter().any(|theirs| same_codec(codec, theirs)), "a codec the router lacks");
        }
        for extension in &sent.header_extensions {
            assert!(router.header_extensions.iter().any(|known| known.uri == extension.uri), "an unknown extension");
        }
    }
}

fn same_codec(ours: &RtpCodecParameters, theirs: &RtpCodecCapabilityFinalized) -> bool {
    match (ours, theirs) {
        (
            RtpCodecParameters::Audio { mime_type: a, clock_rate: ac, .. },
            RtpCodecCapabilityFinalized::Audio { mime_type: b, clock_rate: bc, .. },
        ) => a == b && ac == bc,
        (
            RtpCodecParameters::Video { mime_type: a, clock_rate: ac, .. },
            RtpCodecCapabilityFinalized::Video { mime_type: b, clock_rate: bc, .. },
        ) => a == b && ac == bc,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn seeds(target: &str) -> std::io::Result<Vec<(PathBuf, Vec<u8>)>> {
        proto::fuzz::seeds(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/seeds"), target)
    }

    #[test]
    fn every_kept_server_offer_is_written_as_whole_sdp_lines() {
        for (_, input) in seeds("sdp_from_server").expect("the seeds") {
            sdp_from_server(&input);
        }
    }

    #[test]
    fn every_kept_consumer_keeps_one_codec_and_writes_whole_lines() {
        for (_, input) in seeds("consumer_from_server").expect("the seeds") {
            consumer(&input);
        }
    }

    #[test]
    fn every_kept_ice_pair_is_stated_as_it_was_parsed() {
        for (_, input) in seeds("ice_from_server").expect("the seeds") {
            ice(&input);
        }
    }

    #[test]
    fn every_kept_router_gives_a_section_only_codecs_it_has() {
        for (_, input) in seeds("router_from_server").expect("the seeds") {
            router(&input);
        }
    }

    #[test]
    fn the_fixed_seeds_are_themselves_readable() {
        assert!(seed_transport().is_some() && seed_stream().is_some());
        assert_eq!(LocalSend::parse_all(SEED_OFFER).expect("sections").len(), 2);
    }
}
