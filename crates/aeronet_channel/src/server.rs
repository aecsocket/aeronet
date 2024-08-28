//! Server-side items.

use {
    crate::shared::{Disconnected, MessageKey},
    aeronet::{
        client::{ClientState, DisconnectReason},
        lane::LaneIndex,
        server::{CloseReason, ServerEvent, ServerState, ServerTransport},
        shared::DROP_DISCONNECT_REASON,
        stats::{ConnectedAt, MessageStats},
    },
    bytes::Bytes,
    crossbeam_channel::{Receiver, Sender, TryRecvError},
    slotmap::SlotMap,
    std::{borrow::Borrow, convert::Infallible, num::Saturating},
    web_time::{Duration, Instant},
};

slotmap::new_key_type! {
    /// Key identifying a unique client connected to a [`ChannelServer`].
    ///
    /// If a client is connected, disconnected, and reconnected to the same
    /// server, it will have a different client key.
    pub struct ClientKey;
}

/// Implementation of [`ServerTransport`] using in-memory MPSC channels.
///
/// See the [crate-level documentation](crate).
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct ChannelServer {
    state: State,
}

#[derive(Debug)]
enum State {
    Closed,
    Open(Open),
    Closing { reason: String },
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
    pub bytes_sent: Saturating<usize>,
    /// See [`MessageStats::bytes_recv`]
    pub bytes_recv: Saturating<usize>,
    recv_c2s: Receiver<(MessageKey, Bytes, LaneIndex)>,
    send_s2c: Sender<(MessageKey, Bytes, LaneIndex)>,
    recv_ack_c2s: Receiver<MessageKey>,
    send_ack_s2c: Sender<MessageKey>,
    recv_dc_c2s: Receiver<String>,
    send_dc_s2c: Sender<String>,
    send_initial: bool,
    next_send_msg_key: MessageKey,
}

impl ConnectedAt for Connected {
    fn connected_at(&self) -> Instant {
        self.connected_at
    }
}

impl MessageStats for Connected {
    fn bytes_sent(&self) -> usize {
        self.bytes_sent.0
    }

    fn bytes_recv(&self) -> usize {
        self.bytes_recv.0
    }
}

#[derive(Debug)]
enum Client {
    Disconnected,
    Connected(Connected),
    Disconnecting { reason: String },
}

/// Error type for [`ChannelServer::open`], emitted if the server is already
/// open.
#[derive(Debug, Clone, thiserror::Error)]
#[error("not closed")]
pub struct ServerNotClosed;

/// Error type for [`ChannelServer::send`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum ServerSendError {
    /// Attempted to send over a server which is not open.
    #[error("not open")]
    NotOpen,
    /// Attempted to send to a client which is not connected.
    #[error("client not connected")]
    ClientNotConnected,
    /// Attempted to send to a client which we thought was connected, but the
    /// other side of the channel was disconnected.
    #[error("disconnected")]
    ClientDisconnected,
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

impl Default for ChannelServer {
    fn default() -> Self {
        Self::new()
    }
}

impl ChannelServer {
    /// Creates a server which is not open for connections.
    ///
    /// Use [`ChannelServer::open`] to open this server for clients.
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
    pub fn open(&mut self) -> Result<(), ServerNotClosed> {
        if !matches!(self.state, State::Closed) {
            return Err(ServerNotClosed);
        }

        self.state = State::Open(Open {
            clients: SlotMap::default(),
        });
        Ok(())
    }

    pub(super) fn insert_client(
        &mut self,
        recv_c2s: Receiver<(MessageKey, Bytes, LaneIndex)>,
        send_s2c: Sender<(MessageKey, Bytes, LaneIndex)>,
        recv_ack_c2s: Receiver<MessageKey>,
        send_ack_s2c: Sender<MessageKey>,
        recv_dc_c2s: Receiver<String>,
        send_dc_s2c: Sender<String>,
    ) -> Option<ClientKey> {
        let State::Open(server) = &mut self.state else {
            return None;
        };

        Some(server.clients.insert(Client::Connected(Connected {
            connected_at: Instant::now(),
            bytes_sent: Saturating(0),
            bytes_recv: Saturating(0),
            recv_c2s,
            send_s2c,
            recv_ack_c2s,
            send_ack_s2c,
            recv_dc_c2s,
            send_dc_s2c,
            send_initial: true,
            next_send_msg_key: MessageKey::default(),
        })))
    }
}

impl ServerTransport for ChannelServer {
    type Opening<'this> = Infallible;

    type Open<'this> = &'this Open;

    type Connecting<'this> = Infallible;

    type Connected<'this> = &'this Connected;

    type ClientKey = ClientKey;

    type MessageKey = MessageKey;

    type PollError = Disconnected;

    type SendError = ServerSendError;

    fn state(&self) -> ServerState<Self::Opening<'_>, Self::Open<'_>> {
        match &self.state {
            State::Closed | State::Closing { .. } => ServerState::Closed,
            State::Open(server) => ServerState::Open(server),
        }
    }

