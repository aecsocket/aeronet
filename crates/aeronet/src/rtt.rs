//! Tools for estimating the round-trip time of data transfer along a [session].
//!
//! TODO rtt explanation
//!
//! [session]: crate::session

use std::time::Duration;

use bevy_reflect::prelude::*;

#[derive(Debug, Clone, Reflect)]
#[doc(alias = "ping")]
#[doc(alias = "latency")]
pub struct RttEstimator {
    latest: Duration,
    smoothed: Duration,
    jitter: Duration,
    min: Duration,
}

impl Default for RttEstimator {
    fn default() -> Self {
        Self::new(Duration::from_millis(333))
    }
}

impl RttEstimator {
    #[must_use]
    pub fn new(initial_rtt: Duration) -> Self {
        Self {
            latest: initial_rtt,
            smoothed: initial_rtt,
            jitter: initial_rtt / 2,
            min: initial_rtt,
        }
    }

    #[must_use]
    pub const fn get(&self) -> Duration {
        self.smoothed
    }

    #[must_use]
    pub fn conservative(&self) -> Duration {
        self.smoothed.max(self.latest)
    }

    #[must_use]
    pub const fn min(&self) -> Duration {
        self.min
    }

    #[must_use]
    pub fn pto(&self) -> Duration {
        self.get() + 4 * self.jitter
    }

    pub fn update(&mut self, rtt: Duration) {
        self.latest = rtt;
        self.min = self.min.min(rtt);

        let var_sample = if self.smoothed > rtt {
            self.smoothed - rtt
        } else {
            rtt - self.smoothed
        };
        self.jitter = (3 * self.jitter + var_sample) / 4;
        self.smoothed = (7 * self.smoothed + rtt) / 8;
    }
}
