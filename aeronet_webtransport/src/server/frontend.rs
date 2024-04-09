use std::{collections::HashMap, fmt::Debug, future::Future, net::SocketAddr, time::Duration};

use aeronet::{
    client::ClientState,
    lane::{LaneKind, OnLane},
    message::{TryFromBytes, TryIntoBytes},
    server::{ServerEvent, ServerState, ServerTransport},
};
use aeronet_proto::{
    byte_count::ByteBucket,
    packet::{self, LaneConfig},
};
use bytes::Bytes;
use derivative::Derivative;
use either::Either;
use futures::channel::{mpsc, oneshot};
use slotmap::SlotMap;
use web_time::Instant;

use crate::{
    internal::TryRecv,
    shared::{ConnectionStats, MessageKey, WebTransportProtocol},
};

use super::{
    backend, BackendError, ClientKey, ConnectionResponse, NativeConfig, ServerConfig, ServerError,
};

#[derive(Derivative)]
#[derivative(Debug(bound = "P::Mapper: Debug"), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct WebTransportServer<P: WebTransportProtocol> {
    inner: Inner<P>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = "P::Mapper: Debug"), Clone(bound = ""))]
pub struct InnerConfig<P: WebTransportProtocol> {
    pub lanes_send: Box<[LaneConfig]>,
    pub lanes_recv: Box<[LaneKind]>,
    pub mapper: P::Mapper,
    pub total_bandwidth: usize,
    pub client_bandwidth: usize,
    pub max_packet_len: usize,
    pub default_packet_cap: usize,
}

#[derive(Derivative)]
#[derivative(Debug(bound = "P::Mapper: Debug"), Default(bound = ""))]
enum Inner<P: WebTransportProtocol> {
    #[derivative(Default)]
    Closed,
    Opening(Opening<P>),
    Open(Open<P>),
}

#[derive(Derivative)]
#[derivative(Debug(bound = "P::Mapper: Debug"))]
struct Opening<P: WebTransportProtocol> {
    config: InnerConfig<P>,
    recv_err: oneshot::Receiver<BackendError>,
    recv_open: oneshot::Receiver<backend::Open>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = "P::Mapper: Debug"))]
pub struct Open<P: WebTransportProtocol> {
    pub config: InnerConfig<P>,
    pub local_addr: SocketAddr,
    pub bytes_left: ByteBucket,
    clients: SlotMap<ClientKey, Client<P>>,
    recv_err: oneshot::Receiver<BackendError>,
    recv_connecting: mpsc::Receiver<backend::Connecting>,
}

#[derive(Debug)]
pub struct Connecting {
    pub authority: String,
    pub path: String,
    pub origin: Option<String>,
    pub user_agent: Option<String>,
    pub headers: HashMap<String, String>,
    recv_err: oneshot::Receiver<BackendError>,
    send_conn_resp: Option<oneshot::Sender<ConnectionResponse>>,
    recv_connected: oneshot::Receiver<backend::Connected>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = "P::Mapper: Debug"))]
pub struct Connected<P: WebTransportProtocol> {
    pub remote_addr: SocketAddr,
    pub stats: ConnectionStats,
    pub packets: packet::PacketManager<P::S2C, P::C2S, P::Mapper>,
    recv_err: oneshot::Receiver<BackendError>,
    recv_c2s: mpsc::Receiver<Bytes>,
    send_s2c: mpsc::UnboundedSender<Bytes>,
    recv_stats: mpsc::Receiver<ConnectionStats>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = "P::Mapper: Debug"))]
enum Client<P: WebTransportProtocol> {
    Disconnected,
    Connecting(Connecting),
    Connected(Connected<P>),
}

