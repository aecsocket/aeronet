use aeronet::{
    ClientId, RecvError, SendMessage, ServerEvent, ServerTransport, ServerTransportConfig,
};
use anyhow::Result;
use rustc_hash::FxHashMap;
use tokio::sync::{broadcast, mpsc};

use crate::{SendOn, ServerStream};

use super::{Event, RemoteClientInfo, Request};

/// Server-side transport layer implementation for [`aeronet`] using the WebTransport protocol.
///
/// This is the server-side entry point to the crate, allowing you to interface with the clients
/// by receiving and sending data and commands to the [`crate::WebTransportServerBackend`].
/// This is the type you should store and pass around in your app whenever you want to interface
/// with the server. Use [`crate::create_server`] to create one.
///
/// When dropped, the backend server is shut down and all client connections are dropped.
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct WebTransportServer<C: ServerTransportConfig> {
    pub(crate) send: broadcast::Sender<Request<C::S2C>>,
    pub(crate) recv: mpsc::Receiver<Event<C::C2S>>,
    pub(crate) clients: FxHashMap<ClientId, RemoteClientInfo>,
}

impl<S2C, C> ServerTransport<C> for WebTransportServer<C>
where
    S2C: SendMessage + SendOn<ServerStream>,
    C: ServerTransportConfig<S2C = S2C>,
{
    type ClientInfo = RemoteClientInfo;

    fn recv(&mut self) -> Result<ServerEvent<C::C2S>, RecvError> {
        loop {
            match self.recv.try_recv() {
                // non-returning
                Ok(Event::UpdateInfo { client, info }) => {
                    *self.clients.get_mut(&client).unwrap() = info;
                }
                // returning
                Ok(Event::Incoming { client, info }) => {
                    self.clients.insert(client, info);
                    return Ok(ServerEvent::Connecting { client });
                }
                Ok(Event::Connected { client }) => {
                    return Ok(ServerEvent::Connected { client });
                }
                Ok(Event::Recv { client, msg }) => return Ok(ServerEvent::Recv { client, msg }),
                Ok(Event::Disconnected { client, reason }) => {
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

    fn client_info(&self, client: ClientId) -> Option<Self::ClientInfo> {
        self.clients.get(&client).cloned()
    }
}
