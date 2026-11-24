//! The sending half of the bridge: what the server's produce needs, read from the engine's own offer and never the
//! server's text, and the answer the engine is given back in the server's name.

use crate::h264::H264;
use crate::reached::Reached;
use crate::remote::{Unsafe, Writer, codecs_and_extensions, section_header, session};
use proto::mediasoup_types::rtp_parameters::{
    MediaKind, MimeType, RtcpFeedback, RtcpParameters, RtpCapabilitiesFinalized, RtpCodecCapabilityFinalized,
    RtpCodecParameters, RtpCodecParametersParameters, RtpCodecParametersParametersValue, RtpEncodingParameters,
    RtpEncodingParametersRtx, RtpHeaderExtensionParameters, RtpHeaderExtensionUri, RtpParameters,
};
use std::num::{NonZeroU8, NonZeroU32};
use std::str::FromStr;

/// Why the engine's offer could not be turned into something to produce.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendRefused {
    /// The offer has no media section to send.
    NoMedia,
    /// The engine and the router share no codec for this kind of track.
    NoCommonCodec,
    /// The engine wrote no section id, or no stream.
    Incomplete,
}

/// One media section of the engine's offer, as far as sending it goes.
#[derive(Debug, Clone)]
pub struct LocalSend {
    kind: MediaKind,
    /// The section is closed: its media line has port 0.
    rejected: bool,
    mid: String,
    codecs: Vec<LocalCodec>,
    extensions: Vec<(u16, String)>,
    ssrc: Option<u32>,
    /// The stream that carries retransmissions of `ssrc`, when the engine named one.
    rtx_ssrc: Option<u32>,
    cname: Option<String>,
}

#[derive(Debug, Clone)]
struct LocalCodec {
    payload_type: u8,
    name: String,
    clock_rate: u32,
    channels: u8,
    fmtp: Vec<(String, String)>,
    feedback: Vec<(String, String)>,
}

impl LocalSend {
    /// Reads every media section of the engine's offer, in order. The engine's text is trusted to be well formed; a
    /// line this does not understand is skipped.
    pub fn parse_all(offer: &str) -> Result<Vec<Self>, SendRefused> {
        let mut sections: Vec<Self> = Vec::new();
        for line in offer.lines().map(str::trim) {
            if let Some(media) = line.strip_prefix("m=") {
                let mut words = media.split(' ');
                let kind = match words.next().unwrap_or_default() {
                    "audio" => MediaKind::Audio,
                    "video" => MediaKind::Video,
                    _ => return Err(SendRefused::NoMedia),
                };
                let rejected = words.next() == Some("0");
                sections.push(Self {
                    kind,
                    rejected,
                    mid: String::new(),
                    codecs: Vec::new(),
                    extensions: Vec::new(),
                    ssrc: None,
                    rtx_ssrc: None,
                    cname: None,
                });
            } else if let Some(section) = sections.last_mut() {
                section.attribute(line);
            }
        }
        if sections.is_empty() {
            return Err(SendRefused::NoMedia);
        }
        Ok(sections)
    }

