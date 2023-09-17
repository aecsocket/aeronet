use std::time::Duration;

use tokio::sync::{broadcast, mpsc};

use crate::TransportConfig;

use super::{ClientId, ClientInfo, Event, Request, ServerStream, SharedClients};

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct Frontend<C: TransportConfig> {
    pub(crate) send: broadcast::Sender<Request<C::S2C>>,
    pub(crate) recv: mpsc::Receiver<Event<C::C2S>>,
    pub(crate) clients: SharedClients,
}

impl<C: TransportConfig> Frontend<C> {
    pub fn recv(&mut self) -> Result<Event<C::C2S>, mpsc::error::TryRecvError> {
        self.recv.try_recv()
    }

    pub fn send(&self, client: ClientId, stream: ServerStream, msg: C::S2C) {
        let _ = self.send.send(Request::Send {
            client,
            stream,
            msg,
        });
    }

    pub fn disconnect(&self, client: ClientId) {
        let _ = self.send.send(Request::Disconnect { client });
    }

    pub fn client_info(&self, client: ClientId) -> Option<ClientInfo> {
        self.clients
            .lock()
            .unwrap()
            .get(client.0)
            .and_then(Option::as_ref)
            .cloned()
    }

    pub fn rtt(&self, client: ClientId) -> Option<Duration> {
        self.clients
            .lock()
            .unwrap()
            .get(client.0)
            .and_then(Option::as_ref)
            .map(|client| client.rtt)
    }
}
