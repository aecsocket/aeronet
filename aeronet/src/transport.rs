use std::{net::SocketAddr, time::Duration};

/// Allows access to the round-trip time of a connection.
pub trait Rtt {
    /// Gets the round-trip time to the connected endpoint.
    ///
    /// The round-trip time is defined as the time taken for the following to
    /// happen:
    /// * client sends data
    /// * server receives the data and sends a response
    ///   * the processing time is assumed to be instant
    /// * client receives data
    fn rtt(&self) -> Duration;
}

/// Allows access to the remote socket address of the other side of a
/// connection.
pub trait RemoteAddr {
    /// Gets the remote socket address of the endpoint that this side is
    /// connected to.
    fn remote_addr(&self) -> SocketAddr;
}
