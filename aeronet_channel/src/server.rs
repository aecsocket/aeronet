use std::error::Error;

use crossbeam_channel::{Receiver, Sender, TryRecvError};
use slotmap::SlotMap;

pub trait TransportServer<C2S, S2C> {
    type Client: Send + Sync + 'static;

    type Error: Error + Send + Sync + 'static;

    type SignalIter: Iterator<Item = ServerSignal<Self::Client, Self::Error, C2S>>;

    fn send<M: Into<S2C>>(&mut self, client: Self::Client, msg: M) -> Result<(), Self::Error>;

    fn recv(&mut self) -> Self::SignalIter;

    fn disconnect(&mut self, client: Self::Client) -> Result<(), Self::Error>;
}

pub enum ServerSignal<Client, Error, C2S> {
    Recv {
        from: Client,
        msg: C2S,
    },
    Disconnected {
        client: Client,
        reason: Error,
    },
}

//

slotmap::new_key_type! {
    /// Type used to uniquely identify a connected client.
    pub struct ClientKey;
}

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct ChannelServer<C2S, S2C> {
    pub(crate) clients: SlotMap<ClientKey, ClientState<C2S, S2C>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ChannelServerError {
    #[error("no client with key {0:?}")]
    NoClient(ClientKey),
    #[error("disconnected")]
    Disconnected,
}

#[derive(Debug)]
pub(crate) struct ClientState<C2S, S2C> {
    pub(crate) send_s2c: Sender<S2C>,
    pub(crate) recv_c2s: Receiver<C2S>,
}

impl<C2S, S2C> ChannelServer<C2S, S2C> {
    pub fn new() -> Self {
        Self {
            clients: SlotMap::default(),
        }
    }
}

impl<C2S, S2C> TransportServer<C2S, S2C> for ChannelServer<C2S, S2C> {
    type Client = ClientKey;

    type Error = ChannelServerError;

    type SignalIter = std::vec::IntoIter<ServerSignal<Self::Client, Self::Error, C2S>>;

    fn send<M: Into<S2C>>(&mut self, client: Self::Client, msg: M) -> Result<(), Self::Error> {
        let msg = msg.into();
        let Some(client) = self.clients.get(client) else {
            return Err(ChannelServerError::NoClient(client));
        };
        // transmission errors here will *not* manifest as an error from `send`
        // rather, the disconnection is detected by `recv` later, and a signal
        // for the disconnect is emitted
        let _ = client.send_s2c.send(msg);
        Ok(())
    }

    fn recv(&mut self) -> Self::SignalIter {
        let mut signals = Vec::<ServerSignal<_, _, _>>::new();
        let mut to_remove = Vec::<ClientKey>::new();
        for (client_key, client) in self.clients.iter() {
            loop {
                match client.recv_c2s.try_recv() {
                    Ok(msg) => signals.push(ServerSignal::Recv {
                        from: client_key,
                        msg,
                    }),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        to_remove.push(client_key);
                        break;
                    }
                }
            }
        }

        for client in to_remove {
            debug_assert!(self.clients.contains_key(client));
            self.clients.remove(client);
            signals.push(ServerSignal::Disconnected {
                client,
                reason: ChannelServerError::Disconnected,
            });
        }

        signals.into_iter()
    }

    fn disconnect(&mut self, client: Self::Client) -> Result<(), Self::Error> {
        match self.clients.remove(client) {
            Some(_) => Ok(()),
            None => Err(ChannelServerError::NoClient(client)),
        }
    }
}
