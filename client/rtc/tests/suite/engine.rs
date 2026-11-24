//! The engine is the judge: what this crate writes is given to libwebrtc, which accepts it or does not, and what it
//! answers is read back.

use libwebrtc::peer_connection_factory::PeerConnectionFactory;
use libwebrtc::session_description::{SdpType, SessionDescription};
use proto::mediasoup_types::rtp_parameters::{
    RtpCodecParameters, RtpCodecParametersParameters, RtpCodecParametersParametersValue, RtpParameters,
};
use rtc::{Media, Offered, Reached, RemoteOffer, Resolved, Scope, Transport, Unsafe, dtls_from_sdp};

const FINGERPRINT: &str =
    "AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89:AB:CD:EF:01:23:45:67:89";

/// `transport` as a server on this machine is reached.
fn reached(transport: &Transport) -> Reached {
    Reached::of(transport, &Resolved::new(), Scope::Loopback).expect("candidates that are addresses")
}

/// What a server's `SfuOffer.transport` carries for one member.
fn server_transport() -> Transport {
    let json = format!(
        r#"{{
            "id": "00000000-0000-0000-0000-000000000001",
            "iceParameters": {{"usernameFragment": "serverufrag", "password": "serverpassword0123456789ab", "iceLite": true}},
            "iceCandidates": [{{"foundation": "udpcandidate", "priority": 1076302079, "address": "127.0.0.1", "protocol": "udp", "port": 40001, "type": "host"}}],
            "dtlsParameters": {{"role": "auto", "fingerprints": [{{"algorithm": "sha-256", "value": "{FINGERPRINT}"}}]}}
        }}"#
    );
    Transport::parse(json.as_bytes()).expect("a transport the server could send")
}

/// What a server's consumer carries for an Opus stream.
fn opus_stream() -> RtpParameters {
    serde_json::from_str(
        r#"{
            "codecs": [{"mimeType": "audio/opus", "payloadType": 100, "clockRate": 48000, "channels": 2,
                        "parameters": {"useinbandfec": 1, "minptime": 10}, "rtcpFeedback": [{"type": "transport-cc", "parameter": ""}]}],
            "headerExtensions": [{"uri": "urn:ietf:params:rtp-hdrext:sdes:mid", "id": 1, "encrypt": false},
                                 {"uri": "urn:ietf:params:rtp-hdrext:ssrc-audio-level", "id": 10, "encrypt": false}],
            "encodings": [{"ssrc": 22222222}],
            "rtcp": {"cname": "servercname", "reducedSize": true}
        }"#,
    )
    .expect("parameters the server could send")
}

fn offer() -> RemoteOffer {
    RemoteOffer::new(reached(&server_transport())).with(Media {
        mid: "0".into(),
        rtp_parameters: opus_stream(),
        stream_id: "member-1".into(),
        track_id: "microphone".into(),
    })
}

#[tokio::test]
async fn the_engine_accepts_the_offer_and_answers_receive_only_with_its_own_fingerprint() {
    let factory = PeerConnectionFactory::default();
    let connection = factory.create_peer_connection(Default::default()).expect("connection");
    let text = offer().sdp().expect("sdp");
    let remote = SessionDescription::parse(&text, SdpType::Offer).expect("the engine parses what we wrote");
    connection.set_remote_description(remote).await.expect("the engine accepts it");
    let answer = connection.create_answer(Default::default()).await.expect("answer");
    connection.set_local_description(answer.clone()).await.expect("local answer");

    let answer = answer.to_string();
    assert!(answer.contains("a=recvonly"), "{answer}");
    assert!(answer.contains("a=mid:0"));
    assert!(answer.contains("opus/48000/2"));
    let dtls = dtls_from_sdp(&answer).expect("the engine's identity");
    assert_ne!(dtls.fingerprints[0].value_string(), FINGERPRINT, "it is the engine's, not the server's");
    connection.close();
}

