use std::{fmt::Debug, future::Future, time::Duration};

use aeronet::{
    client::{ClientEvent, ClientState, ClientTransport},
    lane::{LaneKind, OnLane},
    message::{TryFromBytes, TryIntoBytes},
};
use aeronet_proto::{lane::LaneConfig, packet};
use bytes::Bytes;
use derivative::Derivative;
use either::Either;
use futures::channel::{mpsc, oneshot};
use xwt_core::utils::maybe;

use crate::{
    internal::TryRecv,
    shared::{ConnectionStats, MessageKey, WebTransportProtocol},
};

use super::{backend, BackendError, ClientConfig, ClientError, NativeConfig};

#[derive(Derivative)]
#[derivative(Debug(bound = "P::Mapper: Debug"), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct WebTransportClient<P: WebTransportProtocol> {
    inner: Inner<P>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = "P::Mapper: Debug"), Default(bound = ""))]
enum Inner<P: WebTransportProtocol> {
    #[derivative(Default)]
    Disconnected,
    Connecting(Connecting<P>),
    Connected(Connected<P>),
}

#[derive(Derivative)]
#[derivative(Debug(bound = "P::Mapper: Debug"))]
struct Connecting<P: WebTransportProtocol> {
    lanes_in: Box<[LaneKind]>,
    lanes_out: Box<[LaneConfig]>,
    mapper: P::Mapper,
    bandwidth: usize,
    max_packet_len: usize,
    default_packet_cap: usize,
    recv_err: oneshot::Receiver<BackendError>,
    recv_connected: oneshot::Receiver<backend::Connected>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = "P::Mapper: Debug"))]
pub struct Connected<P: WebTransportProtocol> {
    pub stats: ConnectionStats,
    pub bandwidth: usize,
    pub bytes_left: usize,
    packets: packet::Packets<P::C2S, P::S2C, P::Mapper>,
    recv_err: oneshot::Receiver<BackendError>,
    send_c2s: mpsc::UnboundedSender<Bytes>,
    recv_s2c: mpsc::Receiver<Bytes>,
    recv_stats: mpsc::Receiver<ConnectionStats>,
}

impl<P> WebTransportClient<P>
where
    P: WebTransportProtocol,
    P::C2S: TryIntoBytes + OnLane,
    P::S2C: TryFromBytes + OnLane,
{
    #[must_use]
    pub fn disconnected() -> Self {
        Self {
            inner: Inner::Disconnected,
        }
    }

    pub fn disconnect(&mut self) -> Result<(), ClientError<P>> {
        if let Inner::Disconnected = self.inner {
            return Err(ClientError::AlreadyDisconnected);
        }

        self.inner = Inner::Disconnected;
        Ok(())
    }

    #[must_use]
    pub fn connect_new(
        native_config: NativeConfig,
        config: ClientConfig,
        mapper: P::Mapper,
        target: impl Into<String>,
    ) -> (Self, impl Future<Output = ()> + maybe::Send) {
        let ClientConfig {
            version,
            lanes_in,
            lanes_out,
            bandwidth,
            max_packet_len,
            default_packet_cap,
        } = config;
        let target = target.into();
        let (send_err, recv_err) = oneshot::channel::<BackendError>();
        let (send_connected, recv_connected) = oneshot::channel::<backend::Connected>();
        let backend = async move {
            let err = backend::start(native_config, version, target, send_connected).await;
            let _ = send_err.send(err);
        };
        (
            Self {
                inner: Inner::Connecting(Connecting {
                    lanes_in: lanes_in.into_boxed_slice(),
                    lanes_out: lanes_out.into_boxed_slice(),
                    mapper,
                    bandwidth,
                    max_packet_len,
                    default_packet_cap,
                    recv_err,
                    recv_connected,
                }),
            },
            backend,
        )
    }

    pub fn connect(
        &mut self,
        native_config: NativeConfig,
        config: ClientConfig,
        mapper: P::Mapper,
        target: impl Into<String>,
    ) -> Result<impl Future<Output = ()> + maybe::Send, ClientError<P>> {
        let Inner::Disconnected = self.inner else {
            return Err(ClientError::AlreadyConnected);
        };

        let (this, backend) = Self::connect_new(native_config, config, mapper, target);
        *self = this;
        Ok(backend)
    }
}

