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

use crate::session::Session;

/// Stores network statistics collected from a [`Session`].
pub struct SessionStats {
    sample_rate: u32,
    samples: HeapRb<Sample>,
}

/// A single sample of data stored in [`SessionStats`].
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    /// Smoothed session RTT.
    pub rtt: Duration,
    /// Conservative session RTT.
    pub conservative_rtt: Duration,
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
    pub fn update(&mut self, session: &Session) {
        let rtt = session.rtt().get();
        let conservative_rtt = session.rtt().conservative();
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

        /*
        threshold is RTT
        we are computing sample 100
        sample 90 was one threshold ago
        between samples 89 and 90:
        - we sent out 10 packets
        - making a total of 1000 packets sent
        - we had 950 packets acked
        at sample 100, we expect to have 10 more acks than there were in sample 90
        - we expect to have 960 packets acked
        */

        // TODO: this code produces a really inaccurate packet loss estimate
        // I need a better algo

        // Gaffer on Games uses the smoothed RTT
        // I find this too strict and use PTO instead

        let thresh = session.rtt().get();
        let thresh_index = f64::ceil(thresh.as_secs_f64() * f64::from(self.sample_rate()));
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
            0.0
        } else {
            let acked_frac = acks_since_then as f64 / expected_extra_acks as f64;
            1.0 - acked_frac.clamp(0.0, 1.0)
        };

        self.samples.push_overwrite(Sample {
            rtt,
            conservative_rtt,
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
