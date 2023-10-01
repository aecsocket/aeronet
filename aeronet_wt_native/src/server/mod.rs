pub mod back;
pub mod front;

use std::{collections::HashMap, net::SocketAddr, time::Duration};

use aeronet::{ClientId, SendMessage, ServerTransportConfig, SessionError};
use anyhow::Result;
use rustc_hash::FxHashMap;
use tokio::sync::{broadcast, mpsc};
use wtransport::{endpoint::SessionRequest, Connection, ServerConfig};

use crate::{TransportStream, TransportStreams, WebTransportServerBackend, WebTransportServer, ServerStream};

pub(crate) const CHANNEL_BUF: usize = 128;

/// A message that is sent along a specific [`ServerStream`].
/// 
/// This is used to determine along which WebTransport stream a message is sent when it is used
/// by a [`WebTransportServer`]. Note that the type of message received (the type of
/// [`ServerTransportConfig::C2S`]) does *not* have to implement this type, but *may* (if you are
/// using the same message type for both C2S and S2C).
/// 
/// To use this, it is recommended to use the wrapper struct [`ServerStreamMessage`] to provide
/// the stream along which the message is sent. This struct can easily be constructed using
/// [`OnServerStream::on`], which is automatically implemented for all [`SendMessage`] types.
pub trait SendOnServerStream {
    /// Gets along which stream this message should be sent.
    fn stream(&self) -> ServerStream;
}

/// Wrapper around a user-defined message type which bundles which stream the message should
/// be sent along.
/// 
/// Use [`OnServerStream::on`] to easily construct one.
#[derive(Debug, Clone)]
pub struct ServerStreamMessage<T> {
    /// The stream along which to send the message.
    pub stream: ServerStream,
    /// The message.
    pub msg: T,
}

impl<T: SendMessage> SendMessage for ServerStreamMessage<T> {
    fn into_payload(self) -> Result<Vec<u8>> {
        self.msg.into_payload()
    }
}

impl<T: SendMessage> SendOnServerStream for ServerStreamMessage<T> {
    fn stream(&self) -> ServerStream {
        self.stream
    }
}

/// Allows converting a [`SendMessage`] to a [`ServerStreamMessage`].
/// 
/// This is automatically implemented for all [`SendMessage`] types.
pub trait OnServerStream: Sized {
    /// Converts this into a [`ServerStreamMessage`] by providing the stream along which the
    /// message is sent.
    fn on(self, stream: ServerStream) -> ServerStreamMessage<Self>;
}

impl<T: SendMessage> OnServerStream for T {
    fn on(self, stream: ServerStream) -> ServerStreamMessage<Self> {
        ServerStreamMessage { stream, msg: self }
    }
}

/// An error during processing a stream connected to a client.
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
    /// The client closed this stream.
    #[error("closed by client")]
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

/// Details on a client which is connected to this server through the WebTransport protocol.
/// 
/// Info for a specific client can be obtained using [`WebTransportServer::client_info`].
#[derive(Debug, Clone, Default)]
pub enum ClientInfo {
    /// The client has started a connection, but no further info is known.
    #[default]
    Incoming,
    /// The client has requested a connection, and has provided some initial information,
    /// but the request has not been accepted yet.
    Request {
        /// See [`SessionRequest::authority`].
        authority: String,
        /// See [`SessionRequest::path`].
        path: String,
        /// See [`SessionRequest::headers`].
        headers: HashMap<String, String>,
    },
    /// The client has successfully established a connection, and full client info is now
    /// available.
    Connected {
        /// See [`Connection::max_datagram_size`].
        max_datagram_size: Option<usize>,
        /// See [`Connection::remote_address`].
        remote_addr: SocketAddr,
        /// See [`Connection::rtt`].
        rtt: Duration,
        /// See [`Connection::stable_id`].
        stable_id: usize,
    },
}

impl ClientInfo {
    /// Creates a [`ClientInfo::Request`] from a [`SessionRequest`].
    pub fn from_request(req: &SessionRequest) -> Self {
        Self::Request {
            authority: req.authority().to_owned(),
            path: req.path().to_owned(),
            headers: req.headers().clone(),
        }
    }

    /// Creates a [`ClientInfo::Connected`] from a [`Connection`].
    pub fn from_connection(conn: &Connection) -> Self {
        Self::Connected {
            max_datagram_size: conn.max_datagram_size(),
            remote_addr: conn.remote_address(),
            rtt: conn.rtt(),
            stable_id: conn.stable_id(),
        }
    }
}

/// Creates a server-side transport using the WebTransport protocol.
///
/// This returns a [`WebTransportServer`], which provides the API of the server and is the type you
/// should store, pass around, etc; and also a [`WebTransportServerBackend`], which should be started
/// once using [`WebTransportServerBackend::listen`] in an async Tokio runtime when it is first
/// available (this function does not automatically start the backend, because we have no
/// guarantees about the current Tokio runtime at this point).
pub fn create_server<S2C, C>(
    config: ServerConfig,
    streams: TransportStreams,
) -> (WebTransportServer<C>, WebTransportServerBackend<C>)
where
    S2C: SendMessage + SendOnServerStream,
    C: ServerTransportConfig<S2C = S2C>,
{
    let (send_b2f, recv_b2f) = mpsc::channel::<InternalEvent<C::C2S>>(CHANNEL_BUF);
    let (send_f2b, _) = broadcast::channel::<Request<C::S2C>>(CHANNEL_BUF);

    let frontend = WebTransportServer::<C> {
        send: send_f2b.clone(),
        recv: recv_b2f,
        clients: FxHashMap::default(),
    };

    let backend = WebTransportServerBackend::<C> {
        config,
        streams,
        send_b2f,
        send_f2b,
    };

    (frontend, backend)
}

#[derive(Debug, Clone)]
pub(crate) enum Request<S2C> {
    Send {
        client: ClientId,
        stream: ServerStream,
        msg: S2C,
    },
    Disconnect {
        client: ClientId,
    },
}

pub(crate) enum InternalEvent<C2S> {
    Incoming {
        client: ClientId,
        info: ClientInfo,
    },
    Connected {
        client: ClientId,
    },
    UpdateInfo {
        client: ClientId,
        info: ClientInfo,
    },
    Recv {
        client: ClientId,
        msg: C2S,
    },
    Disconnected {
        client: ClientId,
        reason: SessionError,
    },
}
