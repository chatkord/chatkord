//! What a relay does to what it carries. A schedule acts on the server's video on its way down to a client: one packet
//! late behind those after it, one lost for good, one held, then a growing delay; the first stream it sees that is not
//! mediasoup's probe is the video, and every other is a retransmission. A loss drops RTP by chance one way. Both read
//! RTP headers alone (SRTP leaves them in the clear) and write nothing.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// mediasoup's bandwidth probe, sent before and beside the media: never touched.
const PROBATION: u8 = 127;

/// Where in the video, counted in packets from the first seen, each impairment starts, and for how long.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Schedule {
    /// A packet dropped, and every retransmission for `late_by` after held until then: its frame is completed after
    /// the frames that follow it.
    pub late_at: u16,
    /// How long the retransmissions after `late_at` are held.
    pub late_by: Duration,
    /// A packet dropped, and every retransmission for `lost_for` after dropped too: its frame never comes.
    pub lost_at: u16,
    /// How long retransmissions are dropped after `lost_at`.
    pub lost_for: Duration,
    /// A packet held back by `held_by`, so the next ones overtake it.
    pub held_at: u16,
    /// How long `held_at` is held.
    pub held_by: Duration,
    /// From here on every packet is delayed, by a share of `drift_to` growing over `drift_over` and then kept.
    pub drift_at: u16,
    /// The delay the drift grows to.
    pub drift_to: Duration,
    /// How long it takes to grow.
    pub drift_over: Duration,
}

/// What a schedule did, so a test knows each part of it ran.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Impaired {
    /// Whether the packet at `late_at` was dropped.
    pub late: bool,
    /// Retransmissions held after it.
    pub late_retransmissions: usize,
    /// Whether the packet at `lost_at` was dropped.
    pub lost: bool,
    /// Retransmissions dropped after it.
    pub lost_retransmissions: usize,
    /// Whether the packet at `held_at` was held.
    pub held: bool,
    /// Packets delayed by the drift.
    pub drifted: usize,
}

/// Which way a datagram crosses the relay.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Way {
    /// From a client to the server.
    Up,
    /// From the server to a client.
    Down,
}

/// RTP lost by chance one way, as a lossy path loses it: STUN, DTLS and RTCP pass, so the call stays up.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Loss {
    /// The way it is lost on.
    pub way: Way,
    /// How many packets in a thousand are lost.
    pub per_thousand: u16,
    /// Where the draw starts, so a run can be repeated.
    pub seed: u64,
}

/// What a loss did: packets of RTP that crossed its way, and those it lost.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lost {
    /// RTP packets that reached the relay on the loss's way.
    pub seen: usize,
    /// Those dropped.
    pub lost: usize,
}

/// A loss under way.
#[derive(Debug)]
pub(crate) struct Losing {
    loss: Loss,
    draw: u64,
    done: Lost,
}

impl Losing {
    pub(crate) fn new(loss: Loss) -> Self {
        // xorshift's one fixed point is zero.
        Self { loss, draw: loss.seed | 1, done: Lost::default() }
    }

    pub(crate) fn done(&self) -> Lost {
        self.done
    }

    /// Whether `datagram`, crossing `way`, is lost.
    pub(crate) fn verdict(&mut self, way: Way, datagram: &[u8]) -> Verdict {
        if way != self.loss.way || Rtp::read(datagram).is_none() {
            return Verdict::Pass;
        }
        self.done.seen += 1;
        // xorshift64*: even enough for a share of packets, and repeatable from its seed.
        self.draw ^= self.draw >> 12;
        self.draw ^= self.draw << 25;
        self.draw ^= self.draw >> 27;
        let drawn = self.draw.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 32;
        if drawn % 1000 < u64::from(self.loss.per_thousand) {
            self.done.lost += 1;
            return Verdict::Drop;
        }
        Verdict::Pass
    }
}

/// What becomes of one datagram.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    Pass,
    Drop,
    /// Sent once this moment comes.
    Hold(Instant),
}