impl<P> WebTransportServer<P>
where
    P: WebTransportProtocol,
    P::C2S: TryFromBytes + OnLane,
    P::S2C: TryIntoBytes + OnLane,
{
    #[must_use]
    pub fn closed() -> Self {
        Self {
            inner: Inner::Closed,
        }
    }

    pub fn close(&mut self) -> Result<(), ServerError<P>> {
        if let Inner::Closed = self.inner {
            return Err(ServerError::AlreadyClosed);
        }

        self.inner = Inner::Closed;
        Ok(())
    }

    #[must_use]
    pub fn open_new(
        native_config: NativeConfig,
        config: ServerConfig,
        mapper: P::Mapper,
    ) -> (Self, impl Future<Output = ()> + Send) {
        let ServerConfig {
            version,
            lanes_recv: lanes_in,
            lanes_send: lanes_out,
            total_bandwidth,
            client_bandwidth,
            max_packet_len,
            default_packet_cap,
        } = config;
        let (send_err, recv_err) = oneshot::channel::<BackendError>();
        let (send_open, recv_open) = oneshot::channel::<backend::Open>();
        let backend = async move {
            let err = backend::start(native_config, version, send_open).await;
            let _ = send_err.send(err);
        };
        (
            Self {
                inner: Inner::Opening(Opening {
                    config: InnerConfig {
                        lanes_recv: lanes_in.into_boxed_slice(),
                        lanes_send: lanes_out.into_boxed_slice(),
                        mapper,
                        total_bandwidth,
                        client_bandwidth,
                        max_packet_len,
                        default_packet_cap,
                    },
                    recv_err,
                    recv_open,
                }),
            },
            backend,
        )
    }

    pub fn open(
        &mut self,
        native_config: NativeConfig,
        config: ServerConfig,
        mapper: P::Mapper,
    ) -> Result<impl Future<Output = ()> + Send, ServerError<P>> {
        let Inner::Closed = &mut self.inner else {
            return Err(ServerError::AlreadyOpen);
        };

        let (this, backend) = Self::open_new(native_config, config, mapper);
        *self = this;
        Ok(backend)
    }

    pub fn respond_to_request(
        &mut self,
        client_key: ClientKey,
        resp: ConnectionResponse,
    ) -> Result<(), ServerError<P>> {
        let Inner::Open(server) = &mut self.inner else {
            return Err(ServerError::NotOpen);
        };
        let Some(client) = server.clients.get_mut(client_key) else {
            return Err(ServerError::NoClient { client_key });
        };
        let Client::Connecting(client) = client else {
            return Err(ServerError::ClientNotRequesting { client_key });
        };
        let Some(send_conn_resp) = client.send_conn_resp.take() else {
            return Err(ServerError::AlreadyResponded { client_key });
        };

        send_conn_resp
            .send(resp)
            .map_err(|_| ServerError::ClientBackendClosed)
    }
}

