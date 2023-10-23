pub mod back;
pub mod front;

use aeronet::{ClientId, Message, SessionError, TryFromBytes, TryIntoBytes};
use aeronet_wt_core::{OnChannel, Channels};
use rustc_hash::FxHashMap;
use tokio::sync::{broadcast, mpsc};
use wtransport::ServerConfig;

use crate::{shared::CHANNEL_BUF, EndpointInfo, WebTransportServer, WebTransportServerBackend};

/// Creates a server-side transport using the WebTransport protocol.
///
/// This returns a [`WebTransportServer`], which provides the API of the server
/// and is the type you should store, pass around, etc; and also a
/// [`WebTransportServerBackend`], which should be started once using
/// [`WebTransportServerBackend::start`] in an async Tokio runtime when it is
/// first available (this function does not automatically start the backend,
/// because we have no guarantees about the current Tokio runtime at this
/// point).
pub fn create_server<C2S, S2C, C>(
    config: ServerConfig,
) -> (
    WebTransportServer<C2S, S2C, C>,
    WebTransportServerBackend<C2S, S2C, C>,
)
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C> + Clone,
    C: Channels,
{
    let (send_b2f, recv_b2f) = mpsc::channel::<Event<C2S>>(CHANNEL_BUF);
    let (send_f2b, _) = broadcast::channel::<Request<S2C>>(CHANNEL_BUF);

    let frontend = WebTransportServer::<C2S, S2C, C> {
        send: send_f2b.clone(),
        recv: recv_b2f,
        clients: FxHashMap::default(),
        events: Vec::new(),
    };

    let backend = WebTransportServerBackend::<C2S, S2C, C> {
        config,
        send_b2f,
        send_f2b,
    };

    (frontend, backend)
}

#[derive(Debug, Clone)]
pub(crate) enum Request<S2C> {
    Send {
        client: ClientId,
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
