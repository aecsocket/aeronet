//! Items shared between the client and server.

#[cfg(target_family = "wasm")]
mod js_error;
pub use aeronet_proto::session::MessageKey;
#[cfg(target_family = "wasm")]
pub use js_error::*;
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
