use std::{convert::Infallible, io};

use aeronet::{Message, SessionError, TryFromBytes, TryIntoBytes};
use tokio::sync::mpsc;
use tracing::debug;
use wtransport::{endpoint::endpoint_side::Client, ClientConfig, Connection, Endpoint};

use crate::{
    shared::{open_streams, recv_datagram, send_out, CHANNEL_BUF},
    ClientStream, TransportStream, TransportStreams, EndpointInfo,
};

use super::{Event, Request};

/// Runs the actual logic behind a [`crate::WebTransportClient`], intended to be run in an async
/// [`tokio`] runtime.
///
/// The only thing you should do with this struct is to run [`WebTransportClientBackend::start`]
/// in an async task - the frontend will handle the rest.
pub struct WebTransportClientBackend<C2S, S2C> {
    pub(crate) config: ClientConfig,
    pub(crate) streams: TransportStreams,
    pub(crate) send: mpsc::Sender<Event<S2C>>,
    pub(crate) recv: mpsc::Receiver<Request<C2S>>,
}

impl<C2S, S2C> WebTransportClientBackend<C2S, S2C>
where
    C2S: Message + TryIntoBytes,
    S2C: Message + TryFromBytes,
{
    /// Starts the server logic which interfaces with the target server.
    pub async fn start(self) -> Result<(), io::Error> {
        let Self {
            config,
            streams,
            send,
            recv,
        } = self;

        let endpoint = Endpoint::client(config)?;
        debug!("Started WebTransport client backend");
        listen::<C2S, S2C>(endpoint, streams, send.clone(), recv).await;
        debug!("Stopped WebTransport client backend");
        Ok(())
    }
}

async fn listen<C2S, S2C>(
    endpoint: Endpoint<Client>,
    streams: TransportStreams,
    mut send: mpsc::Sender<Event<S2C>>,
    mut recv: mpsc::Receiver<Request<C2S>>,
) where
    C2S: Message + TryIntoBytes,
    S2C: Message + TryFromBytes,
{
    loop {
        debug!("Waiting for connect request");
        let url = loop {
            match recv.recv().await {
                Some(Request::Connect { url }) => break url,
                Some(_) => debug!("Received non-Connect request while not connected"),
                None => return,
            }
        };

        let reason = connect::<C2S, S2C>(&endpoint, &streams, &mut send, &mut recv, url)
            .await
            .unwrap_err();
        if send.send(Event::Disconnected { reason }).await.is_err() {
            return;
        }
    }
}

async fn connect<C2S, S2C>(
    endpoint: &Endpoint<Client>,
    streams: &TransportStreams,
    mut send: &mut mpsc::Sender<Event<S2C>>,
    mut recv: &mut mpsc::Receiver<Request<C2S>>,
    url: String,
) -> Result<Infallible, SessionError>
where
    C2S: Message + TryIntoBytes,
    S2C: Message + TryFromBytes,
{
    debug!("Connecting to {url}");

    let conn = endpoint
        .connect(url)
        .await
        .map_err(|err| SessionError::Connecting(err.into()))?;

    handle_session::<C2S, S2C>(conn, streams, &mut send, &mut recv).await
}

async fn handle_session<C2S, S2C>(
    mut conn: Connection,
    streams: &TransportStreams,
    send: &mut mpsc::Sender<Event<S2C>>,
    recv: &mut mpsc::Receiver<Request<C2S>>,
) -> Result<Infallible, SessionError>
where
    C2S: Message + TryIntoBytes,
    S2C: Message + TryFromBytes,
{
    let (send_in, mut recv_in) = mpsc::channel::<S2C>(CHANNEL_BUF);
    let (send_err, mut recv_err) = mpsc::channel::<SessionError>(CHANNEL_BUF);
    let (mut streams_bi, mut streams_uni_out) =
        open_streams::<C2S, S2C, ClientStream>(streams, &mut conn, send_in, send_err).await?;

    debug!("Connected to {}", conn.remote_address());
    // although we are going to send the endpoint info literally immediately after this Connected
    // is sent, we are contractually obligated to make sure that `info` returns something after a
    // Connected is received
    send.send(Event::Connected { info: EndpointInfo::from_connection(&conn)})
        .await
        .map_err(|_| SessionError::Closed)?;

    loop {
        send.send(Event::UpdateInfo {
            info: EndpointInfo::from_connection(&conn),
        })
        .await
        .map_err(|_| SessionError::Closed)?;

        tokio::select! {
            result = conn.receive_datagram() => {
                let msg = recv_datagram::<S2C>(result)
                    .await
                    .map_err(|err| SessionError::Transport(err.on(TransportStream::Datagram).into()))?;
                send.send(Event::Recv { msg })
                    .await
                    .map_err(|_| SessionError::Closed)?;
            }
            Some(msg) = recv_in.recv() => {
                send.send(Event::Recv { msg })
                    .await
                    .map_err(|_| SessionError::Closed)?;
            }
            Some(err) = recv_err.recv() => {
                return Err(err);
            }
            result = recv.recv() => {
                let req = result.ok_or(SessionError::Closed)?;
                match req {
                    Request::Connect { .. } => debug!("Received Connect request while connected"),
                    Request::Send { stream, msg } => {
                        send_out::<C2S>(&mut conn, &mut streams_bi, &mut streams_uni_out, stream.into(), msg)
                            .await
                            .map_err(|err| SessionError::Transport(err.on(stream.into()).into()))?;
                    }
                    Request::Disconnect => return Err(SessionError::ForceDisconnect),
                }
            }
        }
    }
}
