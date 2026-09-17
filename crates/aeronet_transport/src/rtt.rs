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
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "dividing a Duration by the nonzero constant 2 cannot panic"
    )]
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
        self.get()
            .saturating_add(self.var.saturating_mul(4).max(TIMER_GRANULARITY))
    }

    /// Adds an RTT sample to this estimation.
    pub fn update(&mut self, rtt: Duration) {
        self.latest = rtt;
        self.min = self.min.min(rtt);

        let var_sample = self.smoothed.abs_diff(rtt);
        self.var = weighted_average(self.var, var_sample, 3);
        self.smoothed = weighted_average(self.smoothed, rtt, 7);
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

// Compute the weighted mean in nanoseconds to preserve rounding without
// overflowing Duration's narrower seconds representation.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "Duration nanoseconds times a u32 weight fit in u128, and weight + 1 is nonzero"
)]
fn weighted_average(previous: Duration, sample: Duration, previous_weight: u32) -> Duration {
    let weight = u128::from(previous_weight);
    let nanos = (previous.as_nanos() * weight + sample.as_nanos()) / (weight + 1);
    Duration::new(
        u64::try_from(nanos / 1_000_000_000)
            .expect("a weighted average cannot exceed Duration::MAX"),
        u32::try_from(nanos % 1_000_000_000).expect("subsecond nanoseconds fit in u32"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_preserves_weighted_estimate() {
        let mut rtt = RttEstimator::new(Duration::from_millis(100));
        rtt.update(Duration::from_millis(200));
        assert_eq!(rtt.get(), Duration::from_micros(112_500));
        assert_eq!(rtt.pto(), Duration::from_micros(362_500));
    }

    #[test]
    fn weighted_average_preserves_nanosecond_rounding() {
        for previous in 0..16 {
            for sample in 0..16 {
                for weight in [3, 7] {
                    assert_eq!(
                        weighted_average(
                            Duration::from_nanos(previous),
                            Duration::from_nanos(sample),
                            weight
                        ),
                        Duration::from_nanos(
                            (previous * u64::from(weight) + sample) / u64::from(weight + 1)
                        )
                    );
                }
            }
        }
    }

    #[test]
    fn maximum_duration_does_not_overflow() {
        let mut rtt = RttEstimator::new(Duration::MAX);
        assert_eq!(rtt.pto(), Duration::MAX);
        rtt.update(Duration::MAX);
        assert_eq!(rtt.get(), Duration::MAX);
        assert_eq!(rtt.pto(), Duration::MAX);
        rtt.update(Duration::ZERO);
        assert!(rtt.get() < Duration::MAX);
        assert_eq!(
            weighted_average(Duration::MAX, Duration::MAX, u32::MAX),
            Duration::MAX
        );
    }
}
