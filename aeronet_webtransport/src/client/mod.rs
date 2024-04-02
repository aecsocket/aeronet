mod backend;

use std::{fmt::Debug, future::Future, time::Duration};

use aeronet::{
    client::{ClientEvent, ClientEventFor, ClientState, ClientTransport},
    lane::OnLane,
    message::{TryFromBytes, TryIntoBytes},
    protocol::{ProtocolVersion, TransportProtocol},
};
use aeronet_proto::{packet, seq::Seq};
use bytes::Bytes;
use derivative::Derivative;
use either::Either;
use futures::channel::{mpsc, oneshot};
use xwt_core::utils::maybe;

use crate::{error::BackendError, transport::ConnectionStats};

#[cfg(target_family = "wasm")]
type NativeConfig = web_sys::WebTransportOptions;
#[cfg(not(target_family = "wasm"))]
type NativeConfig = wtransport::ClientConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientMessageKey {
    msg_seq: Seq,
}

#[derive(Derivative, thiserror::Error)]
#[derivative(Debug(bound = "packet::SendError<P::C2S>: Debug, packet::RecvError<P::S2C>: Debug"))]
pub enum WebTransportClientError<P>
where
    P: TransportProtocol,
    P::C2S: TryIntoBytes,
    P::S2C: TryFromBytes,
{
    #[error("already disconnected")]
    AlreadyDisconnected,
    #[error("not connected")]
    NotConnected,
    #[error("backend closed")]
    BackendClosed,

    #[error(transparent)]
    Backend(#[from] BackendError),
    #[error(transparent)]
    Send(#[from] packet::SendError<P::C2S>),
    #[error(transparent)]
    Recv(#[from] packet::RecvError<P::S2C>),
}

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
struct Connecting {
    recv_connected: oneshot::Receiver<Result<backend::Connected, BackendError>>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
struct Connected<P: TransportProtocol> {
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
    pub fn disconnected() -> Self {
        Self {
            inner: Inner::Disconnected,
        }
    }

    pub fn disconnect(&mut self) -> Result<(), WebTransportClientError<P>> {
        match self.inner {
            Inner::Disconnected => Err(WebTransportClientError::AlreadyDisconnected),
            _ => {
                self.inner = Inner::Disconnected;
                Ok(())
            }
        }
    }

    pub fn connect_new(
        config: NativeConfig,
        url: impl Into<String>,
    ) -> (Self, impl Future<Output = ()> + maybe::Send) {
        let url = url.into();
        let (send_connected, recv_connected) =
            oneshot::channel::<Result<backend::Connected, BackendError>>();
        let backend = async move {
            let (send_raw_connected, recv_raw_connected) = oneshot::channel::<backend::Connected>();
            futures::join!(
                async move {
                    if let Ok(connected) = recv_raw_connected.await {
                        let _ = send_connected.send(Ok(connected));
                    }
                },
                async move {
                    let err = backend::open(
                        config,
                        url,
                        ProtocolVersion(0), /* TODO */
                        send_raw_connected,
                    )
                    .await;
                    let _ = send_connected.send(Err(err));
                }
            );
        };
        (
            Self {
                inner: Inner::Connecting(Connecting { recv_connected }),
            },
            backend,
        )
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
        match client.recv_connected.try_recv() {
            Ok(None) => (None, Inner::Connecting(client)),
            Ok(Some(Ok(next))) => (
                Some(ClientEvent::Connected),
                Inner::Connected(Connected {
                    send_c2s: next.send_c2s,
                    recv_s2c: next.recv_s2c,
                    recv_stats: next.recv_stats,
                    stats: next.initial_stats,
                    // TODO
                    packets: packet::Packets::new(1, 1, &[]),
                    max_sent_bytes_per_sec: 1,
                    bytes_left: 1,
                }),
            ),
            Ok(Some(Err(reason))) => (
                Some(ClientEvent::Disconnected {
                    reason: reason.into(),
                }),
                Inner::Disconnected,
            ),
            Err(_) => (
                Some(ClientEvent::Disconnected {
                    reason: WebTransportClientError::BackendClosed,
                }),
                Inner::Disconnected,
            ),
        }
    }

    fn poll_connected(
        mut client: Connected<P>,
        delta_time: Duration,
    ) -> (Vec<ClientEventFor<P, Self>>, Inner<P>) {
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
            Err(reason) => {
                events.push(ClientEvent::Disconnected { reason });
                (events, Inner::Disconnected)
            }
        }
    }
}
