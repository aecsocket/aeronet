use std::{net::SocketAddr, time::Duration};

use aeronet::{RemoteAddr, Rtt};
use aeronet_wt_stream::StreamId;
use wtransport::Connection;

/// Stores data about a connection to an endpoint of the opposite side, captured
/// at a single point in time.
///
/// On the client side, this represents the client's connection to the server.
///
/// On the server side, this represents the server's connection to a specific
/// client.
#[derive(Debug, Clone)]
pub struct EndpointInfo {
    /// See [`Connection::max_datagram_size`].
    pub max_datagram_size: Option<usize>,
    /// See [`Connection::remote_address`].
    pub remote_addr: SocketAddr,
    /// See [`Connection::rtt`].
    pub rtt: Duration,
    /// See [`Connection::stable_id`].
    pub stable_id: usize,
}

impl EndpointInfo {
    /// Creates an [`EndpointInfo`] from a [`Connection`].
    pub fn from_connection(conn: &Connection) -> Self {
        Self {
            max_datagram_size: conn.max_datagram_size(),
            remote_addr: conn.remote_address(),
            rtt: conn.rtt(),
            stable_id: conn.stable_id(),
        }
    }
}

impl Rtt for EndpointInfo {
    fn rtt(&self) -> Duration {
        self.rtt
    }
}

impl RemoteAddr for EndpointInfo {
    fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }
}

/// An error that occurred while processing a stream.
#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    /// Failed to establish this stream.
    #[error("failed to open stream")]
    Open(#[source] anyhow::Error),
    /// Failed to receive data along this stream, either during deserialization
    /// or transport.
    #[error("failed to receive data")]
    Recv(#[source] anyhow::Error),
    /// Failed to send data along this stream, either during serialization or
    /// transport.
    #[error("failed to send data")]
    Send(#[source] anyhow::Error),
    /// The stream was closed by the other side, but the connection is still
    /// active.
    #[error("closed")]
    Closed,
}

/// A wrapper for [`StreamError`] detailing on which stream the error occurred.
#[derive(Debug, thiserror::Error)]
#[error("on {stream:?}")]
pub struct OnStreamError {
    /// The stream on which the error occurred.
    pub stream: StreamId,
    /// The stream error.
    #[source]
    pub source: StreamError,
}

impl StreamError {
    /// Wraps this [`StreamError`] into an [`OnStreamError`] by providing which
    /// stream the error occurred on.
    pub fn on(self, stream: StreamId) -> OnStreamError {
        OnStreamError {
            stream,
            source: self,
        }
    }
}
