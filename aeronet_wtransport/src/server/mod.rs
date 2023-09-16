#![warn(clippy::future_not_send)]

#[cfg(feature = "bevy")]
pub mod plugin;

use futures::{stream::FuturesUnordered, Future, StreamExt};

use std::{collections::HashMap, io};

use anyhow::Result;
use tokio::sync::{broadcast, mpsc};
use wtransport::{
    endpoint::IncomingSession,
    error::{ConnectionError, StreamOpeningError},
    Connection, Endpoint, RecvStream, SendStream, ServerConfig,
};

use crate::{
    stream::{Stream, StreamId, Streams},
    ClientId, Message, TransportConfig,
};

// messages

const INTERNAL_CHANNEL_BUF: usize = 128;
const RECV_BUF: usize = 65536;

#[derive(Debug)]
pub enum B2F<C2S> {
    Start,
    Incoming {
        client: ClientId,
        authority: String,
        path: String,
        headers: HashMap<String, String>,
    },
    Connect {
        client: ClientId,
    },
    Recv {
        client: ClientId,
        msg: C2S,
    },
    Disconnect {
        client: ClientId,
    },
    Error(ServerError),
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
    // creation
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
    // sessions
    #[error("failed to receive incoming session from {client}")]
    RecvSession {
        client: ClientId,
        #[source]
        source: ConnectionError,
    },
    #[error("failed to accept session from {client}")]
    AcceptSession {
        client: ClientId,
        #[source]
        source: ConnectionError,
    },
    #[error("failed to connect stream for {client}")]
    ConnectStream {
        client: ClientId,
        #[source]
        source: ConnectionError,
    },
    #[error("failed to open stream for {client}")]
    OpenStream {
        client: ClientId,
        #[source]
        source: StreamOpeningError,
    },
    #[error("failed to accept stream from {client}")]
    AcceptStream {
        client: ClientId,
        #[source]
        source: ConnectionError,
    },
    // receiving
    #[error("failed to deserialize data received from {client}")]
    Deserialize {
        client: ClientId,
        #[source]
        source: anyhow::Error,
    },
    #[error("failed to receive data from {client}")]
    Recv {
        client: ClientId,
        #[source]
        source: anyhow::Error,
    },
    // sending
    #[error("failed to serialize data to send to {client}")]
    Serialize {
        client: ClientId,
        #[source]
        source: anyhow::Error,
    },
    #[error("failed to send data to {client}")]
    Send {
        client: ClientId,
        #[source]
        source: anyhow::Error,
    },
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
    recv: broadcast::Receiver<F2B<C::S2C>>,
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
        recv: recv_f2b,
    };

    (frontend, backend)
}

impl<C: TransportConfig> WtServerBackend<C> {
    pub async fn listen(self) {
        let Self {
            config,
            streams,
            send_b2f: send_a2s,
            send_f2b: send_s2a,
            recv,
        } = self;

        if let Err(err) = listen::<C>(config, streams, send_a2s.clone(), send_s2a, recv).await {
            let _ = send_a2s.send(B2F::Error(err)).await;
        }
    }
}

async fn listen<C: TransportConfig>(
    config: ServerConfig,
    streams: Streams,
    send_b2f: mpsc::Sender<B2F<C::C2S>>,
    send_f2b: broadcast::Sender<F2B<C::S2C>>,
    mut recv: broadcast::Receiver<F2B<C::S2C>>,
) -> Result<(), ServerError> {
    let endpoint = Endpoint::server(config).map_err(|err| ServerError::CreateEndpoint(err))?;
    let _ = send_b2f.send(B2F::Start).await;

    for client in 0.. {
        let client = ClientId::from_raw(client);

        match recv.try_recv() {
            Ok(_) => {}
            Err(broadcast::error::TryRecvError::Empty) => {}
            Err(_) => break,
        };

        let req = endpoint.accept().await;
        let streams = streams.clone();
        let send = send_b2f.clone();
        let recv = send_f2b.subscribe();
        tokio::spawn(async move {
            if let Err(err) = open_session::<C>(streams, send.clone(), recv, client, req).await {
                let _ = send.send(B2F::Error(err)).await;
            }
            let _ = send.send(B2F::Disconnect { client });
        });
    }

    Ok(())
}

struct Bi {
    send: SendStream,
    recv: Recv,
}

struct Recv {
    stream: RecvStream,
    buf: [u8; RECV_BUF],
}

