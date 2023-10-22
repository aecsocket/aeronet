pub mod back;
pub mod front;

use aeronet::{ClientId, Message, SessionError, TryFromBytes, TryIntoBytes};
use rustc_hash::FxHashMap;
use tokio::sync::{broadcast, mpsc};
use wtransport::ServerConfig;

use crate::{
    shared::CHANNEL_BUF, EndpointInfo, SendOn, ServerStream, TransportStreams, WebTransportServer,
    WebTransportServerBackend,
};

/// Creates a server-side transport using the WebTransport protocol.
///
/// This returns a [`WebTransportServer`], which provides the API of the server
/// and is the type you should store, pass around, etc; and also a
/// [`WebTransportServerBackend`], which should be started once using
/// [`WebTransportServerBackend::start`] in an async Tokio runtime when it is
/// first available (this function does not automatically start the backend,
/// because we have no guarantees about the current Tokio runtime at this
/// point).
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
        events: Vec::new(),
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
    Connected {
        client: ClientId,
        info: EndpointInfo,
    },
    UpdateInfo {
        client: ClientId,
        info: EndpointInfo,
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
