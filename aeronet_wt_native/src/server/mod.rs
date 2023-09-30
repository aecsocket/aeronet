//! Server-side implementation of the transport layer using WebTransport.

pub mod back;
pub mod front;

use std::{net::SocketAddr, time::Duration, collections::HashMap};

use aeronet::{ClientId, SendMessage, ServerTransportConfig, SessionError};
use anyhow::Result;
use rustc_hash::FxHashMap;
use tokio::sync::{broadcast, mpsc};
use wtransport::{endpoint::SessionRequest, Connection, ServerConfig};

use crate::{StreamDefinitions, StreamId, StreamKind, WebTransportBackend, WebTransportServer};

pub(crate) const CHANNEL_BUF: usize = 128;

/// A stream along which the server can send data.
///
/// See [`StreamKind`] for details.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServerStream {
    /// See [`StreamKind::Datagram`].
    Datagram,
    /// See [`StreamKind::Bi`].
    Bi(StreamId),
    /// See [`StreamKind::S2C`].
    S2C(StreamId),
}

impl From<ServerStream> for StreamKind {
    fn from(value: ServerStream) -> Self {
        match value {
            ServerStream::Datagram => Self::Datagram,
            ServerStream::Bi(id) => Self::Bi(id),
            ServerStream::S2C(id) => Self::S2C(id),
        }
    }
}

/// A server sent by the server along a specified stream.
// TODO explain this
/// 
/// To create an object of this type, you can either:
/// * call [`Self::new`] and provide the stream and message, or
/// * call [`OnServerStream::on`] on the message to send, and pass the stream in
///   (make sure to import the trait).
#[derive(Debug, Clone)]
pub struct ServerMessage<T> {
    /// The server-to-client stream along which this message is sent.
    pub stream: ServerStream,
    /// The message to send.
    pub msg: T,
}

impl<T> ServerMessage<T> {
    /// Bundles a message with the stream that it should be sent through by the server, to be used
    /// by a [`WebTransportServer`].
    pub fn new(stream: ServerStream, msg: T) -> Self {
        Self { stream, msg }
    }
}

impl<T: SendMessage> SendMessage for ServerMessage<T> {
    fn into_payload(self) -> Result<Vec<u8>> {
        self.msg.into_payload()
    }
}

/// Allows converting a [`SendMessage`] into a [`ServerMessage`] by specifying the stream along
/// which the message is sent.
/// 
/// This trait is automatically implemented for all [`SendMessage`]s.
/// 
/// # Examples
/// 
/// ```
/// use aeronet_wt_native::{SendMessage, OnServerStream, ServerMessage};
/// 
/// pub struct MyMessage;
/// # impl aeronet::SendMessage for MyMessage {
/// #     fn into_payload(self) -> anyhow::Result<Vec<u8>> { unimplemented!() }
/// # }
/// 
/// fn create_message_on_a_stream() -> ServerMessage<MyMessage> {
///     MyMessage.on(ServerStream::Datagram)
/// }
/// ```
pub trait OnServerStream: Sized {
    /// Creates a [`ServerMessage`] out of this message by providing the stream along which it is
    /// sent.
    fn on(self, stream: ServerStream) -> ServerMessage<Self>;
}

impl<T: SendMessage> OnServerStream for T {
    fn on(self, stream: ServerStream) -> ServerMessage<Self> {
        ServerMessage::new(stream, self)
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

/// A wrapper for [`StreamError`] detailing on which [`StreamKind`] the error occurred.
#[derive(Debug, thiserror::Error)]
#[error("on {stream:?}")]
pub struct OnStreamError {
    /// The stream on which the error occurred.
    stream: StreamKind,
    /// The stream error.
    #[source]
    source: StreamError,
}

impl StreamError {
    pub fn on(self, stream: StreamKind) -> OnStreamError {
        OnStreamError {
            stream,
            source: self,
        }
    }
}

/// Details on a client which is connected to this server through the WebTransport protocol.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
/// should store, pass around, etc; and also a [`WebTransportBackend`], which should be started
/// once using [`WebTransportBackend::listen`] in an async Tokio runtime when it is first
/// available. (This function does not automatically start the backend, because we have no
/// guarantees about the current Tokio runtime at this point.)
pub fn create_server<C: ServerTransportConfig>(
    config: ServerConfig,
    streams: StreamDefinitions,
) -> (WebTransportServer<C>, WebTransportBackend<C>) {
    let (send_b2f, recv_b2f) = mpsc::channel::<InternalEvent<C::C2S>>(CHANNEL_BUF);
    let (send_f2b, _) = broadcast::channel::<Request<C::S2C>>(CHANNEL_BUF);

    let frontend = WebTransportServer::<C> {
        send: send_f2b.clone(),
        recv: recv_b2f,
        clients: FxHashMap::default(),
    };

    let backend = WebTransportBackend::<C> {
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
