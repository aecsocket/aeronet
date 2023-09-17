#![warn(clippy::future_not_send)]

#[cfg(feature = "bevy")]
pub mod plugin;

use futures::{stream::FuturesUnordered, Future, StreamExt};
use log::debug;
use tracing::{debug_span, Instrument};

use std::{collections::HashMap, io};

use anyhow::Result;
use tokio::sync::{broadcast, mpsc};
use wtransport::{
    endpoint::IncomingSession,
    error::{ConnectionError, StreamOpeningError},
    Connection, Endpoint, RecvStream, SendStream, ServerConfig, datagram::Datagram,
};

use crate::{
    stream::{Stream, StreamId, Streams},
    ClientId, DisconnectReason, Message, TransportConfig,
};

const INTERNAL_CHANNEL_BUF: usize = 128;
const RECV_BUF: usize = 65536;

#[derive(Debug)]
pub enum B2F<C2S> {
    Started,
    ServerError(ServerError),
    Incoming {
        client: ClientId,
        authority: String,
        path: String,
        headers: HashMap<String, String>,
    },
    Connected {
        client: ClientId,
    },
    Recv {
        client: ClientId,
        msg: C2S,
    },
    Disconnected {
        client: ClientId,
        reason: ServerDisconnectReason,
    },
}

#[derive(Debug, Clone)]
pub enum F2B<S2C> {
    Send {
        client: ClientId,
        stream: Stream,
        msg: S2C,
    },
    Disconnect {
        client: ClientId,
    },
}

