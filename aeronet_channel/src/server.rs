use std::mem;

use aeronet::{TransportProtocol, TransportServer};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use derivative::Derivative;
use slotmap::SlotMap;

use crate::{ChannelError, ClientKey};

type ServerEvent<P> = aeronet::ServerEvent<P, ChannelServer<P>>;

/// Implementation of [`TransportServer`] using in-memory MPSC channels.
///
/// See the [crate-level docs](crate).
#[derive(Derivative, Default)]
#[derivative(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct ChannelServer<P>
where
    P: TransportProtocol,
{
    #[derivative(Debug = "ignore")]
    pub(super) clients: SlotMap<ClientKey, RemoteClient<P>>,
    #[derivative(Debug = "ignore")]
    pub(super) event_buf: Vec<ServerEvent<P>>,
}

#[derive(Debug)]
pub(super) struct RemoteClient<P>
where
    P: TransportProtocol,
{
    pub(super) send_s2c: Sender<P::S2C>,
    pub(super) recv_c2s: Receiver<P::C2S>,
}

impl<P> ChannelServer<P>
where
    P: TransportProtocol,
{
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

impl<P> TransportServer<P> for ChannelServer<P>
where
    P: TransportProtocol,
{
    type Client = ClientKey;

    type Error = ChannelError;

    type ConnectionInfo = ();

    type Event = ServerEvent<P>;

    fn connection_info(&self, client: Self::Client) -> Option<Self::ConnectionInfo> {
        self.clients.get(client).map(|_| ())
    }

    fn connected_clients(&self) -> impl Iterator<Item = Self::Client> {
        self.clients.keys()
    }

    fn send(&mut self, client: Self::Client, msg: impl Into<P::S2C>) -> Result<(), Self::Error> {
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
