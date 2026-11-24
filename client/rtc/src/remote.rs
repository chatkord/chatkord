//! The SDP offer a receiving transport is given, in the shape mediasoup's own JavaScript client writes so the engine
//! sees nothing unusual: one section per stream, bundled over the one transport.

use crate::reached::Reached;
use proto::mediasoup_types::data_structures::{DtlsFingerprint, IceCandidateTcpType, IceCandidateType, Protocol};
use proto::mediasoup_types::rtp_parameters::{
    MediaKind, MimeType, RtpCodecParameters, RtpCodecParametersParametersValue, RtpParameters,
};
use proto::sdp::{MAX_CNAME, MAX_FMTP_ENTRIES, MAX_FMTP_TOKEN, is_token};
use std::fmt::Write;

const NEWLINE: &str = "\r\n";
/// The most codecs, header extensions and encodings one received stream may name; more is no real stream.
const MAX_CODECS: usize = 16;
const MAX_HEADER_EXTENSIONS: usize = 16;
const MAX_ENCODINGS: usize = 4;
/// The port every section states: media is bundled and the real address is the candidates'.
const DISCARD_PORT: u16 = 7;

/// One stream the server sends, as a section of the offer.
#[derive(Debug, Clone)]
pub struct Media {
    /// The section's id, unique in the offer.
    pub mid: String,
    /// What the server says the stream is made of.
    pub rtp_parameters: RtpParameters,
    /// The media stream the engine reports it under.
    pub stream_id: String,
    /// The track the engine reports it under.
    pub track_id: String,
}

impl Media {
    /// Audio or video, from the stream's first codec; `None` for a stream with no codec.
    pub fn kind(&self) -> Option<MediaKind> {
        match self.rtp_parameters.codecs.first()? {
            RtpCodecParameters::Audio { .. } => Some(MediaKind::Audio),
            RtpCodecParameters::Video { .. } => Some(MediaKind::Video),
        }
    }
}

/// What the offer says of one section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offered {
    /// The server sends the stream.
    Sending,
    /// The stream ended on the section that carries the bundle: closing it would close the connection's
    /// DTLS, so it stays open and idle.
    Resting,
    /// The stream ended: the section is closed and left out of the bundle, and a later stream may take the slot.
    Closed,
}

/// An offer to the engine: one transport reached, and the streams the server sends over it.
#[derive(Debug, Clone)]
pub struct RemoteOffer {
    transport: Reached,
    media: Vec<(Media, Offered)>,
}

impl RemoteOffer {
    /// An offer of no stream yet over `transport`.
    pub fn new(transport: Reached) -> Self {
        Self { transport, media: Vec::new() }
    }

    /// One more stream the server sends.
    pub fn with(self, media: Media) -> Self {
        self.offered(media, Offered::Sending)
    }

    /// One more section, as `offered` says.
    pub fn offered(mut self, media: Media, offered: Offered) -> Self {
        self.media.push((media, offered));
        self
    }

    /// The SDP text, ready for the engine's `set_remote_description`; refused where a server's value cannot be written.
    pub fn sdp(&self) -> Result<String, Unsafe> {
        let mids: Vec<&str> = self
            .media
            .iter()
            .filter(|(_, offered)| *offered != Offered::Closed)
            .map(|(media, _)| media.mid.as_str())
            .collect();
        let mut out = Writer::default();
        session(&self.transport, &mids, &mut out)?;
        for (media, offered) in &self.media {
            let parameters = &media.rtp_parameters;
            if parameters.codecs.len() > MAX_CODECS
                || parameters.header_extensions.len() > MAX_HEADER_EXTENSIONS
                || parameters.encodings.len() > MAX_ENCODINGS
            {
                return Err(Unsafe);
            }
            let Some(kind) = kind_of(parameters) else { continue };
            let direction = match offered {
                Offered::Sending => "sendonly",
                Offered::Resting => "inactive",
                Offered::Closed => {
                    let payload = parameters.codecs.first().map_or(0, RtpCodecParameters::payload_type);
                    out.line(&format!("m={kind} 0 UDP/TLS/RTP/SAVPF {payload}"))?;
                    out.line("c=IN IP4 0.0.0.0")?;
                    out.line(&format!("a=mid:{}", media.mid))?;
                    out.line("a=inactive")?;
                    continue;
                }
            };
            section_header(kind, &media.mid, &self.transport, "actpass", parameters, &mut out)?;
            out.line(&format!("a=msid:{} {}", media.stream_id, media.track_id))?;
            out.line(&format!("a={direction}"))?;
            out.line("a=rtcp-mux")?;
            out.line("a=rtcp-rsize")?;
            codecs_and_extensions(parameters, &mut out)?;
            if let Some(encoding) = parameters.encodings.first() {
                let cname = parameters.rtcp.cname.as_deref();
                if cname.is_some_and(|cname| !is_token(cname, MAX_CNAME)) {
                    return Err(Unsafe);
                }
                if let (Some(ssrc), Some(cname)) = (encoding.ssrc, cname) {
                    out.line(&format!("a=ssrc:{ssrc} cname:{cname}"))?;
                }
                if let Some(rtx) = &encoding.rtx {
                    if let Some(cname) = cname {
                        out.line(&format!("a=ssrc:{} cname:{cname}", rtx.ssrc))?;
                    }
                    if let Some(ssrc) = encoding.ssrc {
                        out.line(&format!("a=ssrc-group:FID {ssrc} {}", rtx.ssrc))?;
                    }
                }
            }
        }
        Ok(out.0)
    }
}

