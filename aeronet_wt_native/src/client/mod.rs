pub mod back;
pub mod front;

use aeronet::{ClientTransportConfig, SendMessage, SessionError};
use tokio::sync::mpsc;
use wtransport::{ClientConfig, Connection};

use crate::{
    ClientStream, EndpointInfo, SendOn, TransportStreams, WebTransportClient,
    WebTransportClientBackend, CHANNEL_BUF,
};

/// Details on the server which this client is connected to through the WebTransport protocol.
///
/// Info for a client transport can be obtained using [`WebTransportClient::info`].
#[derive(Debug, Clone)]
pub enum RemoteServerInfo {
    /// The client has started a connection, but no further info is known.
    Connecting {
        /// The URL of the connection request.
        url: String,
    },
    /// The client has successfully established a connection, and full endpoint info is now
    /// available.
    Connected(EndpointInfo),
}

impl RemoteServerInfo {
    pub fn from_connection(conn: &Connection) -> Self {
        Self::Connected(EndpointInfo::from_connection(conn))
    }
}

/// Creates a client-side transport using the WebTransport protocol.
///
/// This returns a [`WebTransportClient`], which provides the API of the client and is the type you
/// should store, pass around, etc; and also a [`WebTransportClientBackend`], which should be started
/// once using [`WebTransportClientBackend::start`] in an async Tokio runtime when it is first
/// available (this function does not automatically start the backend, because we have no
/// guarantees about the current Tokio runtime at this point).
pub fn create_client<C2S, C>(
    config: ClientConfig,
    streams: TransportStreams,
) -> (WebTransportClient<C>, WebTransportClientBackend<C>)
where
    C2S: SendMessage + SendOn<ClientStream>,
    C: ClientTransportConfig<C2S = C2S>,
{
    let (send_b2f, recv_b2f) = mpsc::channel::<Event<C::S2C>>(CHANNEL_BUF);
    let (send_f2b, recv_f2b) = mpsc::channel::<Request<C::C2S>>(CHANNEL_BUF);

    let frontend = WebTransportClient::<C> {
        send: send_f2b,
        recv: recv_b2f,
        info: None,
    };

    let backend = WebTransportClientBackend::<C> {
        config,
        streams,
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
    Connecting { info: RemoteServerInfo },
    Connected,
    UpdateInfo { info: RemoteServerInfo },
    Recv { msg: S2C },
    Disconnected { reason: SessionError },
}
