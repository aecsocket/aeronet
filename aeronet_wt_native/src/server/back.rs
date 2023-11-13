use std::marker::PhantomData;

use aeronet::{Message, TryIntoBytes, TryFromBytes};
use aeronet_wt_core::{ChannelId, Channels, OnChannel};
use futures::future::try_join_all;
use rustc_hash::FxHashMap;
use tokio::sync::{broadcast, mpsc, oneshot};
use wtransport::{endpoint::IncomingSession, Connection, RecvStream, SendStream, ServerConfig};

use crate::{EndpointInfo, ClientKey};

use super::{Signal, Request, CHANNEL_CAP, DATA_CAP, front::Open, WebTransportError, ChannelError};

type Endpoint = wtransport::endpoint::Endpoint<wtransport::endpoint::endpoint_side::Server>;

pub(super) async fn start<C2S, S2C, C>(
    config: ServerConfig,
    send_next: oneshot::Sender<Result<Open<C2S, S2C, C>, WebTransportError<C2S, S2C>>>,
) where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
    C: Channels,
{
    let endpoint = match create_endpoint(config).await {
        Ok(endpoint) => endpoint,
        Err(err) => {
            let _ = send_next.send(Err(err));
            return;
        }
    };

    let (send_sig, recv_sig) = mpsc::unbounded_channel();
    let (send_req, mut recv_req) = broadcast::channel(CHANNEL_CAP);
    let next = Open {
        local_addr: endpoint.local_addr(),
        clients: FxHashMap::default(),
        recv_sig,
        send_req: send_req.clone(),
        _phantom_c: PhantomData::default(),
    };
    let _ = send_next.send(Ok(next));

    for client in 0.. {
        let client = ClientKey::from_raw(client);

        let session = tokio::select! {
            Err(broadcast::error::RecvError::Closed) = recv_req.recv() => {
                // frontend closed
                return;
            }
            session = endpoint.accept() => session,
        };
        let _ = send_sig.send(Signal::Incoming { client });

        let mut send_sig = send_sig.clone();
        let recv_req = send_req.subscribe();
        tokio::spawn(async move {
            if let Err(reason) = handle_session::<C2S, S2C, C>(client, session, &mut send_sig, recv_req).await {
                let _ = send_sig.send(Signal::Disconnected { client, reason });
            }
        });
    }
}

async fn create_endpoint<C2S, S2C>(config: ServerConfig) -> Result<Endpoint, WebTransportError<C2S, S2C>>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes,
{
    Endpoint::server(config).map_err(WebTransportError::CreateEndpoint)
}

