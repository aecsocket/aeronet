//! Items shared between client and server.

use aeronet::stats::MessageStats;

/// Statistics on a connection using a channel transport.
#[derive(Debug, Clone, Default)]
pub struct ConnectionStats {
    /// See [`MessageStats::msgs_sent`].
    pub msgs_sent: usize,
    /// See [`MessageStats::msgs_recv`]
    pub msgs_recv: usize,
}

impl MessageStats for ConnectionStats {
    fn msgs_sent(&self) -> usize {
        self.msgs_sent
    }

    fn msgs_recv(&self) -> usize {
        self.msgs_recv
    }
}
