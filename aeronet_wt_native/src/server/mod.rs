mod back;
mod front;

pub use back::Backend;
pub use front::Frontend;

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use aeronet::{
    message::SendMessage,
    server::{ClientId, Event, TransportConfig},
    Arena,
};
use anyhow::Result;
use tokio::sync::{broadcast, mpsc};
use wtransport::{endpoint::SessionRequest, Connection, ServerConfig};

use crate::{StreamId, StreamKind, Streams};

pub(crate) const CHANNEL_BUF: usize = 128;

pub(crate) type SharedClients = Arc<Mutex<Arena<ClientInfo>>>;

/// A stream along which the server can send data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutStream {
    Datagram,
    Bi(StreamId),
    S2C(StreamId),
}

impl From<StreamKind> for Option<OutStream> {
    fn from(value: StreamKind) -> Self {
        match value {
            StreamKind::Datagram => Some(OutStream::Datagram),
            StreamKind::Bi(id) => Some(OutStream::Bi(id)),
            StreamKind::S2C(id) => Some(OutStream::S2C(id)),
            _ => None,
        }
    }
}

impl From<OutStream> for StreamKind {
    fn from(value: OutStream) -> Self {
        match value {
            OutStream::Datagram => Self::Datagram,
            OutStream::Bi(id) => Self::Bi(id),
            OutStream::S2C(id) => Self::S2C(id),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StreamMessage<T> {
    pub stream: OutStream,
    pub msg: T,
}

impl<T: SendMessage> SendMessage for StreamMessage<T> {
    fn into_payload(self) -> Result<Vec<u8>> {
        self.msg.into_payload()
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Request<S2C> {
    Send {
        client: ClientId,
        stream: OutStream,
        msg: S2C,
    },
    Disconnect { client: ClientId },
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
        OnStreamError { stream, source: self }
    }
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
