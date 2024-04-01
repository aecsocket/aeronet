use std::time::Duration;

use aeronet::stats::Rtt;

use crate::ty::*;

pub const MTU: usize = 1200;

/// Statistics on a WebTransport client/server connection.
#[derive(Debug, Clone)]
pub struct ConnectionStats {
    /// See [`Rtt`].
    pub rtt: Duration,
}

impl ConnectionStats {
    #[must_use]
    pub fn new(rtt: Duration) -> Self {
        Self { rtt }
    }
}

impl From<&ty::Connection> for ConnectionStats {
    #[cfg(target_family = "wasm")]
    fn from(_: &ty::Connection) -> Self {
        Self::new(Duration::ZERO)
    }

    #[cfg(not(target_family = "wasm"))]
    fn from(value: &ty::Connection) -> Self {
        Self::new(value.0.rtt())
    }
}

impl Rtt for ConnectionStats {
    fn rtt(&self) -> Duration {
        self.rtt
    }
}
