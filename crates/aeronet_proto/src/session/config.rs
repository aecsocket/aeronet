use aeronet::lane::LaneKind;

/// Configuration for a [`Session`].
///
/// Not all session-specific configurations are exposed here. Transport-specific
/// settings such as maximum packet length are not exposed to users, and are
/// instead set directly when calling [`Session::new`].
///
/// [`Session`]: crate::session::Session
/// [`Session::new`]: crate::session::Session::new
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionConfig {
    /// Configurations for the lanes which can be used to send out data.
    pub send_lanes: Vec<LaneKind>,
    /// Configurations for the lanes which can be used to receive data.
    pub recv_lanes: Vec<LaneKind>,
    /// Maximum number of bytes of memory which can be used for buffering
    /// messages.
    ///
    /// By default, this is 4MiB (`4 * 1024 * 1024`). Consider tuning this
    /// number if you see connections fail with an out-of-memory error, or you
    /// see memory usage is too high in your app.
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
    /// session will fail with an out-of-memory error.
    pub max_memory_usage: usize,
    /// How many total bytes we can [`Session::flush`] out per second.
    ///
    /// When flushing, if we do not have enough bytes to send out any more
    /// packets, we will stop returning any packets. The session accumulates
    /// its byte budget back up in [`Session::update`].
    ///
    /// By default, this is set to [`usize::MAX`] so there is effectively no
    /// limit.
    ///
    /// [`Session`]: crate::session::Session
    /// [`Session::flush`]: crate::session::Session::flush
    /// [`Session::update`]: crate::session::Session::update
    pub send_bytes_per_sec: usize,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            send_lanes: Vec::new(),
            recv_lanes: Vec::new(),
            max_memory_usage: 4 * 1024 * 1024,
            send_bytes_per_sec: usize::MAX,
        }
    }
}

impl SessionConfig {
    /// Adds the given lanes to this configuration's
    /// [`SessionConfig::send_lanes`].
    ///
    /// You can implement `From<LaneKind> for [your own type]` to use it as
    /// the item in this iterator.
    #[must_use]
    pub fn with_send_lanes(mut self, lanes: impl IntoIterator<Item = impl Into<LaneKind>>) -> Self {
        self.send_lanes.extend(lanes.into_iter().map(Into::into));
        self
    }

    /// Adds the given lanes to this configuration's
    /// [`SessionConfig::recv_lanes`].
    ///
    /// You can implement `From<LaneKind> for [your own type]` to use it as
    /// the item in this iterator.
    #[must_use]
    pub fn with_recv_lanes(mut self, lanes: impl IntoIterator<Item = impl Into<LaneKind>>) -> Self {
        self.recv_lanes.extend(lanes.into_iter().map(Into::into));
        self
    }

    /// Adds the given lanes to this configuration's
    /// [`SessionConfig::send_lanes`] and [`SessionConfig::recv_lanes`].
    ///
    /// You can implement `From<LaneKind> for [your own type]` to use it as
    /// the item in this iterator.
    #[must_use]
    pub fn with_lanes(mut self, lanes: impl IntoIterator<Item = impl Into<LaneKind>>) -> Self {
        let lanes = lanes.into_iter().map(Into::into).collect::<Vec<_>>();
        self.send_lanes.extend(lanes.iter().copied());
        self.recv_lanes.extend(lanes.iter().copied());
        self
    }

    /// Sets [`SessionConfig::max_memory_usage`] on this value.
    #[must_use]
    pub const fn with_max_memory_usage(mut self, max_memory_usage: usize) -> Self {
        self.max_memory_usage = max_memory_usage;
        self
    }

    /// Sets [`SessionConfig::send_bytes_per_sec`] on this value.
    #[must_use]
    pub const fn with_send_bytes_per_sec(mut self, send_bytes_per_sec: usize) -> Self {
        self.send_bytes_per_sec = send_bytes_per_sec;
        self
    }
}
