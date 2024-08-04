//! Server-side items.

use std::{convert::Infallible, fmt::Display, iter, mem};

use aeronet::{
    client::{ClientState, DisconnectReason},
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
    /// See [`ClientTransport::default_disconnect_reason`].
    pub default_disconnect_reason: Option<String>,
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
    recv_dc_c2s: Receiver<String>,
    send_dc_s2c: Sender<String>,
    send_initial: bool,
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
    Disconnecting { reason: String },
}

/// Error type for operations on a [`ChannelServer`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum ServerError {
    /// Attempted to open a server which is already open.
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
    /// Creates a server which is not open for connections.
    ///
    /// Use [`ChannelServer::open`] to open this server for clients.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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
        recv_dc_c2s: Receiver<String>,
        send_dc_s2c: Sender<String>,
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
            recv_dc_c2s,
            send_dc_s2c,
            send_initial: true,
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
            None | Some(Client::Disconnected | Client::Disconnecting { .. }) => {
                ClientState::Disconnected
            }
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
        let mut events = Vec::new();
        match &mut self.state {
            State::Closed => {}
            State::Open(server) => Self::poll_open(server, &mut events),
        };
        events.into_iter()
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

    fn disconnect(
        &mut self,
        client_key: Self::ClientKey,
        reason: impl Into<String>,
    ) -> Result<(), Self::Error> {
        let State::Open(server) = &mut self.state else {
            return Err(ServerError::NotOpen);
        };

        let reason = reason.into();
        let client = server
            .clients
            .get_mut(client_key)
            .ok_or(ServerError::NotConnected)?;
        match mem::replace(
            client,
            Client::Disconnecting {
                reason: reason.clone(),
            },
        ) {
            Client::Connected(client) => {
                let _ = client.send_dc_s2c.try_send(reason);
                Ok(())
            }
            Client::Disconnected | Client::Disconnecting { .. } => Err(ServerError::NotConnected),
        }
    }

    fn close(&mut self, reason: impl Into<String>) -> Result<(), Self::Error> {
        match mem::take(&mut self.state) {
            State::Open(server) => {
                let reason = reason.into();
                for (_, client) in server.clients {
                    if let Client::Connected(client) = client {
                        let _ = client.send_dc_s2c.try_send(reason.clone());
                    }
                }
                Ok(())
            }
            State::Closed => Err(ServerError::AlreadyClosed),
        }
    }

    fn default_disconnect_reason(&self) -> Option<&str> {
        self.default_disconnect_reason.as_deref()
    }

    fn set_default_disconnect_reason(&mut self, reason: impl Into<String>) {
        self.default_disconnect_reason = Some(reason.into());
    }

    fn unset_default_disconnect_reason(&mut self) {
        self.default_disconnect_reason = None;
    }
}

impl ChannelServer {
    fn poll_open(server: &mut Open, events: &mut Vec<ServerEvent<Self>>) {
        for (client_key, client) in &mut server.clients {
            replace_with::replace_with_or_abort(client, |client| match client {
                Client::Disconnected => client,
                Client::Connected(client) => Self::poll_connected(events, client_key, client),
                Client::Disconnecting { reason } => {
                    events.push(ServerEvent::Disconnected {
                        client_key,
                        reason: DisconnectReason::Local(reason),
                    });
                    Client::Disconnected
                }
            });
        }

        server
            .clients
            .retain(|_, client| !matches!(client, Client::Disconnected));
    }

    fn poll_connected(
        events: &mut Vec<ServerEvent<Self>>,
        client_key: ClientKey,
        mut client: Connected,
    ) -> Client {
        if client.send_initial {
            events.push(ServerEvent::Connecting { client_key });
            events.push(ServerEvent::Connected { client_key });
            client.send_initial = false;
        }

        if let Ok(reason) = client.recv_dc_c2s.try_recv() {
            events.push(ServerEvent::Disconnected {
                client_key,
                reason: DisconnectReason::Remote(reason),
            });
            return Client::Disconnected;
        }

        let res = (|| loop {
            match client.recv_c2s.try_recv() {
                Ok((msg, lane)) => {
                    client.bytes_recv = client.bytes_recv.saturating_add(msg.len());
                    events.push(ServerEvent::Recv {
                        client_key,
                        msg,
                        lane,
                    });
                }
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => return Err(ServerError::Disconnected),
            }
        })();

        match res {
            Ok(()) => Client::Connected(client),
            Err(err) => {
                events.push(ServerEvent::Disconnected {
                    client_key,
                    reason: DisconnectReason::Error(err),
                });
                Client::Disconnected
            }
        }
    }
}

impl Drop for ChannelServer {
    fn drop(&mut self) {
        if let Some(reason) = &self.default_disconnect_reason {
            let reason = reason.clone();
            let _ = self.close(reason);
        }
    }
}