/// A schedule under way.
#[derive(Debug)]
pub(crate) struct Impairing {
    schedule: Schedule,
    video: Option<(u32, u16)>,
    late_since: Option<Instant>,
    lost_since: Option<Instant>,
    drift_since: Option<Instant>,
    done: Impaired,
}

/// The parts of an RTP header the schedule reads.
struct Rtp {
    payload_type: u8,
    sequence: u16,
    ssrc: u32,
}

impl Rtp {
    /// An RTP packet's header; none for STUN, DTLS or RTCP, which RFC 7983 and 5761 tell apart by their first bytes.
    fn read(datagram: &[u8]) -> Option<Rtp> {
        let (first, second) = (*datagram.first()?, *datagram.get(1)?);
        let payload_type = second & 0x7f;
        if datagram.len() < 12 || first >> 6 != 2 || (64..=95).contains(&payload_type) {
            return None;
        }
        let sequence = u16::from_be_bytes([datagram[2], datagram[3]]);
        let ssrc = u32::from_be_bytes([datagram[8], datagram[9], datagram[10], datagram[11]]);
        Some(Rtp { payload_type, sequence, ssrc })
    }
}

impl Impairing {
    pub(crate) fn new(schedule: Schedule) -> Self {
        Self { schedule, video: None, late_since: None, lost_since: None, drift_since: None, done: Impaired::default() }
    }

    pub(crate) fn done(&self) -> Impaired {
        self.done
    }

    /// What becomes of `datagram`, arriving at `now` on its way to a client.
    pub(crate) fn verdict(&mut self, datagram: &[u8], now: Instant) -> Verdict {
        let Some(rtp) = Rtp::read(datagram) else { return Verdict::Pass };
        if rtp.payload_type == PROBATION {
            return Verdict::Pass;
        }
        let (video, first) = *self.video.get_or_insert((rtp.ssrc, rtp.sequence));
        if rtp.ssrc != video {
            return self.retransmission(now);
        }
        let at = rtp.sequence.wrapping_sub(first);
        let s = self.schedule;
        if at == s.late_at && !self.done.late {
            self.done.late = true;
            self.late_since = Some(now);
            return Verdict::Drop;
        }
        if at == s.lost_at && !self.done.lost {
            self.done.lost = true;
            self.lost_since = Some(now);
            return Verdict::Drop;
        }
        if at == s.held_at && !self.done.held {
            self.done.held = true;
            return Verdict::Hold(now + s.held_by);
        }
        // Past the drift's start, and not a wrapped sequence from before the first packet.
        if at >= s.drift_at && at < u16::MAX / 2 {
            let since = *self.drift_since.get_or_insert(now);
            let share = (now.saturating_duration_since(since).as_secs_f64() / s.drift_over.as_secs_f64()).min(1.0);
            self.done.drifted += 1;
            return Verdict::Hold(now + s.drift_to.mul_f64(share));
        }
        Verdict::Pass
    }

