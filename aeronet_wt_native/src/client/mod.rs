pub mod back;
pub mod front;

use aeronet::{ClientTransportConfig, SendMessage, SessionError};
use tokio::sync::mpsc;
use wtransport::ClientConfig;

use crate::{
    ClientStream, SendOn, TransportStreams, WebTransportClient, WebTransportClientBackend,
    CHANNEL_BUF,
};

#[derive(Debug, Clone)]
pub struct RemoteServerInfo {}

pub fn create_client<C2S, C>(
    config: ClientConfig,
    url: impl Into<String>,
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
    };

    let backend = WebTransportClientBackend::<C> {
        config,
        url: url.into(),
        streams,
        send: send_b2f,
        recv: recv_f2b,
    };

    (frontend, backend)
}

#[derive(Debug, Clone)]
pub(crate) enum Request<C2S> {
    Send { stream: ClientStream, msg: C2S },
    Disconnect, // TODO do we need this? just drop the frontend
}

#[derive(Debug)]
pub(crate) enum Event<S2C> {
    Connecting { info: RemoteServerInfo },
    Connected,
    UpdateInfo { info: RemoteServerInfo },
    Recv { msg: S2C },
    Disconnected { reason: SessionError },
}
