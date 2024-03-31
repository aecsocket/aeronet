use std::time::Duration;

use aeronet::{
    client::ClientState,
    protocol::TransportProtocol,
    server::{ServerState, ServerTransport},
};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use derivative::Derivative;
use slotmap::SlotMap;

use crate::{ChannelError, ConnectionInfo};

slotmap::new_key_type! {
    /// Key identifying a unique client connected to a [`ChannelServer`].
    pub struct ClientKey;
}

impl std::fmt::Display for ClientKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

type ServerEvent<P> = aeronet::server::ServerEvent<P, ChannelError, ClientKey, ()>;

/// Implementation of [`ServerTransport`] using in-memory MPSC channels.
///
/// See the [crate-level documentation](crate).
#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct ChannelServer<P: TransportProtocol> {
    clients: SlotMap<ClientKey, Client<P>>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
enum Client<P: TransportProtocol> {
    Connected {
        recv_c2s: Receiver<P::C2S>,
        send_s2c: Sender<P::S2C>,
        info: ConnectionInfo,
        send_connected: bool,
    },
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
        self.clients.insert(Client::Connected {
            recv_c2s,
            send_s2c,
            info: ConnectionInfo::default(),
            send_connected: true,
        })
    }
}

impl<P: TransportProtocol> ServerTransport<P> for ChannelServer<P> {
    type Error = ChannelError;

    type OpeningInfo = ();

    type OpenInfo = ();

    type ConnectingInfo = ();

    type ConnectedInfo = ConnectionInfo;

    type ClientKey = ClientKey;

    type MessageKey = ();

    fn state(&self) -> ServerState<Self::OpeningInfo, Self::OpenInfo> {
        ServerState::Open(())
    }

    fn client_state(
        &self,
        client_key: ClientKey,
    ) -> ClientState<Self::ConnectingInfo, Self::ConnectedInfo> {
        match self.clients.get(client_key) {
            Some(Client::Connected { info, .. }) => ClientState::Connected(info.clone()),
            Some(Client::Disconnected) | None => ClientState::Disconnected,
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
        let Some(Client::Connected { send_s2c, info, .. }) = self.clients.get_mut(client_key)
        else {
            return Err(ChannelError::Disconnected);
        };
        let msg = msg.into();
        send_s2c.send(msg).map_err(|_| ChannelError::Disconnected)?;
        info.msgs_sent += 1;
        Ok(())
    }

    fn disconnect(&mut self, client_key: Self::ClientKey) -> Result<(), Self::Error> {
        self.clients
            .remove(client_key)
            .ok_or(ChannelError::Disconnected)
            .map(drop)
    }

    fn poll(&mut self, _: Duration) -> impl Iterator<Item = ServerEvent<P>> {
        let mut events = Vec::new();
        let mut to_remove = Vec::new();

        for (client, data) in &mut self.clients {
            Self::poll_client(client, data, &mut events, &mut to_remove);
        }

        for client in to_remove {
            self.clients.remove(client);
        }
        events.into_iter()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl<P: TransportProtocol> ChannelServer<P> {
    fn poll_client(
        client_key: ClientKey,
        client: &mut Client<P>,
        events: &mut Vec<ServerEvent<P>>,
        to_remove: &mut Vec<ClientKey>,
    ) {
        match client {
            Client::Connected {
                recv_c2s,
                info,
                send_connected,
                ..
            } => {
                if *send_connected {
                    events.push(ServerEvent::Connecting { client_key });
                    events.push(ServerEvent::Connected { client_key });
                    *send_connected = false;
                }

                match recv_c2s.try_recv() {
                    Ok(msg) => {
                        events.push(ServerEvent::Recv { client_key, msg });
                        info.msgs_recv += 1;
                    }
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => {
                        *client = Client::Disconnected;
                    }
                }
            }
            Client::Disconnected => {
                events.push(ServerEvent::Disconnected {
                    client_key,
                    reason: ChannelError::Disconnected,
                });
                to_remove.push(client_key);
            }
        }
    }
}
