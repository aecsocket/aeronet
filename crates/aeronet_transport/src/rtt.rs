use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct RttEstimator {
    latest: Duration,
    smoothed: Duration,
    var: Duration,
    min: Duration,
}

const TIMER_GRANULARITY: Duration = Duration::from_millis(1);

impl RttEstimator {
    #[must_use]
    pub fn new(initial_rtt: Duration) -> Self {
        Self {
            latest: initial_rtt,
            smoothed: initial_rtt,
            var: initial_rtt / 2,
            min: initial_rtt,
        }
    }

    #[must_use]
    pub const fn get(&self) -> Duration {
        self.smoothed
    }

    #[must_use]
    pub fn conservative(&self) -> Duration {
        self.get().max(self.latest)
    }

    #[must_use]
    pub fn pto(&self) -> Duration {
        self.get() + (self.var * 4).max(TIMER_GRANULARITY)
    }

    pub fn update(&mut self, rtt: Duration) {
        self.latest = rtt;
        self.min = self.min.min(rtt);

        let var_sample = if self.smoothed > rtt {
            self.smoothed - rtt
        } else {
            rtt - self.smoothed
        };
        self.var = (3 * self.var + var_sample) / 4;
        self.smoothed = (7 * self.smoothed + rtt) / 8;
    }
}

impl Default for RttEstimator {
    fn default() -> Self {
        Self::new(Duration::from_millis(333))
    }
}
