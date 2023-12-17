use aeronet::{ClientState, TransportProtocol, TransportServer};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use derivative::Derivative;
use slotmap::SlotMap;

use crate::ChannelError;

slotmap::new_key_type! {
    /// Key type used to uniquely identify a client connected to a
    /// [`ChannelServer`].
    pub struct ClientKey;
}

type ServerEvent<P> = aeronet::ServerEvent<P, ChannelServer<P>>;

/// Implementation of [`TransportServer`] using in-memory MPSC channels.
///
/// See the [crate-level docs](crate).
#[derive(Derivative)]
#[derivative(Debug(bound = ""), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct ChannelServer<P>
where
    P: TransportProtocol,
{
    #[derivative(Debug = "ignore")]
    clients: SlotMap<ClientKey, RemoteClient<P>>,
}

#[derive(Debug)]
enum RemoteClient<P>
where
    P: TransportProtocol,
{
    Connected {
        send_s2c: Sender<P::S2C>,
        recv_c2s: Receiver<P::C2S>,
        send_connect: bool,
    },
    ForceDisconnected,
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
        }
    }

    pub(super) fn add_client(
        &mut self,
        send_s2c: Sender<P::S2C>,
        recv_c2s: Receiver<P::C2S>,
    ) -> ClientKey {
        self.clients.insert(RemoteClient::Connected {
            send_s2c,
            recv_c2s,
            send_connect: true,
        })
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

    fn client_state(&self, client: Self::Client) -> ClientState<Self::ConnectionInfo> {
        match self.clients.get(client) {
            Some(_) => ClientState::Connected(()),
            None => ClientState::Disconnected,
        }
    }

    fn clients(&self) -> impl Iterator<Item = (Self::Client, ClientState<Self::ConnectionInfo>)> {
        self.clients
            .keys()
            .map(|client| (client, self.client_state(client)))
    }

    fn send(&mut self, client: Self::Client, msg: impl Into<P::S2C>) -> Result<(), Self::Error> {
        let msg = msg.into();
        let Some(RemoteClient::Connected { send_s2c, .. }) = self.clients.get(client) else {
            return Err(ChannelError::NoClient(client));
        };
        send_s2c.send(msg).map_err(|_| ChannelError::Disconnected)
    }

    fn recv<'a>(&mut self) -> impl Iterator<Item = Self::Event> + 'a {
        let mut events = Vec::new();

        let mut to_remove = Vec::new();
        for (client, state) in &mut self.clients {
            recv_client::<P>(client, state, &mut events, &mut to_remove);
        }

        for client in to_remove {
            self.clients.remove(client);
        }

        events.into_iter()
    }

    fn disconnect(&mut self, client: impl Into<Self::Client>) -> Result<(), Self::Error> {
        let client = client.into();
        match self.clients.get_mut(client) {
            Some(client @ RemoteClient::Connected { .. }) => {
                *client = RemoteClient::ForceDisconnected;
                Ok(())
            }
            Some(RemoteClient::ForceDisconnected) | None => Err(ChannelError::NoClient(client)),
        }
    }
}

fn recv_client<P>(
    client: ClientKey,
    state: &mut RemoteClient<P>,
    events: &mut Vec<ServerEvent<P>>,
    to_remove: &mut Vec<ClientKey>,
) where
    P: TransportProtocol,
{
    match state {
        RemoteClient::Connected {
            recv_c2s,
            send_connect,
            ..
        } => {
            if *send_connect {
                *send_connect = false;
                events.push(ServerEvent::Connecting { client });
                events.push(ServerEvent::Connected { client });
            }

            loop {
                match recv_c2s.try_recv() {
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
        RemoteClient::ForceDisconnected => {
            events.push(ServerEvent::Disconnected {
                client,
                cause: ChannelError::ForceDisconnect,
            });
            to_remove.push(client);
        }
    }
}
