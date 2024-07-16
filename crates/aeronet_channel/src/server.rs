//! Server-side items.

use std::{fmt::Display, time::Duration};

use aeronet::{
    client::ClientState,
    lane::LaneIndex,
    server::{ServerEvent, ServerState, ServerTransport},
    stats::MessageStats,
};
use bytes::Bytes;
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use derivative::Derivative;
use slotmap::SlotMap;

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
    clients: SlotMap<ClientKey, Client>,
}

/// State of a [`ChannelServer`]'s client when it is [`ClientState::Connected`].
#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct Connected {
    /// See [`MessageStats::bytes_sent`].
    pub bytes_sent: usize,
    /// See [`MessageStats::bytes_recv`]
    pub bytes_recv: usize,
    recv_c2s: Receiver<(Bytes, LaneIndex)>,
    send_s2c: Sender<(Bytes, LaneIndex)>,
    send_connected: bool,
}

impl MessageStats for Connected {
    fn bytes_sent(&self) -> usize {
        self.bytes_sent
    }

    fn bytes_recv(&self) -> usize {
        self.bytes_recv
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
enum Client {
    Disconnected,
    Connected(Connected),
}

/// Error type for operations on a [`ChannelServer`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum ServerError {
    /// No client exists for the given key.
    #[error("no client with this key")]
    NoClient,
    /// Client was unexpectedly disconnected.
    #[error("client disconnected")]
    Disconnected,
}

impl ChannelServer {
    /// Creates a server with no connected clients.
    #[must_use]
    pub fn open() -> Self {
        Self::default()
    }

    pub(super) fn insert_client(
        &mut self,
        recv_c2s: Receiver<(Bytes, LaneIndex)>,
        send_s2c: Sender<(Bytes, LaneIndex)>,
    ) -> ClientKey {
        self.clients.insert(Client::Connected(Connected {
            bytes_sent: 0,
            bytes_recv: 0,
            recv_c2s,
            send_s2c,
            send_connected: true,
        }))
    }
}

impl ServerTransport for ChannelServer {
    type Error = ServerError;

    type Opening<'this> = ();

    type Open<'this> = ();

    type Connecting<'this> = ();

    type Connected<'this> = &'this Connected;

    type ClientKey = ClientKey;

    type MessageKey = ();

    fn state(&self) -> ServerState<Self::Opening<'_>, Self::Open<'_>> {
        ServerState::Open(())
    }

    fn client_state(
        &self,
        client_key: ClientKey,
    ) -> ClientState<Self::Connecting<'_>, Self::Connected<'_>> {
        match self.clients.get(client_key) {
            None | Some(Client::Disconnected) => ClientState::Disconnected,
            Some(Client::Connected(client)) => ClientState::Connected(client),
        }
    }

    fn client_keys(&self) -> impl Iterator<Item = Self::ClientKey> + '_ {
        self.clients.keys()
    }

    fn send(
        &mut self,
        client_key: Self::ClientKey,
        msg: impl Into<Bytes>,
        lane: impl Into<LaneIndex>,
    ) -> Result<Self::MessageKey, Self::Error> {
        let Some(Client::Connected(client)) = self.clients.get_mut(client_key) else {
            return Err(ServerError::NoClient);
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

    fn disconnect(&mut self, client_key: Self::ClientKey) -> Result<(), Self::Error> {
        self.clients
            .remove(client_key)
            .ok_or(ServerError::NoClient)
            .map(drop)
    }

    fn poll(&mut self, _: Duration) -> impl Iterator<Item = ServerEvent<Self>> {
        let mut events = Vec::new();
        for (client_key, client) in &mut self.clients {
            replace_with::replace_with_or_abort(client, |client| match client {
                Client::Disconnected => client,
                Client::Connected(client) => Self::poll_connected(&mut events, client_key, client),
            });
        }

        let removed_clients = self
            .clients
            .iter()
            .filter_map(|(client_key, client)| match client {
                Client::Connected(_) => None,
                Client::Disconnected => Some(client_key),
            })
            .collect::<Vec<_>>();
        for client_key in removed_clients {
            self.clients.remove(client_key);
        }

        events.into_iter()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl ChannelServer {
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
