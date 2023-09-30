use std::{convert::Infallible, io};

use aeronet::{ClientId, RecvMessage, SendMessage, ServerTransportConfig, SessionError};
use log::debug;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug_span, Instrument};
use wtransport::{
    datagram::Datagram,
    endpoint::{IncomingSession, Server},
    error::{ConnectionError, StreamReadError},
    Connection, Endpoint, SendStream, ServerConfig,
};

use crate::{StreamDefinitions, StreamId, StreamKind};

use super::{ClientInfo, InternalEvent, Request, ServerStream, StreamError, CHANNEL_BUF};

const RECV_BUF: usize = 65536;

pub struct WebTransportBackend<C: ServerTransportConfig> {
    pub(crate) config: ServerConfig,
    pub(crate) streams: StreamDefinitions,
    pub(crate) send_b2f: mpsc::Sender<InternalEvent<C::C2S>>,
    pub(crate) send_f2b: broadcast::Sender<Request<C::S2C>>,
}

impl<C: ServerTransportConfig> WebTransportBackend<C> {
    pub async fn listen(self) -> Result<(), io::Error> {
        let Self {
            config,
            streams,
            send_b2f,
            send_f2b,
        } = self;

        let endpoint = Endpoint::server(config)?;
        listen::<C>(endpoint, streams, send_b2f, send_f2b).await;
        Ok(())
    }
}

async fn listen<C: ServerTransportConfig>(
    endpoint: Endpoint<Server>,
    streams: StreamDefinitions,
    send_evt: mpsc::Sender<InternalEvent<C::C2S>>,
    send_req: broadcast::Sender<Request<C::S2C>>,
) {
    debug!("Started WebTransport server backend");

    let (send_close, mut recv_close) = mpsc::channel::<()>(1);
    for client in 0.. {
        debug!("Waiting for connection");
        let req = tokio::select! {
            req = endpoint.accept() => req,
            _ = recv_close.recv() => break
        };

        let client = ClientId::from_raw(client);

        let streams = streams.clone();
        let send = send_evt.clone();
        let recv = send_req.subscribe();

        let send_close = send_close.clone();

        tokio::spawn(
            async move {
                let reason = handle_session::<C>(streams, send.clone(), recv, client, req)
                    .await
                    .unwrap_err();
                if send
                    .send(InternalEvent::Disconnected { client, reason })
                    .await
                    .is_err()
                {
                    let _ = send_close.send(()).await;
                }
            }
            .instrument(debug_span!("Session", id = tracing::field::display(client))),
        );
    }

    debug!("Stopped WebTransport server backend");
}

