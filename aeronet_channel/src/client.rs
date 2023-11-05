use std::marker::PhantomData;

use crossbeam_channel::{Receiver, Sender, TryRecvError};

use crate::{ChannelServer, server, ClientKey};

pub enum ChannelClient<C2S, S2C> {
    Disconnected(ClientState<C2S, S2C, Disconnected>),
    Connected(ClientState<C2S, S2C, Connected<C2S, S2C>>),
}

pub struct ClientState<C2S, S2C, S> {
    state: S,
    phantom_c2s: PhantomData<C2S>,
    phantom_s2c: PhantomData<S2C>,
}

pub struct Disconnected;

impl<C2S, S2C> ClientState<C2S, S2C, Disconnected> {
    pub fn new() -> Self {
        Self {
            state: Disconnected,
            phantom_c2s: PhantomData::default(),
            phantom_s2c: PhantomData::default(),
        }
    }

    pub fn connect(self, server: &mut ChannelServer<C2S, S2C>) -> ClientState<C2S, S2C, Connected<C2S, S2C>> {
        let (send_c2s, recv_c2s) = crossbeam_channel::unbounded::<C2S>();
        let (send_s2c, recv_s2c) = crossbeam_channel::unbounded::<S2C>();

        let remote_state = server::ClientState {
            send_s2c,
            recv_c2s,
        };
        let key = server.clients.insert(remote_state);
        let state = Connected {
            key,
            send_c2s,
            recv_s2c,
            msgs: Vec::new(),
        };

        ClientState {
            state,
            phantom_c2s: self.phantom_c2s,
            phantom_s2c: self.phantom_s2c,
        }
    }
}

pub struct Connected<C2S, S2C> {
    key: ClientKey,
    send_c2s: Sender<C2S>,
    recv_s2c: Receiver<S2C>,
    msgs: Vec<S2C>,
}

impl<C2S, S2C> ClientState<C2S, S2C, Connected<C2S, S2C>> {
    pub fn disconnect(self) -> ClientState<C2S, S2C, Disconnected> {
        ClientState {
            state: Disconnected,
            phantom_c2s: self.phantom_c2s,
            phantom_s2c: self.phantom_s2c,
        }
    }

    pub fn recv(mut self) -> (ChannelClient<C2S, S2C>, impl Iterator<Item = S2C>) {
        let mut msgs = Vec::new();
        loop {
            match self.state.recv_s2c.try_recv() {
                Ok(msg) => msgs.push(msg),
                Err(TryRecvError::Empty) => break,
            }
        }

        self.state.msgs.extend(self.state.recv_s2c.try_iter());

        match self.state.recv_s2c.try_recv() {
            Err(TryRecvError::Disconnected) => (ChannelClient::Disconnected(self.disconnect()), self.state.msgs.drain(..)),
            _ => {
                // if a server sends a message right after we've consumed all
                // its events, this is possible, but we drop it
                // kind of bad but idk
                (ChannelClient::Connected(self), self.state.msgs.drain(..))
            },
        }
    }
}
