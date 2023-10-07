use std::collections::VecDeque;

use aeronet::{ClientId, Message, RecvError, ServerEvent, ServerTransport, SessionError};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use rustc_hash::FxHashMap;

use crate::{shared::CHANNEL_BUF, ChannelTransportClient, DisconnectedError};

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
    queued_recv: VecDeque<ServerEvent<C2S>>,
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
            queued_recv: VecDeque::new(),
        }
    }

    pub fn connect(&mut self) -> ChannelTransportClient<C2S, S2C> {
        let (send_c2s, recv_c2s) = crossbeam_channel::bounded::<C2S>(CHANNEL_BUF);
        let (send_s2c, recv_s2c) = crossbeam_channel::bounded::<S2C>(CHANNEL_BUF);

        let client_id = ClientId::from_raw(self.next_client);
        self.next_client += 1;

        let their_client = ChannelTransportClient {
            id: client_id,
            send: send_c2s,
            recv: recv_s2c,
            connected: true,
        };
        let our_client = ClientInfo {
            send: send_s2c,
            recv: recv_c2s,
        };
        self.clients.insert(client_id, our_client);
        their_client
    }
}

impl<C2S, S2C> ServerTransport<C2S, S2C> for ChannelTransportServer<C2S, S2C>
where
    C2S: Message,
    S2C: Message,
{
    type ClientInfo = ();

    fn recv(&mut self) -> Result<ServerEvent<C2S>, RecvError> {
        // buffer up events so that, on recv, we'll iterate the client map once, buffer the events,
        // then send them out on the next few recv calls
        // this has the disadvantage that the first recv will always be RecvError::Empty
        if let Some(event) = self.queued_recv.pop_front() {
            return Ok(event);
        }

        for (client, ClientInfo { recv, .. }) in self.clients.iter() {
            match recv.try_recv() {
                Ok(msg) => {
                    self.queued_recv.push_back(ServerEvent::Recv {
                        client: *client,
                        msg,
                    });
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.queued_recv.push_back(ServerEvent::Disconnected {
                        client: *client,
                        reason: SessionError::Transport(DisconnectedError.into()),
                    })
                }
            }
        }

        Err(RecvError::Empty)
    }

    fn send(&mut self, client: ClientId, msg: impl Into<S2C>) {
        let msg = msg.into();
        if let Some(ClientInfo { send, .. }) = self.clients.get(&client) {
            // if this channel is disconnected, we'll catch it on the next `recv`
            // so don't do anything here
            let _ = send.send(msg);
        }
    }

    fn disconnect(&mut self, client: ClientId) {
        self.clients.remove(&client);
    }

    fn client_info(&self, client: ClientId) -> Option<Self::ClientInfo> {
        if self.clients.contains_key(&client) {
            Some(())
        } else {
            None
        }
    }

    fn connected(&self, client: ClientId) -> bool {
        self.clients.contains_key(&client)
    }
}
