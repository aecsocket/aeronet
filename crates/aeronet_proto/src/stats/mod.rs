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

pub struct SessionStats {
    sample_rate: u32,
    last_bytes_sent: usize,
    last_bytes_recv: usize,
    samples: HeapRb<Sample>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sample {
    pub rtt: Duration,
    pub conservative_rtt: Duration,
    pub memory_usage: usize,
    pub bytes_used: usize,
    pub tx: usize,
    pub rx: usize,
}

impl SessionStats {
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

    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    #[must_use]
    pub fn into_samples(self) -> HeapRb<Sample> {
        self.samples
    }

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
