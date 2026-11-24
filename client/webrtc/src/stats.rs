//! Reading the engine's stats: which candidate pair a connection is using, and what its streams carried.

use libwebrtc::stats::{CandidatePairStats, InboundRtpStats, OutboundRtpStats, RtcStats};
use std::time::Duration;

/// The pair the transport selected, which is the route the connection is using.
pub(crate) fn selected_pair(all: &[RtcStats]) -> Option<&CandidatePairStats> {
    let id = all.iter().find_map(|one| match one {
        RtcStats::Transport(transport) if !transport.transport.selected_candidate_pair_id.is_empty() => {
            Some(&transport.transport.selected_candidate_pair_id)
        }
        _ => None,
    })?;
    all.iter().find_map(|one| match one {
        RtcStats::CandidatePair(pair) if pair.rtc.id == *id => Some(pair),
        _ => None,
    })
}

/// The round trip of the selected pair; none until a check was answered. The engine reports whole milliseconds, so
/// an answered check on a near route reads zero.
pub(crate) fn round_trip(all: &[RtcStats]) -> Option<Duration> {
    let pair = &selected_pair(all)?.candidate_pair;
    (pair.responses_received > 0).then(|| Duration::from_secs_f64(pair.current_round_trip_time.max(0.0)))
}

/// What the streams received counted: packets that arrived and did not, and the bytes that arrived.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Inbound {
    pub packets: u64,
    pub lost: u64,
    pub bytes: u64,
}

/// Summed over the audio streams, which the call's quality is judged on.
pub(crate) fn inbound_audio(all: &[RtcStats]) -> Inbound {
    inbound(all, |one| one.stream.kind == "audio")
}

/// Summed over every stream, audio and video.
pub(crate) fn inbound_every(all: &[RtcStats]) -> Inbound {
    inbound(all, |_| true)
}

fn inbound(all: &[RtcStats], counted: impl Fn(&InboundRtpStats) -> bool) -> Inbound {
    all.iter()
        .filter_map(|one| match one {
            RtcStats::InboundRtp(inbound) if counted(inbound) => Some(inbound),
            _ => None,
        })
        .fold(Inbound::default(), |sum, one| Inbound {
            packets: sum.packets.saturating_add(one.received.packets_received),
            lost: sum.lost.saturating_add(one.received.packets_lost.max(0).unsigned_abs()),
            bytes: sum.bytes.saturating_add(one.inbound.bytes_received),
        })
}

/// What was sent counted: packets and bytes, summed over every stream, audio and video.
pub(crate) fn outbound_every(all: &[RtcStats]) -> (u64, u64) {
    all.iter()
        .filter_map(|one| match one {
            RtcStats::OutboundRtp(out) => Some(&out.sent),
            _ => None,
        })
        .fold((0, 0), |(packets, bytes), one| {
            (packets.saturating_add(one.packets_sent), bytes.saturating_add(one.bytes_sent))
        })
}

/// The video sent in section `mid`: a connection may hold another, resting with its old counts.
pub(crate) fn outbound_video<'a>(all: &'a [RtcStats], mid: &str) -> Option<&'a OutboundRtpStats> {
    all.iter().find_map(|one| match one {
        RtcStats::OutboundRtp(out) if out.stream.kind == "video" && out.outbound.mid == mid => Some(out),
        _ => None,
    })
}

/// What the transports carried, sent and received, whatever was in it.
pub(crate) fn transport_bytes(all: &[RtcStats]) -> (u64, u64) {
    all.iter()
        .filter_map(|one| match one {
            RtcStats::Transport(transport) => Some(&transport.transport),
            _ => None,
        })
        .fold((0, 0), |(sent, received), one| {
            (sent.saturating_add(one.bytes_sent), received.saturating_add(one.bytes_received))
        })
}

fn audio_sources(all: &[RtcStats]) -> impl Iterator<Item = &libwebrtc::stats::MediaSourceStats> {
    all.iter().filter_map(|one| match one {
        RtcStats::MediaSource(source) if source.source.kind == "audio" => Some(source),
        _ => None,
    })
}

#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "clamped to 0..=100 first")]
fn share_of_full_scale(source: &libwebrtc::stats::MediaSourceStats) -> u8 {
    (source.audio.audio_level.clamp(0.0, 1.0) * 100.0).round() as u8
}

