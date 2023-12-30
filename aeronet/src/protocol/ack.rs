use std::{cmp::Ordering, time::Instant};

pub const DEFAULT_MTU: usize = 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Ord)]
pub struct Seq(pub u16);

impl PartialOrd for Seq {
    /// Gets if the sequence number `self` is logically greater than the
    /// sequence number `other`.
    ///
    /// See https://gafferongames.com/post/reliability_ordering_and_congestion_avoidance_over_udp/,
    /// *Handling Sequence Number Wrap-Around*.
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        const HALF: u16 = u16::MAX / 2;

        let s1 = self.0;
        let s2 = other.0;

        if s1 == s2 {
            return Some(Ordering::Equal);
        }

        if ((s1 > s2) && (s1 - s2 <= HALF)) || (s1 < s2 && (s2 - s1 > HALF)) {
            Some(Ordering::Greater)
        } else {
            Some(Ordering::Less)
        }
    }
}

// todo docs
/// See:
/// * https://gafferongames.com/post/reliable_ordered_messages
/// * https://gafferongames.com/post/reliability_ordering_and_congestion_avoidance_over_udp/,
///   *Reliable Acks*
pub struct PacketHeader {
    /// The sequence number of the current packet.
    pub seq: Seq,
    /// The most recent sequence number received.
    ///
    /// `ack_bits` works relative to this number.
    pub ack: Seq,
    /// A bit set of acked packets.
    ///
    /// If bit `n` is set starting from `n = 0`, then `ack - (n + 1)` has been
    /// acked. For example:
    /// * if the last acked packet was number 300 (`ack: 300`)
    /// * and the five packets before that were also acked (294 to 299)
    /// * but no packets before 294 were acked
    ///
    /// This would hold the value `0b00[..]0011111`.
    pub ack_bits: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct PacketData {
    pub sent_at: Instant,
    pub state: PacketState,
}

#[derive(Debug, Clone, Copy)]
pub enum PacketState {
    Nack,
    Ack,
}

#[derive(Debug, Clone)]
pub struct LaneWriter<const CAP: usize> {
    seq: Seq,
    seq_buf: [Seq; CAP],
    packets: [Option<PacketData>; CAP],
}

impl<const CAP: usize> LaneWriter<CAP> {
    pub fn new() -> Self {
        assert!(CAP > 0);

        Self {
            seq: Seq(0),
            seq_buf: [Seq(0); CAP],
            packets: [None; CAP],
        }
    }

    pub fn get(&self, seq: Seq) -> Option<PacketData> {
        let index = usize::from(seq.0) % CAP;
        if self.seq_buf[index] == seq {
            self.packets[index]
        } else {
            None
        }
    }

    pub fn insert(&mut self, seq: Seq, data: PacketData) {
        let index = usize::from(seq.0) % CAP;
        self.seq_buf[index] = seq;
        self.packets[index] = Some(data);
    }
}
