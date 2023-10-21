use std::{net::SocketAddr, time::Duration};

use aeronet::{RemoteAddr, Rtt, TryIntoBytes};
use anyhow::Result;
use wtransport::Connection;

use crate::TransportStream;

/// Stores data about a connection to an endpoint of the opposite side, captured at a single
/// point in time.
///
/// On the client side, this represents the client's connection to the server.
///
/// On the server side, this represents the server's connection to a specific client.
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

/// An error that occurred while processing a [`TransportStream`].
#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    /// Failed to establish this stream.
    #[error("failed to open stream")]
    Open(#[source] anyhow::Error),
    /// Failed to receive data along this stream, either during deserialization or transport.
    #[error("failed to receive data")]
    Recv(#[source] anyhow::Error),
    /// Failed to send data along this stream, either during serialization or transport.
    #[error("failed to send data")]
    Send(#[source] anyhow::Error),
    /// The stream was closed by the other side, but the connection is still active.
    #[error("closed")]
    Closed,
}

/// A wrapper for [`StreamError`] detailing on which [`TransportStream`] the error occurred.
#[derive(Debug, thiserror::Error)]
#[error("on {stream:?}")]
pub struct OnStreamError {
    /// The stream on which the error occurred.
    pub stream: TransportStream,
    /// The stream error.
    #[source]
    pub source: StreamError,
}

impl StreamError {
    /// Wraps this [`StreamError`] into an [`OnStreamError`] by providing which stream the error
    /// occurred on.
    pub fn on(self, stream: TransportStream) -> OnStreamError {
        OnStreamError {
            stream,
            source: self,
        }
    }
}

/// A message that is sent along a specific [`TransportStream`].
///
/// This is used to determine along which WebTransport stream a message is sent when it is used
/// by a transport side. Note that the type of message received  does *not* have to implement
/// this type, but *may* (if you are using the same message type for both C2S and S2C).
///
/// To use this, it is recommended to use the wrapper struct [`StreamMessage`] to provide
/// the stream along which the message is sent. This struct can easily be constructed using
/// [`OnStream::on`].
pub trait SendOn<S> {
    /// Gets along which stream this message should be sent.
    fn stream(&self) -> S;
}

/// Wrapper around a user-defined message type which bundles which stream the message should
/// be sent along.
///
/// Use [`OnStream::on`] to easily construct one.
#[derive(Debug, Clone)]
pub struct StreamMessage<S, T> {
    /// The stream along which to send the message.
    pub stream: S,
    /// The message.
    pub msg: T,
}

impl<S, T> SendOn<S> for StreamMessage<S, T>
where
    S: Clone,
{
    fn stream(&self) -> S {
        self.stream.clone()
    }
}

impl<S, T> TryIntoBytes for StreamMessage<S, T>
where
    S: Clone,
    T: TryIntoBytes,
{
    fn try_into_bytes(self) -> Result<Vec<u8>> {
        self.msg.try_into_bytes()
    }
}

/// Allows converting a [`Message`] into a [`StreamMessage`].
///
/// This is automatically implemented for all types.
///
/// [`Message`]: aeronet::Message
pub trait OnStream<S>: Sized {
    /// Converts this into a [`StreamMessage`] by providing the stream along which the
    /// message is sent.
    fn on(self, stream: S) -> StreamMessage<S, Self>;
}

impl<S, T> OnStream<S> for T {
    fn on(self, stream: S) -> StreamMessage<S, Self> {
        StreamMessage { stream, msg: self }
    }
}
