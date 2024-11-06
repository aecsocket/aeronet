//! See [`RttEstimator`].

use {core::time::Duration, typesize::derive::TypeSize};

/// Computes an RTT estimation for a network path.
///
/// This is based on [`quinn-proto`'s `RttEstimator`](https://github.com/quinn-rs/quinn/blob/411abe9/quinn-proto/src/connection/paths.rs#L151).
#[derive(Debug, Clone, TypeSize)]
pub struct RttEstimator {
    latest: Duration,
    smoothed: Duration,
    var: Duration,
    min: Duration,
}

const TIMER_GRANULARITY: Duration = Duration::from_millis(1);

impl RttEstimator {
    /// Creates a new estimator from a given initial RTT.
    #[must_use]
    pub fn new(initial_rtt: Duration) -> Self {
        Self {
            latest: initial_rtt,
            smoothed: initial_rtt,
            var: initial_rtt / 2,
            min: initial_rtt,
        }
    }

    /// Gets the current best RTT estimation.
    #[must_use]
    pub const fn get(&self) -> Duration {
        self.smoothed
    }

    /// Gets a conservative estimate of RTT.
    ///
    /// Takes the maximum of smoothed and latest RTT, as recommended
    /// in 6.1.2 of the recovery spec (draft 29).
    #[must_use]
    pub fn conservative(&self) -> Duration {
        self.get().max(self.latest)
    }

    /// Gets the minimum RTT registered so far for this estimator.
    ///
    /// # Examples
    ///
    /// ```
    /// # use {aeronet_transport::rtt::RttEstimator, core::time::Duration};
    /// let mut rtt = RttEstimator::new(Duration::from_millis(500));
    /// assert_eq!(Duration::from_millis(500), rtt.min());
    ///
    /// rtt.update(Duration::from_millis(750));
    /// assert_eq!(Duration::from_millis(500), rtt.min());
    ///
    /// rtt.update(Duration::from_millis(250));
    /// assert_eq!(Duration::from_millis(250), rtt.min());
    /// ```
    #[must_use]
    pub const fn min(&self) -> Duration {
        self.min
    }

    /// Computes the probe timeout duration (PTO) as described in
    /// [RFC 9002 Section 6.2.1].
    ///
    /// [RFC 9002 Section 6.2.1]: https://www.rfc-editor.org/rfc/rfc9002.html#section-6.2.1
    #[must_use]
    pub fn pto(&self) -> Duration {
        self.get() + (self.var * 4).max(TIMER_GRANULARITY)
    }

    /// Adds an RTT sample to this estimation.
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

/// Default initial RTT to use for [`RttEstimator`] before any RTT samples have
/// been provided.
///
/// This value is based on [RFC 9002 Section 6.2.2].
///
/// [RFC 9002 Section 6.2.2]: https://www.rfc-editor.org/rfc/rfc9002.html#section-6.2.2-1
pub const DEFAULT_INITIAL_RTT: Duration = Duration::from_millis(333);

impl Default for RttEstimator {
    fn default() -> Self {
        Self::new(DEFAULT_INITIAL_RTT)
    }
}