/// How loud the sound being sent is, from 0 to 100 as the engine reports its audio level (0 to 1); none when
/// no audio source is reported yet.
pub(crate) fn microphone_level(all: &[RtcStats]) -> Option<u8> {
    audio_sources(all).next().map(share_of_full_scale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use libwebrtc::stats::{InboundRtpStats, MediaSourceStats, TransportStats};

    fn source(kind: &str, level: f64) -> RtcStats {
        let mut one = MediaSourceStats::default();
        one.source.kind = kind.to_string();
        one.audio.audio_level = level;
        RtcStats::MediaSource(one)
    }

    #[test]
    fn the_microphone_level_is_the_audio_sources_as_a_share_of_full_scale() {
        assert_eq!(microphone_level(&[source("video", 0.9), source("audio", 0.25)]), Some(25));
        assert_eq!(microphone_level(&[source("audio", 7.0)]), Some(100), "never over full scale");
        assert_eq!(microphone_level(&[source("audio", 0.0)]), Some(0), "silence is a level, not no reading");
        assert_eq!(microphone_level(&[source("video", 0.5)]), None);
    }

    fn transport(selects: &str) -> RtcStats {
        let mut one = TransportStats::default();
        one.transport.selected_candidate_pair_id = selects.to_string();
        RtcStats::Transport(one)
    }

    fn pair(id: &str, seconds: f64, answered: u64) -> RtcStats {
        let mut one = CandidatePairStats::default();
        one.rtc.id = id.to_string();
        one.candidate_pair.current_round_trip_time = seconds;
        one.candidate_pair.responses_received = answered;
        RtcStats::CandidatePair(one)
    }

    fn audio(kind: &str, received: u64, lost: i64) -> RtcStats {
        let mut one = InboundRtpStats::default();
        one.stream.kind = kind.to_string();
        one.received.packets_received = received;
        one.received.packets_lost = lost;
        one.inbound.bytes_received = received * 100;
        RtcStats::InboundRtp(one)
    }

    fn sent(kind: &str, packets: u64, bytes: u64) -> RtcStats {
        let mut one = OutboundRtpStats::default();
        one.stream.kind = kind.to_string();
        one.sent.packets_sent = packets;
        one.sent.bytes_sent = bytes;
        RtcStats::OutboundRtp(one)
    }

    fn carried(sent: u64, received: u64) -> RtcStats {
        let mut one = TransportStats::default();
        one.transport.bytes_sent = sent;
        one.transport.bytes_received = received;
        RtcStats::Transport(one)
    }

    #[test]
    fn the_video_sent_is_the_one_in_its_own_section_not_a_resting_one() {
        let in_section = |mid: &str, bytes: u64| {
            let RtcStats::OutboundRtp(mut one) = sent("video", 1, bytes) else { panic!("made above") };
            one.outbound.mid = mid.to_string();
            RtcStats::OutboundRtp(one)
        };
        let all = [in_section("1", 900), in_section("0", 5)];
        assert_eq!(outbound_video(&all, "1").map(|out| out.sent.bytes_sent), Some(900));
        assert_eq!(outbound_video(&all, "0").map(|out| out.sent.bytes_sent), Some(5));
        assert!(outbound_video(&all, "2").is_none());
    }

    #[test]
    fn the_round_trip_is_the_selected_pairs_not_another_that_carried_more() {
        let all = [pair("other", 0.9, 5), transport("chosen"), pair("chosen", 0.04, 5)];
        assert_eq!(round_trip(&all), Some(Duration::from_millis(40)));
    }

    #[test]
    fn no_selected_pair_or_an_unmeasured_one_is_no_round_trip() {
        assert_eq!(round_trip(&[pair("a", 0.04, 5)]), None);
        assert_eq!(round_trip(&[transport(""), pair("a", 0.04, 5)]), None);
        assert_eq!(round_trip(&[transport("a"), pair("a", 0.0, 0)]), None);
    }

    #[test]
    fn an_answered_check_that_reads_zero_is_a_round_trip_under_the_engines_resolution() {
        assert_eq!(round_trip(&[transport("a"), pair("a", 0.0, 15)]), Some(Duration::ZERO));
    }

    #[test]
    fn packets_are_summed_over_audio_only_and_a_negative_loss_is_none() {
        let all = [audio("audio", 90, 10), audio("audio", 10, -3), audio("video", 500, 500)];
        assert_eq!(inbound_audio(&all), Inbound { packets: 100, lost: 10, bytes: 10_000 });
    }

    #[test]
    fn every_stream_received_is_counted_beside_the_audio() {
        let all = [audio("audio", 90, 10), audio("video", 500, 5)];
        assert_eq!(inbound_every(&all), Inbound { packets: 590, lost: 15, bytes: 59_000 });
        assert_eq!(inbound_every(&[]), Inbound::default());
    }

    #[test]
    fn what_was_sent_is_summed_over_every_stream() {
        let all = [sent("audio", 40, 4_000), sent("audio", 2, 200), sent("video", 900, 900_000)];
        assert_eq!(outbound_every(&all), (942, 904_200));
        assert_eq!(outbound_every(&[sent("video", 1, 1)]), (1, 1));
    }

    #[test]
    fn the_transports_carried_are_summed_whatever_was_in_them() {
        assert_eq!(transport_bytes(&[carried(10, 20), carried(1, 2), sent("audio", 9, 9)]), (11, 22));
        assert_eq!(transport_bytes(&[]), (0, 0));
    }
}