impl<P> ClientTransport<P> for WebTransportClient<P>
where
    P: WebTransportProtocol,
    P::C2S: TryIntoBytes + OnLane,
    P::S2C: TryFromBytes + OnLane,
{
    type Error = ClientError<P>;

    type Connecting<'t> = ();

    type Connected<'t> = &'t Connected<P>;

    type MessageKey = MessageKey;

    fn state(&self) -> ClientState<Self::Connecting<'_>, Self::Connected<'_>> {
        match &self.inner {
            Inner::Disconnected => ClientState::Disconnected,
            Inner::Connecting { .. } => ClientState::Connecting(()),
            Inner::Connected(client) => ClientState::Connected(client),
        }
    }

    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<Self::MessageKey, Self::Error> {
        let Inner::Connected(client) = &mut self.inner else {
            return Err(ClientError::NotConnected);
        };

        let msg = msg.into();
        let msg_seq = client.packets.buffer_send(msg)?;
        Ok(MessageKey::from_raw(msg_seq))
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        let Inner::Connected(client) = &mut self.inner else {
            return Err(ClientError::NotConnected);
        };

        for packet in client.packets.flush(&mut client.bytes_left) {
            client
                .send_c2s
                .unbounded_send(packet)
                .map_err(|_| ClientError::BackendClosed)?;
        }
        Ok(())
    }

    fn poll(&mut self, delta_time: Duration) -> impl Iterator<Item = ClientEvent<P, Self>> {
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
    P: WebTransportProtocol,
    P::C2S: TryIntoBytes + OnLane,
    P::S2C: TryFromBytes + OnLane,
{
    fn poll_connecting(mut client: Connecting<P>) -> (Option<ClientEvent<P, Self>>, Inner<P>) {
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
                    stats: next.initial_stats,
                    bandwidth: client.bandwidth,
                    bytes_left: client.bandwidth,
                    packets: packet::Packets::new(
                        client.max_packet_len,
                        client.default_packet_cap,
                        client.lanes_in.iter(),
                        client.lanes_out.iter(),
                        client.mapper,
                    ),
                    recv_err: client.recv_err,
                    send_c2s: next.send_c2s,
                    recv_s2c: next.recv_s2c,
                    recv_stats: next.recv_stats,
                }),
            ),
            Err(_) => (
                Some(ClientEvent::Disconnected {
                    error: ClientError::BackendClosed,
                }),
                Inner::Disconnected,
            ),
        }
    }

    fn poll_connected(
        mut client: Connected<P>,
        delta_time: Duration,
    ) -> (Vec<ClientEvent<P, Self>>, Inner<P>) {
        // refill bytes token bucket
        let bytes_restored = ((client.bandwidth as f64) * delta_time.as_secs_f64()) as usize;
        client.bytes_left = client
            .bytes_left
            .saturating_add(bytes_restored)
            .min(client.bandwidth);

        let mut events = Vec::new();
        let res = (|| {
            if let Some(error) = client
                .recv_err
                .try_recv()
                .map_err(|_| ClientError::BackendClosed)?
            {
                return Err(error.into());
            }

            // update connection stats
            while let Ok(Some(stats)) = client.recv_stats.try_recv() {
                client.stats = stats;
            }

            while let Ok(Some(mut packet)) = client.recv_s2c.try_recv() {
                // receive acks
                events.extend(client.packets.read_acks(&mut packet)?.map(|msg_seq| {
                    ClientEvent::Ack {
                        msg_key: MessageKey::from_raw(msg_seq),
                    }
                }));

                // receive messages
                while let Some(msgs) = client.packets.read_next_frag(&mut packet)? {
                    events.extend(msgs.map(|msg| ClientEvent::Recv { msg }));
                }
            }

            Ok::<_, ClientError<P>>(())
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