    fn attribute(&mut self, line: &str) {
        let Some(attribute) = line.strip_prefix("a=") else { return };
        let (name, value) = attribute.split_once(':').unwrap_or((attribute, ""));
        match name {
            "mid" => self.mid = value.to_string(),
            "extmap" => {
                if let Some((id, uri)) = value.split_once(' ') {
                    let id = id.split('/').next().and_then(|id| id.parse().ok());
                    if let Some(id) = id {
                        self.extensions.push((id, uri.to_string()));
                    }
                }
            }
            "rtpmap" => {
                if let Some(codec) = LocalCodec::from_rtpmap(value) {
                    self.codecs.push(codec);
                }
            }
            "fmtp" => {
                let Some((payload, settings)) = value.split_once(' ') else { return };
                if let Some(codec) = self.codec_mut(payload) {
                    codec.fmtp = settings
                        .split(';')
                        .filter_map(|pair| pair.trim().split_once('='))
                        .map(|(key, value)| (key.to_string(), value.to_string()))
                        .collect();
                }
            }
            "rtcp-fb" => {
                let Some((payload, feedback)) = value.split_once(' ') else { return };
                let (kind, parameter) = feedback.split_once(' ').unwrap_or((feedback, ""));
                if let Some(codec) = self.codec_mut(payload) {
                    codec.feedback.push((kind.to_string(), parameter.to_string()));
                }
            }
            "ssrc-group" => {
                // `FID <media> <retransmission>`: the second stream repairs the first.
                let mut words = value.split(' ');
                if words.next() == Some("FID") {
                    let media = words.next().and_then(|ssrc| ssrc.parse::<u32>().ok());
                    let repair = words.next().and_then(|ssrc| ssrc.parse::<u32>().ok());
                    if let (Some(media), Some(repair)) = (media, repair) {
                        self.ssrc = self.ssrc.or(Some(media));
                        if self.ssrc == Some(media) {
                            self.rtx_ssrc = Some(repair);
                        }
                    }
                }
            }
            "ssrc" => {
                let Some((ssrc, rest)) = value.split_once(' ') else { return };
                let Ok(ssrc) = ssrc.parse::<u32>() else { return };
                if self.ssrc.is_none() || self.ssrc == Some(ssrc) {
                    self.ssrc = Some(ssrc);
                    if let Some(cname) = rest.strip_prefix("cname:") {
                        self.cname = Some(cname.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    fn codec_mut(&mut self, payload: &str) -> Option<&mut LocalCodec> {
        let payload: u8 = payload.parse().ok()?;
        self.codecs.iter_mut().find(|codec| codec.payload_type == payload)
    }

    /// The section id the engine chose.
    pub fn mid(&self) -> &str {
        &self.mid
    }

    /// Whether the engine closed this section (its track ended and nothing has taken it since).
    pub fn is_rejected(&self) -> bool {
        self.rejected
    }

    /// What to give the server's produce: the codecs the router also offers, with the engine's payload types and
    /// parameters, the extensions the router knows, and this stream's SSRC and name.
    pub fn rtp_parameters(&self, router: &RtpCapabilitiesFinalized) -> Result<RtpParameters, SendRefused> {
        if self.mid.is_empty() || self.ssrc.is_none() {
            return Err(SendRefused::Incomplete);
        }
        let offered: Vec<(&LocalCodec, &RtpCodecCapabilityFinalized)> =
            self.codecs.iter().filter_map(|codec| Some((codec, matching(codec, self.kind, router)?))).collect();
        // The router's payload type for each codec this end keeps, which is how the router names what an `apt` repairs.
        let primaries: Vec<(u8, u8)> = offered
            .iter()
            .filter(|(codec, _)| !codec.is_retransmission())
            .map(|(codec, theirs)| (codec.payload_type, preferred_payload_type(theirs)))
            .collect();
        // A retransmission codec repairs one other codec, and is kept only when the router repairs that same codec.
        let repairs_what_router_repairs = |codec: &LocalCodec| {
            let ours = codec.repairs().and_then(|payload| primaries.iter().find(|(local, _)| *local == payload));
            ours.is_some_and(|(_, theirs)| router_repairs(router, *theirs))
        };
        let kept: Vec<&LocalCodec> = offered
            .into_iter()
            .map(|(codec, _)| codec)
            .filter(|codec| !codec.is_retransmission() || repairs_what_router_repairs(codec))
            .collect();
        let has_retransmission = kept.iter().any(|codec| codec.is_retransmission());
        let codecs: Vec<RtpCodecParameters> = kept.into_iter().filter_map(|codec| codec.typed(self.kind)).collect();
        if codecs.is_empty() {
            return Err(SendRefused::NoCommonCodec);
        }
        let header_extensions = self
            .extensions
            .iter()
            .filter_map(|(id, uri)| {
                let uri = RtpHeaderExtensionUri::from_str(uri).ok()?;
                router
                    .header_extensions
                    .iter()
                    .any(|known| known.kind == self.kind && known.uri == uri)
                    .then_some(RtpHeaderExtensionParameters { uri, id: *id, encrypt: false })
            })
            .collect();
        Ok(RtpParameters {
            mid: Some(self.mid.clone()),
            codecs,
            header_extensions,
            encodings: vec![RtpEncodingParameters {
                ssrc: self.ssrc,
                // A repair stream with no codec to carry it is a parameter the router refuses.
                rtx: self.rtx_ssrc.filter(|_| has_retransmission).map(|ssrc| RtpEncodingParametersRtx { ssrc }),
                ..RtpEncodingParameters::default()
            }],
            rtcp: RtcpParameters { cname: self.cname.clone(), reduced_size: true },
            msid: None,
        })
    }
}

/// What the answer says of one section of the engine's offer.
#[derive(Debug, Clone, Copy)]
pub enum Answered<'a> {
    /// The server receives this track, over these parameters (what its produce was given).
    Receiving(&'a RtpParameters),
    /// No track is on the section: it is closed, and the engine may use the slot again.
    Rejected,
    /// The track on the section that carries the bundle ended: closing that section would close the connection's DTLS,
    /// so it stays open and idle, over the parameters it last carried.
    Resting(&'a RtpParameters),
}

/// The answer the engine is given for its offer, as if from the server: the server's transport, receive-only, over
/// the same codecs and extension ids, every section answered in the offer's order and a closed one out of the bundle.
pub fn answer(transport: &Reached, sections: &[(&LocalSend, Answered<'_>)]) -> Result<String, Unsafe> {
    let mut out = Writer::default();
    let open: Vec<&str> = sections
        .iter()
        .filter(|(_, answered)| matches!(answered, Answered::Receiving(_) | Answered::Resting(_)))
        .map(|(section, _)| section.mid.as_str())
        .collect();
    session(transport, &open, &mut out)?;
    for (section, answered) in sections {
        let kind = match section.kind {
            MediaKind::Audio => "audio",
            MediaKind::Video => "video",
        };
        match answered {
            Answered::Receiving(parameters) => {
                answered_open(kind, section, transport, "recvonly", parameters, &mut out)?
            }
            Answered::Resting(parameters) => answered_open(kind, section, transport, "inactive", parameters, &mut out)?,
            Answered::Rejected => {
                let payload = section.codecs.first().map_or(0, |codec| codec.payload_type);
                out.line(&format!("m={kind} 0 UDP/TLS/RTP/SAVPF {payload}"))?;
                out.line("c=IN IP4 0.0.0.0")?;
                out.line(&format!("a=mid:{}", section.mid))?;
                out.line("a=inactive")?;
            }
        }
    }
    Ok(out.0)
}

/// An open section of the answer, receiving or idle as `direction` says.
fn answered_open(
    kind: &str,
    section: &LocalSend,
    transport: &Reached,
    direction: &str,
    parameters: &RtpParameters,
    out: &mut Writer,
) -> Result<(), Unsafe> {
    // The server is the DTLS server: this end offered `actpass` and takes the client role.
    section_header(kind, &section.mid, transport, "passive", parameters, out)?;
    out.line(&format!("a={direction}"))?;
    out.line("a=rtcp-mux")?;
    out.line("a=rtcp-rsize")?;
    codecs_and_extensions(parameters, out)
}

impl LocalCodec {
    fn is_retransmission(&self) -> bool {
        self.name.eq_ignore_ascii_case("rtx")
    }

    /// The payload type a retransmission codec repairs (`apt`).
    fn repairs(&self) -> Option<u8> {
        self.fmtp.iter().find(|(key, _)| key == "apt").and_then(|(_, value)| value.parse().ok())
    }

    fn from_rtpmap(value: &str) -> Option<Self> {
        let (payload, rest) = value.split_once(' ')?;
        let mut parts = rest.split('/');
        let name = parts.next()?.to_string();
        let clock_rate = parts.next()?.parse().ok()?;
        let channels = parts.next().and_then(|channels| channels.parse().ok()).unwrap_or(1);
        Some(Self {
            payload_type: payload.parse().ok()?,
            name,
            clock_rate,
            channels,
            fmtp: Vec::new(),
            feedback: Vec::new(),
        })
    }

    fn mime(&self, kind: MediaKind) -> Option<MimeType> {
        let prefix = match kind {
            MediaKind::Audio => "audio",
            MediaKind::Video => "video",
        };
        MimeType::from_str(&format!("{prefix}/{}", self.name)).ok()
    }

    fn typed(&self, kind: MediaKind) -> Option<RtpCodecParameters> {
        let mut parameters = RtpCodecParametersParameters::default();
        for (key, value) in &self.fmtp {
            // H.264's profile-level-id is hex, which High at level 5.2 (640034) writes in digits alone; mediasoup
            // reads it only as text (mediasoup-client special-cases it the same way).
            match value.parse::<u32>() {
                Ok(number) if key != "profile-level-id" => parameters.insert(key.clone(), number),
                _ => parameters.insert(key.clone(), value.clone()),
            };
        }
        let rtcp_feedback = self
            .feedback
            .iter()
            .filter_map(|(kind, parameter)| RtcpFeedback::from_type_parameter(kind, parameter).ok())
            .collect();
        let clock_rate = NonZeroU32::new(self.clock_rate)?;
        Some(match self.mime(kind)? {
            MimeType::Audio(mime_type) => RtpCodecParameters::Audio {
                mime_type,
                payload_type: self.payload_type,
                clock_rate,
                channels: NonZeroU8::new(self.channels)?,
                parameters,
                rtcp_feedback,
            },
            MimeType::Video(mime_type) => RtpCodecParameters::Video {
                mime_type,
                payload_type: self.payload_type,
                clock_rate,
                parameters,
                rtcp_feedback,
            },
        })
    }
}

/// The router's codec this one can be sent as: the same media type, clock rate and, for audio, channel count; and for
/// H.264 parameters that agree.
fn matching<'a>(
    codec: &LocalCodec,
    kind: MediaKind,
    router: &'a RtpCapabilitiesFinalized,
) -> Option<&'a RtpCodecCapabilityFinalized> {
    let mime = codec.mime(kind)?;
    router.codecs.iter().find(|offered| match offered {
        RtpCodecCapabilityFinalized::Audio { mime_type, clock_rate, channels, .. } => {
            MimeType::Audio(*mime_type) == mime
                && clock_rate.get() == codec.clock_rate
                && u8::from(*channels) == codec.channels
        }
        RtpCodecCapabilityFinalized::Video { mime_type, clock_rate, parameters, .. } => {
            MimeType::Video(*mime_type) == mime
                && clock_rate.get() == codec.clock_rate
                && h264_agrees(codec, parameters)
        }
    })
}

fn h264_agrees(codec: &LocalCodec, offered: &RtpCodecParametersParameters) -> bool {
    if !codec.name.eq_ignore_ascii_case("h264") {
        return true;
    }
    let ours = H264::read(|key| codec.fmtp.iter().find(|(name, _)| name == key).map(|(_, value)| value.clone()));
    let theirs = H264::read(|key| offered.get(key).map(parameter_text));
    matches!((ours, theirs), (Some(ours), Some(theirs)) if ours.agrees_with(&theirs))
}

fn parameter_text(value: &RtpCodecParametersParametersValue) -> String {
    match value {
        RtpCodecParametersParametersValue::String(text) => text.to_string(),
        RtpCodecParametersParametersValue::Number(number) => number.to_string(),
    }
}

fn preferred_payload_type(codec: &RtpCodecCapabilityFinalized) -> u8 {
    let (RtpCodecCapabilityFinalized::Audio { preferred_payload_type, .. }
    | RtpCodecCapabilityFinalized::Video { preferred_payload_type, .. }) = codec;
    *preferred_payload_type
}

/// Whether the router has a retransmission codec for the codec it names by payload type.
fn router_repairs(router: &RtpCapabilitiesFinalized, payload_type: u8) -> bool {
    router.codecs.iter().filter(|codec| codec.is_rtx()).any(|rtx| {
        let (RtpCodecCapabilityFinalized::Audio { parameters, .. }
        | RtpCodecCapabilityFinalized::Video { parameters, .. }) = rtx;
        parameters.get("apt").map(parameter_text).is_some_and(|apt| apt == payload_type.to_string())
    })
}
