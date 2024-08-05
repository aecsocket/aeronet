//! Items shared between the client and server.

pub use aeronet_proto::session::MessageKey;

use web_time::Duration;

/// Low-level [`Rtt`] value provided by the underlying WebTransport connection.
///
/// The [`Rtt`] impl for connection structs return the [`Session`]'s RTT, *not*
/// this value. This value is more representative of RTT at a packet level,
/// but less representative of RTT at the application level.
///
/// [`Rtt`]: aeronet::stats::Rtt
/// [`Session`]: aeronet_proto::session::Session
pub trait RawRtt {
    /// Gets the low-level RTT value.
    fn raw_rtt(&self) -> Duration;
}
