use aeronet::lane::LaneKind;
use web_time::Duration;

/// Configuration for a [`Session`].
///
/// Not all session-specific configurations are exposed here. Transport-specific
/// settings such as maximum packet length are not exposed to users, and are
/// instead set directly when calling [`Session::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionConfig {
    /// Configurations for the lanes which can be used to send out data.
    pub send_lanes: Vec<LaneConfig>,
    /// Configurations for the lanes which can be used to receive data.
    pub recv_lanes: Vec<LaneConfig>,
    /// Maximum number of bytes of memory which can be used for buffering
    /// messages.
    ///
    /// The default is 0. You **must** either use [`SessionConfig::new`] or
    /// override this value explicitly, otherwise your session will always
    /// error with [`OutOfMemory`]!
    ///
    /// A malicious peer may send us an infinite amount of fragments which
    /// never get fully reassembled, leaving us having to buffer up all of their
    /// fragments. We are not allowed to drop any fragments since they may be
    /// part of a reliable message, in which case dropping breaks the guarantees
    /// of the lane (we don't know if a fragment is part of a reliable or
    /// unreliable message until we fully reassemble it).
    ///
    /// Alternatively, a malicious peer may never send us acknowledgements for
    /// our messages, causing us to never drop the reliable messages that we
    /// want to send over.
    ///
    /// To avoid running out of memory in these situations, if the total memory
    /// usage of this struct exceeds this maximum value, operations on this
    /// session will fail with an [`OutOfMemory`].
    pub max_memory_usage: usize,
    /// How many total bytes we can [`Session::flush`] out per second.
    ///
    /// When flushing, if we do not have enough bytes to send out any more
    /// packets, we will stop returning any packets. You must remember to call
    /// [`Session::refill_bytes`] in your update loop to refill this!
    ///
    /// By default, this is set to [`usize::MAX`] so there is effectively no
    /// limit.
    pub send_bytes_per_sec: usize,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            send_lanes: Vec::new(),
            recv_lanes: Vec::new(),
            max_memory_usage: 0,
            send_bytes_per_sec: usize::MAX,
        }
    }
}

impl SessionConfig {
    /// Creates a new configuration with the default values set, apart from
    /// [`SessionConfig::max_memory_usage`], which must be manually
    /// defined.
    #[must_use]
    pub fn new(max_memory_usage: usize) -> Self {
        Self {
            max_memory_usage,
            ..Default::default()
        }
    }

    /// Adds the given lanes to this configuration's
    /// [`SessionConfig::send_lanes`].
    ///
    /// You can implement `From<LaneConfig> for [your own type]` to use it as
    /// the item in this iterator.
    #[must_use]
    pub fn with_send_lanes(
        mut self,
        lanes: impl IntoIterator<Item = impl Into<LaneConfig>>,
    ) -> Self {
        self.send_lanes.extend(lanes.into_iter().map(Into::into));
        self
    }

    /// Adds the given lanes to this configuration's
    /// [`SessionConfig::recv_lanes`].
    ///
    /// You can implement `From<LaneConfig> for [your own type]` to use it as
    /// the item in this iterator.
    #[must_use]
    pub fn with_recv_lanes(
        mut self,
        lanes: impl IntoIterator<Item = impl Into<LaneConfig>>,
    ) -> Self {
        self.recv_lanes.extend(lanes.into_iter().map(Into::into));
        self
    }

    /// Adds the given lanes to this configuration's
    /// [`SessionConfig::send_lanes`] and [`SessionConfig::recv_lanes`].
    ///
    /// You can implement `From<LaneConfig> for [your own type]` to use it as
    /// the item in this iterator.
    #[must_use]
    pub fn with_lanes(mut self, lanes: impl IntoIterator<Item = impl Into<LaneConfig>>) -> Self {
        let lanes = lanes.into_iter().map(Into::into).collect::<Vec<_>>();
        self.send_lanes.extend(lanes.iter().cloned());
        self.recv_lanes.extend(lanes.iter().cloned());
        self
    }

    /// Sets [`SessionConfig::send_bytes_per_sec`] on this value.
    #[must_use]
    pub const fn with_send_bytes_per_sec(mut self, send_bytes_per_sec: usize) -> Self {
        self.send_bytes_per_sec = send_bytes_per_sec;
        self
    }
}

/// Configuration for a lane in a [`Session`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneConfig {
    /// Kind of lane that this creates.
    pub kind: LaneKind,
    /// For send lanes: how many total bytes we can [`Session::flush`] out on
    /// this lane per second.
    ///
    /// See [`SessionConfig::send_bytes_per_sec`].
    pub send_bytes_per_sec: usize,
    /// For reliable send lanes: after flushing out a fragment, how long do we
    /// wait until attempting to flush out this fragment again?
    ///
    /// If last update we flushed out a fragment of a reliable message, then it
    /// would be pointless to flush out the same fragment on this update, since
    /// [RTT] probably hasn't even elapsed yet, and there's no way the peer
    /// could have acknowledged it yet.
    ///
    /// [RTT]: aeronet::stats::Rtt
    // TODO: could we make this automatic and base it on RTT? i.e. if RTT is
    // 100ms, then we set resend_after to 100ms by default, and if RTT changes,
    // then we re-adjust it
    pub resend_after: Duration,
}

impl Default for LaneConfig {
    fn default() -> Self {
        Self::new(LaneKind::UnreliableUnordered)
    }
}

impl LaneConfig {
    /// Creates a new default lane configuration for the given lane kind.
    #[must_use]
    pub const fn new(kind: LaneKind) -> Self {
        Self {
            kind,
            send_bytes_per_sec: usize::MAX,
            resend_after: Duration::from_millis(100),
        }
    }
}
