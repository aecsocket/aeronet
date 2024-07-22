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
    pub bytes_sent: usize,
    /// Total number of bytes received up to now.
    pub bytes_recv: usize,
    /// Total number of packets sent up to now.
    pub packets_sent: usize,
    /// Total number of packets received up to now.
    pub packets_recv: usize,
    /// Total number of our sent packets that have been acknowledged by the peer
    /// up to now.
    pub packets_acked: usize,
    /// How many bytes were sent between this and the previous sample.
    pub tx: usize,
    /// How many bytes were received between this and the previous sample.
    pub rx: usize,
    /// What proportion of packets sent recently are believed to have been lost
    /// in transit.
    ///
    /// If the receiver has not acknowledged a packet within `rtt`, then they
    /// have probably lost that packet.
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
        let (last_bytes_sent, last_bytes_recv) = self
            .samples
            .iter()
            .next_back()
            .map(|sample| (sample.bytes_sent, sample.bytes_recv))
            .unwrap_or_default();
        let (tx, rx) = (
            session.bytes_sent() - last_bytes_sent,
            session.bytes_recv() - last_bytes_recv,
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

        // Gaffer on Games uses the smoothed RTT
        // I find this too strict and use PTO instead
        let lost_thresh = session.rtt().pto();
        let (tx_at_thresh, packets_acked_at_thresh) = self
            .samples
            .iter()
            .rev()
            .nth((lost_thresh.as_secs_f64() * f64::from(self.sample_rate)) as usize)
            .map(|sample| (sample.tx, sample.packets_acked))
            .unwrap_or_default();
        let expected_acked_now = packets_acked_at_thresh + tx_at_thresh;

        let loss = if expected_acked_now == 0 {
            0.0
        } else {
            let acked_frac = session.packets_acked() as f64 / expected_acked_now as f64;
            1.0 - acked_frac.clamp(0.0, 1.0)
        };

        self.samples.push_overwrite(Sample {
            rtt: session.rtt().get(),
            conservative_rtt: session.rtt().conservative(),
            memory_usage: session.memory_usage(),
            bytes_used: session.bytes_left().used(),
            bytes_sent: session.bytes_sent(),
            bytes_recv: session.bytes_recv(),
            packets_sent: session.packets_sent(),
            packets_recv: session.packets_recv(),
            packets_acked: session.packets_acked(),
            tx,
            rx,
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
