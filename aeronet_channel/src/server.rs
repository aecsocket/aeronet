use std::mem;

use aeronet::{Message, ServerEvent, TransportServer};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use slotmap::SlotMap;

use crate::{ChannelError, ClientKey};

/// Implementation of [`TransportServer`] using in-memory MPSC channels.
///
/// See the [crate-level docs](crate).
#[derive(Debug, Default)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct ChannelServer<C2S, S2C> {
    pub(super) clients: SlotMap<ClientKey, ClientState<C2S, S2C>>,
    pub(super) event_buf: Vec<ServerEvent<C2S, ClientKey, ChannelError>>,
}

#[derive(Debug)]
pub(super) struct ClientState<C2S, S2C> {
    pub(super) send_s2c: Sender<S2C>,
    pub(super) recv_c2s: Receiver<C2S>,
}

impl<C2S, S2C> ChannelServer<C2S, S2C> {
    /// Creates a new server with no clients connected.
    ///
    /// See [`ChannelClient`] on how to create and connect a client to this
    /// server.
    ///
    /// [`ChannelClient`]: crate::ChannelClient
    #[must_use]
    pub fn new() -> Self {
        Self {
            clients: SlotMap::default(),
            event_buf: Vec::default(),
        }
    }
}

impl<C2S, S2C> TransportServer<C2S, S2C> for ChannelServer<C2S, S2C>
where
    C2S: Message,
    S2C: Message,
{
    type Client = ClientKey;

    type Error = ChannelError;

    type ConnectionInfo = ();

    type Event = ServerEvent<C2S, Self::Client, Self::Error>;

    fn connection_info(&self, client: Self::Client) -> Option<Self::ConnectionInfo> {
        self.clients.get(client).map(|_| ())
    }

    fn connected_clients(&self) -> impl Iterator<Item = Self::Client> {
        self.clients.keys()
    }

    fn send(&mut self, client: Self::Client, msg: impl Into<S2C>) -> Result<(), Self::Error> {
        let msg = msg.into();
        let Some(client) = self.clients.get(client) else {
            return Err(ChannelError::NoClient(client));
        };
        client
            .send_s2c
            .send(msg)
            .map_err(|_| ChannelError::Disconnected)
    }

    fn recv<'a>(&mut self) -> impl Iterator<Item = Self::Event> + 'a {
        let mut events = mem::take(&mut self.event_buf);

        let mut to_remove = Vec::new();
        for (client, state) in &self.clients {
            loop {
                match state.recv_c2s.try_recv() {
                    Ok(msg) => events.push(ServerEvent::Recv { client, msg }),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        events.push(ServerEvent::Disconnected {
                            client,
                            cause: ChannelError::Disconnected,
                        });
                        to_remove.push(client);
                    }
                }
            }
        }

        for client in to_remove {
            self.clients.remove(client);
        }

        events.into_iter()
    }

    fn disconnect(&mut self, client: impl Into<Self::Client>) -> Result<(), Self::Error> {
        let client = client.into();
        match self.clients.remove(client) {
            Some(_) => {
                self.event_buf.push(ServerEvent::Disconnected {
                    client,
                    cause: ChannelError::ForceDisconnect,
                });
                Ok(())
            }
            None => Err(ChannelError::NoClient(client)),
        }
    }
}
