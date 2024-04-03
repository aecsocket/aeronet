use std::{collections::HashMap, future::Future, net::SocketAddr, time::Duration};

use aeronet::{
    client::ClientState,
    error::pretty_error,
    lane::{LaneKind, OnLane},
    message::{TryFromBytes, TryIntoBytes},
    protocol::TransportProtocol,
    server::{ServerEvent, ServerState, ServerTransport},
};
use aeronet_proto::packet;
use bytes::Bytes;
use derivative::Derivative;
use either::Either;
use futures::channel::{mpsc, oneshot};
use slotmap::SlotMap;
use tracing::debug;

use crate::shared::{self, ConnectionStats, MessageKey};

use super::{
    backend, ClientKey, ServerBackendError, WebTransportServerConfig, WebTransportServerError,
};

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct WebTransportServer<P: TransportProtocol> {
    inner: Inner<P>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
enum Inner<P: TransportProtocol> {
    #[derivative(Default)]
    Closed,
    Opening(Opening),
    Open(Open<P>),
}

#[derive(Debug)]
struct Opening {
    lanes: Box<[LaneKind]>,
    total_bandwidth: usize,
    bandwidth_per_client: usize,
    max_packet_len: usize,
    default_packet_cap: usize,
    recv_err: oneshot::Receiver<ServerBackendError>,
    recv_open: oneshot::Receiver<backend::Open>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct Open<P: TransportProtocol> {
    pub local_addr: SocketAddr,
    pub bandwidth: usize,
    pub bytes_left: usize,
    clients: SlotMap<ClientKey, Client<P>>,
    recv_err: oneshot::Receiver<ServerBackendError>,
    recv_connecting: mpsc::Receiver<backend::Connecting>,
}

#[derive(Debug)]
struct Connecting {
    recv_err: oneshot::Receiver<ServerBackendError>,
    recv_requesting: oneshot::Receiver<backend::Requesting>,
}

#[derive(Debug)]
pub struct Requesting {
    pub authority: String,
    pub path: String,
    pub origin: Option<String>,
    pub user_agent: Option<String>,
    pub headers: HashMap<String, String>,
    recv_err: oneshot::Receiver<ServerBackendError>,
    recv_connected: oneshot::Receiver<backend::Connected>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct Connected<P: TransportProtocol> {
    pub stats: ConnectionStats,
    pub bandwidth: usize,
    pub bytes_left: usize,
    packets: packet::Packets<P::S2C, P::C2S>,
    recv_err: oneshot::Receiver<ServerBackendError>,
    recv_c2s: mpsc::Receiver<Bytes>,
    send_s2c: mpsc::UnboundedSender<Bytes>,
    recv_stats: mpsc::Receiver<ConnectionStats>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
enum Client<P: TransportProtocol> {
    Disconnected,
    Connecting(Connecting),
    Requesting(Requesting),
    Connected(Connected<P>),
}

impl<P> WebTransportServer<P>
where
    P: TransportProtocol,
    P::C2S: TryFromBytes + OnLane,
    P::S2C: TryIntoBytes + OnLane,
{
    #[must_use]
    pub fn closed() -> Self {
        Self {
            inner: Inner::Closed,
        }
    }

    pub fn close(&mut self) -> Result<(), WebTransportServerError<P>> {
        if let Inner::Closed = self.inner {
            return Err(WebTransportServerError::AlreadyClosed);
        }

        self.inner = Inner::Closed;
        Ok(())
    }

    pub fn open_new(config: WebTransportServerConfig) -> (Self, impl Future<Output = ()> + Send) {
        let WebTransportServerConfig {
            native: native_config,
            version,
            lanes,
            total_bandwidth,
            bandwidth_per_client,
            max_packet_len,
            default_packet_cap,
        } = config;
        let (send_err, recv_err) = oneshot::channel::<ServerBackendError>();
        let (send_open, recv_open) = oneshot::channel::<backend::Open>();
        let backend = async move {
            let Err(err) = backend::start(native_config, version, send_open).await else {
                unreachable!()
            };
            match err {
                ServerBackendError::Generic(shared::BackendError::FrontendClosed) => {
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
                inner: Inner::Opening(Opening {
                    lanes,
                    total_bandwidth,
                    bandwidth_per_client,
                    max_packet_len,
                    default_packet_cap,
                    recv_err,
                    recv_open,
                }),
            },
            backend,
        )
    }
}

impl<P> ServerTransport<P> for WebTransportServer<P>
where
    P: TransportProtocol,
    P::C2S: TryFromBytes + OnLane,
    P::S2C: TryIntoBytes + OnLane,
{
    type Error = WebTransportServerError<P>;

    type Opening<'t> = ();

    type Open<'t> = &'t Open<P>;

    type Connecting<'t> = &'t Requesting;

    type Connected<'t> = &'t Connected<P>;

    type ClientKey = ClientKey;

    type MessageKey = MessageKey;

    fn state(&self) -> ServerState<Self::Opening<'_>, Self::Open<'_>> {
        match &self.inner {
            Inner::Closed => ServerState::Closed,
            Inner::Opening(_) => ServerState::Opening(()),
            Inner::Open(server) => ServerState::Open(server),
        }
    }

    fn client_state(
        &self,
        client_key: Self::ClientKey,
    ) -> ClientState<Self::Connecting<'_>, Self::Connected<'_>> {
        let Inner::Open(server) = &self.inner else {
            return ClientState::Disconnected;
        };

        match server.clients.get(client_key) {
            None | Some(Client::Disconnected) | Some(Client::Connecting(_)) => {
                ClientState::Disconnected
            }
            Some(Client::Requesting(client)) => ClientState::Connecting(client),
            Some(Client::Connected(client)) => ClientState::Connected(client),
        }
    }

    fn client_keys(&self) -> impl Iterator<Item = Self::ClientKey> + '_ {
        let Inner::Open(server) = &self.inner else {
            return Either::Left(std::iter::empty());
        };
        Either::Right(server.clients.keys())
    }

    fn send(
        &mut self,
        client_key: Self::ClientKey,
        msg: impl Into<P::S2C>,
    ) -> Result<Self::MessageKey, Self::Error> {
        let Inner::Open(server) = &mut self.inner else {
            return Err(WebTransportServerError::NotOpen);
        };
        let Some(Client::Connected(client)) = server.clients.get_mut(client_key) else {
            return Err(WebTransportServerError::ClientNotConnected);
        };

        let msg = msg.into();
        let msg_seq = client.packets.buffer_send(msg)?;
        Ok(MessageKey::from_raw(msg_seq))
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        let Inner::Open(server) = &mut self.inner else {
            return Err(WebTransportServerError::NotOpen);
        };

        for (_, client) in &mut server.clients {
            let Client::Connected(client) = client else {
                continue;
            };

            let bytes_start = (server.bytes_left).min(client.bytes_left);
            let mut bytes_left = bytes_start;
            for packet in client.packets.flush(&mut bytes_left) {
                client
                    .send_s2c
                    .unbounded_send(packet)
                    .map_err(|_| WebTransportServerError::BackendClosed)?;
            }
            let bytes_used = bytes_start - bytes_left;
            server.bytes_left -= bytes_used;
            client.bytes_left -= bytes_used;
        }
        Ok(())
    }

    fn disconnect(&mut self, client_key: Self::ClientKey) -> Result<(), Self::Error> {
        let Inner::Open(server) = &mut self.inner else {
            return Err(WebTransportServerError::NotOpen);
        };

        server
            .clients
            .remove(client_key)
            .map(|_| ())
            .ok_or(WebTransportServerError::ClientNotConnected)
    }

    fn poll(&mut self, delta_time: Duration) -> impl Iterator<Item = ServerEvent<P, Self>> {
        replace_with::replace_with_or_abort_and_return(&mut self.inner, |inner| match inner {
            Inner::Closed => (Either::Left(None), inner),
            Inner::Opening(server) => {
                let (res, new) = Self::poll_opening(server);
                (Either::Left(res), new)
            }
            Inner::Open(server) => {
                let (res, new) = Self::poll_open(server, delta_time);
                (Either::Right(res), new)
            }
        })
        .into_iter()
    }
}

impl<P> WebTransportServer<P>
where
    P: TransportProtocol,
    P::C2S: TryFromBytes + OnLane,
    P::S2C: TryIntoBytes + OnLane,
{
    fn poll_opening(mut server: Opening) -> (Option<ServerEvent<P, Self>>, Inner<P>) {
        if let Ok(Some(err)) = server.recv_err.try_recv() {
            return (
                Some(ServerEvent::Closed { error: err.into() }),
                Inner::Closed,
            );
        }
        match server.recv_open.try_recv() {
            Ok(None) => (None, Inner::Opening(server)),
            Ok(Some(next)) => (
                Some(ServerEvent::Opened),
                Inner::Open(Open {
                    local_addr: next.local_addr,
                    bandwidth: server.total_bandwidth,
                    bytes_left: server.total_bandwidth,
                    clients: SlotMap::default(),
                    recv_err: server.recv_err,
                    recv_connecting: next.recv_connecting,
                }),
            ),
            Err(_) => (
                Some(ServerEvent::Closed {
                    error: WebTransportServerError::BackendClosed,
                }),
                Inner::Closed,
            ),
        }
    }

    fn poll_open(
        mut server: Open<P>,
        delta_time: Duration,
    ) -> (Vec<ServerEvent<P, Self>>, Inner<P>) {
        let res = (|| {
            if let Some(err) = server
                .recv_err
                .try_recv()
                .map_err(|_| WebTransportServerError::BackendClosed)?
            {
                return Err(err.into());
            }

            // track new clients
            while let Some(connecting) = server
                .recv_connecting
                .try_next()
                .map_err(|_| WebTransportServerError::BackendClosed)?
            {
                let client_key = server.clients.insert(Client::Connecting(Connecting {
                    recv_err: connecting.recv_err,
                    recv_requesting: connecting.recv_requesting,
                }));
                connecting
                    .send_key
                    .send(client_key)
                    .map_err(|_| WebTransportServerError::BackendClosed)?;
            }

            Ok::<_, WebTransportServerError<P>>(())
        })();

        // close if there was a server error
        if let Err(err) = res {
            return (
                vec![ServerEvent::Closed { error: err.into() }],
                Inner::Closed,
            );
        }

        // refill bytes token bucket
        let bytes_restored = ((server.bandwidth as f64) * delta_time.as_secs_f64()) as usize;
        server.bytes_left = (server.bytes_left + bytes_restored).min(server.bandwidth);

        // process clients
        let mut events = Vec::new();
        for (client_key, client) in &mut server.clients {
            replace_with::replace_with_or_abort(client, |client| match client {
                Client::Disconnected => Client::Disconnected,
                Client::Connecting(client) => {
                    Self::poll_connecting(&mut events, client_key, client)
                }
                Client::Requesting(client) => {
                    Self::poll_requesting(&mut events, client_key, client)
                }
                Client::Connected(client) => {
                    Self::poll_connected(&mut events, client_key, client, delta_time)
                }
            });
        }

        let removed_clients = server
            .clients
            .iter()
            .filter_map(|(client_key, client)| match client {
                Client::Disconnected => Some(client_key),
                _ => None,
            })
            .collect::<Vec<_>>();
        for client_key in removed_clients {
            server.clients.remove(client_key);
        }

        (events, Inner::Open(server))
    }

    fn poll_connecting(
        events: &mut Vec<ServerEvent<P, Self>>,
        client_key: ClientKey,
        mut client: Connecting,
    ) -> Client<P> {
        let res = (|| {
            if let Some(err) = client
                .recv_err
                .try_recv()
                .map_err(|_| WebTransportServerError::ClientBackendClosed)?
            {
                return Err(err.into());
            }

            if let Some(requesting) = client
                .recv_requesting
                .try_recv()
                .map_err(|_| WebTransportServerError::ClientBackendClosed)?
            {
                Ok(Client::Requesting(Requesting {
                    authority: requesting.authority,
                    path: requesting.path,
                    origin: requesting.origin,
                    user_agent: requesting.user_agent,
                    headers: requesting.headers,
                    recv_err: client.recv_err,
                    recv_connected: requesting.recv_connected,
                }))
            } else {
                Ok(Client::Connecting(client))
            }
        })();

        match res {
            Ok(new) => new,
            Err(error) => {
                events.push(ServerEvent::Disconnected { client_key, error });
                Client::Disconnected
            }
        }
    }

    fn poll_connected(
        events: &mut Vec<ServerEvent<P, Self>>,
        client_key: ClientKey,
        client: &mut Connected<P>,
        delta_time: Duration,
    ) -> Client<P> {
        // refill bytes token bucket
        let bytes_restored = ((client.bandwidth as f64) * delta_time.as_secs_f64()) as usize;
        client.bytes_left = (client.bytes_left + bytes_restored).min(client.bandwidth);

        while let Some(mut packet) = client
            .recv_c2s
            .try_next()
            .map_err(|_| WebTransportServerError::ClientBackendClosed)?
        {
            // receive acks
            events.extend(
                client
                    .packets
                    .read_acks(&mut packet)?
                    .map(|msg_seq| ServerEvent::Ack {
                        client_key,
                        msg_key: MessageKey::from_raw(msg_seq),
                    }),
            );

            // receive messages
            while let Some(msgs) = client.packets.read_next_frag(&mut packet)? {
                events.extend(msgs.map(|msg| ServerEvent::Recv { client_key, msg }));
            }
        }

        Ok(())
    }
}