/// SDP text, one line at a time.
#[derive(Default)]
pub(crate) struct Writer(pub(crate) String);

/// A value from the server cannot be written into SDP safely: a line break, a string that is not a token where a
/// token belongs, or more entries than a real stream has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unsafe;

impl Writer {
    pub(crate) fn line(&mut self, text: &str) -> Result<(), Unsafe> {
        if text.contains(['\r', '\n']) {
            return Err(Unsafe);
        }
        self.0.push_str(text);
        self.0.push_str(NEWLINE);
        Ok(())
    }
}

pub(crate) fn kind_of(parameters: &RtpParameters) -> Option<&'static str> {
    match parameters.codecs.first() {
        Some(RtpCodecParameters::Audio { .. }) => Some("audio"),
        Some(RtpCodecParameters::Video { .. }) => Some("video"),
        None => None,
    }
}

/// The lines that open a description: the server's identity as ICE lite and its DTLS fingerprint, and the bundle of
/// every section.
pub(crate) fn session(transport: &Reached, mids: &[&str], out: &mut Writer) -> Result<(), Unsafe> {
    out.line("v=0")?;
    out.line("o=chatkord 10000 1 IN IP4 0.0.0.0")?;
    out.line("s=-")?;
    out.line("t=0 0")?;
    if transport.ice_parameters.ice_lite == Some(true) {
        out.line("a=ice-lite")?;
    }
    out.line("a=ice-options:ice2")?;
    out.line("a=msid-semantic: WMS *")?;
    if let Some(fingerprint) = transport.dtls_parameters.fingerprints.last() {
        out.line(&format!("a=fingerprint:{} {}", algorithm(fingerprint), fingerprint.value_string()))?;
    }
    // A description whose every section is closed bundles nothing.
    if !mids.is_empty() {
        out.line(&format!("a=group:BUNDLE {}", mids.join(" ")))?;
    }
    Ok(())
}

/// The lines that open one section: its media line, how to reach the server, and the DTLS setup this side of the
/// description takes.
pub(crate) fn section_header(
    kind: &str,
    mid: &str,
    transport: &Reached,
    setup: &str,
    parameters: &RtpParameters,
    out: &mut Writer,
) -> Result<(), Unsafe> {
    let payloads: Vec<String> = parameters.codecs.iter().map(|codec| codec.payload_type().to_string()).collect();
    out.line(&format!("m={kind} {DISCARD_PORT} UDP/TLS/RTP/SAVPF {}", payloads.join(" ")))?;
    out.line("c=IN IP4 127.0.0.1")?;
    out.line(&format!("a=ice-ufrag:{}", transport.ice_parameters.username_fragment))?;
    out.line(&format!("a=ice-pwd:{}", transport.ice_parameters.password))?;
    for candidate in &transport.ice_candidates {
        let mut text = format!(
            "a=candidate:{} 1 {} {} {} {} typ {}",
            candidate.foundation,
            protocol(candidate.protocol),
            candidate.priority,
            candidate.address,
            candidate.port,
            candidate_type(candidate.kind),
        );
        if let Some(IceCandidateTcpType::Passive) = candidate.tcp_type {
            text.push_str(" tcptype passive");
        }
        out.line(&text)?;
    }
    out.line("a=end-of-candidates")?;
    out.line("a=ice-options:renomination")?;
    out.line(&format!("a=setup:{setup}"))?;
    out.line(&format!("a=mid:{mid}"))?;
    Ok(())
}

