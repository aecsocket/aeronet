use std::{fmt::Debug, future::Future, time::Duration};

use aeronet::{
    client::{ClientEvent, ClientEventFor, ClientState, ClientTransport},
    error::pretty_error,
    lane::{LaneKind, OnLane},
    message::{TryFromBytes, TryIntoBytes},
    protocol::TransportProtocol,
};
use aeronet_proto::packet;
use bytes::Bytes;
use derivative::Derivative;
use either::Either;
use futures::channel::{mpsc, oneshot};
use tracing::debug;
use xwt_core::utils::maybe;

use crate::{error::BackendError, transport::ConnectionStats};

use super::{
    backend, ClientBackendError, ClientMessageKey, WebTransportClientConfig,
    WebTransportClientError,
};

impl<P> From<mpsc::TryRecvError> for WebTransportClientError<P>
where
    P: TransportProtocol,
    P::C2S: TryIntoBytes,
    P::S2C: TryFromBytes,
{
    fn from(_: mpsc::TryRecvError) -> Self {
        Self::BackendClosed
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct WebTransportClient<P: TransportProtocol> {
    inner: Inner<P>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
enum Inner<P: TransportProtocol> {
    #[derivative(Default)]
    Disconnected,
    Connecting(Connecting),
    Connected(Connected<P>),
}

#[derive(Debug)]
struct FrontendConfig {
    lanes: Box<[LaneKind]>,
    max_sent_bytes_per_sec: usize,
    max_packet_len: usize,
    default_packet_cap: usize,
}

#[derive(Debug)]
struct Connecting {
    config: FrontendConfig,
    recv_err: oneshot::Receiver<ClientBackendError>,
    recv_connected: oneshot::Receiver<backend::Connected>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
struct Connected<P: TransportProtocol> {
    recv_err: oneshot::Receiver<ClientBackendError>,
    send_c2s: mpsc::UnboundedSender<Bytes>,
    recv_s2c: mpsc::Receiver<Bytes>,
    recv_stats: mpsc::Receiver<ConnectionStats>,
    stats: ConnectionStats,
    packets: packet::Packets<P::C2S, P::S2C>,
    max_sent_bytes_per_sec: usize,
    bytes_left: usize,
}

impl<P> WebTransportClient<P>
where
    P: TransportProtocol,
    P::C2S: TryIntoBytes + OnLane,
    P::S2C: TryFromBytes + OnLane,
{
    #[must_use]
    pub fn disconnected() -> Self {
        Self {
            inner: Inner::Disconnected,
        }
    }

    pub fn disconnect(&mut self) -> Result<(), WebTransportClientError<P>> {
        if let Inner::Disconnected = self.inner {
            return Err(WebTransportClientError::AlreadyDisconnected);
        }

        self.inner = Inner::Disconnected;
        Ok(())
    }

    #[must_use]
    pub fn connect_new(
        config: WebTransportClientConfig,
        target: impl Into<String>,
    ) -> (Self, impl Future<Output = ()> + maybe::Send) {
        let WebTransportClientConfig {
            native: native_config,
            version,
            lanes,
            max_sent_bytes_per_sec,
            max_packet_len,
            default_packet_cap,
        } = config;
        let target = target.into();

        let (send_err, recv_err) = oneshot::channel::<ClientBackendError>();
        let (send_connected, recv_connected) = oneshot::channel::<backend::Connected>();
        let backend = async move {
            let Err(err) = backend::start(native_config, version, target, send_connected).await
            else {
                unreachable!()
            };
            match err {
                ClientBackendError::Generic(BackendError::FrontendClosed) => {
                    debug!("Connection closed");
                }
                err => {
                    debug!("Connection closed: {:#}", pretty_error(&err));
                    let _ = send_err.send(err);
                }
            }
        };

        (
            Self {
                inner: Inner::Connecting(Connecting {
                    config: FrontendConfig {
                        lanes,
                        max_sent_bytes_per_sec,
                        max_packet_len,
                        default_packet_cap,
                    },
                    recv_err,
                    recv_connected,
                }),
            },
            backend,
        )
    }

    pub fn connect(
        &mut self,
        config: WebTransportClientConfig,
        target: impl Into<String>,
    ) -> Result<impl Future<Output = ()> + maybe::Send, WebTransportClientError<P>> {
        let Inner::Disconnected = self.inner else {
            return Err(WebTransportClientError::AlreadyConnected);
        };

        let (this, backend) = Self::connect_new(config, target);
        *self = this;
        Ok(backend)
    }
}

impl<P> ClientTransport<P> for WebTransportClient<P>
where
    P: TransportProtocol,
    P::C2S: TryIntoBytes + OnLane,
    P::S2C: TryFromBytes + OnLane,
{
    type Error = WebTransportClientError<P>;

    type ConnectingInfo = ();

    type ConnectedInfo = ConnectionStats;

    type MessageKey = ClientMessageKey;

    fn state(&self) -> ClientState<Self::ConnectingInfo, Self::ConnectedInfo> {
        match &self.inner {
            Inner::Disconnected => ClientState::Disconnected,
            Inner::Connecting { .. } => ClientState::Connecting(()),
            Inner::Connected(client) => ClientState::Connected(client.stats.clone()),
        }
    }

    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<Self::MessageKey, Self::Error> {
        let Inner::Connected(client) = &mut self.inner else {
            return Err(WebTransportClientError::NotConnected);
        };

        let msg = msg.into();
        let msg_seq = client.packets.buffer_send(msg)?;
        Ok(ClientMessageKey { msg_seq })
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        let Inner::Connected(client) = &mut self.inner else {
            return Err(WebTransportClientError::NotConnected);
        };

        for packet in client.packets.flush(&mut client.bytes_left) {
            client
                .send_c2s
                .unbounded_send(packet)
                .map_err(|_| WebTransportClientError::BackendClosed)?;
        }
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
            Inner::Connected(client) => {
                let (res, new) = Self::poll_connected(client, delta_time);
                (Either::Right(res), new)
            }
        })
        .into_iter()
    }
}

impl<P> WebTransportClient<P>
where
    P: TransportProtocol,
    P::C2S: TryIntoBytes + OnLane,
    P::S2C: TryFromBytes + OnLane,
{
    fn poll_connecting(mut client: Connecting) -> (Option<ClientEventFor<P, Self>>, Inner<P>) {
        if let Ok(Some(err)) = client.recv_err.try_recv() {
            return (
                Some(ClientEvent::Disconnected { error: err.into() }),
                Inner::Disconnected,
            );
        }
        match client.recv_connected.try_recv() {
            Ok(None) => (None, Inner::Connecting(client)),
            Ok(Some(next)) => (
                Some(ClientEvent::Connected),
                Inner::Connected(Connected {
                    recv_err: client.recv_err,
                    send_c2s: next.send_c2s,
                    recv_s2c: next.recv_s2c,
                    recv_stats: next.recv_stats,
                    stats: next.initial_stats,
                    packets: packet::Packets::new(
                        client.config.max_packet_len,
                        client.config.default_packet_cap,
                        &client.config.lanes,
                    ),
                    max_sent_bytes_per_sec: client.config.max_sent_bytes_per_sec,
                    bytes_left: client.config.max_sent_bytes_per_sec,
                }),
            ),
            Err(_) => (
                Some(ClientEvent::Disconnected {
                    error: WebTransportClientError::BackendClosed,
                }),
                Inner::Disconnected,
            ),
        }
    }

    fn poll_connected(
        mut client: Connected<P>,
        delta_time: Duration,
    ) -> (Vec<ClientEventFor<P, Self>>, Inner<P>) {
        if let Ok(Some(error)) = client.recv_err.try_recv() {
            return (
                vec![ClientEvent::Disconnected {
                    error: error.into(),
                }],
                Inner::Disconnected,
            );
        }

        // refill bytes token bcucket
        let bytes_restored =
            ((client.max_sent_bytes_per_sec as f64) * delta_time.as_secs_f64()) as usize;
        client.bytes_left = (client.bytes_left + bytes_restored).min(client.max_sent_bytes_per_sec);

        let mut events = Vec::new();
        let res = (|| {
            // update connection stats
            while let Some(stats) = client.recv_stats.try_next()? {
                client.stats = stats;
            }

            while let Some(mut packet) = client.recv_s2c.try_next()? {
                // receive acks
                events.extend(client.packets.read_acks(&mut packet)?.map(|msg_seq| {
                    ClientEvent::Ack {
                        msg_key: ClientMessageKey { msg_seq },
                    }
                }));

                // receive messages
                while let Some(msgs) = client.packets.read_next_frag(&mut packet)? {
                    events.extend(msgs.map(|msg| ClientEvent::Recv { msg }));
                }
            }

            Ok(())
        })();

        // disconnect if errors found
        match res {
            Ok(()) => (events, Inner::Connected(client)),
            Err(error) => {
                events.push(ClientEvent::Disconnected { error });
                (events, Inner::Disconnected)
            }
        }
    }
}
