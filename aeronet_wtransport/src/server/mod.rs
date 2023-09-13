#[cfg(feature = "bevy")]
mod plugin;

use futures::{stream::FuturesUnordered, Future, StreamExt};
#[cfg(feature = "bevy")]
pub use plugin::{WebTransportServer, WebTransportServerPlugin};

use std::io;

use anyhow::Result;
use tokio::sync::{broadcast, mpsc};
use wtransport::{
    endpoint::IncomingSession,
    error::{ConnectionError, StreamOpeningError},
    Endpoint, ServerConfig, SendStream, RecvStream,
};

use crate::{ClientId, Message, StreamC2S, Streams, TransportConfig, stream::StreamKind};

const BUFFER_SIZE: usize = 128;

#[derive(Debug)]
pub enum A2S<C2S> {
    Start,
    Incoming { client: ClientId },
    Connect { client: ClientId },
    Recv { client: ClientId, msg: C2S },
    Disconnect { client: ClientId },
    Error(ServerError),
}

#[derive(Debug, Clone)]
pub enum S2A<S2C> {
    Send {
        client: ClientId,
        stream: StreamC2S,
        msg: S2C,
    },
    Disconnect {
        client: ClientId,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
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
    #[error("failed to receive data from {client}")]
    Recv {
        client: ClientId,
        #[source]
        source: anyhow::Error,
    },
    #[error("failed to parse data from {client}")]
    Parse {
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
pub struct SyncServer<C: TransportConfig> {
    pub send: broadcast::Sender<S2A<C::S2C>>,
    pub recv: mpsc::Receiver<A2S<C::C2S>>,
    _private: (),
}

pub struct AsyncServer<C: TransportConfig> {
    config: ServerConfig,
    streams: Streams,
    send_a2s: mpsc::Sender<A2S<C::C2S>>,
    send_s2a: broadcast::Sender<S2A<C::S2C>>,
    recv: broadcast::Receiver<S2A<C::S2C>>,
}

pub fn create<C: TransportConfig>(
    config: ServerConfig,
    streams: Streams,
) -> (SyncServer<C>, AsyncServer<C>) {
    let (send_a2s, recv_a2s) = mpsc::channel::<A2S<C::C2S>>(BUFFER_SIZE);
    let (send_s2a, recv_s2a) = broadcast::channel::<S2A<C::S2C>>(BUFFER_SIZE);

    let sync_chans = SyncServer::<C> {
        send: send_s2a.clone(),
        recv: recv_a2s,
        _private: (),
    };

    let async_chans = AsyncServer::<C> {
        config,
        streams,
        send_a2s,
        send_s2a,
        recv: recv_s2a,
    };

    (sync_chans, async_chans)
}

impl<C: TransportConfig> AsyncServer<C> {
    pub async fn listen(self) {
        let Self {
            config,
            streams,
            send_a2s,
            send_s2a,
            recv,
        } = self;

        if let Err(err) = listen::<C>(config, streams, send_a2s.clone(), send_s2a, recv).await {
            let _ = send_a2s.send(A2S::Error(err)).await;
        }
    }
}

async fn listen<C: TransportConfig>(
    config: ServerConfig,
    streams: Streams,
    send_a2s: mpsc::Sender<A2S<C::C2S>>,
    send_s2a: broadcast::Sender<S2A<C::S2C>>,
    mut recv: broadcast::Receiver<S2A<C::S2C>>,
) -> Result<(), ServerError> {
    let endpoint = Endpoint::server(config).map_err(|err| ServerError::CreateEndpoint(err))?;
    let _ = send_a2s.send(A2S::Start).await;

    for client in 0.. {
        let client = ClientId::from_raw(client);

        match recv.try_recv() {
            Ok(_) => {}
            Err(broadcast::error::TryRecvError::Empty) => {}
            Err(_) => break,
        };

        let req = endpoint.accept().await;
        let send = send_a2s.clone();
        let recv = send_s2a.subscribe();
        // tokio::spawn(async move {
        //     if let Err(err) = session::<C>(&streams, send.clone(), recv, req, client).await {
        //         let _ = send.send(A2S::Error(err)).await;
        //     }
        //     let _ = send.send(A2S::Disconnect { client });
        // });
    }

    Ok(())
}

async fn session<C: TransportConfig>(
    streams: &Streams,
    send: mpsc::Sender<A2S<C::C2S>>,
    mut recv: broadcast::Receiver<S2A<C::S2C>>,
    req: IncomingSession,
    client: ClientId,
) -> Result<(), ServerError> {
    let conn = req
        .await
        .map_err(|source| ServerError::RecvSession { client, source })?;
    let _ = send.send(A2S::Incoming { client }).await;

    let conn = conn
        .accept()
        .await
        .map_err(|source| ServerError::AcceptSession { client, source })?;

    async fn collect_results<T>(
        iter: impl Iterator<Item = impl Future<Output = Result<T, ServerError>>>,
    ) -> Result<Vec<T>, ServerError> {
        iter.collect::<FuturesUnordered<_>>()
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, ServerError>>()
    }

    let mut bi: Vec<(SendStream, RecvStream)> = collect_results((0..streams.bi).map(|_| async {
        Ok(conn
            .open_bi()
            .await
            .map_err(|source| ServerError::ConnectStream { client, source })?
            .await
            .map_err(|source| ServerError::OpenStream { client, source })?)
    }))
    .await?;
    let mut s2c: Vec<SendStream> = collect_results((0..streams.s2c).map(|_| async {
        Ok(conn
            .open_uni()
            .await
            .map_err(|source| ServerError::ConnectStream { client, source })?
            .await
            .map_err(|source| ServerError::OpenStream { client, source })?)
    }))
    .await?;
    let mut c2s: Vec<RecvStream> = collect_results((0..streams.c2s).map(|_| async {
        Ok(conn
            .accept_uni()
            .await
            .map_err(|source| ServerError::AcceptSession { client, source })?)
    }))
    .await?;

    loop {
        tokio::select! {
            // recv from sync server, send to client
            result = recv.recv() => {
                match result {
                    Ok(S2A::Send { client: target, stream, msg }) if target == client => {
                        let payload: &[u8];
                        match stream.0 {
                            StreamKind::Datagram => {
                                conn.send_datagram(payload);
                            }
                            StreamKind::Bi(index) => {
                                let (send, _) = &mut bi[index];
                                send.write_all(payload);
                            }
                            StreamKind::Uni(index) => {
                                let send = &mut s2c[index];
                                send.write_all(payload);
                            }
                        }
                    }
                    Ok(S2A::Disconnect { client: target }) if target == client => break,
                    Ok(_) => {},
                    Err(_) => break,
                }
            }
            // recv from client, send to sync server
            result = conn.recv_datagram() => {
                // AAAAAAAAAAA
                match result {

                }
            }
        }
    }

    Ok(())
}
