use std::{fmt::Display, time::Duration};

use aeronet::{
    client::ClientState,
    protocol::TransportProtocol,
    server::{ServerEvent, ServerEventFor, ServerState, ServerTransport},
};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use derivative::Derivative;
use slotmap::SlotMap;

use crate::shared::{ChannelError, ConnectionStats};

slotmap::new_key_type! {
    /// Key identifying a unique client connected to a [`ChannelServer`].
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
    clients: SlotMap<ClientKey, Connected<P>>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct Connected<P: TransportProtocol> {
    recv_c2s: Receiver<P::C2S>,
    send_s2c: Sender<P::S2C>,
    pub stats: ConnectionStats,
    send_connected: bool,
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
        self.clients.insert(Connected {
            recv_c2s,
            send_s2c,
            stats: ConnectionStats::default(),
            send_connected: true,
        })
    }
}

impl<P: TransportProtocol> ServerTransport<P> for ChannelServer<P> {
    type Error = ChannelError;

    type Opening<'this> = ();

    type Open<'this> = ();

    type Connecting<'this> = ();

    type Connected<'this> = &'this Connected<P>;

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
            None => ClientState::Disconnected,
            Some(client) => ClientState::Connected(client),
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
        let Some(client) = self.clients.get_mut(client_key) else {
            return Err(ChannelError::Disconnected);
        };
        let msg = msg.into();
        client
            .send_s2c
            .send(msg)
            .map_err(|_| ChannelError::Disconnected)?;
        client.stats.msgs_sent += 1;
        Ok(())
    }

    fn disconnect(&mut self, client_key: Self::ClientKey) -> Result<(), Self::Error> {
        self.clients
            .remove(client_key)
            .ok_or(ChannelError::Disconnected)
            .map(drop)
    }

    fn poll(
        &mut self,
        _: Duration,
    ) -> impl Iterator<Item = ServerEvent<P, Self::Error, Self::ClientKey, Self::MessageKey>> {
        let mut events = Vec::new();
        let mut clients_to_remove = Vec::new();
        for (client_key, client) in &mut self.clients {
            if let Err(error) = Self::poll_client(&mut events, client_key, client) {
                events.push(ServerEvent::Disconnected { client_key, error });
                clients_to_remove.push(client_key);
            }
        }

        for client_key in clients_to_remove {
            self.clients.remove(client_key);
        }
        events.into_iter()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl<P: TransportProtocol> ChannelServer<P> {
    fn poll_client(
        events: &mut Vec<ServerEventFor<P, Self>>,
        client_key: ClientKey,
        client: &mut Connected<P>,
    ) -> Result<(), ChannelError> {
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
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => return Err(ChannelError::Disconnected),
            }
        }
    }
}
