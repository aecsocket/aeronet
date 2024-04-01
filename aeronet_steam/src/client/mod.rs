use std::{fmt::Debug, future::Future, marker::PhantomData, net::SocketAddr, time::Duration};

use aeronet::{
    client::{ClientEvent, ClientEventFor, ClientState, ClientTransport},
    lane::{LaneKind, OnLane},
    message::{TryFromBytes, TryIntoBytes},
    protocol::{ProtocolVersion, TransportProtocol},
};
use aeronet_proto::{
    packet::{self},
    seq::Seq,
};
use bytes::Bytes;
use derivative::Derivative;
use either::Either;
use futures::channel::{mpsc, oneshot};
use steamworks::{ClientManager, SteamId};

use crate::transport::ConnectionStats;

pub mod backend;

#[derive(Derivative, thiserror::Error)]
#[derivative(
    Debug(bound = "packet::SendError<P::C2S>: Debug, packet::RecvError<P::S2C>: Debug"),
    Clone(bound = "packet::SendError<P::C2S>: Clone, packet::RecvError<P::S2C>: Clone")
)]
pub enum Error<P>
where
    P: TransportProtocol,
    P::C2S: TryIntoBytes,
    P::S2C: TryFromBytes,
{
    #[error("already connected")]
    AlreadyConnected,
    #[error("already disconnected")]
    AlreadyDisconnected,
    #[error("not connected")]
    NotConnected,
    #[error("backend closed")]
    BackendClosed,

    #[error("failed to send message")]
    Send(#[source] packet::SendError<P::C2S>),
    #[error("failed to receive message")]
    Recv(#[source] packet::RecvError<P::S2C>),
    #[error(transparent)]
    Backend(#[from] backend::Error),
}

/// Identifier of a peer which a Steam client wants to connect to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectTarget {
    /// Peer identified by its IP address.
    Ip(SocketAddr),
    /// Peer identified by its Steam ID.
    Peer {
        /// Steam ID of the peer.
        id: SteamId,
        /// Port to connect on.
        virtual_port: i32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientMessageKey {
    msg_seq: Seq,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
pub struct SteamClientTransport<P: TransportProtocol, M = ClientManager> {
    inner: Inner<P>,
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<M>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportConfig {
    pub version: ProtocolVersion,
    pub recv_batch_size: usize,
    pub max_packet_len: usize,
    pub default_packet_cap: usize,
    pub max_sent_bytes_per_sec: usize,
    pub lanes: Box<[LaneKind]>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
enum Inner<P: TransportProtocol> {
    #[derivative(Default)]
    Disconnected,
    Connecting {
        config: TransportConfig,
        recv_err: oneshot::Receiver<backend::Error>,
        recv_negotiating: oneshot::Receiver<backend::Negotiating>,
    },
    Negotiating {
        config: TransportConfig,
        recv_err: oneshot::Receiver<backend::Error>,
        send_poll: mpsc::Sender<()>,
        recv_connected: oneshot::Receiver<backend::Connected>,
    },
    Connected {
        stats: ConnectionStats,
        packets: packet::Packets<P::C2S, P::S2C>,
        max_sent_bytes_per_sec: usize,
        bytes_left: usize,
        recv_err: oneshot::Receiver<backend::Error>,
        send_poll: mpsc::Sender<()>,
        recv_stats: mpsc::Receiver<ConnectionStats>,
        recv_s2c: mpsc::Receiver<Bytes>,
        send_c2s: mpsc::UnboundedSender<Bytes>,
    },
}

impl<P, M> SteamClientTransport<P, M>
where
    P: TransportProtocol,
    P::C2S: TryIntoBytes + OnLane,
    P::S2C: TryFromBytes + OnLane,
    M: steamworks::Manager + Send + Sync + 'static,
{
    pub fn disconnected() -> Self {
        Self {
            inner: Inner::Disconnected,
            _phantom: PhantomData,
        }
    }

    pub fn connect_new(
        steam: steamworks::Client<M>,
        target: ConnectTarget,
        config: TransportConfig,
    ) -> (Self, impl Future<Output = ()> + Send) {
        let (send_err, recv_err) = oneshot::channel();
        let (send_negotiating, recv_negotiating) = oneshot::channel();
        let backend = async move {
            if let Err(err) = backend::open(
                steam,
                target,
                config.version,
                config.recv_batch_size,
                send_negotiating,
            )
            .await
            {
                let _ = send_err.send(err);
            }
        };

        (
            Self {
                inner: Inner::Connecting {
                    config,
                    recv_err,
                    recv_negotiating,
                },
                _phantom: PhantomData,
            },
            backend,
        )
    }

    pub fn connect(
        &mut self,
        steam: steamworks::Client<M>,
        target: ConnectTarget,
        config: TransportConfig,
    ) -> Result<impl Future<Output = ()> + Send, Error<P>> {
        match self.inner {
            Inner::Disconnected => {
                let (this, backend) = Self::connect_new(steam, target, config);
                *self = this;
                Ok(backend)
            }
            Inner::Connecting { .. } | Inner::Negotiating { .. } | Inner::Connected { .. } => {
                Err(Error::AlreadyConnected)
            }
        }
    }

    fn recv_disconnect_event(
        recv_err: &mut oneshot::Receiver<backend::Error>,
    ) -> Option<ClientEventFor<P, Self>> {
        match recv_err.try_recv().map_err(|_| Error::<P>::BackendClosed) {
            Ok(Some(err)) => Some(ClientEvent::Disconnected { reason: err.into() }),
            Ok(None) => None,
            Err(_) => Some(ClientEvent::Disconnected {
                reason: Error::BackendClosed,
            }),
        }
    }
}

impl<P, M> ClientTransport<P> for SteamClientTransport<P, M>
where
    P: TransportProtocol,
    P::C2S: TryIntoBytes + OnLane,
    P::S2C: TryFromBytes + OnLane,
    M: steamworks::Manager + Send + Sync + 'static,
{
    type Error = Error<P>;

    type ConnectingInfo = ();

    type ConnectedInfo = ConnectionStats;

    type MessageKey = ClientMessageKey;

    fn state(&self) -> ClientState<Self::ConnectingInfo, Self::ConnectedInfo> {
        match &self.inner {
            Inner::Disconnected => ClientState::Disconnected,
            Inner::Connecting { .. } | Inner::Negotiating { .. } => ClientState::Connecting(()),
            Inner::Connected { stats, .. } => ClientState::Connected(stats.clone()),
        }
    }

    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<Self::MessageKey, Self::Error> {
        let Inner::Connected { packets, .. } = &mut self.inner else {
            return Err(Error::NotConnected);
        };

        let msg = msg.into();
        let msg_seq = packets.buffer_send(msg).map_err(Error::Send)?;
        Ok(ClientMessageKey { msg_seq })
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        let Inner::Connected {
            packets,
            bytes_left,
            send_c2s,
            ..
        } = &mut self.inner
        else {
            return Err(Error::NotConnected);
        };

        for packet in packets.flush(bytes_left) {
            send_c2s
                .unbounded_send(packet)
                .map_err(|_| Error::BackendClosed)?;
        }

        Ok(())
    }

    fn poll(
        &mut self,
        delta_time: Duration,
    ) -> impl Iterator<Item = ClientEvent<P, Self::Error, Self::MessageKey>> {
        match &mut self.inner {
            Inner::Disconnected => Either::Left(None),
            Inner::Connecting {
                config,
                recv_err,
                recv_negotiating,
            } => {
                if let Some(event) = Self::recv_disconnect_event(recv_err) {
                    Either::Left(Some(event))
                } else {
                    if let Ok(Some(backend::Negotiating {
                        send_poll,
                        recv_connected,
                    })) = recv_negotiating.try_recv()
                    {
                        take_mut::take(&mut self.inner, |inner| {
                            let Inner::Connecting {
                                config, recv_err, ..
                            } = inner
                            else {
                                unreachable!();
                            };
                            Inner::Negotiating {
                                config,
                                recv_err,
                                send_poll,
                                recv_connected,
                            }
                        });
                    }
                    Either::Left(None)
                }
            }
            Inner::Negotiating {
                recv_err,
                send_poll,
                recv_connected,
                ..
            } => {
                if let Some(event) = Self::recv_disconnect_event(recv_err) {
                    Either::Left(Some(event))
                } else {
                    let _ = send_poll.try_send(());
                    if let Ok(Some(backend::Connected {
                        stats,
                        recv_s2c,
                        recv_stats,
                        send_c2s,
                        ..
                    })) = recv_connected.try_recv()
                    {
                        take_mut::take(&mut self.inner, |inner| {
                            let Inner::Negotiating {
                                config,
                                recv_err,
                                send_poll,
                                ..
                            } = inner
                            else {
                                unreachable!();
                            };
                            Inner::Connected {
                                stats,
                                packets: packet::Packets::new(
                                    config.max_packet_len,
                                    config.default_packet_cap,
                                    &config.lanes,
                                ),
                                max_sent_bytes_per_sec: config.max_sent_bytes_per_sec,
                                bytes_left: config.max_sent_bytes_per_sec,
                                recv_err,
                                send_poll,
                                recv_stats,
                                recv_s2c,
                                send_c2s,
                            }
                        })
                    }
                    Either::Left(None)
                }
            }
            Inner::Connected {
                stats,
                packets,
                max_sent_bytes_per_sec,
                bytes_left,
                recv_err,
                send_poll,
                send_c2s,
                recv_s2c,
                recv_stats,
            } => {
                let added = ((*max_sent_bytes_per_sec as f64) * delta_time.as_secs_f64()) as usize;
                *bytes_left = (*bytes_left + added).min(*max_sent_bytes_per_sec);
                Either::Right(None)
            }
        }
        .into_iter()
    }
}
