//! Items shared between client and server.

use aeronet::stats::MessageStats;

/// Statistics on a connection using a channel transport.
#[derive(Debug, Clone, Default)]
pub struct ConnectionStats {
    /// See [`MessageStats::bytes_sent`].
    pub bytes_sent: usize,
    /// See [`MessageStats::bytes_recv`]
    pub bytes_recv: usize,
}

impl MessageStats for ConnectionStats {
    fn bytes_sent(&self) -> usize {
        self.bytes_sent
    }

    fn bytes_recv(&self) -> usize {
        self.bytes_recv
    }
}
