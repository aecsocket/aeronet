use aeronet::{ClientKey, ServerTransport, TransportProtocol};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use derivative::Derivative;
use slotmap::SlotMap;

use crate::{ChannelError, ConnectionInfo};

type ServerState = aeronet::ServerState<(), ()>;

type ClientState = aeronet::ClientState<(), ConnectionInfo>;

type ServerEvent<P> = aeronet::ServerEvent<P, ChannelError>;

/// Implementation of [`ServerTransport`] using in-memory MPSC channels for
/// transport.
///
/// See the [crate-level docs](crate).
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

    fn state(&self) -> ServerState {
        ServerState::Open(())
    }

    fn client_state(&self, client: ClientKey) -> ClientState {
        match self.clients.get(client) {
            Some(Client::Connected { info, .. }) => ClientState::Connected(info.clone()),
            Some(Client::Disconnected) | None => ClientState::Disconnected,
        }
    }

    fn client_keys(&self) -> impl Iterator<Item = ClientKey> + '_ {
        self.clients.keys()
    }

    fn send(&mut self, client: ClientKey, msg: impl Into<P::S2C>) -> Result<(), Self::Error> {
        let Some(Client::Connected { send_s2c, info, .. }) = self.clients.get_mut(client) else {
            return Err(ChannelError::Disconnected);
        };
        let msg = msg.into();
        send_s2c.send(msg).map_err(|_| ChannelError::Disconnected)?;
        info.msgs_sent += 1;
        Ok(())
    }

    fn update(&mut self) -> impl Iterator<Item = ServerEvent<P>> {
        let mut events = Vec::new();
        let mut to_remove = Vec::new();

        for (client, data) in &mut self.clients {
            update_client(client, data, &mut events, &mut to_remove);
        }

        for client in to_remove {
            self.clients.remove(client);
        }
        events.into_iter()
    }

    fn disconnect(&mut self, client: ClientKey) -> Result<(), Self::Error> {
        self.clients
            .remove(client)
            .ok_or(ChannelError::Disconnected)
            .map(drop)
    }
}

fn update_client<P: TransportProtocol>(
    client: ClientKey,
    data: &mut Client<P>,
    events: &mut Vec<ServerEvent<P>>,
    to_remove: &mut Vec<ClientKey>,
) {
    match data {
        Client::Connected {
            recv_c2s,
            info,
            send_connected,
            ..
        } => {
            if *send_connected {
                events.push(ServerEvent::Connecting { client });
                events.push(ServerEvent::Connected { client });
                *send_connected = false;
            }

            match recv_c2s.try_recv() {
                Ok(msg) => {
                    events.push(ServerEvent::Recv { client, msg });
                    info.msgs_recv += 1;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    *data = Client::Disconnected;
                }
            }
        }
        Client::Disconnected => {
            events.push(ServerEvent::Disconnected {
                client,
                reason: ChannelError::Disconnected,
            });
            to_remove.push(client);
        }
    }
}
