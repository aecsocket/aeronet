use std::{convert::Infallible, io};

use aeronet::{Message, SessionError, TryFromBytes, TryIntoBytes};
use aeronet_wt_core::{ChannelId, Channels, OnChannel};
use tokio::sync::mpsc;
use tracing::debug;
use wtransport::{endpoint::endpoint_side::Client, ClientConfig, Connection, Endpoint};

use crate::{
    shared::{open_channels, recv_datagram, send_out, CHANNEL_BUF},
    EndpointInfo,
};

use super::{Event, Request};

/// Runs the actual logic behind a [`crate::WebTransportClient`], intended to be
/// run in an async [`tokio`] runtime.
///
/// The only thing you should do with this struct is to run
/// [`WebTransportClientBackend::start`] in an async task - the frontend will
/// handle the rest.
pub struct WebTransportClientBackend<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: Channels,
{
    pub(crate) config: ClientConfig,
    pub(crate) send: mpsc::Sender<Event<S2C>>,
    pub(crate) recv: mpsc::Receiver<Request<C2S>>,
}

impl<C2S, S2C, C> WebTransportClientBackend<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: Channels,
{
    /// Starts the server logic which interfaces with the target server.
    pub async fn start(self) -> Result<(), io::Error> {
        let Self { config, send, recv } = self;

        let endpoint = Endpoint::client(config)?;
        debug!("Started WebTransport client backend");
        listen::<C2S, S2C, C>(endpoint, send.clone(), recv).await;
        debug!("Stopped WebTransport client backend");
        Ok(())
    }
}

async fn listen<C2S, S2C, C>(
    endpoint: Endpoint<Client>,
    mut send: mpsc::Sender<Event<S2C>>,
    mut recv: mpsc::Receiver<Request<C2S>>,
) where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: Channels,
{
    loop {
        debug!("Waiting for connect request");
        let url = loop {
            match recv.recv().await {
                Some(Request::Connect(url)) => break url,
                Some(_) => debug!("Received non-Connect request while not connected"),
                None => return,
            }
        };

        let reason = connect::<C2S, S2C, C>(&endpoint, &mut send, &mut recv, url)
            .await
            .unwrap_err();
        if send.send(Event::Disconnected(reason)).await.is_err() {
            return;
        }
    }
}

async fn connect<C2S, S2C, C>(
    endpoint: &Endpoint<Client>,
    send: &mut mpsc::Sender<Event<S2C>>,
    recv: &mut mpsc::Receiver<Request<C2S>>,
    url: String,
) -> Result<Infallible, SessionError>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: Channels,
{
    debug!("Connecting to {url}");

    let conn = endpoint
        .connect(url)
        .await
        .map_err(|err| SessionError::Connecting(err.into()))?;

    handle_session::<C2S, S2C, C>(conn, send, recv).await
}

async fn handle_session<C2S, S2C, C>(
    mut conn: Connection,
    send: &mut mpsc::Sender<Event<S2C>>,
    recv: &mut mpsc::Receiver<Request<C2S>>,
) -> Result<Infallible, SessionError>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: Channels,
{
    let (send_in, mut recv_in) = mpsc::channel::<S2C>(CHANNEL_BUF);
    let (send_err, mut recv_err) = mpsc::channel::<SessionError>(CHANNEL_BUF);
    let mut streams = open_channels::<C2S, S2C, C>(&mut conn, send_in, send_err).await?;

    debug!("Connected to {}", conn.remote_address());
    // although we are going to send the endpoint info literally immediately after
    // this Connected is sent, we are contractually obligated to make sure that
    // `info` returns something after a Connected is received
    send.send(Event::Connected(EndpointInfo::from_connection(&conn)))
        .await
        .map_err(|_| SessionError::Closed)?;

    loop {
        send.send(Event::UpdateInfo(EndpointInfo::from_connection(&conn)))
            .await
            .map_err(|_| SessionError::Closed)?;

        tokio::select! {
            result = conn.receive_datagram() => {
                let msg = recv_datagram::<S2C>(result)
                    .await
                    .map_err(|err| SessionError::Transport(err.on(ChannelId::Datagram).into()))?;
                send.send(Event::Recv(msg))
                    .await
                    .map_err(|_| SessionError::Closed)?;
            }
            Some(msg) = recv_in.recv() => {
                send.send(Event::Recv(msg))
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
                    Request::Send(msg) => {
                        let channel = msg.channel().channel_id();
                        send_out::<C2S>(&mut conn, &mut streams, channel, msg)
                            .await
                            .map_err(|err| SessionError::Transport(err.on(channel).into()))?;
                    }
                    Request::Disconnect => return Err(SessionError::ForceDisconnect),
                }
            }
        }
    }
}
