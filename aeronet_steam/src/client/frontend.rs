use std::{fmt::Debug, future::Future, marker::PhantomData, time::Duration};

use aeronet::{
    client::{ClientEvent, ClientEventFor, ClientState, ClientTransport},
    lane::OnLane,
    message::{TryFromBytes, TryIntoBytes},
    protocol::TransportProtocol,
};
use aeronet_proto::packet;
use bytes::Bytes;
use derivative::Derivative;
use either::Either;
use futures::channel::{mpsc, oneshot};
use steamworks::ClientManager;
use tracing::debug;

use crate::transport::ConnectionStats;

use super::{
    backend, BackendError, ClientMessageKey, ConnectTarget, SteamClientConfig, SteamClientError,
};

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct SteamClientTransport<P: TransportProtocol, M = ClientManager> {
    inner: Inner<P>,
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<M>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
enum Inner<P: TransportProtocol> {
    #[derivative(Default)]
    Disconnected,
    Connecting(Connecting),
    Negotiating(Negotiating),
    Connected(Connected<P>),
}

#[derive(Debug)]
struct Connecting {
    config: SteamClientConfig,
    recv_err: oneshot::Receiver<BackendError>,
    recv_negotiating: oneshot::Receiver<backend::Negotiating>,
}

#[derive(Debug)]
struct Negotiating {
    config: SteamClientConfig,
    recv_err: oneshot::Receiver<BackendError>,
    send_poll: mpsc::Sender<()>,
    recv_connected: oneshot::Receiver<backend::Connected>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
struct Connected<P: TransportProtocol> {
    stats: ConnectionStats,
    packets: packet::Packets<P::C2S, P::S2C>,
    max_sent_bytes_per_sec: usize,
    bytes_left: usize,
    recv_err: oneshot::Receiver<BackendError>,
    send_poll: mpsc::Sender<()>,
    recv_stats: mpsc::Receiver<ConnectionStats>,
    recv_s2c: mpsc::Receiver<Bytes>,
    send_c2s: mpsc::UnboundedSender<Bytes>,
    send_flush: mpsc::Sender<()>,
}

const RECV_BATCH_SIZE: usize = 64;

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
        config: SteamClientConfig,
    ) -> (Self, impl Future<Output = ()> + Send) {
        let (send_err, recv_err) = oneshot::channel();
        let (send_negotiating, recv_negotiating) = oneshot::channel();
        let backend = async move {
            if let Err(err) = backend::open(
                steam,
                target,
                config.version,
                RECV_BATCH_SIZE,
                send_negotiating,
            )
            .await
            {
                debug!(
                    "Connection closed: {:#}",
                    aeronet::error::pretty_error(&err)
                );
                let _ = send_err.send(err);
            } else {
                debug!("Connection closed");
            }
        };

        (
            Self {
                inner: Inner::Connecting(Connecting {
                    config,
                    recv_err,
                    recv_negotiating,
                }),
                _phantom: PhantomData,
            },
            backend,
        )
    }

    pub fn connect(
        &mut self,
        steam: steamworks::Client<M>,
        target: ConnectTarget,
        config: SteamClientConfig,
    ) -> Result<impl Future<Output = ()> + Send, SteamClientError<P>> {
        match &mut self.inner {
            Inner::Disconnected => {
                let (this, backend) = Self::connect_new(steam, target, config);
                *self = this;
                Ok(backend)
            }
            Inner::Connecting { .. } | Inner::Negotiating { .. } | Inner::Connected { .. } => {
                Err(SteamClientError::AlreadyConnected)
            }
        }
    }

    pub fn disconnect(&mut self) -> Result<(), SteamClientError<P>> {
        match &mut self.inner {
            Inner::Disconnected => Err(SteamClientError::AlreadyDisconnected),
            Inner::Connecting { .. } | Inner::Negotiating { .. } | Inner::Connected { .. } => {
                self.inner = Inner::Disconnected;
                Ok(())
            }
        }
    }

    fn recv_disconnect_event(
        recv_err: &mut oneshot::Receiver<BackendError>,
    ) -> Option<ClientEventFor<P, Self>> {
        match recv_err
            .try_recv()
            .map_err(|_| SteamClientError::<P>::BackendClosed)
        {
            Ok(Some(err)) => Some(ClientEvent::Disconnected { reason: err.into() }),
            Ok(None) => None,
            Err(_) => Some(ClientEvent::Disconnected {
                reason: SteamClientError::BackendClosed,
            }),
        }
    }

    fn poll_connecting(mut client: Connecting) -> (Option<ClientEventFor<P, Self>>, Inner<P>) {
        if let Some(event) = Self::recv_disconnect_event(&mut client.recv_err) {
            return (Some(event), Inner::Disconnected);
        }

        if let Ok(Some(next)) = client.recv_negotiating.try_recv() {
            (
                None,
                Inner::Negotiating(Negotiating {
                    config: client.config,
                    recv_err: client.recv_err,
                    send_poll: next.send_poll,
                    recv_connected: next.recv_connected,
                }),
            )
        } else {
            (None, Inner::Connecting(client))
        }
    }

    fn poll_negotiating(mut client: Negotiating) -> (Option<ClientEventFor<P, Self>>, Inner<P>) {
        if let Some(event) = Self::recv_disconnect_event(&mut client.recv_err) {
            return (Some(event), Inner::Disconnected);
        }

        let _ = client.send_poll.try_send(());

        if let Ok(Some(next)) = client.recv_connected.try_recv() {
            let config = client.config;
            (
                Some(ClientEvent::Connected),
                Inner::Connected(Connected {
                    stats: next.stats,
                    packets: packet::Packets::new(
                        config.max_packet_len,
                        config.default_packet_cap,
                        &config.lanes,
                    ),
                    max_sent_bytes_per_sec: config.max_sent_bytes_per_sec,
                    bytes_left: config.max_sent_bytes_per_sec,
                    recv_err: client.recv_err,
                    send_poll: client.send_poll,
                    recv_stats: next.recv_stats,
                    send_c2s: next.send_c2s,
                    recv_s2c: next.recv_s2c,
                    send_flush: next.send_flush,
                }),
            )
        } else {
            (None, Inner::Negotiating(client))
        }
    }

    fn poll_connected(
        mut client: Connected<P>,
        delta_time: Duration,
    ) -> (Vec<ClientEventFor<P, Self>>, Inner<P>) {
        if let Some(event) = Self::recv_disconnect_event(&mut client.recv_err) {
            return (vec![event], Inner::Disconnected);
        }

        // refill token bucket
        let bytes_added =
            ((client.max_sent_bytes_per_sec as f64) * delta_time.as_secs_f64()) as usize;
        client.bytes_left = (client.bytes_left + bytes_added).min(client.max_sent_bytes_per_sec);

        // request backend to poll messages - note that this will only produce
        // messages on the next `poll` call
        let _ = client.send_poll.try_send(());

        // read messages
        let mut events = Vec::new();
        while let Ok(Some(packet)) = client.recv_s2c.try_next() {
            if let Err(reason) = Self::recv_packet(&mut client, &mut events, packet) {
                events.push(ClientEvent::Disconnected { reason });
                return (events, Inner::Disconnected);
            }
        }

        (events, Inner::Connected(client))
    }

    fn recv_packet(
        client: &mut Connected<P>,
        events: &mut Vec<ClientEventFor<P, Self>>,
        mut packet: Bytes,
    ) -> Result<(), SteamClientError<P>> {
        for msg_seq in client
            .packets
            .read_acks(&mut packet)
            .map_err(SteamClientError::Recv)?
        {
            events.push(ClientEvent::Ack {
                msg_key: ClientMessageKey { msg_seq },
            });
        }

        while let Some(msgs) = client
            .packets
            .read_next_frag(&mut packet)
            .map_err(SteamClientError::Recv)?
        {
            for msg in msgs {
                events.push(ClientEvent::Recv { msg });
            }
        }

        Ok(())
    }
}