#[test]
fn a_server_value_with_a_line_break_is_refused() {
    let mut transport = server_transport();
    transport.ice_parameters.password = "x\r\na=evil".into();
    let offer = RemoteOffer::new(reached(&transport)).with(Media {
        mid: "0".into(),
        rtp_parameters: opus_stream(),
        stream_id: "member-1".into(),
        track_id: "microphone".into(),
    });
    assert_eq!(offer.sdp(), Err(Unsafe));
}

fn fmtp(codec: &mut RtpCodecParameters) -> &mut RtpCodecParametersParameters {
    match codec {
        RtpCodecParameters::Audio { parameters, .. } | RtpCodecParameters::Video { parameters, .. } => parameters,
    }
}

/// A stream another member produced, with one of its strings or counts changed.
fn offer_of(change: impl FnOnce(&mut RtpParameters)) -> RemoteOffer {
    let mut rtp_parameters = opus_stream();
    change(&mut rtp_parameters);
    RemoteOffer::new(reached(&server_transport())).with(Media {
        mid: "0".into(),
        rtp_parameters,
        stream_id: "member-1".into(),
        track_id: "microphone".into(),
    })
}

#[test]
fn a_members_string_that_is_not_an_sdp_token_is_refused() {
    assert!(offer_of(|_| {}).sdp().is_ok());
    assert_eq!(offer_of(|p| p.rtcp.cname = Some("a b".into())).sdp(), Err(Unsafe), "a space starts another attribute");
    assert_eq!(offer_of(|p| p.rtcp.cname = Some(String::new())).sdp(), Err(Unsafe));
    for (key, value) in [("useinbandfec", "1;x=y"), ("a b", "1")] {
        let changed = offer_of(|p| {
            fmtp(&mut p.codecs[0]).insert(key.to_string(), value.to_string());
        });
        assert_eq!(changed.sdp(), Err(Unsafe), "{key:?}={value:?}");
    }
}

#[test]
fn a_stream_with_more_entries_than_a_real_one_is_refused() {
    let codec = opus_stream().codecs[0].clone();
    assert_eq!(offer_of(|p| p.codecs = vec![codec; 17]).sdp(), Err(Unsafe));
    let extension = opus_stream().header_extensions[0].clone();
    assert_eq!(offer_of(|p| p.header_extensions = vec![extension; 17]).sdp(), Err(Unsafe));
    let encoding = opus_stream().encodings[0].clone();
    assert_eq!(offer_of(|p| p.encodings = vec![encoding; 5]).sdp(), Err(Unsafe));
    let many = offer_of(|p| {
        for n in 0..=32 {
            fmtp(&mut p.codecs[0]).insert(format!("k{n}"), 1u32);
        }
    });
    assert_eq!(many.sdp(), Err(Unsafe));
}

#[test]
fn a_tcp_candidate_is_written_passive_beside_the_udp_one_and_the_engine_takes_both() {
    let json = format!(
        r#"{{
            "iceParameters": {{"usernameFragment": "serverufrag", "password": "serverpassword0123456789ab", "iceLite": true}},
            "iceCandidates": [
                {{"foundation": "udpcandidate", "priority": 1076302079, "address": "127.0.0.1", "protocol": "udp", "port": 40000, "type": "host"}},
                {{"foundation": "tcpcandidate", "priority": 1076276479, "address": "127.0.0.1", "protocol": "tcp", "port": 40000, "type": "host", "tcpType": "passive"}}
            ],
            "dtlsParameters": {{"role": "auto", "fingerprints": [{{"algorithm": "sha-256", "value": "{FINGERPRINT}"}}]}}
        }}"#
    );
    let transport = Transport::parse(json.as_bytes()).expect("a transport on one UDP and one TCP port");
    let media = Media {
        mid: "0".into(),
        rtp_parameters: opus_stream(),
        stream_id: "member-1".into(),
        track_id: "microphone".into(),
    };
    let text = RemoteOffer::new(reached(&transport)).with(media).sdp().expect("sdp");
    for line in [
        "a=candidate:udpcandidate 1 udp 1076302079 127.0.0.1 40000 typ host",
        "a=candidate:tcpcandidate 1 tcp 1076276479 127.0.0.1 40000 typ host tcptype passive",
    ] {
        assert!(text.lines().any(|candidate| candidate == line), "missing {line:?} in\n{text}");
    }
    SessionDescription::parse(&text, SdpType::Offer).expect("the engine parses both candidates");
}

