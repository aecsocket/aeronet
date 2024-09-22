//! See [`RttEstimator`].

use {std::cmp, web_time::Duration};

/// Default initial RTT to use before any RTT samples have been provided.
///
/// This value is based on [RFC 9002 Section 6.2.2].
///
/// [RFC 9002 Section 6.2.2]: https://www.rfc-editor.org/rfc/rfc9002.html#section-6.2.2-1
pub const INITIAL_RTT: Duration = Duration::from_millis(333);

/// Computes an RTT estimation for a particular network path.
///
/// This is based on [`quinn-proto`'s `RttEstimator`](https://github.com/quinn-rs/quinn/blob/411abe9/quinn-proto/src/connection/paths.rs#L151).
#[derive(Debug, Clone, datasize::DataSize)]
pub struct RttEstimator {
    /// Most recent RTT measurement made when receiving an ack for a
    /// previously unacked packet.
    latest: Duration,
    /// Smoothed RTT of the connection, computed as described in RFC 6298.
    smoothed: Option<Duration>,
    /// RTT variance, computed as described in RFC 6298.
    var: Duration,
    /// Minimum RTT seen in the connection, ignoring ack delay.
    min: Duration,
}

const TIMER_GRANULARITY: Duration = Duration::from_millis(1);

impl RttEstimator {
    /// Creates a new estimator from a given initial RTT.
    ///
    /// If you are unsure about `initial_rtt`, use [`INITIAL_RTT`].
    #[must_use]
    pub fn new(initial_rtt: Duration) -> Self {
        Self {
            latest: initial_rtt,
            smoothed: None,
            var: initial_rtt / 2,
            min: initial_rtt,
        }
    }

    /// Current best RTT estimation.
    #[must_use]
    pub fn get(&self) -> Duration {
        self.smoothed.unwrap_or(self.latest)
    }

    /// Conservative estimate of RTT.
    ///
    /// Takes the maximum of smoothed and latest RTT, as recommended
    /// in 6.1.2 of the recovery spec (draft 29).
    #[must_use]
    pub fn conservative(&self) -> Duration {
        self.get().max(self.latest)
    }

    /// Minimum RTT registered so far for this estimator.
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
        self.get() + cmp::max(4 * self.var, TIMER_GRANULARITY)
    }

    /// Adds an RTT sample to this estimation.
    pub fn update(&mut self, rtt: Duration) {
        self.latest = rtt;
        // min_rtt ignores ack delay.
        self.min = cmp::min(self.min, self.latest);
        // Based on RFC6298.
        if let Some(smoothed) = self.smoothed {
            let adjusted_rtt = self.latest;
            let var_sample = if smoothed > adjusted_rtt {
                smoothed - adjusted_rtt
            } else {
                adjusted_rtt - smoothed
            };
            self.var = (3 * self.var + var_sample) / 4;
            self.smoothed = Some((7 * smoothed + adjusted_rtt) / 8);
        } else {
            self.smoothed = Some(self.latest);
            self.var = self.latest / 2;
            self.min = self.latest;
        }
    }
}
