//! Server-side items.

use std::{convert::Infallible, fmt::Display, iter};

use aeronet::{
    client::ClientState,
    lane::LaneIndex,
    server::{ServerEvent, ServerState, ServerTransport},
    stats::{ConnectedAt, MessageStats},
};
use bytes::Bytes;
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use either::Either;
use slotmap::SlotMap;
use web_time::{Duration, Instant};

slotmap::new_key_type! {
    /// Key identifying a unique client connected to a [`ChannelServer`].
    ///
    /// If a client is connected, disconnected, and reconnected to the same
    /// server, it will have a different client key.
    pub struct ClientKey;
}

impl Display for ClientKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// Implementation of [`ServerTransport`] using in-memory MPSC channels.
///
/// See the [crate-level documentation](crate).
#[derive(Debug, Default)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct ChannelServer {
    state: State,
}

#[derive(Debug, Default)]
enum State {
    #[default]
    Closed,
    Open(Open),
}

/// State of a [`ChannelServer`] when it is [`ServerState::Open`].
#[derive(Debug)]
pub struct Open {
    clients: SlotMap<ClientKey, Client>,
}

/// State of a [`ChannelServer`]'s client when it is [`ClientState::Connected`].
#[derive(Debug)]
pub struct Connected {
    /// See [`ConnectedAt::connected_at`].
    pub connected_at: Instant,
    /// See [`MessageStats::bytes_sent`].
    pub bytes_sent: usize,
    /// See [`MessageStats::bytes_recv`]
    pub bytes_recv: usize,
    recv_c2s: Receiver<(Bytes, LaneIndex)>,
    send_s2c: Sender<(Bytes, LaneIndex)>,
    send_connected: bool,
}

impl ConnectedAt for Connected {
    fn connected_at(&self) -> Instant {
        self.connected_at
    }
}

impl MessageStats for Connected {
    fn bytes_sent(&self) -> usize {
        self.bytes_sent
    }

    fn bytes_recv(&self) -> usize {
        self.bytes_recv
    }
}

#[derive(Debug)]
enum Client {
    Disconnected,
    Connected(Connected),
}

/// Error type for operations on a [`ChannelServer`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum ServerError {
    #[error("already open")]
    AlreadyOpen,
    /// Server is not open.
    #[error("not open")]
    NotOpen,
    /// Server is already closed.
    #[error("already closed")]
    AlreadyClosed,
    /// There is no connected client with this key.
    #[error("client with this key not connected")]
    NotConnected,
    /// Client was unexpectedly disconnected.
    #[error("client disconnected")]
    Disconnected,
}

impl ChannelServer {
    /// Creates a server which starts closed.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: State::Closed,
        }
    }

    /// Allows accepting connections on this server.
    ///
    /// # Errors
    ///
    /// Errors if this server is already open.
    pub fn open(&mut self) -> Result<(), ServerError> {
        if !matches!(self.state, State::Closed) {
            return Err(ServerError::AlreadyOpen);
        }

        self.state = State::Open(Open {
            clients: SlotMap::default(),
        });
        Ok(())
    }

    pub(super) fn insert_client(
        &mut self,
        recv_c2s: Receiver<(Bytes, LaneIndex)>,
        send_s2c: Sender<(Bytes, LaneIndex)>,
    ) -> Option<ClientKey> {
        let State::Open(server) = &mut self.state else {
            return None;
        };

        Some(server.clients.insert(Client::Connected(Connected {
            connected_at: Instant::now(),
            bytes_sent: 0,
            bytes_recv: 0,
            recv_c2s,
            send_s2c,
            send_connected: true,
        })))
    }
}

impl ServerTransport for ChannelServer {
    type Error = ServerError;

    type Opening<'this> = Infallible;

    type Open<'this> = &'this Open;

    type Connecting<'this> = Infallible;

    type Connected<'this> = &'this Connected;

    type ClientKey = ClientKey;

    type MessageKey = ();

    fn state(&self) -> ServerState<Self::Opening<'_>, Self::Open<'_>> {
        match &self.state {
            State::Closed => ServerState::Closed,
            State::Open(server) => ServerState::Open(server),
        }
    }

    fn client_state(
        &self,
        client_key: ClientKey,
    ) -> ClientState<Self::Connecting<'_>, Self::Connected<'_>> {
        let State::Open(server) = &self.state else {
            return ClientState::Disconnected;
        };

        match server.clients.get(client_key) {
            None | Some(Client::Disconnected) => ClientState::Disconnected,
            Some(Client::Connected(client)) => ClientState::Connected(client),
        }
    }

    fn client_keys(&self) -> impl Iterator<Item = Self::ClientKey> + '_ {
        match &self.state {
            State::Closed => Either::Left(iter::empty()),
            State::Open(server) => Either::Right(server.clients.keys()),
        }
        .into_iter()
    }

    fn poll(&mut self, _: Duration) -> impl Iterator<Item = ServerEvent<Self>> {
        match &mut self.state {
            State::Closed => Either::Left(iter::empty()),
            State::Open(server) => Either::Right(Self::poll_open(server)),
        }
        .into_iter()
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
            return Err(ServerError::NotConnected);
        };

        let msg = msg.into();
        let lane = lane.into();

        let msg_len = msg.len();
        client
            .send_s2c
            .send((msg, lane))
            .map_err(|_| ServerError::Disconnected)?;
        client.bytes_sent = client.bytes_sent.saturating_add(msg_len);

        Ok(())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn disconnect(&mut self, client_key: Self::ClientKey) -> Result<(), Self::Error> {
        let State::Open(server) = &mut self.state else {
            return Err(ServerError::NotOpen);
        };

        server
            .clients
            .remove(client_key)
            .ok_or(ServerError::NotConnected)
            .map(drop)
    }

    fn close(&mut self) -> Result<(), Self::Error> {
        if matches!(self.state, State::Closed) {
            return Err(ServerError::AlreadyClosed);
        }

        self.state = State::Closed;

        Ok(())
    }
}

impl ChannelServer {
    fn poll_open(server: &mut Open) -> Vec<ServerEvent<Self>> {
        let mut events = Vec::new();
        for (client_key, client) in &mut server.clients {
            replace_with::replace_with_or_abort(client, |client| match client {
                Client::Disconnected => client,
                Client::Connected(client) => Self::poll_connected(&mut events, client_key, client),
            });
        }

        server
            .clients
            .retain(|_, client| !matches!(client, Client::Disconnected));

        events
    }

    fn poll_connected(
        events: &mut Vec<ServerEvent<Self>>,
        client_key: ClientKey,
        mut client: Connected,
    ) -> Client {
        if client.send_connected {
            events.push(ServerEvent::Connecting { client_key });
            events.push(ServerEvent::Connected { client_key });
            client.send_connected = false;
        }

        loop {
            match client.recv_c2s.try_recv() {
                Ok((msg, lane)) => {
                    client.bytes_recv = client.bytes_recv.saturating_add(msg.len());
                    events.push(ServerEvent::Recv {
                        client_key,
                        msg,
                        lane,
                    });
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    events.push(ServerEvent::Disconnected {
                        client_key,
                        error: ServerError::Disconnected,
                    });
                    return Client::Disconnected;
                }
            }
        }

        Client::Connected(client)
    }
}
