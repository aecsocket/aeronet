#[cfg(feature = "bevy")]
mod plugin;

#[cfg(feature = "bevy")]
pub use plugin::{WebTransportServer, WebTransportServerPlugin};

use std::io;

use anyhow::Result;
use tokio::sync::{broadcast, mpsc};
use wtransport::{
    endpoint::IncomingSession,
    error::{ConnectionError, SendDatagramError, StreamWriteError, StreamReadError},
    Endpoint, ServerConfig,
};

use crate::{ClientId, TransportConfig, Message, TransportStream};

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
    Send { client: ClientId, msg: S2C },
    Disconnect { client: ClientId },
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
    #[error("failed to open bidirectional stream for {client}")]
    OpenBi {
        client: ClientId,
        #[source]
        source: ConnectionError,
    },
    #[error("failed to receive datagram from {client}")]
    RecvDatagram {
        client: ClientId,
        #[source]
        source: ConnectionError,
    },
    #[error("failed to receive stream data from {client}")]
    RecvStream {
        client: ClientId,
        #[source]
        source: StreamReadError,
    },
    #[error("failed to send datagram to {client}")]
    SendDatagram {
        client: ClientId,
        #[source]
        source: SendDatagramError,
    },
    #[error("failed to send stream data to {client}")]
    SendStream {
        client: ClientId,
        #[source]
        source: StreamWriteError,
    }
}

#[derive(Debug)]
pub struct SyncServer<C: TransportConfig> {
    send: broadcast::Sender<S2A<C::S2C>>,
    pub recv: mpsc::Receiver<A2S<C::C2S>>,
}

pub struct AsyncServer<C: TransportConfig> {
    config: ServerConfig,
    send_a2s: mpsc::Sender<A2S<C::C2S>>,
    send_s2a: broadcast::Sender<S2A<C::S2C>>,
    recv: broadcast::Receiver<S2A<C::S2C>>,
}

pub fn create<C: TransportConfig>(config: ServerConfig) -> (SyncServer<C>, AsyncServer<C>) {
    let (send_a2s, recv_a2s) = mpsc::channel::<A2S<C::C2S>>(BUFFER_SIZE);
    let (send_s2a, recv_s2a) = broadcast::channel::<S2A<C::S2C>>(BUFFER_SIZE);

    let sync_chans = SyncServer::<C> {
        send: send_s2a.clone(),
        recv: recv_a2s,
    };

    let async_chans = AsyncServer::<C> {
        config,
        send_a2s,
        send_s2a,
        recv: recv_s2a,
    };

    (sync_chans, async_chans)
}

impl<C: TransportConfig> AsyncServer<C> {
    pub async fn listen(self) {
        let Self { config, send_a2s, send_s2a, recv } = self;

        if let Err(err) = listen::<C>(config, send_a2s.clone(), send_s2a, recv).await {
            let _ = send_a2s.send(A2S::Error(err)).await;
        }
    }
}

async fn listen<C: TransportConfig>(
    config: ServerConfig,
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
        tokio::spawn(async move {
            if let Err(err) = session::<C>(send.clone(), recv, req, client).await {
                let _ = send.send(A2S::Error(err)).await;
            }
            let _ = send.send(A2S::Disconnect { client });
        });
    }

    Ok(())
}

async fn session<C: TransportConfig>(
    send: mpsc::Sender<A2S<C::C2S>>,
    mut recv: broadcast::Receiver<S2A<C::S2C>>,
    req: IncomingSession,
    client: ClientId,
) -> Result<(), ServerError> {
    let _ = send.send(A2S::Incoming { client }).await;

    let conn = req
        .await
        .map_err(|source| ServerError::RecvSession { client, source })?
        .accept()
        .await
        .map_err(|source| ServerError::AcceptSession { client, source })?;

    let (mut bi_send, mut bi_recv) = conn.accept_bi().await
        .map_err(|source| ServerError::OpenBi { client, source })?;
    let mut buf = [0u8; 0x10_000];

    let _ = send.send(A2S::Connect { client }).await;

    loop {
        tokio::select! {
            // recv from client, send to sync server
            result = conn.receive_datagram() => {
                match result {
                    Ok(msg) => {
                        let msg: &[u8] = msg.as_ref();
                    },
                    Err(source) => return Err(ServerError::RecvDatagram { client, source }),
                }
            }
            result = bi_recv.read(&mut buf) => {
                match result {
                    Ok(_) => {},
                    Err(source) => return Err(ServerError::RecvStream { client, source }),
                }
            }
            // recv from sync server, send to client
            result = recv.recv() => {
                match result {
                    Ok(S2A::Send { client: target, msg }) if target == client => {
                        match msg.stream() {
                            TransportStream::Datagram => {
                                if let Err(source) = conn.send_datagram(msg.payload()) {
                                    let _ = send.send(A2S::Error(ServerError::SendDatagram { client, source })).await;
                                }
                            }
                            TransportStream::Bi => {
                                if let Err(source) = bi_send.write(msg.payload()).await {
                                    let _ = send.send(A2S::Error(ServerError::SendStream { client, source })).await;
                                }
                            }
                        }
                    },
                    Ok(S2A::Disconnect { client: target }) if target == client => break,
                    Ok(_) => {},
                    Err(_) => break,
                };
            }
        }
    }

    Ok(())
}
