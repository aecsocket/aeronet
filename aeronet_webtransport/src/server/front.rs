use std::{net::SocketAddr, time::Duration};

use aeronet::{
    server::{ClientId, ClientRemoteAddr, ClientRtt, Event, RecvError, Transport},
    Message, TransportConfig,
};
use anyhow::Result;
use tokio::sync::{broadcast, mpsc};

use super::{ClientInfo, Request, SharedClients, StreamMessage};

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct Frontend<C: TransportConfig> {
    pub(crate) send: broadcast::Sender<Request<C::S2C>>,
    pub(crate) recv: mpsc::Receiver<Event<C::C2S>>,
    pub(crate) clients: SharedClients,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("connection to backend closed")]
struct BackendError;

impl<S2C, C> Transport<C> for Frontend<C>
where
    S2C: Message,
    C: TransportConfig<S2C = StreamMessage<S2C>>,
{
    fn recv(&mut self) -> Result<Event<C::C2S>, RecvError> {
        self.recv.try_recv().map_err(|err| match err {
            mpsc::error::TryRecvError::Empty => RecvError::Empty,
            _ => RecvError::Closed,
        })
    }

    fn send(&mut self, client: ClientId, msg: StreamMessage<S2C>) {
        let _ = self.send.send(Request::Send {
            client,
            stream: msg.stream,
            msg: msg.msg,
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
            .and_then(|client| match client {
                ClientInfo::Connected { rtt, .. } => Some(*rtt),
                _ => None,
            })
    }
}

impl<C: TransportConfig> ClientRemoteAddr for Frontend<C> {
    fn remote_addr(&self, client: ClientId) -> Option<SocketAddr> {
        self.clients
            .lock()
            .unwrap()
            .get(client.into_raw())
            .and_then(|client| match client {
                ClientInfo::Connected { remote_addr, .. } => Some(*remote_addr),
                _ => None,
            })
    }
}

impl<C: TransportConfig> Frontend<C> {
    pub fn recv(&mut self) -> Result<Event<C::C2S>, mpsc::error::TryRecvError> {
        self.recv.try_recv()
    }

    pub fn client_info(&self, client: ClientId) -> Option<ClientInfo> {
        self.clients.lock().unwrap().get(client.into_raw()).cloned()
    }
}
