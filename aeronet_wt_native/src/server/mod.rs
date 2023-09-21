mod back;
mod front;

use aeronet::{
    anyhow::Result,
    message::SendMessage,
    server::{ClientId, Event, TransportConfig},
    Arena,
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

use wtransport::{endpoint::SessionRequest, Connection, ServerConfig};

use crate::{StreamId, StreamKind, Streams};

pub(crate) const CHANNEL_BUF: usize = 128;

pub(crate) type SharedClients = Arc<Mutex<Arena<ClientInfo>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stream {
    Datagram,
    Bi(StreamId),
    S2C(StreamId),
}

impl From<StreamKind> for Option<Stream> {
    fn from(value: StreamKind) -> Self {
        match value {
            StreamKind::Datagram => Some(Stream::Datagram),
            StreamKind::Bi(id) => Some(Stream::Bi(id)),
            StreamKind::S2C(id) => Some(Stream::S2C(id)),
            _ => None,
        }
    }
}

impl From<Stream> for StreamKind {
    fn from(value: Stream) -> Self {
        match value {
            Stream::Datagram => Self::Datagram,
            Stream::Bi(id) => Self::Bi(id),
            Stream::S2C(id) => Self::S2C(id),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StreamMessage<T> {
    pub stream: Stream,
    pub msg: T,
}

impl<T: SendMessage> SendMessage for StreamMessage<T> {
    fn into_payload(self) -> Result<Vec<u8>> {
        self.msg.into_payload()
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Request<S2C> {
    Send { client: ClientId, msg: S2C },
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