impl<P, M> ClientTransport<P> for SteamClientTransport<P, M>
where
    P: TransportProtocol,
    P::C2S: TryIntoBytes + OnLane,
    P::S2C: TryFromBytes + OnLane,
    M: steamworks::Manager + Send + Sync + 'static,
{
    type Error = SteamClientError<P>;

    type ConnectingInfo = ();

    type ConnectedInfo = ConnectionStats;

    type MessageKey = ClientMessageKey;

    fn state(&self) -> ClientState<Self::ConnectingInfo, Self::ConnectedInfo> {
        match &self.inner {
            Inner::Disconnected => ClientState::Disconnected,
            Inner::Connecting { .. } | Inner::Negotiating { .. } => ClientState::Connecting(()),
            Inner::Connected(client) => ClientState::Connected(client.stats.clone()),
        }
    }

    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<Self::MessageKey, Self::Error> {
        let Inner::Connected(client) = &mut self.inner else {
            return Err(SteamClientError::NotConnected);
        };

        let msg = msg.into();
        let msg_seq = client
            .packets
            .buffer_send(msg)
            .map_err(SteamClientError::Send)?;
        Ok(ClientMessageKey { msg_seq })
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        let Inner::Connected(client) = &mut self.inner else {
            return Ok(());
        };

        for packet in client.packets.flush(&mut client.bytes_left) {
            client
                .send_c2s
                .unbounded_send(packet)
                .map_err(|_| SteamClientError::BackendClosed)?;
        }
        client
            .send_flush
            .try_send(())
            .map_err(|_| SteamClientError::BackendClosed)?;

        Ok(())
    }

    fn poll(
        &mut self,
        delta_time: Duration,
    ) -> impl Iterator<Item = ClientEvent<P, Self::Error, Self::MessageKey>> {
        replace_with::replace_with_or_abort_and_return(&mut self.inner, |inner| match inner {
            Inner::Disconnected => (Either::Left(None), inner),
            Inner::Connecting(client) => {
                let (res, new) = Self::poll_connecting(client);
                (Either::Left(res), new)
            }
            Inner::Negotiating(client) => {
                let (res, new) = Self::poll_negotiating(client);
                (Either::Left(res), new)
            }
            Inner::Connected(client) => {
                let (res, new) = Self::poll_connected(client, delta_time);
                (Either::Right(res.into_iter()), new)
            }
        })
        .into_iter()
    }
}
