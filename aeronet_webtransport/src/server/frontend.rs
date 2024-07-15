use std::future::Future;

use aeronet::{
    client::ClientState,
    error::pretty_error,
    lane::LaneIndex,
    server::{ServerEvent, ServerState, ServerTransport},
};
use aeronet_proto::session::{Session, SessionConfig};
use bytes::Bytes;
use either::Either;
use futures::channel::oneshot;
use replace_with::{replace_with_or_abort, replace_with_or_abort_and_return};
use slotmap::SlotMap;
use tracing::{debug, info};
use web_time::{Duration, Instant};

use crate::shared::MessageKey;

use super::{
    backend, Client, ClientKey, Connected, Connecting, ConnectionResponse, Open, Opening,
    ServerConfig, ServerError, State, ToOpen, WebTransportServer,
};

impl WebTransportServer {
    pub fn closed() -> Self {
        Self {
            state: State::Closed,
        }
    }

    pub fn close(&mut self) -> Result<(), ServerError> {
        match self.state {
            State::Closed => Err(ServerError::AlreadyClosed),
            State::Opening(_) | State::Open(_) => {
                *self = Self::closed();
                Ok(())
            }
        }
    }

    pub fn open_new(
        net_config: ServerConfig,
        session_config: SessionConfig,
    ) -> (Self, impl Future<Output = ()> + Send) {
        let (send_open, recv_open) = oneshot::channel::<ToOpen>();
        let (send_err, recv_err) = oneshot::channel::<ServerError>();

        let frontend = Self {
            state: State::Opening(Opening {
                recv_open,
                recv_err,
            }),
        };
        let backend = async move {
            if let Err(err) = backend::start(net_config, session_config, send_open).await {
                let _ = send_err.send(err);
            }
        };
        (frontend, backend)
    }

    pub fn open(
        &mut self,
        net_config: ServerConfig,
        session_config: SessionConfig,
    ) -> Result<impl Future<Output = ()> + Send, ServerError> {
        match self.state {
            State::Closed => {
                let (frontend, backend) = Self::open_new(net_config, session_config);
                *self = frontend;
                Ok(backend)
            }
            State::Opening(_) | State::Open(_) => Err(ServerError::AlreadyOpen),
        }
    }
}

impl ServerTransport for WebTransportServer {
    type Error = ServerError;

    type Opening<'this> = &'this Opening;

    type Open<'this> = &'this Open;

    type Connecting<'this> = &'this Connecting;

    type Connected<'this> = &'this Connected;

    type ClientKey = ClientKey;

    type MessageKey = MessageKey;

    fn state(&self) -> ServerState<Self::Opening<'_>, Self::Open<'_>> {
        self.state.as_ref()
    }

    fn client_state(
        &self,
        client_key: Self::ClientKey,
    ) -> ClientState<Self::Connecting<'_>, Self::Connected<'_>> {
        let State::Open(server) = &self.state else {
            return ClientState::Disconnected;
        };
        server
            .clients
            .get(client_key)
            .map(ClientState::as_ref)
            .unwrap_or(ClientState::Disconnected)
    }

    fn client_keys(&self) -> impl Iterator<Item = Self::ClientKey> + '_ {
        match &self.state {
            State::Closed | State::Opening(_) => None,
            State::Open(server) => Some(server.clients.keys()),
        }
        .into_iter()
        .flatten()
    }

    fn send(
        &mut self,
        client_key: Self::ClientKey,
        msg: impl Into<Bytes>,
        lane: impl Into<LaneIndex>,
    ) -> Result<Self::MessageKey, Self::Error> {
        let State::Open(server) = &mut self.state else {
            return Err(ServerError::NotOpen);
        };
        let Some(Client::Connected(client)) = server.clients.get_mut(client_key) else {
            return Err(ServerError::ClientNotConnected);
        };

        let msg = msg.into();
        let lane = lane.into();
        client
            .session
            .send(Instant::now(), &msg, lane)
            .map(MessageKey::from_raw)
            .map_err(ServerError::Send)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        let State::Open(server) = &mut self.state else {
            return Err(ServerError::NotOpen);
        };

        for (_, client) in server.clients.iter_mut() {
            let Client::Connected(client) = client else {
                continue;
            };

            for packet in client.session.flush(Instant::now()) {
                // ignore errors here, pick them up in `poll`
                let _ = client.send_s2c.unbounded_send(packet);
            }
        }
        Ok(())
    }

    fn disconnect(&mut self, client_key: Self::ClientKey) -> Result<(), Self::Error> {
        let State::Open(server) = &mut self.state else {
            return Err(ServerError::NotOpen);
        };

        server
            .clients
            .remove(client_key)
            .map(|_| ())
            .ok_or(ServerError::ClientNotConnected)
    }

    fn poll(&mut self, delta_time: Duration) -> impl Iterator<Item = ServerEvent<Self>> {
        replace_with_or_abort_and_return(&mut self.state, |state| match state {
            State::Closed => (Either::Left(None), State::Closed),
            State::Opening(server) => {
                let (res, state) = Self::poll_opening(server);
                (Either::Left(res), state)
            }
            State::Open(server) => {
                let (res, state) = Self::poll_open(server, delta_time);
                (Either::Right(res), state)
            }
        })
        .into_iter()
    }
}

