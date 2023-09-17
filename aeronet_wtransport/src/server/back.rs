use std::{
    convert::Infallible,
    io,
};

use log::debug;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug_span, Instrument};
use wtransport::{
    datagram::Datagram,
    endpoint::{IncomingSession, Server},
    error::{ConnectionError, StreamReadError},
    Connection, Endpoint, RecvStream, SendStream, ServerConfig,
};

use crate::{Message, StreamId, StreamKind, Streams, TransportConfig};

use super::{
    ClientId, ClientInfo, ServerStream, SessionError, SharedClients, StreamError, Event, Request,
};

pub struct Backend<C: TransportConfig> {
    pub(crate) config: ServerConfig,
    pub(crate) streams: Streams,
    pub(crate) send_b2f: mpsc::Sender<Event<C::C2S>>,
    pub(crate) send_f2b: broadcast::Sender<Request<C::S2C>>,
    pub(crate) clients: SharedClients,
}

impl<C: TransportConfig> Backend<C> {
    pub async fn listen(self) -> Result<(), io::Error> {
        let Self {
            config,
            streams,
            send_b2f,
            send_f2b,
            clients,
        } = self;

        let endpoint = Endpoint::server(config)?;
        listen::<C>(endpoint, streams, send_b2f, send_f2b, clients).await;
        Ok(())
    }
}

async fn listen<C: TransportConfig>(
    endpoint: Endpoint<Server>,
    streams: Streams,
    send_evt: mpsc::Sender<Event<C::C2S>>,
    send_req: broadcast::Sender<Request<C::S2C>>,
    clients: SharedClients,
) {
    debug!("Started WebTransport server backend");

    let (send_close, mut recv_close) = mpsc::channel::<()>(1);
    loop {
        debug!("Waiting for connection");
        let req = tokio::select! {
            req = endpoint.accept() => req,
            _ = recv_close.recv() => break
        };

        let client = ClientId(clients.lock().unwrap().insert(None));

        let streams = streams.clone();
        let send = send_evt.clone();
        let recv = send_req.subscribe();
        let clients = clients.clone();

        let send_close = send_close.clone();

        tokio::spawn(
            async move {
                let reason =
                    handle_session::<C>(streams, send.clone(), recv, clients.clone(), client, req)
                        .await
                        .unwrap_err();
                if send
                    .send(Event::Disconnected { client, reason })
                    .await
                    .is_err()
                {
                    let _ = send_close.send(()).await;
                }
                clients.lock().unwrap().remove(client.0);
            }
            .instrument(debug_span!("Session", id = tracing::field::display(client))),
        );
    }

    debug!("Stopped WebTransport server backend");
}

async fn handle_session<C: TransportConfig>(
    streams: Streams,
    mut send: mpsc::Sender<Event<C::C2S>>,
    mut recv: broadcast::Receiver<Request<C::S2C>>,
    clients: SharedClients,
    client: ClientId,
    req: IncomingSession,
) -> Result<Infallible, SessionError> {
    let mut conn = accept_session::<C>(&mut send, client, req).await?;

    loop {
        tokio::select! {
            result = conn.receive_datagram() => {
                let msg = recv_datagram::<C>(result)
                    .await
                    .map_err(|source| SessionError::Stream {
                        stream: StreamKind::Datagram,
                        source,
                    })?;
                send.send(Event::Recv { client, msg })
                    .await
                    .map_err(|_| SessionError::ServerClosed)?;
            }
            result = recv.recv() => {
                let req = result.map_err(|_| SessionError::ServerClosed)?;
                match req {
                    Request::Send { client: target, stream, msg } if target == client => {
                        send_client::<C>(&mut conn, stream, msg)
                            .await
                            .map_err(|source| SessionError::Stream {
                                stream: stream.as_kind(),
                                source,
                            })?;
                    }
                    Request::Disconnect { client: target } if target == client => {
                        return Err(SessionError::ForceDisconnect);
                    }
                    Request::UpdateInfo { client: target } if target == client => {
                        let new_info = Some(ClientInfo::from(&conn));
                        *&mut clients.lock().unwrap()[client.0] = new_info;
                    }
                    _ => {},
                }
            }
        }
    }
}

async fn accept_session<C: TransportConfig>(
    send: &mut mpsc::Sender<Event<C::C2S>>,
    client: ClientId,
    req: IncomingSession,
) -> Result<Connection, SessionError> {
    debug!("Incoming connection");

    let conn = req.await.map_err(|err| SessionError::RecvSession(err))?;

    let authority = conn.authority();
    let path = conn.path();
    debug!("Connecting, authority: {authority} / path: {path}");
    send.send(Event::Connecting {
        client,
        authority: authority.to_owned(),
        path: path.to_owned(),
        headers: conn.headers().clone(),
    })
    .await
    .map_err(|_| SessionError::ServerClosed)?;

    let conn = conn
        .accept()
        .await
        .map_err(|err| SessionError::AcceptSession(err))?;

    let remote_addr = conn.remote_address();
    debug!("Connected from {remote_addr}");
    send.send(Event::Connected { client })
        .await
        .map_err(|_| SessionError::ServerClosed)?;

    Ok(conn)
}

