use std::{convert::Infallible, io};

use aeronet::{ClientId, Message, SessionError, TryFromBytes, TryIntoBytes};
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, debug_span, Instrument};
use wtransport::{
    endpoint::{endpoint_side::Server, IncomingSession},
    Connection, Endpoint, ServerConfig,
};

use crate::{
    shared::{open_streams, recv_datagram, send_out},
    ServerStream, TransportStream, TransportStreams,
};

use super::{Event, RemoteClientInfo, Request, CHANNEL_BUF};

/// Runs the actual logic behind a [`crate::WebTransportServer`], intended to be run in an async
/// [`tokio`] runtime.
///
/// The only thing you should do with this struct is to run [`WebTransportServerBackend::start`]
/// in an async task - the frontend will handle the rest.
pub struct WebTransportServerBackend<C2S, S2C> {
    pub(crate) config: ServerConfig,
    pub(crate) streams: TransportStreams,
    pub(crate) send_b2f: mpsc::Sender<Event<C2S>>,
    pub(crate) send_f2b: broadcast::Sender<Request<S2C>>,
}

impl<C2S, S2C> WebTransportServerBackend<C2S, S2C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone,
{
    /// Starts the server logic which interfaces with clients.
    pub async fn start(self) -> Result<(), io::Error> {
        let Self {
            config,
            streams,
            send_b2f,
            send_f2b,
        } = self;

        let endpoint = Endpoint::server(config)?;
        debug!("Started WebTransport server backend");
        listen::<C2S, S2C>(endpoint, streams, send_b2f, send_f2b).await;
        debug!("Stopped WebTransport server backend");
        Ok(())
    }
}

async fn listen<C2S, S2C>(
    endpoint: Endpoint<Server>,
    streams: TransportStreams,
    send_evt: mpsc::Sender<Event<C2S>>,
    send_req: broadcast::Sender<Request<S2C>>,
) where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone,
{
    let (send_close, mut recv_close) = mpsc::channel::<()>(1);
    for client in 0.. {
        debug!("Waiting for connection");
        let req = tokio::select! {
            req = endpoint.accept() => req,
            _ = recv_close.recv() => return,
        };

        let client = ClientId::from_raw(client);

        let streams = streams.clone();
        let send = send_evt.clone();
        let recv = send_req.subscribe();

        let send_close = send_close.clone();

        tokio::spawn(
            async move {
                let reason = handle_session::<C2S, S2C>(streams, send.clone(), recv, client, req)
                    .await
                    .unwrap_err();
                if send
                    .send(Event::Disconnected { client, reason })
                    .await
                    .is_err()
                {
                    let _ = send_close.send(()).await;
                }
            }
            .instrument(debug_span!("Session", id = tracing::field::display(client))),
        );
    }
}

async fn handle_session<C2S, S2C>(
    streams: TransportStreams,
    mut send: mpsc::Sender<Event<C2S>>,
    mut recv: broadcast::Receiver<Request<S2C>>,
    client: ClientId,
    req: IncomingSession,
) -> Result<Infallible, SessionError>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone,
{
    let mut conn = accept_session::<C2S>(&mut send, client, req).await?;

    let (send_in, mut recv_in) = mpsc::channel::<C2S>(CHANNEL_BUF);
    let (send_err, mut recv_err) = mpsc::channel::<SessionError>(CHANNEL_BUF);
    let (mut streams_bi, mut streams_uni_out) =
        open_streams::<S2C, C2S, ServerStream>(&streams, &mut conn, send_in, send_err).await?;

    loop {
        send.send(Event::UpdateInfo {
            client,
            info: RemoteClientInfo::from_connection(&conn),
        })
        .await
        .map_err(|_| SessionError::Closed)?;

        tokio::select! {
            result = conn.receive_datagram() => {
                let msg = recv_datagram::<C2S>(result)
                    .await
                    .map_err(|err| SessionError::Transport(err.on(TransportStream::Datagram).into()))?;
                send.send(Event::Recv { client, msg })
                    .await
                    .map_err(|_| SessionError::Closed)?;
            }
            Some(msg) = recv_in.recv() => {
                send.send(Event::Recv { client, msg })
                    .await
                    .map_err(|_| SessionError::Closed)?;
            }
            Some(err) = recv_err.recv() => {
                return Err(err);
            }
            result = recv.recv() => {
                let req = result.map_err(|_| SessionError::Closed)?;
                match req {
                    Request::Send { client: target, stream, msg } if target == client => {
                        send_out::<S2C>(&mut conn, &mut streams_bi, &mut streams_uni_out, stream.into(), msg)
                            .await
                            .map_err(|err| SessionError::Transport(err.on(stream.into()).into()))?;
                    }
                    Request::Disconnect { client: target } if target == client => {
                        return Err(SessionError::ForceDisconnect);
                    }
                    _ => {},
                }
            }
        }
    }
}

async fn accept_session<C2S>(
    send: &mut mpsc::Sender<Event<C2S>>,
    client: ClientId,
    req: IncomingSession,
) -> Result<Connection, SessionError>
where
    C2S: Message + TryFromBytes,
{
    debug!("Incoming connection");

    let req = req
        .await
        .map_err(|err| SessionError::Connecting(err.into()))?;

    debug!(
        "Requesting session, authority: {:?} / path: {:?}",
        req.authority(),
        req.path()
    );
    send.send(Event::Incoming {
        client,
        info: RemoteClientInfo::from_request(&req),
    })
    .await
    .map_err(|_| SessionError::Closed)?;

    let conn = req
        .accept()
        .await
        .map_err(|err| SessionError::Connecting(err.into()))?;

    let remote_addr = conn.remote_address();
    debug!("Connected from {remote_addr}");
    send.send(Event::Connected { client })
        .await
        .map_err(|_| SessionError::Closed)?;

    Ok(conn)
}