/// The header extensions and each codec with its parameters and feedback.
pub(crate) fn codecs_and_extensions(parameters: &RtpParameters, out: &mut Writer) -> Result<(), Unsafe> {
    for extension in &parameters.header_extensions {
        out.line(&format!("a=extmap:{} {}", extension.id, extension.uri.as_str()))?;
    }
    out.line("a=extmap-allow-mixed")?;
    for codec in &parameters.codecs {
        out.line(&rtpmap(codec))?;
        let settings = fmtp(codec)?;
        if !settings.is_empty() {
            out.line(&format!("a=fmtp:{} {settings}", codec.payload_type()))?;
        }
        for feedback in codec_feedback(codec) {
            let (kind, parameter) = feedback.as_type_parameter();
            let space = if parameter.is_empty() { "" } else { " " };
            out.line(&format!("a=rtcp-fb:{} {kind}{space}{parameter}", codec.payload_type()))?;
        }
    }
    Ok(())
}

fn algorithm(fingerprint: &DtlsFingerprint) -> &'static str {
    match fingerprint {
        DtlsFingerprint::Sha1 { .. } => "sha-1",
        DtlsFingerprint::Sha224 { .. } => "sha-224",
        DtlsFingerprint::Sha256 { .. } => "sha-256",
        DtlsFingerprint::Sha384 { .. } => "sha-384",
        DtlsFingerprint::Sha512 { .. } => "sha-512",
    }
}

fn protocol(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::Udp => "udp",
        Protocol::Tcp => "tcp",
    }
}

fn candidate_type(kind: IceCandidateType) -> &'static str {
    match kind {
        IceCandidateType::Host => "host",
        IceCandidateType::Srflx => "srflx",
        IceCandidateType::Prflx => "prflx",
        IceCandidateType::Relay => "relay",
    }
}

fn rtpmap(codec: &RtpCodecParameters) -> String {
    let (name, rate, channels) = match codec {
        RtpCodecParameters::Audio { mime_type, clock_rate, channels, .. } => {
            (MimeType::Audio(*mime_type).as_str(), clock_rate.get(), channels.get())
        }
        RtpCodecParameters::Video { mime_type, clock_rate, .. } => {
            (MimeType::Video(*mime_type).as_str(), clock_rate.get(), 1)
        }
    };
    let name = name.split_once('/').map_or(name, |(_, subtype)| subtype);
    let mut text = String::new();
    let _ = write!(text, "a=rtpmap:{} {name}/{rate}", codec.payload_type());
    if channels > 1 {
        let _ = write!(text, "/{channels}");
    }
    text
}

/// The `a=fmtp` settings, each key and string value an SDP token, as the server requires too.
fn fmtp(codec: &RtpCodecParameters) -> Result<String, Unsafe> {
    let parameters = codec.parameters();
    if parameters.iter().count() > MAX_FMTP_ENTRIES {
        return Err(Unsafe);
    }
    let settings = parameters.iter().map(|(key, value)| {
        let value = match value {
            RtpCodecParametersParametersValue::String(text) if is_token(text, MAX_FMTP_TOKEN) => text.to_string(),
            RtpCodecParametersParametersValue::String(_) => return Err(Unsafe),
            RtpCodecParametersParametersValue::Number(number) => number.to_string(),
        };
        if is_token(key, MAX_FMTP_TOKEN) { Ok(format!("{key}={value}")) } else { Err(Unsafe) }
    });
    Ok(settings.collect::<Result<Vec<_>, _>>()?.join(";"))
}

fn codec_feedback(codec: &RtpCodecParameters) -> &[proto::mediasoup_types::rtp_parameters::RtcpFeedback] {
    match codec {
        RtpCodecParameters::Audio { rtcp_feedback, .. } | RtpCodecParameters::Video { rtcp_feedback, .. } => {
            rtcp_feedback
        }
    }
}
