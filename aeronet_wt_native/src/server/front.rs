use std::{net::SocketAddr, time::Duration};

use aeronet::{
    ClientId, RecvError, SendMessage, ServerEvent, ServerRemoteAddr, ServerRtt, ServerTransport,
    ServerTransportConfig,
};
use anyhow::Result;
use rustc_hash::FxHashMap;
use tokio::sync::{broadcast, mpsc};

use crate::SendOnServerStream;

use super::{ClientInfo, InternalEvent, Request};

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct WebTransportServer<C: ServerTransportConfig> {
    pub(crate) send: broadcast::Sender<Request<C::S2C>>,
    pub(crate) recv: mpsc::Receiver<InternalEvent<C::C2S>>,
    pub(crate) clients: FxHashMap<ClientId, ClientInfo>,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("connection to backend closed")]
struct BackendError;

impl<S2C, C> ServerTransport<C> for WebTransportServer<C>
where
    S2C: SendMessage + SendOnServerStream,
    C: ServerTransportConfig<S2C = S2C>,
{
    fn recv(&mut self) -> Result<ServerEvent<C::C2S>, RecvError> {
        loop {
            match self.recv.try_recv() {
                // non-returning
                Ok(InternalEvent::UpdateInfo { client, info }) => {
                    *self.clients.get_mut(&client).unwrap() = info;
                }
                // returning
                Ok(InternalEvent::Incoming { client, info }) => {
                    self.clients.insert(client, info);
                    return Ok(ServerEvent::Incoming { client });
                }
                Ok(InternalEvent::Connected { client }) => {
                    return Ok(ServerEvent::Connected { client });
                }
                Ok(InternalEvent::Recv { client, msg }) => {
                    return Ok(ServerEvent::Recv { client, msg })
                }
                Ok(InternalEvent::Disconnected { client, reason }) => {
                    self.clients.remove(&client);
                    return Ok(ServerEvent::Disconnected { client, reason });
                }
                Err(mpsc::error::TryRecvError::Empty) => return Err(RecvError::Empty),
                Err(_) => return Err(RecvError::Closed),
            }
        }
    }

    fn send(&mut self, client: ClientId, msg: impl Into<S2C>) {
        let msg = msg.into();
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

impl<C: ServerTransportConfig> ServerRtt for WebTransportServer<C> {
    fn rtt(&self, client: ClientId) -> Option<Duration> {
        self.clients.get(&client).and_then(|client| match client {
            ClientInfo::Connected { rtt, .. } => Some(*rtt),
            _ => None,
        })
    }
}

impl<C: ServerTransportConfig> ServerRemoteAddr for WebTransportServer<C> {
    fn remote_addr(&self, client: ClientId) -> Option<SocketAddr> {
        self.clients.get(&client).and_then(|client| match client {
            ClientInfo::Connected { remote_addr, .. } => Some(*remote_addr),
            _ => None,
        })
    }
}

impl<C: ServerTransportConfig> WebTransportServer<C> {
    pub fn client_info(&self, client: ClientId) -> Option<ClientInfo> {
        self.clients.get(&client).cloned()
    }
}
