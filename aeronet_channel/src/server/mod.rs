use aeronet::{ClientId, RecvError, ServerEvent, ServerTransport, Message};
use crossbeam_channel::{Receiver, Sender};
use rustc_hash::FxHashMap;

use crate::{shared::CHANNEL_BUF, ChannelTransportClient};

#[derive(Debug)]
struct ClientInfo<C2S, S2C> {
    send: Sender<S2C>,
    recv: Receiver<C2S>,
}

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct ChannelTransportServer<C2S, S2C> {
    clients: FxHashMap<ClientId, ClientInfo<C2S, S2C>>,
    next_client: usize,
}

impl<C2S, S2C> ChannelTransportServer<C2S, S2C>
where
    C2S: Message,
    S2C: Message,
{
    pub fn new() -> Self {
        Self {
            clients: FxHashMap::default(),
            next_client: 0,
        }
    }

    pub fn connect(&mut self) -> (ClientId, ChannelTransportClient<C2S, S2C>) {
        let (send_c2s, recv_c2s) = crossbeam_channel::bounded::<C2S>(CHANNEL_BUF);
        let (send_s2c, recv_s2c) = crossbeam_channel::bounded::<S2C>(CHANNEL_BUF);

        let client_id = ClientId::from_raw(self.next_client);
        self.next_client += 1;

        let their_client = ChannelTransportClient {
            send: send_c2s,
            recv: recv_s2c,
        };
        let our_client = ClientInfo {
            send: send_s2c,
            recv: recv_c2s,
        };
        self.clients.insert(client_id, our_client);
        (client_id, their_client)
    }
}

impl<C2S, S2C> ServerTransport<C2S, S2C> for ChannelTransportServer<C2S, S2C>
where
    C2S: Message,
    S2C: Message,
{
    type ClientInfo = ();

    fn recv(&mut self) -> Result<ServerEvent<C2S>, RecvError> {
        todo!()
    }

    fn send(&mut self, client: ClientId, msg: impl Into<S2C>) {
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
