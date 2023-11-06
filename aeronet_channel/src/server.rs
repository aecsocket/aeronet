use crossbeam_channel::{Receiver, Sender, TryRecvError};
use slotmap::SlotMap;

slotmap::new_key_type! {
    /// Type used to uniquely identify a connected client.
    pub struct ClientKey;
}

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct ChannelServer<C2S, S2C> {
    pub(crate) clients: SlotMap<ClientKey, ClientState<C2S, S2C>>,
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

    pub fn recv(&mut self) -> impl Iterator<Item = ServerSignal<C2S>> {
        let mut signals = Vec::<ServerSignal<_>>::new();
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
            signals.push(ServerSignal::Disconnected { client });
        }

        signals.into_iter()
    }

    pub fn send<M: Into<S2C>>(&mut self, client: ClientKey, msg: M) {
        if let Some(client) = self.clients.get(client) {}
    }
}

pub enum ServerSignal<C2S> {
    Recv { from: ClientKey, msg: C2S },
    Disconnected { client: ClientKey },
}
