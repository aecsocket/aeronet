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
    last_bytes_sent: usize,
    last_bytes_recv: usize,
    samples: HeapRb<Sample>,
}

/// A single sample of data stored in [`SessionStats`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sample {
    /// Smoothed session RTT.
    pub rtt: Duration,
    /// Conservative session RTT.
    pub conservative_rtt: Duration,
    /// Number of bytes of memory used for buffering messages.
    pub memory_usage: usize,
    /// Number of send bytes used.
    pub bytes_used: usize,
    /// How many bytes were sent between this and the previous sample.
    pub tx: usize,
    /// How many bytes were received between this and the previous sample.
    pub rx: usize,
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
    #[must_use]
    pub fn new(sample_rate: u32, history: usize) -> Self {
        let freq_u = usize::try_from(sample_rate).expect("`sample_rate` must fit into a usize");
        Self {
            sample_rate,
            last_bytes_sent: 0,
            last_bytes_recv: 0,
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
        let bytes_sent = session.bytes_sent();
        let tx = session.bytes_sent().saturating_sub(self.last_bytes_sent);
        self.last_bytes_sent = bytes_sent;

        let bytes_recv = session.bytes_recv();
        let rx = session.bytes_recv().saturating_sub(self.last_bytes_recv);
        self.last_bytes_recv = bytes_recv;

        self.samples.push_overwrite(Sample {
            rtt: session.rtt().get(),
            conservative_rtt: session.rtt().conservative(),
            memory_usage: session.memory_usage(),
            bytes_used: session.bytes_left().used(),
            tx,
            rx,
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