async fn recv_datagram<C: TransportConfig>(
    result: Result<Datagram, ConnectionError>,
) -> Result<C::C2S, StreamError> {
    let datagram = result.map_err(|err| StreamError::Recv(err.into()))?;
    let msg = C::C2S::from_payload(&datagram).map_err(|err| StreamError::Deserialize(err))?;
    Ok(msg)
}

async fn send_client<C: TransportConfig>(
    conn: &mut Connection,
    stream: ServerStream,
    msg: C::S2C,
) -> Result<(), StreamError> {
    let payload = msg.into_payload()
        .map_err(|err| StreamError::Serialize(err))?;
    match stream {
        ServerStream::Datagram => {
            conn.send_datagram(payload)
                .map_err(|err| StreamError::Send(err.into()))?;
        }
        _ => todo!(),
    }
    Ok(())
}

/*
async fn handle_connection<C: TransportConfig>(
    streams: Streams,
    mut send: mpsc::Sender<Event<C::C2S>>,
    mut recv: broadcast::Receiver<Request<C::S2C>>,
    client: ClientId,
    conn: Connection,
) -> Result<Infallible, SessionError> {
    let mut conn = open_connection::<C>(&mut send, client, req).await?;

    let (send_c2s, mut recv_c2s) = mpsc::channel::<C::C2S>(INTERNAL_CHANNEL_BUF);
    let (send_err, mut recv_err) = mpsc::channel::<SessionError>(INTERNAL_CHANNEL_BUF);

    let (mut send_bi, mut send_uni) =
        open_streams::<C>(&mut conn, &streams, send_c2s, send_err).await?;

    debug!("Connected");
    send.send(Event::Connected { client })
        .await
        .map_err(|_| SessionError::ServerClosed)?;

    loop {
        tokio::select! {
            // recv from client (through other tasks), send to frontend
            result = conn.receive_datagram() => {
                let msg = recv_datagram::<C>(result)
                    .await
                    .map_err(|source| SessionError::Stream {
                        stream: StreamKind::Datagram,
                        source,
                    })?;
                forward_recv::<C>(&mut send, client, msg).await?;
            }
            Some(msg) = recv_c2s.recv() => {
                forward_recv::<C>(&mut send, client, msg).await?;
            }
            Some(err) = recv_err.recv() => {
                return Err(err);
            }
            // recv from frontend, send to client
            result = recv.recv() => {
                match result.map_err(|_| SessionError::ServerClosed)? {
                    Request::Send { client: target, stream, msg } if target == client => {
                        send_msg::<C>(stream, &mut conn, &mut send_bi, &mut send_uni, msg)
                            .await
                            .map_err(|source| SessionError::Stream {
                                stream: stream.as_kind(),
                                source,
                            })?;
                    }
                    Request::Disconnect{ client: target } if target == client => {
                        return Err(SessionError::ForceDisconnect);
                    }
                    _ => {},
                }
            }
        }
    }
}

async fn open_connection<C: TransportConfig>(
    send: &mut mpsc::Sender<Event<C::C2S>>,
    client: ClientId,
    req: IncomingSession,
) -> Result<Connection, SessionError> {
    let conn = req.await.map_err(|err| SessionError::RecvSession(err))?;

    let authority = conn.authority();
    debug!("Connecting from {authority}");
    send.send(Event::Connecting {
        client,
        authority: authority.to_owned(),
        path: conn.path().to_owned(),
        headers: conn.headers().clone(),
    })
    .await
    .map_err(|_| SessionError::ServerClosed)?;

    let conn = conn
        .accept()
        .await
        .map_err(|err| SessionError::AcceptSession(err))?;

    Ok(conn)
}

async fn open_streams<C: TransportConfig>(
    conn: &mut Connection,
    streams: &Streams,
    send_c2s: mpsc::Sender<C::C2S>,
    send_err: mpsc::Sender<SessionError>,
) -> Result<(Vec<mpsc::Sender<Vec<u8>>>, Vec<mpsc::Sender<Vec<u8>>>), SessionError> {
    let mut send_bi = Vec::new();
    for id in 0..streams.bi {
        let stream = StreamKind::Bi(StreamId(id));

        let (send, recv) = conn
            .open_bi()
            .await
            .map_err(|err| SessionError::Stream {
                stream,
                source: StreamError::Connect(err),
            })?
            .await
            .map_err(|err| SessionError::Stream {
                stream,
                source: StreamError::Open(err),
            })?;

        let send_c2s = send_c2s.clone();
        let send_err = send_err.clone();
        let (send_s2c_buf, recv_s2c_buf) = mpsc::channel::<Vec<u8>>(INTERNAL_CHANNEL_BUF);

        tokio::spawn(async move {
            let source = handle_bi::<C>(send, recv, send_c2s, recv_s2c_buf)
                .await
                .unwrap_err();
            let _ = send_err.send(SessionError::Stream { stream, source }).await;
        });

        send_bi.push(send_s2c_buf);
    }

    let mut send_uni = Vec::new();
    for id in 0..streams.s2c {
        let stream = StreamKind::S2C(StreamId(id));

        let send = conn
            .open_uni()
            .await
            .map_err(|err| SessionError::Stream {
                stream,
                source: StreamError::Connect(err),
            })?
            .await
            .map_err(|err| SessionError::Stream {
                stream,
                source: StreamError::Open(err),
            })?;

        let send_err = send_err.clone();
        let (send_s2c_buf, recv_s2c_buf) = mpsc::channel::<Vec<u8>>(INTERNAL_CHANNEL_BUF);

        tokio::spawn(async move {
            let source = handle_s2c::<C>(send, recv_s2c_buf).await.unwrap_err();
            let _ = send_err.send(SessionError::Stream { stream, source }).await;
        });

        send_uni.push(send_s2c_buf);
    }

    for id in 0..streams.c2s {
        let stream = StreamKind::C2S(StreamId(id));

        let recv = conn
            .accept_uni()
            .await
            .map_err(|err| SessionError::Stream {
                stream,
                source: StreamError::Accept(err),
            })?;

        let send_c2s = send_c2s.clone();
        let send_err = send_err.clone();

        tokio::spawn(async move {
            let source = handle_c2s::<C>(recv, send_c2s).await.unwrap_err();
            let _ = send_err.send(SessionError::Stream { stream, source }).await;
        });
    }

    Ok((send_bi, send_uni))
}

async fn stream_recv<C: TransportConfig>(
    result: Result<Option<usize>, StreamReadError>,
    buf: &[u8; RECV_BUF],
    send_c2s: &mpsc::Sender<C::C2S>,
) -> Result<(), StreamError> {
    let read = result
        .map_err(|err| StreamError::Recv(err.into()))?
        .ok_or_else(|| StreamError::Closed)?;

    let msg = C::C2S::from_payload(&buf[..read]).map_err(|err| StreamError::Deserialize(err))?;

    send_c2s.send(msg).await.map_err(|_| StreamError::Closed)?;

    Ok(())
}

async fn stream_send<C: TransportConfig>(
    result: Option<Vec<u8>>,
    send: &mut SendStream,
) -> Result<(), StreamError> {
    let payload = result.ok_or_else(|| StreamError::Closed)?;
    send.write_all(&payload)
        .await
        .map_err(|err| StreamError::Send(err.into()))?;

    Ok(())
}

async fn handle_bi<C: TransportConfig>(
    mut send: SendStream,
    mut recv: RecvStream,
    send_c2s: mpsc::Sender<C::C2S>,
    mut recv_s2c_buf: mpsc::Receiver<Vec<u8>>,
) -> Result<Infallible, StreamError> {
    let mut buf = [0u8; RECV_BUF];
    loop {
        tokio::select! {
            result = recv.read(&mut buf) => {
                stream_recv::<C>(result, &buf, &send_c2s).await?;
            }
            result = recv_s2c_buf.recv() => {
                stream_send::<C>(result, &mut send).await?;
            }
        }
    }
}

async fn handle_s2c<C: TransportConfig>(
    mut send: SendStream,
    mut recv_s2c_buf: mpsc::Receiver<Vec<u8>>,
) -> Result<Infallible, StreamError> {
    loop {
        let result = recv_s2c_buf.recv().await;
        stream_send::<C>(result, &mut send).await?;
    }
}

async fn handle_c2s<C: TransportConfig>(
    mut recv: RecvStream,
    send_c2s: mpsc::Sender<C::C2S>,
) -> Result<Infallible, StreamError> {
    let mut buf = [0u8; RECV_BUF];
    loop {
        let result = recv.read(&mut buf).await;
        stream_recv::<C>(result, &buf, &send_c2s).await?;
    }
}

async fn forward_recv<C: TransportConfig>(
    send: &mut mpsc::Sender<Event<C::C2S>>,
    client: ClientId,
    msg: C::C2S,
) -> Result<(), SessionError> {
    send.send(Event::Recv { client, msg })
        .await
        .map_err(|_| SessionError::ServerClosed)
}

async fn send_msg<C: TransportConfig>(
    stream: ServerStream,
    conn: &mut Connection,
    send_bi: &mut [mpsc::Sender<Vec<u8>>],
    send_uni: &mut [mpsc::Sender<Vec<u8>>],
    msg: C::S2C,
) -> Result<(), StreamError> {
    let payload = msg
        .into_payload()
        .map_err(|err| StreamError::Serialize(err))?;

    match stream {
        ServerStream::Datagram => conn
            .send_datagram(payload)
            .map_err(|err| StreamError::Send(err.into())),
        ServerStream::Bi(StreamId(index)) => {
            let send = &mut send_bi[index];
            send.send(payload)
                .await
                .map_err(|err| StreamError::Send(err.into()))
        }
        ServerStream::S2C(StreamId(index)) => {
            let send = &mut send_uni[index];
            send.send(payload)
                .await
                .map_err(|err| StreamError::Send(err.into()))
        }
    }
}
*/
