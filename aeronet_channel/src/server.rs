//! Server-side items.

use std::{fmt::Display, time::Duration};

use aeronet::{
    client::ClientState,
    protocol::TransportProtocol,
    server::{ServerEvent, ServerState, ServerTransport},
};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use derivative::Derivative;
use slotmap::SlotMap;

use crate::shared::ConnectionStats;

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
#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct ChannelServer<P: TransportProtocol> {
    clients: SlotMap<ClientKey, Client<P>>,
}

/// State of a [`ChannelServer`]'s client when it is [`ClientState::Connected`].
#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct Connected<P: TransportProtocol> {
    /// Statistics of this connection.
    pub stats: ConnectionStats,
    recv_c2s: Receiver<P::C2S>,
    send_s2c: Sender<P::S2C>,
    send_connected: bool,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
enum Client<P: TransportProtocol> {
    Disconnected,
    Connected(Connected<P>),
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

impl<P: TransportProtocol> ChannelServer<P> {
    /// Creates a server with no connected clients.
    #[must_use]
    pub fn open() -> Self {
        Self::default()
    }

    pub(super) fn insert_client(
        &mut self,
        recv_c2s: Receiver<P::C2S>,
        send_s2c: Sender<P::S2C>,
    ) -> ClientKey {
        self.clients.insert(Client::Connected(Connected {
            stats: ConnectionStats::default(),
            recv_c2s,
            send_s2c,
            send_connected: true,
        }))
    }
}

impl<P: TransportProtocol> ServerTransport<P> for ChannelServer<P> {
    type Error = ServerError;

    type Opening<'t> = ();

    type Open<'t> = ();

    type Connecting<'t> = ();

    type Connected<'t> = &'t Connected<P>;

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
        msg: impl Into<P::S2C>,
    ) -> Result<Self::MessageKey, Self::Error> {
        let Some(Client::Connected(client)) = self.clients.get_mut(client_key) else {
            return Err(ServerError::NoClient);
        };
        let msg = msg.into();
        client
            .send_s2c
            .send(msg)
            .map_err(|_| ServerError::Disconnected)?;
        client.stats.msgs_sent += 1;
        Ok(())
    }

    fn disconnect(&mut self, client_key: Self::ClientKey) -> Result<(), Self::Error> {
        self.clients
            .remove(client_key)
            .ok_or(ServerError::NoClient)
            .map(drop)
    }

    fn poll(&mut self, _: Duration) -> impl Iterator<Item = ServerEvent<P, Self>> {
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

impl<P: TransportProtocol> ChannelServer<P> {
    fn poll_connected(
        events: &mut Vec<ServerEvent<P, Self>>,
        client_key: ClientKey,
        mut client: Connected<P>,
    ) -> Client<P> {
        if client.send_connected {
            events.push(ServerEvent::Connecting { client_key });
            events.push(ServerEvent::Connected { client_key });
            client.send_connected = false;
        }

        loop {
            match client.recv_c2s.try_recv() {
                Ok(msg) => {
                    events.push(ServerEvent::Recv { client_key, msg });
                    client.stats.msgs_recv += 1;
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