#[test]
fn the_offer_has_the_shape_mediasoup_clients_write() {
    let text = offer().sdp().expect("sdp");
    for line in [
        "a=ice-lite",
        "a=group:BUNDLE 0",
        "m=audio 7 UDP/TLS/RTP/SAVPF 100",
        "a=ice-ufrag:serverufrag",
        "a=candidate:udpcandidate 1 udp 1076302079 127.0.0.1 40001 typ host",
        "a=end-of-candidates",
        "a=setup:actpass",
        "a=msid:member-1 microphone",
        "a=sendonly",
        "a=rtcp-mux",
        "a=extmap:1 urn:ietf:params:rtp-hdrext:sdes:mid",
        "a=rtpmap:100 opus/48000/2",
        "a=rtcp-fb:100 transport-cc",
        "a=ssrc:22222222 cname:servercname",
    ] {
        assert!(text.lines().any(|candidate| candidate == line), "missing {line:?} in\n{text}");
    }
    assert!(text.lines().any(|candidate| candidate.starts_with("a=fmtp:100 ") && candidate.contains("useinbandfec=1")));
    assert!(text.lines().any(|candidate| candidate == format!("a=fingerprint:sha-256 {FINGERPRINT}")));
    assert!(text.ends_with("\r\n"));
}

/// A section for an Opus stream the server made with its own SSRC, as every consumer has.
fn section(mid: &str, ssrc: u32) -> Media {
    let mut rtp_parameters = opus_stream();
    rtp_parameters.encodings[0].ssrc = Some(ssrc);
    Media { mid: mid.into(), rtp_parameters, stream_id: format!("member-{mid}"), track_id: format!("microphone-{mid}") }
}

/// The engine's answer to an offer of `sections`, once it has taken it.
async fn answered(
    connection: &libwebrtc::peer_connection::PeerConnection,
    sections: &[(Media, Offered)],
) -> (String, String) {
    let offer = sections
        .iter()
        .cloned()
        .fold(RemoteOffer::new(reached(&server_transport())), |offer, (media, offered)| offer.offered(media, offered));
    let text = offer.sdp().expect("sdp");
    connection
        .set_remote_description(SessionDescription::parse(&text, SdpType::Offer).expect("parsed"))
        .await
        .expect("the engine takes the offer");
    let answer = connection.create_answer(Default::default()).await.expect("answer");
    connection.set_local_description(answer.clone()).await.expect("local answer");
    (text, answer.to_string())
}

#[tokio::test]
async fn an_ended_stream_closes_its_section_the_first_rests_and_a_later_stream_takes_the_closed_slot() {
    let factory = PeerConnectionFactory::default();
    let connection = factory.create_peer_connection(Default::default()).expect("connection");
    let three = [section("0", 1), section("1", 2), section("2", 3)];
    answered(&connection, &three.clone().map(|media| (media, Offered::Sending))).await;

    let [first, second, third] = three;
    let (offer, answer) = answered(
        &connection,
        &[(first.clone(), Offered::Resting), (second, Offered::Closed), (third.clone(), Offered::Sending)],
    )
    .await;
    assert!(offer.contains("a=group:BUNDLE 0 2\r\n"), "a closed section is left out of the bundle:\n{offer}");
    assert!(offer.contains("m=audio 0 UDP/TLS/RTP/SAVPF 100\r\nc=IN IP4 0.0.0.0\r\na=mid:1\r\na=inactive"), "{offer}");
    let sections: Vec<&str> = answer.split("m=").skip(1).collect();
    assert!(!sections[0].starts_with("audio 0 ") && sections[0].contains("a=inactive"), "the first rests: {answer}");
    assert!(sections[1].starts_with("audio 0 "), "the second is closed: {answer}");
    assert!(sections[2].contains("a=recvonly"), "{answer}");

    let (_, answer) = answered(
        &connection,
        &[(first, Offered::Resting), (section("3", 4), Offered::Sending), (third, Offered::Sending)],
    )
    .await;
    let sections: Vec<&str> = answer.split("m=").skip(1).collect();
    assert_eq!(sections.len(), 3, "the slot was taken again, not added: {answer}");
    assert!(sections[1].contains("a=mid:3") && sections[1].contains("a=recvonly"), "{answer}");
    connection.close();
}