async fn handle_session<C2S, S2C, C>(
    client: ClientKey,
    session: IncomingSession,
    send_sig: &mut mpsc::UnboundedSender<Signal<C2S, S2C>>,
    mut recv_req: broadcast::Receiver<Request<S2C>>,
) -> Result<(), WebTransportError<C2S, S2C>>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
    C: Channels,
{
    let conn = accept_session::<C2S, S2C>(session, client, send_sig).await?;
    open_streams::<C2S, S2C, C>(client, &conn, send_sig.clone(), recv_req.resubscribe()).await?;

    let _ = send_sig.send(Signal::Connected { client });
    loop {
        let _ = send_sig.send(Signal::UpdateEndpointInfo {
            client,
            info: EndpointInfo::from_connection(&conn),
        });

        tokio::select! {
            req = recv_req.recv() => {
                let Ok(req) = req else {
                    return Ok(());
                };
                match req {
                    Request::Send { to, msg } if to == client && msg.channel().channel_id() == ChannelId::Datagram => {
                        send_datagram::<C2S, S2C>(&conn, msg).await
                            .map_err(|err| WebTransportError::on(ChannelId::Datagram, err))?;
                    }
                    Request::Disconnect { target } if target == client => {
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn send_datagram<C2S, S2C>(conn: &Connection, msg: S2C) -> Result<(), ChannelError<C2S, S2C>>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes,
{
    let payload = msg.try_into_bytes().map_err(ChannelError::Serialize)?;
    conn.send_datagram(payload).map_err(ChannelError::SendDatagram)
}

async fn accept_session<C2S, S2C>(
    session: IncomingSession,
    client: ClientKey,
    send_sig: &mut mpsc::UnboundedSender<Signal<C2S, S2C>>,
) -> Result<Connection, WebTransportError<C2S, S2C>>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes,
{
    let req = session.await.map_err(WebTransportError::IncomingSession)?;

    let _ = send_sig.send(Signal::Accepted {
        client,
        authority: req.authority().to_owned(),
        path: req.path().to_owned(),
        origin: req.origin().map(ToOwned::to_owned),
        user_agent: req.user_agent().map(ToOwned::to_owned),
    });

    let conn = req
        .accept()
        .await
        .map_err(WebTransportError::AcceptSession)?;

    Ok(conn)
}

//
// IMPORTANT:
// The SERVER will OPEN the streams!!!
// The CLIENT will ACCEPT them!!!
//

async fn open_streams<C2S, S2C, C>(
    client: ClientKey,
    conn: &Connection,
    send_sig: mpsc::UnboundedSender<Signal<C2S, S2C>>,
    recv_req: broadcast::Receiver<Request<S2C>>,
) -> Result<Vec<oneshot::Receiver<ChannelError<C2S, S2C>>>, WebTransportError<C2S, S2C>>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
    C: Channels,
{
    try_join_all((0..C::NUM_STREAMS).map(|stream_id| {
        let channel = ChannelId::Stream(stream_id);
        let send_sig = send_sig.clone();
        let recv_req = recv_req.resubscribe();
        async move {
            let recv_error = open_stream::<C2S, S2C>(client, conn, channel, send_sig, recv_req)
                .await
                .map_err(|err| WebTransportError::on(channel, err))?;
            Ok::<_, WebTransportError<_, _>>(recv_error)
        }
    }))
    .await
}

async fn open_stream<C2S, S2C>(
    client: ClientKey,
    conn: &Connection,
    channel: ChannelId,
    send_sig: mpsc::UnboundedSender<Signal<C2S, S2C>>,
    recv_req: broadcast::Receiver<Request<S2C>>,
) -> Result<oneshot::Receiver<ChannelError<C2S, S2C>>, ChannelError<C2S, S2C>>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel,
{
    let (send, recv) = conn
        .open_bi()
        .await
        .map_err(ChannelError::RequestOpenStream)?
        .await
        .map_err(ChannelError::OpenStream)?;

    let (send_err, recv_err) = oneshot::channel();
    tokio::spawn(async move {
        if let Err(err) = handle_stream(client, channel, send, recv, send_sig, recv_req).await {
            let _ = send_err.send(err);
        }
    });

    Ok(recv_err)
}

async fn handle_stream<C2S, S2C>(
    client: ClientKey,
    channel: ChannelId,
    mut send: SendStream,
    mut recv: RecvStream,
    send_sig: mpsc::UnboundedSender<Signal<C2S, S2C>>,
    mut recv_req: broadcast::Receiver<Request<S2C>>,
) -> Result<(), ChannelError<C2S, S2C>>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel,
{
    let mut buf = [0u8; DATA_CAP];
    loop {
        tokio::select! {
            req = recv_req.recv() => {
                let Ok(req) = req else {
                    // frontend closed
                    return Ok(());
                };
                match req {
                    Request::Send { to, msg } if to == client && msg.channel().channel_id() == channel => {
                        let payload = msg.try_into_bytes().map_err(ChannelError::Serialize)?;
                        send.write_all(&payload).await
                            .map_err(ChannelError::WriteStream)?;
                    }
                    Request::Disconnect { target } if target == client => {
                        return Ok(());
                    }
                    _ => {}
                }
            }
            read = recv.read(&mut buf) => {
                let Some(bytes_read) = read.map_err(ChannelError::ReadStream)? else {
                    continue;
                };
                let msg = C2S::try_from_bytes(&buf[..bytes_read])
                    .map_err(ChannelError::Deserialize)?;
                let _ = send_sig.send(Signal::Recv { from: client, msg });
            }
        }
    }
}
