use aeronet::{ClientId, RecvError, ServerEvent, ServerTransport, ServerTransportConfig};
use crossbeam_channel::{Receiver, Sender};
use rustc_hash::FxHashMap;

use crate::{shared::CHANNEL_BUF, ChannelTransportClient};

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

    pub fn connect(&mut self) -> (ClientId, ChannelTransportClient<C>) {
        let (send_c2s, recv_c2s) = crossbeam_channel::bounded::<C::C2S>(CHANNEL_BUF);
        let (send_s2c, recv_s2c) = crossbeam_channel::bounded::<C::S2C>(CHANNEL_BUF);

        let client_id = ClientId::from_raw(self.next_client);
        self.next_client += 1;

        let their_client = ChannelTransportClient {
            send: send_c2s,
            recv: recv_s2c,
        };
        let our_client = ClientInfo {
            send: send_c2s,
            recv: recv_c2s,
        };
        self.clients.insert(client_id, our_client);
        (client_id, their_client)
    }
}

impl<C: ServerTransportConfig> ServerTransport<C> for ChannelTransportServer<C> {
    type ClientInfo = ();

    fn recv(&mut self) -> Result<ServerEvent<C::C2S>, RecvError> {
        todo!()
    }

    fn send(&mut self, client: ClientId, msg: impl Into<C::S2C>) {
        todo!()
    }

    fn disconnect(&mut self, client: ClientId) {
        todo!()
    }

    fn client_info(&self, client: ClientId) -> Option<Self::ClientInfo> {
        if self.clients.contains_key(&client) {
            Some(())
        } else {
            None
        }
    }
}
