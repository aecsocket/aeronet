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
    update_freq: u32,
    samples: HeapRb<Sample>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sample {
    pub rtt: Duration,
    pub rtt_conservative: Duration,
    pub memory_usage: usize,
}

impl SessionStats {
    #[must_use]
    pub fn new(update_freq: u32, history: usize) -> Self {
        let freq_u = usize::try_from(update_freq).expect("`update_freq` must fit into a usize");
        Self {
            update_freq,
            samples: HeapRb::new(freq_u * history),
        }
    }

    #[must_use]
    pub const fn update_freq(&self) -> u32 {
        self.update_freq
    }

    #[must_use]
    pub fn into_samples(self) -> HeapRb<Sample> {
        self.samples
    }

    pub fn update(&mut self, session: &Session) {
        self.samples.push_overwrite(Sample {
            rtt: session.rtt().get(),
            rtt_conservative: session.rtt().conservative(),
            memory_usage: session.memory_usage(),
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
