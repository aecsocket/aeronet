use aeronet::{Message, ServerEvent, TransportServer};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use slotmap::SlotMap;

use crate::{ChannelError, ClientKey};

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct ChannelServer<C2S, S2C> {
    pub(super) clients: SlotMap<ClientKey, ClientState<C2S, S2C>>,
}

#[derive(Debug)]
pub(super) struct ClientState<C2S, S2C> {
    pub(super) send_s2c: Sender<S2C>,
    pub(super) recv_c2s: Receiver<C2S>,
}

impl<C2S, S2C> ChannelServer<C2S, S2C> {
    pub fn new() -> Self {
        Self {
            clients: SlotMap::default(),
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

    type RecvIter<'a> = std::vec::IntoIter<Self::Event>
        where Self: 'a;

    fn connection_info(&self, client: Self::Client) -> Option<Self::ConnectionInfo> {
        self.clients.get(client).map(|_| ())
    }

    fn send<M: Into<S2C>>(&mut self, client: Self::Client, msg: M) -> Result<(), Self::Error> {
        let msg = msg.into();
        let Some(client) = self.clients.get(client) else {
            return Err(ChannelError::NoClient(client));
        };
        let _ = client.send_s2c.send(msg);
        Ok(())
    }

    fn recv(&mut self) -> Self::RecvIter<'_> {
        let mut events = Vec::new();
        let mut to_remove = Vec::new();
        for (client, state) in self.clients.iter() {
            loop {
                match state.recv_c2s.try_recv() {
                    Ok(msg) => events.push(ServerEvent::Recv { from: client, msg }),
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

    fn disconnect<C: Into<Self::Client>>(&mut self, client: C) -> Result<(), Self::Error> {
        let client = client.into();
        match self.clients.remove(client) {
            Some(_) => Ok(()),
            None => Err(ChannelError::NoClient(client)),
        }
    }
}
