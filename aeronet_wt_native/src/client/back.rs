use std::{convert::Infallible, io};

use aeronet::{ClientTransportConfig, SessionError};
use tokio::sync::mpsc;
use tracing::debug;
use wtransport::{endpoint::Client, ClientConfig, Endpoint};

use crate::{shared::open_streams, TransportStreams, CHANNEL_BUF};

use super::{Event, Request};

pub struct WebTransportClientBackend<C: ClientTransportConfig> {
    pub(crate) config: ClientConfig,
    pub(crate) url: String,
    pub(crate) streams: TransportStreams,
    pub(crate) send: mpsc::Sender<Event<C::S2C>>,
    pub(crate) recv: mpsc::Receiver<Request<C::C2S>>,
}

impl<C: ClientTransportConfig> WebTransportClientBackend<C> {
    /// Starts the server logic which interfaces with the target server.
    pub async fn listen(self) -> Result<(), io::Error> {
        let Self {
            config,
            url,
            streams,
            send,
            recv,
        } = self;

        let endpoint = Endpoint::client(config)?;
        let reason = listen::<C>(endpoint, url, streams, send.clone(), recv)
            .await
            .unwrap_err();
        let _ = send.send(Event::Disconnected { reason }).await;
        Ok(())
    }
}

async fn listen<C: ClientTransportConfig>(
    endpoint: Endpoint<Client>,
    url: String,
    streams: TransportStreams,
    send: mpsc::Sender<Event<C::S2C>>,
    recv: mpsc::Receiver<Request<C::C2S>>,
) -> Result<Infallible, SessionError> {
    debug!("Started WebTransport client backend");

    // TODO
    let mut conn = endpoint
        .connect(url)
        .await
        .map_err(|err| SessionError::Connecting(err.into()))?;

    let (send_in, mut recv_in) = mpsc::channel::<C::S2C>(CHANNEL_BUF);
    let (send_err, mut recv_err) = mpsc::channel::<SessionError>(CHANNEL_BUF);
    let (mut streams_bi, mut streams_uni_out) =
        open_streams::<C::C2S, C::S2C>(&streams, &mut conn, send_in, send_err).await?;

    loop {}
}
