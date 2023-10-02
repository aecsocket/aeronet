use aeronet::{ClientId, ServerTransportConfig, ServerTransport, ServerEvent, RecvError};
use crossbeam_channel::{Sender, Receiver};
use rustc_hash::FxHashMap;

use crate::{ChannelTransportClient, shared::CHANNEL_BUF};

struct ClientInfo<C: ServerTransportConfig> {
    send: Sender<C::S2C>,
    recv: Receiver<C::C2S>,
}

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct ChannelTransportServer<C: ServerTransportConfig> {
    clients: FxHashMap<ClientId, ClientInfo<C>>,
    next_client: usize,
}

impl<C: ServerTransportConfig> ChannelTransportServer<C> {
    pub fn new() -> Self {
        Self {
            clients: FxHashMap::default(),
            next_client: 0,
        }
    }

    pub fn connect(&mut self) -> ClientId {
        let (send_c2s, recv_c2s) = crossbeam_channel::bounded::<C::C2S>(CHANNEL_BUF);
        let (send_s2c, recv_s2c) = crossbeam_channel::bounded::<C::S2C>(CHANNEL_BUF);
        
        let client_id = ClientId::from_raw(self.next_client);
        self.next_client += 1;
        
        let client = ChannelTransportClient {
            send: send_c2s,
            recv: recv_s2c,
        };
        self.clients.insert(client_id, ());
        client_id
    }
}

impl<C: ServerTransportConfig> ServerTransport for ChannelTransportServer<C> {
    type ClientInfo = ();

    fn recv(&mut self) -> Result<ServerEvent<C::C2S>, RecvError> {
        
    }

    fn client_info(&self, client: ClientId) -> Option<Self::ClientInfo> {
        if self.clients.contains_key(&client) {
            Some(())
        } else {
            None
        }
    }
}