mod sending {
    use super::*;
    use libwebrtc::audio_source::AudioSourceOptions;
    use libwebrtc::audio_source::native::NativeAudioSource;
    use libwebrtc::peer_connection::OfferOptions;
    use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
    use libwebrtc::rtp_transceiver::RtpTransceiverDirection;
    use proto::mediasoup_types::rtp_parameters::{RtpCapabilitiesFinalized, RtpCodecParameters};
    use rtc::{Answered, LocalSend, SendRefused, answer};

    /// What a server's `SfuOffer.router_rtp` carries for a router offering Opus.
    fn router() -> RtpCapabilitiesFinalized {
        serde_json::from_str(
            r#"{
                "codecs": [{"kind": "audio", "mimeType": "audio/opus", "preferredPayloadType": 100, "clockRate": 48000, "channels": 2,
                            "parameters": {"useinbandfec": 1}, "rtcpFeedback": [{"type": "transport-cc", "parameter": ""}]}],
                "headerExtensions": [
                    {"kind": "audio", "uri": "urn:ietf:params:rtp-hdrext:sdes:mid", "preferredId": 1, "preferredEncrypt": false, "direction": "sendrecv"},
                    {"kind": "audio", "uri": "urn:ietf:params:rtp-hdrext:ssrc-audio-level", "preferredId": 10, "preferredEncrypt": false, "direction": "sendrecv"}
                ]
            }"#,
        )
        .expect("capabilities the server could send")
    }

    fn first(offer: &str) -> LocalSend {
        LocalSend::parse_all(offer).expect("the engine's offer").remove(0)
    }

    async fn local_offer() -> (libwebrtc::peer_connection::PeerConnection, String) {
        let factory = PeerConnectionFactory::default();
        let connection = factory.create_peer_connection(Default::default()).expect("connection");
        let source = NativeAudioSource::new(AudioSourceOptions::default(), 48_000, 1, 1000);
        connection.add_track(factory.create_audio_track("mic", source).into(), &["member"]).expect("track");
        let offer = connection.create_offer(OfferOptions::default()).await.expect("offer");
        connection.set_local_description(offer.clone()).await.expect("local offer");
        (connection, offer.to_string())
    }

    #[tokio::test]
    async fn the_engines_offer_gives_the_parameters_a_produce_needs() {
        let (_connection, offer) = local_offer().await;
        let local = first(&offer);
        let parameters = local.rtp_parameters(&router()).expect("shared codecs");
        assert_eq!(parameters.mid.as_deref(), Some(local.mid()));
        assert_eq!(
            parameters.codecs.len(),
            1,
            "only Opus is shared; RED, G.722, PCM and telephone-event are not offered"
        );
        let RtpCodecParameters::Audio { payload_type, .. } = &parameters.codecs[0] else { panic!("not audio") };
        assert_eq!(*payload_type, 111, "the engine's own payload type");
        let uris: Vec<&str> = parameters.header_extensions.iter().map(|ext| ext.uri.as_str()).collect();
        assert_eq!(uris, ["urn:ietf:params:rtp-hdrext:ssrc-audio-level", "urn:ietf:params:rtp-hdrext:sdes:mid"]);
        assert!(parameters.encodings[0].ssrc.is_some());
        assert!(parameters.rtcp.cname.is_some());
    }

    #[tokio::test]
    async fn the_engine_accepts_the_answer_written_in_the_servers_name() {
        let (connection, offer) = local_offer().await;
        let local = first(&offer);
        let parameters = local.rtp_parameters(&router()).expect("shared codecs");
        let answer = answer(&reached(&server_transport()), &[(&local, Answered::Receiving(&parameters))]).expect("sdp");
        let remote = SessionDescription::parse(&answer, SdpType::Answer).expect("the engine parses it");
        connection.set_remote_description(remote).await.expect("the engine accepts the answer");
        connection.close();
    }

    /// A router offering H.264 (constrained baseline, packetization mode 1) with its retransmission codec.
    fn video_router() -> RtpCapabilitiesFinalized {
        serde_json::from_str(
            r#"{
                "codecs": [
                    {"kind": "video", "mimeType": "video/H264", "preferredPayloadType": 101, "clockRate": 90000,
                     "parameters": {"level-asymmetry-allowed": 1, "packetization-mode": 1, "profile-level-id": "42e01f"},
                     "rtcpFeedback": [{"type": "nack", "parameter": ""}, {"type": "nack", "parameter": "pli"}]},
                    {"kind": "video", "mimeType": "video/rtx", "preferredPayloadType": 102, "clockRate": 90000,
                     "parameters": {"apt": 101}, "rtcpFeedback": []}
                ],
                "headerExtensions": []
            }"#,
        )
        .expect("capabilities the server could send")
    }

    const VIDEO_OFFER: &str = "v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96 97 109 114\r\na=mid:0\r\n\
        a=rtpmap:96 VP8/90000\r\na=rtpmap:97 rtx/90000\r\na=fmtp:97 apt=96\r\n\
        a=rtpmap:109 H264/90000\r\na=fmtp:109 level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f\r\n\
        a=rtpmap:114 rtx/90000\r\na=fmtp:114 apt=109\r\n\
        a=ssrc-group:FID 1000 2000\r\na=ssrc:1000 cname:x\r\na=ssrc:2000 cname:x\r\n";

    #[test]
    fn a_retransmission_codec_is_kept_only_for_a_codec_that_is_kept() {
        let local = first(VIDEO_OFFER);
        let parameters = local.rtp_parameters(&video_router()).expect("H.264 is shared");
        let payloads: Vec<u8> = parameters.codecs.iter().map(|codec| codec.payload_type()).collect();
        assert_eq!(
            payloads,
            [109, 114],
            "VP8 is not offered by the router, so its retransmission codec (97) goes with it"
        );
    }

    #[test]
    fn the_retransmission_stream_is_named_so_the_server_can_recover_lost_packets() {
        let local = first(VIDEO_OFFER);
        let parameters = local.rtp_parameters(&video_router()).expect("H.264 is shared");
        assert_eq!(parameters.encodings[0].ssrc, Some(1000));
        assert_eq!(parameters.encodings[0].rtx.as_ref().map(|rtx| rtx.ssrc), Some(2000));
    }

    fn h264_router(parameters: &str) -> RtpCapabilitiesFinalized {
        serde_json::from_str(&format!(
            r#"{{"codecs": [
                {{"kind": "video", "mimeType": "video/H264", "preferredPayloadType": 101, "clockRate": 90000,
                  "parameters": {{{parameters}}}, "rtcpFeedback": []}},
                {{"kind": "video", "mimeType": "video/rtx", "preferredPayloadType": 102, "clockRate": 90000,
                  "parameters": {{"apt": 101}}, "rtcpFeedback": []}}
            ], "headerExtensions": []}}"#
        ))
        .expect("capabilities the server could send")
    }

    fn h264_offer(fmtp: &str) -> String {
        format!(
            "v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 109\r\na=mid:0\r\na=rtpmap:109 H264/90000\r\na=fmtp:109 {fmtp}\r\n\
             a=ssrc:1000 cname:x\r\n"
        )
    }

    fn shares_h264(router: &str, offer: &str) -> bool {
        first(&h264_offer(offer)).rtp_parameters(&h264_router(router)).is_ok()
    }

    #[test]
    fn an_h264_level_the_router_does_not_name_is_shared_when_both_allow_a_level_difference() {
        let router = r#""level-asymmetry-allowed": 1, "packetization-mode": 1, "profile-level-id": "42e01f""#;
        assert!(shares_h264(router, "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e034"));
        assert!(
            !shares_h264(router, "packetization-mode=1;profile-level-id=42e034"),
            "this end did not allow another level"
        );
    }

    #[test]
    fn an_h264_profile_is_compared_as_a_profile_not_as_text() {
        let router = r#""level-asymmetry-allowed": 1, "packetization-mode": 1, "profile-level-id": "42e01f""#;
        assert!(
            shares_h264(router, "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42E01F"),
            "case is not meaning"
        );
        assert!(
            !shares_h264(router, "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=640c1f"),
            "High is not Constrained Baseline"
        );
        assert!(
            !shares_h264(router, "level-asymmetry-allowed=1;packetization-mode=0;profile-level-id=42e01f"),
            "another packetization mode"
        );
        assert!(
            !shares_h264(router, "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=zz"),
            "not a profile at all"
        );
    }

    #[test]
    fn a_profile_level_id_of_digits_alone_is_still_text() {
        let router = r#""level-asymmetry-allowed": 1, "packetization-mode": 1, "profile-level-id": "640034""#;
        let offer = h264_offer("level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=640034");
        let parameters = first(&offer).rtp_parameters(&h264_router(router)).expect("High is shared");
        let id = parameters.codecs[0].parameters().get("profile-level-id").cloned();
        assert_eq!(id, Some(RtpCodecParametersParametersValue::String("640034".into())), "hex, never a number");
    }

    #[test]
    fn an_absent_h264_parameter_means_its_default_on_both_sides() {
        // Packetization mode 0 and Constrained Baseline level 3.1 are what each end means by silence.
        assert!(shares_h264(
            r#""level-asymmetry-allowed": 1"#,
            "level-asymmetry-allowed=1;packetization-mode=0;profile-level-id=42e01f"
        ));
        assert!(shares_h264(r#""packetization-mode": 1, "profile-level-id": "42e01f""#, "packetization-mode=1"));
    }

    #[test]
    fn a_retransmission_codec_is_kept_only_when_the_router_repairs_that_same_codec() {
        let router: RtpCapabilitiesFinalized = serde_json::from_str(
            r#"{"codecs": [
                {"kind": "video", "mimeType": "video/H264", "preferredPayloadType": 101, "clockRate": 90000,
                 "parameters": {"packetization-mode": 1, "profile-level-id": "42e01f"}, "rtcpFeedback": []},
                {"kind": "video", "mimeType": "video/VP8", "preferredPayloadType": 103, "clockRate": 90000,
                 "parameters": {}, "rtcpFeedback": []},
                {"kind": "video", "mimeType": "video/rtx", "preferredPayloadType": 104, "clockRate": 90000,
                 "parameters": {"apt": 103}, "rtcpFeedback": []}
            ], "headerExtensions": []}"#,
        )
        .expect("capabilities the server could send");
        let local = first(VIDEO_OFFER);
        let parameters = local.rtp_parameters(&router).expect("H.264 and VP8 are shared");
        let payloads: Vec<u8> = parameters.codecs.iter().map(|codec| codec.payload_type()).collect();
        assert_eq!(
            payloads,
            [96, 97, 109],
            "the router repairs VP8 only, so H.264's retransmission codec (114) is not sent"
        );
        assert!(
            parameters.encodings[0].rtx.is_some(),
            "VP8 still has a retransmission stream, so the encoding names one"
        );
    }

    #[test]
    fn an_offer_with_no_shared_codec_or_no_media_is_refused() {
        let local =
            first("v=0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 8\r\na=mid:0\r\na=rtpmap:8 PCMA/8000\r\na=ssrc:1 cname:x\r\n");
        assert_eq!(local.rtp_parameters(&router()).unwrap_err(), SendRefused::NoCommonCodec);
        assert_eq!(LocalSend::parse_all("v=0\r\n").unwrap_err(), SendRefused::NoMedia);
        let no_stream = first("v=0\r\nm=audio 9 X 111\r\na=rtpmap:111 opus/48000/2\r\n");
        assert_eq!(no_stream.rtp_parameters(&router()).unwrap_err(), SendRefused::Incomplete);
        assert_eq!(
            LocalSend::parse_all("v=0\r\nm=application 9 X webrtc-datachannel\r\n").unwrap_err(),
            SendRefused::NoMedia
        );
    }

    /// A router offering Opus and VP8, which the engine can send without a hardware encoder.
    fn router_with_vp8() -> RtpCapabilitiesFinalized {
        serde_json::from_str(
            r#"{
                "codecs": [
                    {"kind": "audio", "mimeType": "audio/opus", "preferredPayloadType": 100, "clockRate": 48000, "channels": 2,
                     "parameters": {"useinbandfec": 1}, "rtcpFeedback": [{"type": "transport-cc", "parameter": ""}]},
                    {"kind": "video", "mimeType": "video/VP8", "preferredPayloadType": 101, "clockRate": 90000,
                     "parameters": {}, "rtcpFeedback": [{"type": "nack", "parameter": ""}]}
                ],
                "headerExtensions": []
            }"#,
        )
        .expect("capabilities the server could send")
    }

    /// The engine's next offer, given to `connection`, and the sections it holds.
    async fn next_offer(connection: &libwebrtc::peer_connection::PeerConnection) -> Vec<LocalSend> {
        let offer = connection.create_offer(OfferOptions::default()).await.expect("offer");
        connection.set_local_description(offer.clone()).await.expect("local offer");
        LocalSend::parse_all(&offer.to_string()).expect("the engine's offer")
    }

    /// Gives `connection` the server's answer over `sections`, and says what the engine thought of it.
    async fn answered(
        connection: &libwebrtc::peer_connection::PeerConnection,
        sections: &[(&LocalSend, Answered<'_>)],
    ) -> Result<(), libwebrtc::RtcError> {
        let text = answer(&reached(&server_transport()), sections).expect("sdp");
        connection
            .set_remote_description(SessionDescription::parse(&text, SdpType::Answer).expect("the engine parses it"))
            .await
    }

    #[tokio::test]
    async fn a_connection_sends_several_tracks_and_reuses_the_slot_of_one_that_ended() {
        use libwebrtc::video_source::VideoResolution;
        use libwebrtc::video_source::native::NativeVideoSource;
        let factory = PeerConnectionFactory::default();
        let connection = factory.create_peer_connection(Default::default()).expect("connection");
        let router = router_with_vp8();
        let microphone = || {
            factory.create_audio_track("mic", NativeAudioSource::new(AudioSourceOptions::default(), 48_000, 1, 1000))
        };

        // A microphone, then a screen: the offer grows by a section, and the answer covers both.
        connection.add_track(microphone().into(), &["tone"]).expect("track");
        let sections = next_offer(&connection).await;
        let mic = sections[0].rtp_parameters(&router).expect("opus");
        answered(&connection, &[(&sections[0], Answered::Receiving(&mic))]).await.expect("the first answer");

        let video = NativeVideoSource::new(VideoResolution { width: 640, height: 360 }, true);
        connection.add_track(factory.create_video_track("screen", video).into(), &["screen"]).expect("track");
        let sections = next_offer(&connection).await;
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].mid(), mic.mid.as_deref().expect("a mid"), "the microphone keeps its section");
        let screen = sections[1].rtp_parameters(&router).expect("vp8");
        answered(
            &connection,
            &[(&sections[0], Answered::Receiving(&mic)), (&sections[1], Answered::Receiving(&screen))],
        )
        .await
        .expect("an answer over both tracks");

        // The microphone ends: the engine closes its section, and the screen is untouched.
        let ended = connection
            .transceivers()
            .into_iter()
            .find(|t| t.mid().as_deref() == mic.mid.as_deref())
            .expect("the microphone's transceiver");
        ended.stop().expect("stopped");
        let sections = next_offer(&connection).await;
        assert_eq!((sections.len(), sections[0].is_rejected(), sections[1].is_rejected()), (2, true, false));
        answered(&connection, &[(&sections[0], Answered::Rejected), (&sections[1], Answered::Receiving(&screen))])
            .await
            .expect("an answer that closes the microphone's section");

        // A new microphone takes the closed slot under a new section id, and the bundle follows it.
        connection.add_track(microphone().into(), &["tone"]).expect("track");
        let sections = next_offer(&connection).await;
        assert_eq!((sections.len(), sections[0].is_rejected()), (2, false), "the slot is reused, not a third section");
        assert_ne!(sections[0].mid(), mic.mid.as_deref().expect("a mid"), "under a new id");
        let again = sections[0].rtp_parameters(&router).expect("opus");
        answered(
            &connection,
            &[(&sections[0], Answered::Receiving(&again)), (&sections[1], Answered::Receiving(&screen))],
        )
        .await
        .expect("an answer over the reused slot");

        // Everything ends: every section is closed, and a description that bundles nothing is accepted.
        for transceiver in connection.transceivers() {
            transceiver.stop().expect("stopped");
        }
        let sections = next_offer(&connection).await;
        assert!(sections.iter().all(LocalSend::is_rejected));
        answered(&connection, &[(&sections[0], Answered::Rejected), (&sections[1], Answered::Rejected)])
            .await
            .expect("an answer that closes everything");
        connection.close();
    }

    #[tokio::test]
    async fn the_first_section_rests_when_its_track_ends_and_the_bundle_stays_on_it() {
        let factory = PeerConnectionFactory::default();
        let connection = factory.create_peer_connection(Default::default()).expect("connection");
        let router = router_with_vp8();
        let microphone = || {
            factory.create_audio_track("mic", NativeAudioSource::new(AudioSourceOptions::default(), 48_000, 1, 1000))
        };
        let sender = connection.add_track(microphone().into(), &["tone"]).expect("track");
        let sections = next_offer(&connection).await;
        let mic = sections[0].rtp_parameters(&router).expect("opus");
        answered(&connection, &[(&sections[0], Answered::Receiving(&mic))]).await.expect("the first answer");

        // The track ends: the engine keeps the section open, and the answer rests it, still the bundle's tag.
        connection.remove_track(sender).expect("the track comes off");
        let sections = next_offer(&connection).await;
        assert_eq!((sections.len(), sections[0].is_rejected()), (1, false), "the first section stays open");
        let text = answer(&reached(&server_transport()), &[(&sections[0], Answered::Resting(&mic))]).expect("sdp");
        assert!(text.contains("a=group:BUNDLE 0\r\n") || text.contains("a=group:BUNDLE 0\n"), "{text}");
        assert!(text.contains("a=inactive") && !text.contains("m=audio 0 "), "open and idle: {text}");
        answered(&connection, &[(&sections[0], Answered::Resting(&mic))]).await.expect("the engine takes it");

        // A new track takes a section of its own and the first one keeps resting under it.
        connection.add_track(microphone().into(), &["tone"]).expect("track");
        let sections = next_offer(&connection).await;
        assert_eq!(sections.len(), 2, "a section of its own");
        assert!(sections.iter().all(|section| !section.is_rejected()));
        let directions: Vec<_> = connection.transceivers().iter().map(|t| (t.mid(), t.direction())).collect();
        assert!(
            matches!(
                &directions[..],
                [(Some(first), RtpTransceiverDirection::Inactive), (Some(second), RtpTransceiverDirection::SendOnly)]
                    if first == "0" && second == "1"
            ),
            "{directions:?}"
        );
        connection.close();
    }
}
