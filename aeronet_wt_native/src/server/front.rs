use aeronet::{ClientId, Message, ServerEvent, ServerTransport, TryFromBytes, TryIntoBytes};
use rustc_hash::FxHashMap;
use tokio::sync::{broadcast, mpsc};

use crate::{EndpointInfo, SendOn, ServerStream};

use super::{Event, Request};

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
pub struct WebTransportServer<C2S, S2C> {
    pub(crate) send: broadcast::Sender<Request<S2C>>,
    pub(crate) recv: mpsc::Receiver<Event<C2S>>,
    pub(crate) clients: FxHashMap<ClientId, EndpointInfo>,
    pub(crate) events: Vec<ServerEvent<C2S>>,
}

impl<C2S, S2C> ServerTransport<C2S, S2C> for WebTransportServer<C2S, S2C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + SendOn<ServerStream>,
{
    type ClientInfo = EndpointInfo;

    fn recv(&mut self) {
        while let Ok(event) = self.recv.try_recv() {
            match event {
                Event::Connected { client, info } => {
                    debug_assert!(!self.clients.contains_key(&client));
                    self.clients.insert(client, info);
                    self.events.push(ServerEvent::Connected { client });
                }
                Event::UpdateInfo { client, info } => {
                    debug_assert!(self.clients.contains_key(&client));
                    self.clients.insert(client, info);
                }
                Event::Recv { client, msg } => {
                    self.events.push(ServerEvent::Recv { client, msg });
                }
                Event::Disconnected { client, reason } => {
                    debug_assert!(self.clients.contains_key(&client));
                    self.clients.remove(&client);
                    self.events
                        .push(ServerEvent::Disconnected { client, reason });
                }
            }
        }
    }

    fn take_events(&mut self) -> impl Iterator<Item = ServerEvent<C2S>> {
        self.events.drain(..)
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

    fn connected(&self, client: ClientId) -> bool {
        self.clients.contains_key(&client)
    }
}