#[derive(Debug, thiserror::Error)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Event))]
pub enum ServerError {
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum ServerDisconnectReason {
    #[error("forced by server")]
    Forced,
    #[error("transport error")]
    Error(#[from] SessionError),
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("frontend closed")]
    Closed,
    // setup
    #[error("failed to receive incoming session")]
    RecvSession(#[source] ConnectionError),
    #[error("failed to accept session")]
    AcceptSession(#[source] ConnectionError),
    #[error("failed to connect stream")]
    ConnectStream(#[source] ConnectionError),
    #[error("failed to open stream")]
    OpenStream(#[source] StreamOpeningError),
    #[error("failed to accept stream")]
    AcceptStream(#[source] ConnectionError),
    // recv
    #[error("failed to receive data")]
    Recv(#[source] anyhow::Error),
    #[error("failed to deserialize incoming data")]
    Deserialize(#[source] anyhow::Error),
    // send
    #[error("failed to send data")]
    Send(#[source] anyhow::Error),
    #[error("failed to serialize outgoing data")]
    Serialize(anyhow::Error),
}

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
#[non_exhaustive]
pub struct WtServerFrontend<C: TransportConfig> {
    pub send: broadcast::Sender<F2B<C::S2C>>,
    pub recv: mpsc::Receiver<B2F<C::C2S>>,
}

pub struct WtServerBackend<C: TransportConfig> {
    config: ServerConfig,
    streams: Streams,
    send_b2f: mpsc::Sender<B2F<C::C2S>>,
    send_f2b: broadcast::Sender<F2B<C::S2C>>,
}

pub fn create<C: TransportConfig>(
    config: ServerConfig,
    streams: Streams,
) -> (WtServerFrontend<C>, WtServerBackend<C>) {
    let (send_b2f, recv_b2f) = mpsc::channel::<B2F<C::C2S>>(INTERNAL_CHANNEL_BUF);
    let (send_f2b, recv_f2b) = broadcast::channel::<F2B<C::S2C>>(INTERNAL_CHANNEL_BUF);

    let frontend = WtServerFrontend::<C> {
        send: send_f2b.clone(),
        recv: recv_b2f,
    };

    let backend = WtServerBackend::<C> {
        config,
        streams,
        send_b2f,
        send_f2b,
    };

    (frontend, backend)
}

impl<C: TransportConfig> WtServerBackend<C> {
    pub async fn listen(self) {
        let Self {
            config,
            streams,
            send_b2f,
            send_f2b,
        } = self;

        if let Err(err) = listen::<C>(config, streams, send_b2f.clone(), send_f2b).await {
            let _ = send_b2f.send(B2F::ServerError(err)).await;
        }
    }
}

async fn listen<C: TransportConfig>(
    config: ServerConfig,
    streams: Streams,
    send_b2f: mpsc::Sender<B2F<C::C2S>>,
    send_f2b: broadcast::Sender<F2B<C::S2C>>,
) -> Result<(), ServerError> {
    let endpoint = Endpoint::server(config).map_err(|err| ServerError::CreateEndpoint(err))?;

    debug!("Started WebTransport server backend");
    let _ = send_b2f.send(B2F::Started).await;

    // if one of the sessions encounters a fatal error (e.g. frontend closed), it sends a message
    // to stop this function
    let (send_close, mut recv_close) = mpsc::channel::<()>(1);
    for client in 0.. {
        debug!("Waiting for connection or close");
        let req = tokio::select! {
            req = endpoint.accept() => req,
            _ = recv_close.recv() => break
        };

        let streams = streams.clone();
        let send = send_b2f.clone();
        let recv = send_f2b.subscribe();
        let send_close = send_close.clone();

        tokio::spawn(async move {
            if let Err(_) = accept_session::<C>(streams, send, recv, ClientId::from_raw(client), req)
                .instrument(debug_span!("Session", id = client))
                .await
            {
                let _ = send_close.send(()).await;
            }
        });
    }

    debug!("Stopped WebTransport server backend");
    Ok(())
}

async fn accept_session<C: TransportConfig>(
    streams: Streams,
    send: mpsc::Sender<B2F<C::C2S>>,
    recv: broadcast::Receiver<F2B<C::S2C>>,
    client: ClientId,
    req: IncomingSession,
) -> Result<(), ()> {
    debug!("Incoming connection");

    let reason = match handle_session::<C>(streams, send.clone(), recv, client, req).await {
        Ok(reason) => reason,
        Err(err) => ServerDisconnectReason::from(err),
    };

    debug!("Disconnected: {:#}", aeronet::error::AsPrettyError::as_pretty(&reason));
    send
        .send(B2F::Disconnected { client, reason })
        .await 
        .map_err(|_| ())?;

    Ok(())
}

struct Recv {
    stream: RecvStream,
    buf: [u8; RECV_BUF],
}

struct Bi {
    send: SendStream,
    recv: Recv,
}

async fn handle_session<C: TransportConfig>(
    streams: Streams,
    mut send: mpsc::Sender<B2F<C::C2S>>,
    mut recv: broadcast::Receiver<F2B<C::S2C>>,
    client: ClientId,
    req: IncomingSession,
) -> Result<ServerDisconnectReason, SessionError> {
    let mut conn = open_connection::<C>(&mut send, client, req).await?;
    let (mut bi, mut c2s, mut s2c) = open_streams(&mut conn, &streams).await?;

    debug!("Connected");
    let _ = send.send(B2F::Connected { client }).await;

    async fn forward_recv<C: TransportConfig>(
        send: &mut mpsc::Sender<B2F<C::C2S>>,
        client: ClientId,
        msg: C::C2S,
    ) -> Result<(), SessionError> {
        send.send(B2F::Recv { client, msg })
            .await
            .map_err(|_| SessionError::Closed)
    }

    loop {
        tokio::select! {
            // recv from client, send to frontend
            result = conn.receive_datagram() => {
                let msg = recv_datagram::<C>(result).await?;
                let _ = forward_recv::<C>(&mut send, client, msg).await;
            }
            // Some(result) = async {
            //     let iter = bi.iter_mut().map(|Bi { recv, .. }| recv);
            //     recv_stream::<C>(assert_send(iter)).await
            // } => {
            //     let result = result?;
            //     forward_recv::<C>(&mut send, client, result).await?;
            // }
            Some(result) = recv_stream::<C>(c2s.iter_mut()) => {
                let result = result?;
                forward_recv::<C>(&mut send, client, result).await?;
            }
            // recv from frontend, send to client
            result = recv.recv() => {
                match result.map_err(|_| SessionError::Closed)? {
                    F2B::Send { client: target, stream, msg } if target == client => {
                        send_msg::<C>(stream, &mut conn, &mut bi, &mut s2c, msg).await?;
                    }
                    F2B::Disconnect { client: target } if target == client => {
                        debug!("Forcing disconnect");
                        return Ok(ServerDisconnectReason::Forced);
                    }
                    _ => {},
                }
            }
        }
    }
}

// https://github.com/rust-lang/rust/issues/102211
fn assert_send<'a, T>(
    it: impl 'a + Send + Iterator<Item = T>,
) -> impl 'a + Send + Iterator<Item = T> {
    it
}

async fn open_connection<C: TransportConfig>(
    send: &mut mpsc::Sender<B2F<C::C2S>>,
    client: ClientId,
    req: IncomingSession,
) -> Result<Connection, SessionError> {
    let conn = req
        .await
        .map_err(|err| SessionError::RecvSession(err))?;

    let authority = conn.authority();
    debug!("Connecting from {authority}");
    send.send(B2F::Incoming {
            client,
            authority: authority.to_owned(),
            path: conn.path().to_owned(),
            headers: conn.headers().clone(),
        })
        .await
        .map_err(|_| SessionError::Closed)?;

    let conn = conn
        .accept()
        .await
        .map_err(|err| SessionError::AcceptSession(err))?;

    Ok(conn)
}

async fn open_streams(
    conn: &mut Connection,
    streams: &Streams,
) -> Result<(Vec<Bi>, Vec<Recv>, Vec<SendStream>), SessionError> {
    async fn collect_results<T>(
        iter: impl Iterator<Item = impl Future<Output = Result<T, SessionError>>>,
    ) -> Result<Vec<T>, SessionError> {
        iter.collect::<FuturesUnordered<_>>()
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, SessionError>>()
    }

    let bi: Vec<Bi> = collect_results((0..streams.bi).map(|_| async {
        let (send, recv) = conn
            .open_bi()
            .await
            .map_err(|err| SessionError::ConnectStream(err))?
            .await
            .map_err(|err| SessionError::OpenStream(err))?;
        let buf = [0; RECV_BUF];
        Ok(Bi {
            send,
            recv: Recv { stream: recv, buf },
        })
    }))
    .await?;

    let c2s: Vec<Recv> = collect_results((0..streams.c2s).map(|_| async {
        let recv = conn
            .accept_uni()
            .await
            .map_err(|err| SessionError::AcceptSession(err))?;
        let buf = [0; RECV_BUF];
        Ok(Recv { stream: recv, buf })
    }))
    .await?;

    let s2c: Vec<SendStream> = collect_results((0..streams.s2c).map(|_| async {
        let send = conn
            .open_uni()
            .await
            .map_err(|err| SessionError::ConnectStream(err))?
            .await
            .map_err(|err| SessionError::OpenStream(err))?;
        Ok(send)
    }))
    .await?;

    Ok((bi, c2s, s2c))
}

async fn recv_datagram<C: TransportConfig>(
    result: Result<Datagram, ConnectionError>,
) -> Result<C::C2S, SessionError> {
    let datagram = result.map_err(|err| SessionError::Recv(err.into()))?;
    let msg = C::C2S::from_payload(&datagram)
        .map_err(|err| SessionError::Deserialize(err))?;
    Ok(msg)
}

async fn recv_stream<C: TransportConfig>(
    streams: impl IntoIterator<Item = &mut Recv>,
) -> Option<Result<C::C2S, SessionError>> {
    streams
        .into_iter()
        .map(|Recv { stream, buf }| async {
            let read = loop {
                match stream
                    .read(&mut buf[..])
                    .await
                    .map_err(|err| SessionError::Recv(err.into()))?
                {
                    Some(read) => break read,
                    None => {}
                }
            };
            let msg = C::C2S::from_payload(&buf[..read])
                .map_err(|err| SessionError::Deserialize(err))?;
            Ok::<_, SessionError>(msg)
        })
        .collect::<FuturesUnordered<_>>()
        .next()
        .await
}

async fn send_msg<C: TransportConfig>(
    stream: Stream,
    conn: &mut Connection,
    bi: &mut [Bi],
    s2c: &mut [SendStream],
    msg: C::S2C,
) -> Result<(), SessionError> {
    let payload = msg
        .into_payload()
        .map_err(|err| SessionError::Serialize(err))?;
    
    let res: Result<_, anyhow::Error> = match stream {
        Stream::Datagram => {
            conn.send_datagram(payload).map_err(|err| err.into())
        }
        Stream::Bi(StreamId(index)) => {
            let Bi { send, .. } = &mut bi[index];
            send.write_all(&payload).await.map_err(|err| err.into())
        }
        Stream::Uni(StreamId(index)) => {
            let send = &mut s2c[index];
            send.write_all(&payload).await.map_err(|err| err.into())
        }
    };
    res.map_err(|err| SessionError::Send(err))
}