async fn handle_session<C: ServerTransportConfig>(
    streams: StreamDefinitions,
    mut send: mpsc::Sender<InternalEvent<C::C2S>>,
    mut recv: broadcast::Receiver<Request<C::S2C>>,
    client: ClientId,
    req: IncomingSession,
) -> Result<Infallible, SessionError> {
    let mut conn = accept_session::<C>(&mut send, client, req).await?;

    let (send_c2s, mut recv_c2s) = mpsc::channel::<C::C2S>(CHANNEL_BUF);
    let (send_err, mut recv_err) = mpsc::channel::<SessionError>(CHANNEL_BUF);
    let (mut streams_bi, mut streams_s2c) =
        open_streams::<C>(&streams, &mut conn, send_c2s, send_err).await?;

    loop {
        send.send(InternalEvent::UpdateInfo {
            client,
            info: ClientInfo::from_connection(&conn),
        });
        tokio::select! {
            result = conn.receive_datagram() => {
                let msg = recv_datagram::<C>(result)
                    .await
                    .map_err(|err| SessionError::Transport(err.on(StreamKind::Datagram).into()))?;
                send.send(InternalEvent::Recv { client, msg })
                    .await
                    .map_err(|_| SessionError::ServerClosed)?;
            }
            Some(msg) = recv_c2s.recv() => {
                send.send(InternalEvent::Recv { client, msg })
                    .await
                    .map_err(|_| SessionError::ServerClosed)?;
            }
            Some(err) = recv_err.recv() => {
                return Err(err);
            }
            result = recv.recv() => {
                let req = result.map_err(|_| SessionError::ServerClosed)?;
                match req {
                    Request::Send { client: target, stream, msg } if target == client => {
                        send_client::<C>(&mut conn, &mut streams_bi, &mut streams_s2c, stream, msg)
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

async fn accept_session<C: ServerTransportConfig>(
    send: &mut mpsc::Sender<InternalEvent<C::C2S>>,
    client: ClientId,
    req: IncomingSession,
) -> Result<Connection, SessionError> {
    debug!("Incoming connection");

    let req = req
        .await
        .map_err(|err| SessionError::Connecting(err.into()))?;

    debug!(
        "Requesting session, authority: {:?} / path: {:?}",
        req.authority(),
        req.path()
    );
    send.send(InternalEvent::Incoming {
        client,
        info: ClientInfo::from_request(&req),
    })
    .await
    .map_err(|_| SessionError::ServerClosed)?;

    let conn = req
        .accept()
        .await
        .map_err(|err| SessionError::Connecting(err.into()))?;

    let remote_addr = conn.remote_address();
    debug!("Connected from {remote_addr}");
    send.send(InternalEvent::Connected { client })
        .await
        .map_err(|_| SessionError::ServerClosed)?;

    Ok(conn)
}

// streams

async fn open_streams<C: ServerTransportConfig>(
    streams: &StreamDefinitions,
    mut conn: &mut Connection,
    send_c2s: mpsc::Sender<C::C2S>,
    send_err: mpsc::Sender<SessionError>,
) -> Result<(Vec<mpsc::Sender<C::S2C>>, Vec<mpsc::Sender<C::S2C>>), SessionError> {
    let mut streams_bi = Vec::new();
    for stream_id in 0..streams.bi {
        let stream = StreamKind::Bi(StreamId(stream_id));
        let send = open_bi::<C>(&mut conn, stream, send_c2s.clone(), send_err.clone())
            .await
            .map_err(|err| SessionError::Transport(err.on(stream).into()))?;
        streams_bi.push(send);
    }

    let mut streams_s2c = Vec::new();
    for stream_id in 0..streams.s2c {
        let stream = StreamKind::S2C(StreamId(stream_id));
        let send = open_s2c::<C>(&mut conn, stream, send_err.clone())
            .await
            .map_err(|err| SessionError::Transport(err.on(stream).into()))?;
        streams_s2c.push(send);
    }

    for stream_id in 0..streams.c2s {
        let stream = StreamKind::C2S(StreamId(stream_id));
        open_c2s::<C>(&mut conn, stream, send_c2s.clone(), send_err.clone())
            .await
            .map_err(|err| SessionError::Transport(err.on(stream).into()))?;
    }

    Ok((streams_bi, streams_s2c))
}

async fn open_bi<C: ServerTransportConfig>(
    conn: &mut Connection,
    stream: StreamKind,
    mut send_c2s: mpsc::Sender<C::C2S>,
    send_err: mpsc::Sender<SessionError>,
) -> Result<mpsc::Sender<C::S2C>, StreamError> {
    let (mut send, mut recv) = conn
        .open_bi()
        .await
        .map_err(|err| StreamError::Open(err.into()))?
        .await
        .map_err(|err| StreamError::Open(err.into()))?;

    let (send_s2c, mut recv_s2c) = mpsc::channel::<C::S2C>(CHANNEL_BUF);
    let f = async move {
        let mut buf = [0u8; RECV_BUF];
        loop {
            tokio::select! {
                result = recv.read(&mut buf) => {
                    recv_stream::<C>(&mut send_c2s, &buf, result).await?;
                }
                result = recv_s2c.recv() => {
                    send_stream::<C>(&mut send, result).await?;
                }
            }
        }
    };

    tokio::spawn(async move {
        if let Err::<(), StreamError>(err) = f.await {
            let _ = send_err
                .send(SessionError::Transport(err.on(stream).into()))
                .await;
        }
    });
    Ok(send_s2c)
}

async fn open_s2c<C: ServerTransportConfig>(
    conn: &mut Connection,
    stream: StreamKind,
    send_err: mpsc::Sender<SessionError>,
) -> Result<mpsc::Sender<C::S2C>, StreamError> {
    let mut send = conn
        .open_uni()
        .await
        .map_err(|err| StreamError::Open(err.into()))?
        .await
        .map_err(|err| StreamError::Open(err.into()))?;

    let (send_s2c, mut recv_s2c) = mpsc::channel::<C::S2C>(CHANNEL_BUF);
    let f = async move {
        loop {
            let result = recv_s2c.recv().await;
            send_stream::<C>(&mut send, result).await?;
        }
    };

    tokio::spawn(async move {
        if let Err::<(), StreamError>(err) = f.await {
            let _ = send_err
                .send(SessionError::Transport(err.on(stream).into()))
                .await;
        }
    });
    Ok(send_s2c)
}

async fn open_c2s<C: ServerTransportConfig>(
    conn: &mut Connection,
    stream: StreamKind,
    mut send_c2s: mpsc::Sender<C::C2S>,
    send_err: mpsc::Sender<SessionError>,
) -> Result<(), StreamError> {
    let mut recv = conn
        .accept_uni()
        .await
        .map_err(|err| StreamError::Open(err.into()))?;

    let f = async move {
        let mut buf = [0u8; RECV_BUF];
        loop {
            let result = recv.read(&mut buf).await;
            recv_stream::<C>(&mut send_c2s, &buf, result).await?;
        }
    };

    tokio::spawn(async move {
        if let Err::<(), StreamError>(err) = f.await {
            let _ = send_err
                .send(SessionError::Transport(err.on(stream).into()))
                .await;
        }
    });
    Ok(())
}

// receiving

fn from_payload<C2S: RecvMessage>(buf: &[u8]) -> Result<C2S, StreamError> {
    C2S::from_payload(buf).map_err(|err| StreamError::Recv(err.into()))
}

async fn recv_datagram<C: ServerTransportConfig>(
    result: Result<Datagram, ConnectionError>,
) -> Result<C::C2S, StreamError> {
    let datagram = result.map_err(|err| StreamError::Recv(err.into()))?;
    let msg = from_payload::<C::C2S>(&datagram)?;
    Ok(msg)
}

async fn recv_stream<C: ServerTransportConfig>(
    send: &mpsc::Sender<C::C2S>,
    buf: &[u8; RECV_BUF],
    result: Result<Option<usize>, StreamReadError>,
) -> Result<(), StreamError> {
    let read = result
        .map_err(|err| StreamError::Recv(err.into()))?
        .ok_or_else(|| StreamError::Closed)?;
    let msg = from_payload::<C::C2S>(&buf[..read])?;
    send.send(msg).await.map_err(|_| StreamError::Closed)?;
    Ok(())
}

// sending

fn into_payload<S2C: SendMessage>(msg: S2C) -> Result<Vec<u8>, StreamError> {
    msg.into_payload()
        .map_err(|err| StreamError::Send(err.into()))
}

async fn send_client<C: ServerTransportConfig>(
    conn: &mut Connection,
    streams_bi: &mut [mpsc::Sender<C::S2C>],
    streams_s2c: &mut [mpsc::Sender<C::S2C>],
    stream: ServerStream,
    msg: C::S2C,
) -> Result<(), StreamError> {
    async fn on_stream<C: ServerTransportConfig>(
        stream: &mut mpsc::Sender<C::S2C>,
        msg: C::S2C,
    ) -> Result<(), StreamError> {
        stream.send(msg).await.map_err(|_| StreamError::Closed)?;
        Ok(())
    }

    match stream {
        ServerStream::Datagram => {
            let buf = into_payload(msg)?;
            conn.send_datagram(buf)
                .map_err(|err| StreamError::Send(err.into()))?;
        }
        ServerStream::Bi(i) => {
            on_stream::<C>(&mut streams_bi[i.0], msg).await?;
        }
        ServerStream::S2C(i) => {
            on_stream::<C>(&mut streams_s2c[i.0], msg).await?;
        }
    }
    Ok(())
}

async fn send_stream<C: ServerTransportConfig>(
    send: &mut SendStream,
    result: Option<C::S2C>,
) -> Result<(), StreamError> {
    let msg = result.ok_or_else(|| StreamError::Closed)?;
    let buf = into_payload(msg)?;
    send.write_all(&buf)
        .await
        .map_err(|err| StreamError::Send(err.into()))?;
    Ok(())
}