    fn retransmission(&mut self, now: Instant) -> Verdict {
        let s = self.schedule;
        if let Some(since) = self.late_since
            && now < since + s.late_by
        {
            self.done.late_retransmissions += 1;
            return Verdict::Hold(since + s.late_by);
        }
        if let Some(since) = self.lost_since
            && now < since + s.lost_for
        {
            self.done.lost_retransmissions += 1;
            return Verdict::Drop;
        }
        Verdict::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIDEO: u32 = 0x1111_1111;
    const RETRANSMISSION: u32 = 0x2222_2222;

    fn rtp(payload_type: u8, sequence: u16, ssrc: u32) -> Vec<u8> {
        let mut packet = vec![0x80, payload_type];
        packet.extend(sequence.to_be_bytes());
        packet.extend([0; 4]);
        packet.extend(ssrc.to_be_bytes());
        packet.extend([0xaa; 20]);
        packet
    }

    fn schedule() -> Schedule {
        Schedule {
            late_at: 2,
            late_by: Duration::from_millis(100),
            lost_at: 4,
            lost_for: Duration::from_secs(2),
            held_at: 6,
            held_by: Duration::from_millis(120),
            drift_at: 8,
            drift_to: Duration::from_millis(150),
            drift_over: Duration::from_secs(5),
        }
    }

    #[test]
    fn each_part_of_the_schedule_falls_on_its_packet_and_the_rest_pass() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut impairing = Impairing::new(schedule());
        let mut verdict = |packet: Vec<u8>, ms| impairing.verdict(&packet, at(ms));
        assert_eq!(verdict(rtp(PROBATION, 900, 1234), 0), Verdict::Pass, "the probe is not the video");
        assert_eq!(verdict(rtp(100, 10, VIDEO), 0), Verdict::Pass);
        assert_eq!(verdict(rtp(100, 11, VIDEO), 10), Verdict::Pass);
        assert_eq!(verdict(rtp(100, 12, VIDEO), 20), Verdict::Drop, "late");
        assert_eq!(verdict(rtp(101, 1, RETRANSMISSION), 30), Verdict::Hold(at(120)), "its retransmission waits");
        assert_eq!(verdict(rtp(101, 2, RETRANSMISSION), 130), Verdict::Pass, "a later one does not");
        assert_eq!(verdict(rtp(100, 14, VIDEO), 200), Verdict::Drop, "lost");
        assert_eq!(verdict(rtp(101, 3, RETRANSMISSION), 300), Verdict::Drop, "and its retransmission");
        assert_eq!(verdict(rtp(100, 16, VIDEO), 2_300), Verdict::Hold(at(2_420)), "held");
        assert_eq!(verdict(rtp(101, 4, RETRANSMISSION), 2_300), Verdict::Pass, "the loss is over");
        assert_eq!(verdict(rtp(100, 18, VIDEO), 3_000), Verdict::Hold(at(3_000)), "the drift starts at nothing");
        assert_eq!(verdict(rtp(100, 19, VIDEO), 5_500), Verdict::Hold(at(5_575)), "grows");
        assert_eq!(verdict(rtp(100, 20, VIDEO), 9_000), Verdict::Hold(at(9_150)), "and is kept");
        let done = impairing.done();
        assert!(done.late && done.lost && done.held);
        assert_eq!((done.late_retransmissions, done.lost_retransmissions, done.drifted), (1, 1, 3));
    }

    #[test]
    fn a_loss_drops_its_share_of_rtp_one_way_repeatably_and_nothing_else() {
        let run = |seed| {
            let mut losing = Losing::new(Loss { way: Way::Up, per_thousand: 20, seed });
            let verdicts: Vec<Verdict> = (0..10_000u16).map(|n| losing.verdict(Way::Up, &rtp(100, n, VIDEO))).collect();
            (verdicts, losing.done())
        };
        let (first, done) = run(7);
        assert_eq!(done.seen, 10_000);
        assert!((150..=250).contains(&done.lost), "about 2 % lost: {}", done.lost);
        assert_eq!(first, run(7).0, "the same seed loses the same packets");
        assert_ne!(first, run(8).0, "another seed, others");
        let mut losing = Losing::new(Loss { way: Way::Up, per_thousand: 1000, seed: 1 });
        assert_eq!(losing.verdict(Way::Down, &rtp(100, 1, VIDEO)), Verdict::Pass, "the other way");
        let rtcp = [0x80, 200, 0, 6, 0, 0, 0, 1, 0, 0, 0, 0];
        assert_eq!(losing.verdict(Way::Up, &rtcp), Verdict::Pass, "not RTP");
        assert_eq!(losing.verdict(Way::Up, &rtp(100, 2, VIDEO)), Verdict::Drop);
    }

    #[test]
    fn what_is_not_rtp_passes_untouched() {
        let mut impairing = Impairing::new(Schedule { late_at: 0, ..schedule() });
        let stun = [0x00, 0x01, 0, 0, 0x21, 0x12, 0xa4, 0x42, 0, 0, 0, 0];
        let rtcp = [0x80, 200, 0, 6, 0, 0, 0, 1, 0, 0, 0, 0];
        for datagram in [&stun[..], &rtcp[..], &[0x16, 0xfe, 0xfd][..]] {
            assert_eq!(impairing.verdict(datagram, Instant::now()), Verdict::Pass);
        }
        assert!(!impairing.done().late, "nothing above was the video");
    }
}
