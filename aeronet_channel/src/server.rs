use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use aeronet::{
    Arena, ClientId, ServerClientsError, ServerTransport, ServerTransportEvent, TransportSettings, DisconnectReason,
};
use anyhow::Result;
use crossbeam_channel::{unbounded, Receiver, Sender, TryRecvError};

use crate::ChannelClientTransport;

#[derive(Debug, derivative::Derivative)]
#[derivative(Default)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct ChannelServerTransport<S: TransportSettings> {
    clients: Arena<ClientData<S::S2C, S::C2S>>,
    events: VecDeque<ServerTransportEvent>,
}

#[derive(Debug)]
struct ClientData<S2C, C2S> {
    send: Sender<S2C>,
    recv: Receiver<C2S>,
    connected: Arc<AtomicBool>,
}

impl<S: TransportSettings> ChannelServerTransport<S> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn connect(&mut self) -> (ChannelClientTransport<S>, ClientId) {
        let (send_c2s, recv_c2s) = unbounded::<S::C2S>();
        let (send_s2c, recv_s2c) = unbounded::<S::S2C>();

        let (transport, connected) = ChannelClientTransport::new(send_c2s, recv_s2c);
        let client = ClientId::from_raw(self.clients.insert(ClientData {
            send: send_s2c,
            recv: recv_c2s,
            connected,
        }));

        self.events
            .push_back(ServerTransportEvent::Connect { client });
        (transport, client)
    }
}

impl<S: TransportSettings> ServerTransport<S> for ChannelServerTransport<S> {
    fn pop_event(&mut self) -> Option<ServerTransportEvent> {
        self.events.pop_front()
    }

    fn recv(&mut self, from: ClientId) -> Result<Option<S::C2S>> {
        match self.clients.get(from.into_raw()) {
            Some(ClientData {
                recv, connected, ..
            }) if connected.load(Ordering::SeqCst) => match recv.try_recv() {
                Ok(msg) => Ok(Some(msg)),
                Err(TryRecvError::Empty) => Ok(None),
                Err(err) => Err(err.into()),
            },
            Some(_) => Err(ServerClientsError::Disconnected.into()),
            None => Err(ServerClientsError::Invalid.into()),
        }
    }

    fn send(&mut self, to: ClientId, msg: impl Into<S::S2C>) -> Result<()> {
        match self.clients.get(to.into_raw()) {
            Some(ClientData {
                send, connected, ..
            }) if connected.load(Ordering::SeqCst) => {
                send.try_send(msg.into()).map_err(|err| err.into())
            }
            Some(_) => Err(ServerClientsError::Disconnected.into()),
            None => Err(ServerClientsError::Invalid.into()),
        }
    }

    fn disconnect(&mut self, client: ClientId) -> Result<()> {
        match self.clients.remove(client.into_raw()) {
            Some(data) => {
                if data
                    .connected
                    .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    self.events.push_back(ServerTransportEvent::Disconnect { client, reason: DisconnectReason::ByServer });
                    Ok(())
                } else {
                    Err(ServerClientsError::Disconnected.into())
                }
            }
            None => Err(ServerClientsError::Invalid.into()),
        }
    }
}