impl<P> ServerTransport<P> for WebTransportServer<P>
where
    P: WebTransportProtocol,
    P::C2S: TryFromBytes + OnLane,
    P::S2C: TryIntoBytes + OnLane,
{
    type Error = ServerError<P>;

    type Opening<'t> = ();

    type Open<'t> = &'t Open<P>;

    type Connecting<'t> = &'t Connecting;

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
            None | Some(Client::Disconnected) => ClientState::Disconnected,
            Some(Client::Connecting(client)) => ClientState::Connecting(client),
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
            return Err(ServerError::NotOpen);
        };
        let Some(client) = server.clients.get_mut(client_key) else {
            return Err(ServerError::NoClient { client_key });
        };
        let Client::Connected(client) = client else {
            return Err(ServerError::ClientNotConnected { client_key });
        };

        let msg = msg.into();
        let msg_seq = client.packets.buffer_send(msg, Instant::now())?;
        Ok(MessageKey::from_raw(msg_seq))
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        let Inner::Open(server) = &mut self.inner else {
            return Err(ServerError::NotOpen);
        };

        let now = Instant::now();
        for (_, client) in &mut server.clients {
            let Client::Connected(client) = client else {
                continue;
            };

            // TODO use self bytes_left
            for packet in client.packets.flush(now) {
                client
                    .send_s2c
                    .unbounded_send(packet)
                    .map_err(|_| ServerError::BackendClosed)?;
            }
        }
        Ok(())
    }

    fn disconnect(&mut self, client_key: Self::ClientKey) -> Result<(), Self::Error> {
        let Inner::Open(server) = &mut self.inner else {
            return Err(ServerError::NotOpen);
        };

        server
            .clients
            .remove(client_key)
            .map(|_| ())
            .ok_or(ServerError::NoClient { client_key })
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
    P: WebTransportProtocol,
    P::C2S: TryFromBytes + OnLane,
    P::S2C: TryIntoBytes + OnLane,
{
    fn poll_opening(mut server: Opening<P>) -> (Option<ServerEvent<P, Self>>, Inner<P>) {
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
                    bytes_left: ByteBucket::new(server.config.total_bandwidth),
                    config: server.config,
                    local_addr: next.local_addr,
                    clients: SlotMap::default(),
                    recv_err: server.recv_err,
                    recv_connecting: next.recv_connecting,
                }),
            ),
            Err(_) => (
                Some(ServerEvent::Closed {
                    error: ServerError::BackendClosed,
                }),
                Inner::Closed,
            ),
        }
    }

    fn poll_open(
        mut server: Open<P>,
        delta_time: Duration,
    ) -> (Vec<ServerEvent<P, Self>>, Inner<P>) {
        let mut events = Vec::new();

        let res = (|| {
            if let Some(err) = server
                .recv_err
                .try_recv()
                .map_err(|_| ServerError::BackendClosed)?
            {
                return Err(err.into());
            }

            // track new clients
            while let Some(connecting) = server
                .recv_connecting
                .try_recv()
                .map_err(|_| ServerError::BackendClosed)?
            {
                let client_key = server.clients.insert(Client::Connecting(Connecting {
                    authority: connecting.authority,
                    path: connecting.path,
                    origin: connecting.origin,
                    user_agent: connecting.user_agent,
                    headers: connecting.headers,
                    recv_err: connecting.recv_err,
                    send_conn_resp: Some(connecting.send_conn_resp),
                    recv_connected: connecting.recv_connected,
                }));
                connecting
                    .send_key
                    .send(client_key)
                    .map_err(|_| ServerError::BackendClosed)?;
                events.push(ServerEvent::Connecting { client_key });
            }

            Ok::<_, ServerError<P>>(())
        })();

        // close if there was a server error
        if let Err(err) = res {
            return (
                vec![ServerEvent::Closed { error: err.into() }],
                Inner::Closed,
            );
        }

        // refill bytes token bucket
        let refill_portion = delta_time.as_secs_f32();
        server.bytes_left.refill(refill_portion);

        // process clients
        let config = server.config.clone();
        for (client_key, client) in &mut server.clients {
            replace_with::replace_with_or_abort(client, |client| match client {
                Client::Disconnected => Client::Disconnected,
                Client::Connecting(client) => {
                    Self::poll_connecting(&mut events, client_key, client, &config)
                }
                Client::Connected(client) => {
                    Self::poll_connected(&mut events, client_key, client, refill_portion)
                }
            });
        }

        server.clients.retain(|_, client| match client {
            Client::Disconnected => false,
            _ => true,
        });

        (events, Inner::Open(server))
    }

    fn poll_connecting(
        events: &mut Vec<ServerEvent<P, Self>>,
        client_key: ClientKey,
        mut client: Connecting,
        config: &InnerConfig<P>,
    ) -> Client<P> {
        let res = (|| {
            if let Some(err) = client
                .recv_err
                .try_recv()
                .map_err(|_| ServerError::ClientBackendClosed)?
            {
                return Err(err.into());
            }

            if let Ok(Some(connected)) = client.recv_connected.try_recv() {
                events.push(ServerEvent::Connected { client_key });
                Ok(Client::Connected(Connected {
                    remote_addr: connected.remote_addr,
                    stats: connected.initial_stats,
                    packets: packet::PacketManager::new(
                        config.max_packet_len,
                        config.default_packet_cap,
                        config.client_bandwidth,
                        config.lanes_send.iter(),
                        config.lanes_recv.iter(),
                        config.mapper.clone(),
                    ),
                    recv_err: client.recv_err,
                    recv_c2s: connected.recv_c2s,
                    send_s2c: connected.send_s2c,
                    recv_stats: connected.recv_stats,
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
        mut client: Connected<P>,
        refill_portion: f32,
    ) -> Client<P> {
        client.packets.refill_bytes(refill_portion);

        let res = (|| {
            if let Some(err) = client
                .recv_err
                .try_recv()
                .map_err(|_| ServerError::ClientBackendClosed)?
            {
                return Err(err.into());
            }

            while let Ok(Some(packet)) = client.recv_c2s.try_recv() {
                let recv = client.packets.recv(packet);
                let (acks, mut recv) = recv.read_acks()?;

                // receive acks
                events.extend(acks.map(|msg_seq| ServerEvent::Ack {
                    client_key,
                    msg_key: MessageKey::from_raw(msg_seq),
                }));

                // receive messages
                while let Some(msgs) = recv.read_next_frag()? {
                    events.extend(msgs.map(|msg| ServerEvent::Recv { client_key, msg }));
                }
            }

            Ok::<_, ServerError<P>>(())
        })();

        match res {
            Ok(()) => Client::Connected(client),
            Err(error) => {
                events.push(ServerEvent::Disconnected { client_key, error });
                Client::Disconnected
            }
        }
    }
}
