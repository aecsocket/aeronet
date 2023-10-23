pub mod back;
pub mod front;

use aeronet::{Message, SessionError, TryFromBytes, TryIntoBytes};
use aeronet_wt_core::{Channels, OnChannel};
use tokio::sync::mpsc;
use wtransport::ClientConfig;

use crate::{
    shared::CHANNEL_BUF, EndpointInfo, WebTransportClient,
    WebTransportClientBackend,
};

/// Creates a client-side transport using the WebTransport protocol.
///
/// This returns a [`WebTransportClient`], which provides the API of the client
/// and is the type you should store, pass around, etc; and also a
/// [`WebTransportClientBackend`], which should be started once using
/// [`WebTransportClientBackend::start`] in an async Tokio runtime when it is
/// first available (this function does not automatically start the backend,
/// because we have no guarantees about the current Tokio runtime at this
/// point).
pub fn create_client<C2S, S2C, C>(
    config: ClientConfig,
) -> (
    WebTransportClient<C2S, S2C>,
    WebTransportClientBackend<C2S, S2C>,
)
where
    C2S: Message + TryIntoBytes + OnChannel,
    S2C: Message + TryFromBytes,
    C: Channels
{
    let (send_b2f, recv_b2f) = mpsc::channel::<Event<S2C>>(CHANNEL_BUF);
    let (send_f2b, recv_f2b) = mpsc::channel::<Request<C2S>>(CHANNEL_BUF);

    let frontend = WebTransportClient::<C2S, S2C> {
        send: send_f2b,
        recv: recv_b2f,
        info: None,
        events: Vec::new(),
    };

    let backend = WebTransportClientBackend::<C2S, S2C> {
        config,
        send: send_b2f,
        recv: recv_f2b,
    };

    (frontend, backend)
}

#[derive(Debug, Clone)]
pub(crate) enum Request<C2S> {
    Connect { url: String },
    Send { stream: ClientStream, msg: C2S },
    Disconnect,
}

#[derive(Debug)]
pub(crate) enum Event<S2C> {
    Connected { info: EndpointInfo },
    UpdateInfo { info: EndpointInfo },
    Recv { msg: S2C },
    Disconnected { reason: SessionError },
}