async fn open_session<C: TransportConfig>(
    streams: Streams,
    mut send: mpsc::Sender<B2F<C::C2S>>,
    mut recv: broadcast::Receiver<F2B<C::S2C>>,
    client: ClientId,
    req: IncomingSession,
) -> Result<(), ServerError> {
    let mut conn = open_connection::<C>(&mut send, client, req).await?;
    let (mut bi, mut c2s, mut s2c) = open_streams(&mut conn, client, &streams).await?;
    let _ = send.send(B2F::Connect { client }).await;

    async fn forward_recv<C: TransportConfig>(
        send: &mut mpsc::Sender<B2F<C::C2S>>,
        client: ClientId,
        result: Result<C::C2S, ServerError>,
    ) {
        let _ = send
            .send(match result {
                Ok(msg) => B2F::Recv { client, msg },
                Err(err) => B2F::Error(err),
            })
            .await;
    }

    loop {
        tokio::select! {
            result = recv_datagram::<C>(client, &mut conn) => {
                forward_recv::<C>(&mut send, client, result).await;
            }
            Some(result) = async {
                let iter = bi.iter_mut().map(|Bi { recv, .. }| recv);
                recv_stream::<C>(client, assert_send(iter)).await
            } => {
                forward_recv::<C>(&mut send, client, result).await;
            }
            Some(result) = recv_stream::<C>(client, c2s.iter_mut()) => {
                forward_recv::<C>(&mut send, client, result).await;
            }
            result = recv.recv() => {
                match result {
                    Ok(F2B::Send { client: target, stream, msg }) if target == client => {
                        if let Err(err) = send_msg::<C>(client, stream, &mut conn, &mut bi, &mut s2c, msg).await {
                            let _ = send.send(B2F::Error(err)).await;
                        }
                    }
                    Ok(F2B::Disconnect { client: target }) if target == client => break,
                    Ok(_) => {},
                    Err(_) => break,
                }
            }
        }
    }
    Ok(())
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
) -> Result<Connection, ServerError> {
    let conn = req
        .await
        .map_err(|source| ServerError::RecvSession { client, source })?;
    let _ = send
        .send(B2F::Incoming {
            client,
            authority: conn.authority().to_owned(),
            path: conn.path().to_owned(),
            headers: conn.headers().clone(),
        })
        .await;

    let conn = conn
        .accept()
        .await
        .map_err(|source| ServerError::AcceptSession { client, source })?;
    Ok(conn)
}

async fn open_streams(
    conn: &mut Connection,
    client: ClientId,
    streams: &Streams,
) -> Result<(Vec<Bi>, Vec<Recv>, Vec<SendStream>), ServerError> {
    async fn collect_results<T>(
        iter: impl Iterator<Item = impl Future<Output = Result<T, ServerError>>>,
    ) -> Result<Vec<T>, ServerError> {
        iter.collect::<FuturesUnordered<_>>()
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, ServerError>>()
    }

    let bi: Vec<Bi> = collect_results((0..streams.bi).map(|_| async {
        let (send, recv) = conn
            .open_bi()
            .await
            .map_err(|source| ServerError::ConnectStream { client, source })?
            .await
            .map_err(|source| ServerError::OpenStream { client, source })?;
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
            .map_err(|source| ServerError::AcceptSession { client, source })?;
        let buf = [0; RECV_BUF];
        Ok(Recv { stream: recv, buf })
    }))
    .await?;

    let s2c: Vec<SendStream> = collect_results((0..streams.s2c).map(|_| async {
        let send = conn
            .open_uni()
            .await
            .map_err(|source| ServerError::ConnectStream { client, source })?
            .await
            .map_err(|source| ServerError::OpenStream { client, source })?;
        Ok(send)
    }))
    .await?;

    Ok((bi, c2s, s2c))
}

async fn recv_datagram<C: TransportConfig>(
    client: ClientId,
    conn: &mut Connection,
) -> Result<C::C2S, ServerError> {
    let datagram = conn
        .receive_datagram()
        .await
        .map_err(|err| ServerError::Recv {
            client,
            source: err.into(),
        })?;
    let msg: C::C2S = C::C2S::from_payload(&datagram)
        .map_err(|source| ServerError::Deserialize { client, source })?;
    Ok(msg)
}

async fn recv_stream<C: TransportConfig>(
    client: ClientId,
    streams: impl IntoIterator<Item = &mut Recv>,
) -> Option<Result<C::C2S, ServerError>> {
    streams
        .into_iter()
        .map(|Recv { stream, buf }| async {
            let read = loop {
                match stream
                    .read(&mut buf[..])
                    .await
                    .map_err(|err| ServerError::Recv {
                        client,
                        source: err.into(),
                    })? {
                    Some(read) => break read,
                    None => {}
                }
            };
            let msg = C::C2S::from_payload(&buf[..read])
                .map_err(|source| ServerError::Deserialize { client, source })?;
            Ok::<_, ServerError>(msg)
        })
        .collect::<FuturesUnordered<_>>()
        .next()
        .await
}

async fn send_msg<C: TransportConfig>(
    client: ClientId,
    stream: Stream,
    conn: &mut Connection,
    bi: &mut [Bi],
    s2c: &mut [SendStream],
    msg: C::S2C,
) -> Result<(), ServerError> {
    let payload = msg
        .into_payload()
        .map_err(|source| ServerError::Serialize { client, source })?;
    let res: Result<_, anyhow::Error> = match stream {
        Stream::Datagram => conn.send_datagram(payload).map_err(|err| err.into()),
        Stream::Bi(StreamId(index)) => {
            let Bi { send, .. } = &mut bi[index];
            send.write_all(&payload).await.map_err(|err| err.into())
        }
        Stream::Uni(StreamId(index)) => {
            let send = &mut s2c[index];
            send.write_all(&payload).await.map_err(|err| err.into())
        }
    };
    res.map_err(|source| ServerError::Send { client, source })
}