    fn client_state(
        &self,
        client_key: impl Borrow<ClientKey>,
    ) -> ClientState<Self::Connecting<'_>, Self::Connected<'_>> {
        let State::Open(server) = &self.state else {
            return ClientState::Disconnected;
        };

        let client_key = client_key.borrow();
        match server.clients.get(*client_key) {
            None | Some(Client::Disconnected | Client::Disconnecting { .. }) => {
                ClientState::Disconnected
            }
            Some(Client::Connected(client)) => ClientState::Connected(client),
        }
    }

    fn client_keys(&self) -> impl Iterator<Item = Self::ClientKey> + '_ {
        match &self.state {
            State::Closed | State::Closing { .. } => None,
            State::Open(server) => Some(server.clients.keys()),
        }
        .into_iter()
        .flatten()
    }

    fn poll(&mut self, _: Duration) -> impl Iterator<Item = ServerEvent<Self>> {
        let mut events = Vec::new();
        replace_with::replace_with_or_abort(&mut self.state, |state| match state {
            State::Closed => state,
            State::Open(server) => Self::poll_open(server, &mut events),
            State::Closing { reason } => {
                events.push(ServerEvent::Closed {
                    reason: CloseReason::Local(reason),
                });
                State::Closed
            }
        });
        events.into_iter()
    }

    fn send(
        &mut self,
        client_key: impl Borrow<Self::ClientKey>,
        msg: impl Into<Bytes>,
        lane: impl Into<LaneIndex>,
    ) -> Result<Self::MessageKey, Self::SendError> {
        let State::Open(server) = &mut self.state else {
            return Err(ServerSendError::NotOpen);
        };

        let client_key = client_key.borrow();
        let Some(Client::Connected(client)) = server.clients.get_mut(*client_key) else {
            return Err(ServerSendError::ClientNotConnected);
        };

        let msg = msg.into();
        let lane = lane.into();

        let msg_key = client.next_send_msg_key;
        let msg_len = msg.len();
        client
            .send_s2c
            .send((msg_key, msg, lane))
            .map_err(|_| ServerSendError::ClientDisconnected)?;
        client.bytes_sent += msg_len;
        client.next_send_msg_key.inc();
        Ok(msg_key)
    }

    fn flush(&mut self) {}

    fn disconnect(&mut self, client_key: impl Borrow<Self::ClientKey>, reason: impl Into<String>) {
        let State::Open(server) = &mut self.state else {
            return;
        };

        let client_key = client_key.borrow();
        let Some(client) = server.clients.get_mut(*client_key) else {
            return;
        };

        let reason = reason.into();
        replace_with::replace_with_or_abort(client, |state| match state {
            Client::Connected(client) => {
                let _ = client.send_dc_s2c.try_send(reason.clone());
                Client::Disconnecting { reason }
            }
            Client::Disconnected | Client::Disconnecting { .. } => state,
        });
    }

    fn close(&mut self, reason: impl Into<String>) {
        let reason = reason.into();
        replace_with::replace_with_or_abort(&mut self.state, |state| match state {
            State::Open(server) => {
                for (_, client) in server.clients {
                    if let Client::Connected(client) = client {
                        let _ = client.send_dc_s2c.try_send(reason.clone());
                    }
                }
                State::Closing { reason }
            }
            State::Closed | State::Closing { .. } => state,
        });
    }
}

impl ChannelServer {
    fn poll_open(mut server: Open, events: &mut Vec<ServerEvent<Self>>) -> State {
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

        State::Open(server)
    }

    fn poll_connected(
        events: &mut Vec<ServerEvent<Self>>,
        client_key: ClientKey,
        mut client: Connected,
    ) -> Client {
        match client.recv_dc_c2s.try_recv() {
            Ok(reason) => {
                events.push(ServerEvent::Disconnected {
                    client_key,
                    reason: DisconnectReason::Remote(reason),
                });
                return Client::Disconnected;
            }
            Err(TryRecvError::Disconnected) => {
                events.push(ServerEvent::Disconnected {
                    client_key,
                    reason: DisconnectReason::Error(Disconnected),
                });
                return Client::Disconnected;
            }
            Err(TryRecvError::Empty) => {}
        }

        if client.send_initial {
            events.push(ServerEvent::Connecting { client_key });
            events.push(ServerEvent::Connected { client_key });
            client.send_initial = false;
        }

        for (msg_key, msg, lane) in client.recv_c2s.try_iter() {
            client.bytes_recv += msg.len();
            let _ = client.send_ack_s2c.send(msg_key);
            events.push(ServerEvent::Recv {
                client_key,
                msg,
                lane,
            });
        }

        for msg_key in client.recv_ack_c2s.try_iter() {
            events.push(ServerEvent::Ack {
                client_key,
                msg_key,
            });
        }

        Client::Connected(client)
    }
}

impl Drop for ChannelServer {
    fn drop(&mut self) {
        self.close(DROP_DISCONNECT_REASON);
    }
}