impl WebTransportServer {
    pub fn respond_to_request(
        &mut self,
        client_key: ClientKey,
        resp: ConnectionResponse,
    ) -> Result<(), ServerError> {
        let State::Open(server) = &mut self.state else {
            return Err(ServerError::NotOpen);
        };
        let Some(Client::Connecting(client)) = server.clients.get_mut(client_key) else {
            return Err(ServerError::ClientNotConnecting);
        };

        // ignore errors here because we'll pick up errors in `poll`
        let _ = client
            .send_conn_resp
            .take()
            .ok_or(ServerError::AlreadyResponded)?
            .send(resp);
        Ok(())
    }

    fn poll_opening(mut server: Opening) -> (Option<ServerEvent<Self>>, State) {
        if let Ok(Some(error)) = server.recv_err.try_recv() {
            return (Some(ServerEvent::Closed { error }), State::Closed);
        }

        match server.recv_open.try_recv() {
            Ok(None) => (None, State::Opening(server)),
            Ok(Some(next)) => (
                Some(ServerEvent::Opened),
                State::Open(Open {
                    local_addr: next.local_addr,
                    recv_connecting: next.recv_connecting,
                    clients: SlotMap::default(),
                    _send_closed: next.send_closed,
                }),
            ),
            Err(_) => (
                Some(ServerEvent::Closed {
                    error: ServerError::BackendClosed,
                }),
                State::Closed,
            ),
        }
    }

    fn poll_open(mut server: Open, delta_time: Duration) -> (Vec<ServerEvent<Self>>, State) {
        let mut events = Vec::new();
        let res = (|| {
            while let Ok(client) = server.recv_connecting.try_next() {
                let client = client.ok_or(ServerError::BackendClosed)?;
                let client_key = server.clients.insert(Client::Connecting(Connecting {
                    authority: client.authority,
                    path: client.path,
                    origin: client.origin,
                    user_agent: client.user_agent,
                    headers: client.headers,
                    recv_err: client.recv_err,
                    send_conn_resp: Some(client.send_conn_resp),
                    recv_connected: client.recv_connected,
                }));
                let _ = client.send_key.send(client_key);
                events.push(ServerEvent::Connecting { client_key });
            }

            for (client_key, client) in &mut server.clients {
                replace_with_or_abort(client, |client_state| match client_state {
                    Client::Disconnected => ClientState::Disconnected,
                    Client::Connecting(client) => {
                        Self::poll_connecting(client_key, client, &mut events)
                    }
                    Client::Connected(client) => {
                        Self::poll_connected(client_key, client, &mut events, delta_time)
                    }
                });
            }

            server
                .clients
                .retain(|_, client| !matches!(client, Client::Disconnected));

            Ok(())
        })();

        match res {
            Ok(()) => (events, State::Open(server)),
            Err(error) => {
                events.push(ServerEvent::Closed { error });
                (events, State::Closed)
            }
        }
    }

    fn poll_connecting(
        client_key: ClientKey,
        mut client: Connecting,
        events: &mut Vec<ServerEvent<Self>>,
    ) -> Client {
        let res = (|| {
            if let Some(err) = client
                .recv_err
                .try_recv()
                .map_err(|_| ServerError::BackendClosed)?
            {
                return Err(err);
            }

            if let Ok(Some(next)) = client.recv_connected.try_recv() {
                events.push(ServerEvent::Connected { client_key });
                Ok(Client::Connected(Connected {
                    remote_addr: next.remote_addr,
                    rtt: next.initial_rtt,
                    session: next.session,
                    recv_err: client.recv_err,
                    recv_meta: next.recv_meta,
                    recv_c2s: next.recv_c2s,
                    send_s2c: next.send_s2c,
                }))
            } else {
                Ok(Client::Connecting(client))
            }
        })();

        match res {
            Ok(client) => client,
            Err(error) => {
                events.push(ServerEvent::Disconnected { client_key, error });
                Client::Disconnected
            }
        }
    }

    fn poll_connected(
        client_key: ClientKey,
        mut client: Connected,
        events: &mut Vec<ServerEvent<Self>>,
        delta_time: Duration,
    ) -> Client {
        let res = (|| {
            if let Some(err) = client
                .recv_err
                .try_recv()
                .map_err(|_| ServerError::BackendClosed)?
            {
                return Err(err);
            }

            while let Ok(Some(meta)) = client.recv_meta.try_next() {
                client.rtt = meta.rtt;
                client
                    .session
                    .set_mtu(meta.mtu)
                    .map_err(ServerError::MtuTooSmall)?;
            }

            client.session.refill_bytes(delta_time);

            while let Ok(Some(packet)) = client.recv_c2s.try_next() {
                let (acks, mut msgs) = match client.session.recv(Instant::now(), packet) {
                    Ok(x) => x,
                    Err(err) => {
                        debug!(
                            "Error while reading packet from server: {:#}",
                            pretty_error(&err)
                        );
                        continue;
                    }
                };

                events.extend(acks.map(|seq| ServerEvent::Ack {
                    client_key,
                    msg_key: MessageKey::from_raw(seq),
                }));

                let res = msgs.for_each_msg(|res| match res {
                    Ok((msg, lane)) => {
                        events.push(ServerEvent::Recv {
                            client_key,
                            msg,
                            lane,
                        });
                    }
                    Err(err) => {
                        debug!(
                            "Error while reading packet from server: {:#}",
                            pretty_error(&err)
                        );
                    }
                });

                if let Err(err) = res {
                    return Err(ServerError::OutOfMemory(err));
                }
            }

            Ok(())
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
