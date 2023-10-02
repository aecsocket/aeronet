use std::{convert::Infallible, io};

use aeronet::{MessageTypes, SessionError};
use tokio::sync::mpsc;
use tracing::debug;
use wtransport::{endpoint::Client, ClientConfig, Connection, Endpoint};

use crate::{
    client::RemoteServerInfo,
    shared::{open_streams, recv_datagram, send_out, CHANNEL_BUF},
    ClientStream, TransportStream, TransportStreams,
};

use super::{Event, Request};

/// Runs the actual logic behind a [`crate::WebTransportClient`], intended to be run in an async
/// [`tokio`] runtime.
///
/// The only thing you should do with this struct is to run [`WebTransportClientBackend::start`]
/// in an async task - the frontend will handle the rest.
pub struct WebTransportClientBackend<M: MessageTypes> {
    pub(crate) config: ClientConfig,
    pub(crate) streams: TransportStreams,
    pub(crate) send: mpsc::Sender<Event<M::S2C>>,
    pub(crate) recv: mpsc::Receiver<Request<M::C2S>>,
}

impl<M: MessageTypes> WebTransportClientBackend<M> {
    /// Starts the server logic which interfaces with the target server.
    pub async fn start(self) -> Result<(), io::Error> {
        let Self {
            config,
            streams,
            send,
            recv,
        } = self;

        let endpoint = Endpoint::client(config)?;
        let reason = listen::<M>(endpoint, streams, send.clone(), recv)
            .await
            .unwrap_err();
        let _ = send.send(Event::Disconnected { reason }).await;
        Ok(())
    }
}

async fn listen<M: MessageTypes>(
    endpoint: Endpoint<Client>,
    streams: TransportStreams,
    mut send: mpsc::Sender<Event<M::S2C>>,
    mut recv: mpsc::Receiver<Request<M::C2S>>,
) -> Result<Infallible, SessionError> {
    debug!("Started WebTransport client backend");

    loop {
        debug!("Waiting for connect request");
        let url = loop {
            match recv.recv().await.ok_or(SessionError::Closed)? {
                Request::Connect { url } => break url,
                _ => debug!("Received non-Connect request while not connected"),
            }
        };

        debug!("Connecting to {url}");
        send.send(Event::Connecting {
            info: RemoteServerInfo::Connecting { url: url.clone() },
        })
        .await
        .map_err(|_| SessionError::Closed)?;
        let conn = endpoint
            .connect(url)
            .await
            .map_err(|err| SessionError::Connecting(err.into()))?;

        if let Err(reason) = handle_session::<M>(conn, &streams, &mut send, &mut recv).await {
            send.send(Event::Disconnected { reason })
                .await
                .map_err(|_| SessionError::Closed)?;
        }
    }
}

async fn handle_session<M: MessageTypes>(
    mut conn: Connection,
    streams: &TransportStreams,
    send: &mut mpsc::Sender<Event<M::S2C>>,
    recv: &mut mpsc::Receiver<Request<M::C2S>>,
) -> Result<Infallible, SessionError> {
    let (send_in, mut recv_in) = mpsc::channel::<M::S2C>(CHANNEL_BUF);
    let (send_err, mut recv_err) = mpsc::channel::<SessionError>(CHANNEL_BUF);
    let (mut streams_bi, mut streams_uni_out) =
        open_streams::<M::C2S, M::S2C, ClientStream>(streams, &mut conn, send_in, send_err).await?;

    debug!("Connected to {}", conn.remote_address());
    send.send(Event::Connected)
        .await
        .map_err(|_| SessionError::Closed)?;

    loop {
        send.send(Event::UpdateInfo {
            info: RemoteServerInfo::from_connection(&conn),
        })
        .await
        .map_err(|_| SessionError::Closed)?;

        tokio::select! {
            result = conn.receive_datagram() => {
                let msg = recv_datagram::<M::S2C>(result)
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
                        send_out::<M::C2S>(&mut conn, &mut streams_bi, &mut streams_uni_out, stream.into(), msg)
                            .await
                            .map_err(|err| SessionError::Transport(err.on(stream.into()).into()))?;
                    }
                    Request::Disconnect => return Err(SessionError::ForceDisconnect),
                }
            }
        }
    }
}
