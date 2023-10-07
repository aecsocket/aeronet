pub mod back;
pub mod front;

use std::collections::HashMap;

use aeronet::{ClientId, Message, SessionError, TryFromBytes, TryIntoBytes};
use rustc_hash::FxHashMap;
use tokio::sync::{broadcast, mpsc};
use wtransport::{endpoint::SessionRequest, Connection, ServerConfig};

use crate::{
    shared::CHANNEL_BUF, EndpointInfo, SendOn, ServerStream, TransportStreams, WebTransportServer,
    WebTransportServerBackend,
};

/// Details on a client which is connected to this server through the WebTransport protocol.
///
/// Info for a specific client can be obtained using [`aeronet::ServerTransport::client_info`].
#[derive(Debug, Clone)]
pub enum RemoteClientInfo {
    /// The client has requested a connection, and has provided some initial information,
    /// but the request has not been accepted yet.
    Connecting {
        /// See [`SessionRequest::authority`].
        authority: String,
        /// See [`SessionRequest::path`].
        path: String,
        /// See [`SessionRequest::headers`].
        headers: HashMap<String, String>,
    },
    /// The client has successfully established a connection, and full endpoint info is now
    /// available.
    Connected(EndpointInfo),
}

impl RemoteClientInfo {
    /// Creates a [`RemoteClientInfo::Connecting`] from a [`SessionRequest`].
    pub fn from_request(req: &SessionRequest) -> Self {
        Self::Connecting {
            authority: req.authority().to_owned(),
            path: req.path().to_owned(),
            headers: req.headers().clone(),
        }
    }

    /// Creates a [`RemoteClientInfo::Connected`] from a [`Connection`].
    pub fn from_connection(conn: &Connection) -> Self {
        Self::Connected(EndpointInfo::from_connection(conn))
    }
}

/// Creates a server-side transport using the WebTransport protocol.
///
/// This returns a [`WebTransportServer`], which provides the API of the server and is the type you
/// should store, pass around, etc; and also a [`WebTransportServerBackend`], which should be started
/// once using [`WebTransportServerBackend::start`] in an async Tokio runtime when it is first
/// available (this function does not automatically start the backend, because we have no
/// guarantees about the current Tokio runtime at this point).
pub fn create_server<C2S, S2C>(
    config: ServerConfig,
    streams: TransportStreams,
) -> (
    WebTransportServer<C2S, S2C>,
    WebTransportServerBackend<C2S, S2C>,
)
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + SendOn<ServerStream> + Clone,
{
    let (send_b2f, recv_b2f) = mpsc::channel::<Event<C2S>>(CHANNEL_BUF);
    let (send_f2b, _) = broadcast::channel::<Request<S2C>>(CHANNEL_BUF);

    let frontend = WebTransportServer::<C2S, S2C> {
        send: send_f2b.clone(),
        recv: recv_b2f,
        clients: FxHashMap::default(),
    };

    let backend = WebTransportServerBackend::<C2S, S2C> {
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

#[derive(Debug)]
pub(crate) enum Event<C2S> {
    Incoming {
        client: ClientId,
        info: RemoteClientInfo,
    },
    Connected {
        client: ClientId,
    },
    UpdateInfo {
        client: ClientId,
        info: RemoteClientInfo,
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
