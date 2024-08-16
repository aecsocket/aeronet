use aeronet::lane::LaneKind;
use web_time::Duration;

/// Configuration for a [`Session`].
///
/// Not all session-specific configurations are exposed here. Transport-specific
/// settings such as maximum packet length are not exposed to users, and are
/// instead set directly when creating a new session.
///
/// [`Session`]: crate::session::Session
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionConfig {
    /// Configurations for the lanes which the client uses to send data, and
    /// which the server uses to receive data.
    pub client_lanes: Vec<LaneKind>,
    /// Configurations for the lanes which the server uses to send data, and
    /// which the client uses to receive data.
    pub server_lanes: Vec<LaneKind>,
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
    /// If we haven't sent a packet to the peer in a while, how long should we
    /// wait until sending an empty acknowledgement/keep-alive packet?
    ///
    /// Even if your user code doesn't send out any packets, we still need to
    /// periodically exchange some data with the peer to ensure that:
    /// - the connection is still active and that we can still successfully
    ///   send data (keep-alive/timeout)
    /// - we send any outstanding packet acknowledgements to the peer, in case
    ///   they didn't receive some of our earlier acknowledgements
    /// - we have an accurate RTT estimate
    ///
    /// If we haven't sent a packet with any actual message fragment within
    /// `max_ack_delay`, the transport will automatically send out an empty ack
    /// packet. The delay is to avoid flooding the connection with ack packets,
    /// since although they are small they are not free.
    pub max_ack_delay: Duration,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            client_lanes: Vec::new(),
            server_lanes: Vec::new(),
            max_memory_usage: 4 * 1024 * 1024,
            send_bytes_per_sec: usize::MAX,
            max_ack_delay: Duration::from_millis(1000),
        }
    }
}

impl SessionConfig {
    /// Adds the given lanes to this configuration's
    /// [`SessionConfig::client_lanes`].
    ///
    /// You can `impl From<LaneKind> for [your own type]` to use it as the item
    /// in this iterator.
    #[must_use]
    pub fn with_client_lanes(
        mut self,
        lanes: impl IntoIterator<Item = impl Into<LaneKind>>,
    ) -> Self {
        self.client_lanes.extend(lanes.into_iter().map(Into::into));
        self
    }

    /// Adds the given lanes to this configuration's
    /// [`SessionConfig::server_lanes`].
    ///
    /// You can implement `From<LaneKind> for [your own type]` to use it as
    /// the item in this iterator.
    #[must_use]
    pub fn with_server_lanes(
        mut self,
        lanes: impl IntoIterator<Item = impl Into<LaneKind>>,
    ) -> Self {
        self.server_lanes.extend(lanes.into_iter().map(Into::into));
        self
    }

    /// Adds the given lanes to this configuration's
    /// [`SessionConfig::client_lanes`] and [`SessionConfig::server_lanes`].
    ///
    /// You can implement `From<LaneKind> for [your own type]` to use it as
    /// the item in this iterator.
    #[must_use]
    pub fn with_lanes(mut self, lanes: impl IntoIterator<Item = impl Into<LaneKind>>) -> Self {
        let lanes = lanes.into_iter().map(Into::into).collect::<Vec<_>>();
        self.client_lanes.extend(lanes.iter().copied());
        self.server_lanes.extend(lanes.iter().copied());
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

    /// Sets [`SessionConfig::max_ack_delay`] on this value.
    #[must_use]
    pub const fn with_max_ack_delay(mut self, max_ack_delay: Duration) -> Self {
        self.max_ack_delay = max_ack_delay;
        self
    }
}
