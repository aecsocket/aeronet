use std::{time::Duration, net::SocketAddr};

use aeronet::{
    server::{ClientId, Transport, ClientRtt, ClientRemoteAddr},
    TransportConfig,
};
use tokio::sync::{broadcast, mpsc};

use super::{ClientInfo, Event, Request, StreamMessage, SharedClients};

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct Frontend<C: TransportConfig> {
    pub(crate) send: broadcast::Sender<Request<C::S2C>>,
    pub(crate) recv: mpsc::Receiver<Event<C::C2S>>,
    pub(crate) clients: SharedClients,
}

impl<C2S, S2C, C> Transport<C> for Frontend<C>
where
    C2S: StreamMessage,
    S2C: StreamMessage,
    C: TransportConfig<C2S = C2S, S2C = S2C>,
{
    fn send(&mut self, client: ClientId, msg: S2C) {
        let _ = self.send.send(Request::Send {
            client,
            stream: msg.stream(),
            msg,
        });
    }

    fn disconnect(&mut self, client: ClientId) {
        let _ = self.send.send(Request::Disconnect { client });
    }
}

impl<C: TransportConfig> ClientRtt for Frontend<C> {
    fn rtt(&self, client: ClientId) -> Option<Duration> {
        self.clients
            .lock()
            .unwrap()
            .get(client.into_raw())
            .and_then(Option::as_ref)
            .map(|client| client.rtt)
    }
}

impl<C: TransportConfig> ClientRemoteAddr for Frontend<C> {
    fn remote_addr(&self, client: ClientId) -> Option<SocketAddr> {
        self.clients
            .lock()
            .unwrap()
            .get(client.into_raw())
            .and_then(Option::as_ref)
            .map(|client| client.remote_addr)
    }
}

impl<C: TransportConfig> Frontend<C> {
    pub fn recv(&mut self) -> Result<Event<C::C2S>, mpsc::error::TryRecvError> {
        self.recv.try_recv()
    }

    pub fn client_info(&self, client: ClientId) -> Option<ClientInfo> {
        self.clients
            .lock()
            .unwrap()
            .get(client.into_raw())
            .and_then(Option::as_ref)
            .cloned()
    }
}
