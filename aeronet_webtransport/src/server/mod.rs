#![warn(clippy::future_not_send)]

mod back;
mod front;

use aeronet::{
    server::{ClientId, Event},
    Arena, Message, TransportConfig,
};
pub use back::Backend;
pub use front::Frontend;

use tokio::sync::{broadcast, mpsc};

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use wtransport::{endpoint::SessionRequest, error::ConnectionError, Connection, ServerConfig};

use crate::{StreamId, StreamKind, Streams};

pub(crate) const CHANNEL_BUF: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stream {
    Datagram,
    Bi(StreamId),
    S2C(StreamId),
}

impl Stream {
    pub fn from_kind(kind: StreamKind) -> Option<Self> {
        match kind {
            StreamKind::Datagram => Some(Self::Datagram),
            StreamKind::Bi(id) => Some(Self::Bi(id)),
            StreamKind::S2C(id) => Some(Self::S2C(id)),
            _ => None,
        }
    }

    pub fn as_kind(self) -> StreamKind {
        match self {
            Self::Datagram => StreamKind::Datagram,
            Self::Bi(id) => StreamKind::Bi(id),
            Self::S2C(id) => StreamKind::S2C(id),
        }
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StreamMessage<T> {
    pub stream: Stream,
    pub msg: T,
}

impl<T> Message for StreamMessage<T> {}

#[derive(Debug, Clone)]
pub(crate) enum Request<S2C> {
    Send {
        client: ClientId,
        stream: Stream,
        msg: S2C,
    },
    Disconnect {
        client: ClientId,
    },
}

/// An error during a client's connected session.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The connection to the [`Frontend`] was lost.
    #[error("connection to frontend closed")]
    Frontend,
    /// The server forced this client to disconnect.
    #[error("forced disconnect by server")]
    ForceDisconnect,
    /// Failed to receive the incoming session.
    #[error("failed to receive incoming session")]
    RecvSession(#[source] ConnectionError),
    /// Failed to accept the session.
    #[error("failed to accept session")]
    AcceptSession(#[source] ConnectionError),
    /// Failed while handling sending or receiving data along a stream.
    #[error("stream {stream:?}")]
    Stream {
        /// The stream along which the error occurred.
        stream: StreamKind,
        /// The stream error.
        #[source]
        source: StreamError,
    },
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

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ClientInfo {
    #[default]
    Incoming,
    Request {
        authority: String,
        path: String,
        headers: HashMap<String, String>,
    },
    Connected {
        max_datagram_size: Option<usize>,
        remote_addr: SocketAddr,
        rtt: Duration,
        stable_id: usize,
    },
}

impl ClientInfo {
    pub fn from_request(req: &SessionRequest) -> Self {
        Self::Request {
            authority: req.authority().to_owned(),
            path: req.path().to_owned(),
            headers: req.headers().clone(),
        }
    }

    pub fn from_connection(conn: &Connection) -> Self {
        Self::Connected {
            max_datagram_size: conn.max_datagram_size(),
            remote_addr: conn.remote_address(),
            rtt: conn.rtt(),
            stable_id: conn.stable_id(),
        }
    }
}

pub(crate) type SharedClients = Arc<Mutex<Arena<ClientInfo>>>;

pub fn create<C: TransportConfig>(
    config: ServerConfig,
    streams: Streams,
) -> (Frontend<C>, Backend<C>) {
    let (send_b2f, recv_b2f) = mpsc::channel::<Event<C::C2S>>(CHANNEL_BUF);
    let (send_f2b, _) = broadcast::channel::<Request<C::S2C>>(CHANNEL_BUF);
    let clients: SharedClients = Arc::new(Mutex::new(Arena::new()));

    let frontend = Frontend::<C> {
        send: send_f2b.clone(),
        recv: recv_b2f,
        clients: clients.clone(),
    };

    let backend = Backend::<C> {
        config,
        streams,
        send_b2f,
        send_f2b,
        clients,
    };

    (frontend, backend)
}
