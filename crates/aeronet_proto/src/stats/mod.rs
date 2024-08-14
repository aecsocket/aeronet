//! See [`SessionStats`].

#[cfg(feature = "bevy")]
mod bevy;

use std::ops::Deref;

#[cfg(feature = "bevy")]
pub use bevy::*;

use ringbuf::{
    traits::{Consumer, RingBuffer},
    HeapRb,
};
use web_time::Duration;

use crate::session::{RttStrategy, Session};

/// Stores network statistics collected from a [`Session`].
///
/// See [`Sample`] for a description of what network statistics are tracked.
pub struct SessionStats {
    sample_rate: u32,
    samples: HeapRb<Sample>,
}

/// A single sample of data stored in [`SessionStats`].
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    /// Smoothed session RTT.
    pub rtt: Duration,
    /// Number of bytes of memory used for buffering messages.
    pub memory_usage: usize,
    /// Number of send bytes used.
    pub bytes_used: usize,

    /// Total number of bytes sent up to now.
    pub bytes_sent_total: usize,
    /// Number of bytes sent between the last sample and this.
    pub bytes_sent_delta: usize,

    /// Total number of bytes received up to now.
    pub bytes_recv_total: usize,
    /// Number of bytes received between the last sample and this.
    pub bytes_recv_delta: usize,

    /// Total number of packets sent up to now.
    pub packets_sent_total: usize,
    /// Number of packets sent between the last sample and this.
    pub packets_sent_delta: usize,

    /// Total number of packets received up to now.
    pub packets_recv_total: usize,
    /// Number of packets received between the last sample and this.
    pub packets_recv_delta: usize,

    /// Total number of our sent packets that have been acknowledged by the peer
    /// up to now.
    pub packets_acked_total: usize,
    /// Number of sent packets that have between acknowledged by the peer
    /// between the last sample and this.
    pub packets_acked_delta: usize,

    /// What proportion of packets sent recently are believed to have been lost
    /// in transit.
    ///
    /// If the receiver has not acknowledged a packet within a variable time
    /// threshold (which is a function of the RTT), then they have probably lost
    /// that packet.
    ///
    /// # Algorithm
    ///
    /// We want to figure out how many packets have been lost during this
    /// sample. To do this, we find out how many packets, that we sent out
    /// earlier, should have been acknowledged by our peer by now; and how many
    /// of those have actually been acknowledged. "By now" is defined as a
    /// function of the current RTT estimate. Currently it is just [the PTO],
    /// however the implementation may change this in the future.
    ///
    /// Let's assume that we are calculating sample 100, and our RTT is such
    /// that we expect to have received acknowledgements for all packets sent
    /// out in sample 90 by now.
    /// In sample 90 (between samples 89 and 90) we sent out 10 packets, and up
    /// to that point we had received 950 total acknowledgements from our peer.
    /// Therefore, at sample 100, we expect to have received 950 + 10 = 960
    /// total acknowledgements.
    ///
    /// - If by now we have received 960 acknowledgements, then we have 0%
    ///   packet loss, and our RTT estimate is very accurate.
    /// - If we have more than 960 acknowledgements, our packet loss is still
    ///   0%, but our RTT estimate is too high, and the peer actually
    ///   acknowledges packets faster than we think.
    /// - If we have between 950 and 960 acknowledgements, we have some
    ///   percentage of packet loss i.e. 959 acks means 10% packet loss.
    /// - If we still only have 950 acks, we have 100% packet loss.
    ///
    /// [the PTO]: crate::rtt::RttEstimator::pto
    pub loss: f64,
}

impl SessionStats {
    /// Creates a new stats tracker.
    ///
    /// `sample_rate` defines how often you intend to call
    /// [`SessionStats::update`] on this value, inserting a new sample. The
    /// value is given in hertz (samples per second).
    ///
    /// `history` defines how many seconds of history this buffer should keep
    /// before overwriting old entries.
    ///
    /// # Panics
    ///
    /// Panics if `sample_rate` cannot fit into a [`usize`].
    #[must_use]
    pub fn new(sample_rate: u32, history: usize) -> Self {
        let freq_u = usize::try_from(sample_rate).expect("`sample_rate` must fit into a usize");
        Self {
            sample_rate,
            samples: HeapRb::new(freq_u * history),
        }
    }

    /// Gets the sample rate.
    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Converts this into the raw samples.
    #[must_use]
    pub fn into_samples(self) -> HeapRb<Sample> {
        self.samples
    }

    /// Inserts a new sample into this stats tracker by reading the state of a
    /// [`Session`].
    pub fn update<R: RttStrategy>(&mut self, session: &Session<R>) {
        let rtt = session.rtt();
        let memory_usage = session.memory_usage();
        let bytes_used = session.bytes_left().used();

        let (
            bytes_sent_total,
            bytes_recv_total,
            packets_sent_total,
            packets_recv_total,
            packets_acked_total,
        ) = (
            session.bytes_sent(),
            session.bytes_recv(),
            session.packets_sent(),
            session.packets_recv(),
            session.packets_acked(),
        );

        let (
            bytes_sent_last,
            bytes_recv_last,
            packets_sent_last,
            packets_recv_last,
            packets_acked_last,
            loss_last,
        ) = self
            .samples
            .iter()
            .next_back()
            .map(|sample| {
                (
                    sample.bytes_sent_total,
                    sample.bytes_recv_total,
                    sample.packets_sent_total,
                    sample.packets_recv_total,
                    sample.packets_acked_total,
                    sample.loss,
                )
            })
            .unwrap_or_default();

        let (
            bytes_sent_delta,
            bytes_recv_delta,
            packets_sent_delta,
            packets_recv_delta,
            packets_acked_delta,
        ) = (
            bytes_sent_total - bytes_sent_last,
            bytes_recv_total - bytes_recv_last,
            packets_sent_total - packets_sent_last,
            packets_recv_total - packets_recv_last,
            packets_acked_total - packets_acked_last,
        );

        let thresh = session.rtt().pto();
        let thresh_index = (thresh.as_secs_f64() * f64::from(self.sample_rate())).ceil();
        let thresh_index = thresh_index as usize;

        // number of packets sent `thresh` ago = how many extra acks we should have now
        let (expected_extra_acks, packets_acked_then) = self
            .samples
            .iter()
            .rev()
            .nth(thresh_index)
            .map(|sample| (sample.packets_sent_delta, sample.packets_acked_total))
            .unwrap_or_default();
        let packets_acked_now = session.packets_acked();
        let acks_since_then = packets_acked_now - packets_acked_then;

        let loss = if expected_extra_acks == 0 {
            // reuse the previous sample (or 0.0)
            loss_last
        } else {
            let acked_frac = acks_since_then as f64 / expected_extra_acks as f64;
            1.0 - acked_frac.clamp(0.0, 1.0)
        };

        self.samples.push_overwrite(Sample {
            rtt,
            memory_usage,
            bytes_used,
            bytes_sent_total,
            bytes_sent_delta,
            bytes_recv_total,
            bytes_recv_delta,
            packets_sent_total,
            packets_sent_delta,
            packets_recv_total,
            packets_recv_delta,
            packets_acked_total,
            packets_acked_delta,
            loss,
        });
    }

    /// Clears all samples.
    pub fn clear(&mut self) {
        self.samples.clear();
    }
}

impl Deref for SessionStats {
    type Target = HeapRb<Sample>;

    fn deref(&self) -> &Self::Target {
        &self.samples
    }
}
