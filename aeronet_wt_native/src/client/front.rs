use aeronet::{ClientEvent, ClientTransport, MessageTypes, RecvError, TryIntoBytes};
use tokio::sync::mpsc;

use crate::{ClientStream, SendOn};

use super::{Event, RemoteServerInfo, Request};

/// Client-side transport layer implementation for [`aeronet`] using the WebTransport protocol.
///
/// This is the client-side entry point to the crate, allowing you to connect the
/// [`crate::WebTransportClientBackend`] to a server, then send and receive data to/from the
/// backend.
/// This is the type you should store and pass around in your app whenever you want to interface
/// with the server. Use [`crate::create_client`] to create one.
///
/// # Usage
///
/// After creation, use [`WebTransportClient::connect`] to request a connection to a specified
/// URL. This request may only work when the client is not yet connected
///
/// When dropped, the backend client is shut down and the current connection is dropped.
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct WebTransportClient<M: MessageTypes> {
    pub(crate) send: mpsc::Sender<Request<M::C2S>>,
    pub(crate) recv: mpsc::Receiver<Event<M::S2C>>,
    pub(crate) info: Option<RemoteServerInfo>,
}

impl<M: MessageTypes> WebTransportClient<M> {
    /// Requests the client to connect to a given URL.
    ///
    /// If the client is not [connected], this request has no effect.
    ///
    /// [connected]: ClientTransport::is_connected
    pub fn connect(&self, url: impl Into<String>) {
        let _ = self.send.try_send(Request::Connect { url: url.into() });
    }

    /// Requests the client to disconnect from the current connection.
    ///
    /// If the client is not [connected], this request has no effect.
    ///
    /// [connected]: ClientTransport::is_connected
    pub fn disconnect(&self) {
        let _ = self.send.try_send(Request::Disconnect);
    }
}

impl<C2S, M> ClientTransport<M> for WebTransportClient<M>
where
    C2S: TryIntoBytes + SendOn<ClientStream>,
    M: MessageTypes<C2S = C2S>,
{
    type Info = RemoteServerInfo;

    fn recv(&mut self) -> Result<ClientEvent<M::S2C>, RecvError> {
        loop {
            match self.recv.try_recv() {
                // non-returning
                Ok(Event::UpdateInfo { info }) => {
                    *self.info.as_mut().unwrap() = info;
                }
                // returning
                Ok(Event::Connecting { info }) => {
                    self.info = Some(info);
                }
                Ok(Event::Connected) => {
                    return Ok(ClientEvent::Connected);
                }
                Ok(Event::Recv { msg }) => return Ok(ClientEvent::Recv { msg }),
                Ok(Event::Disconnected { reason }) => {
                    self.info = None;
                    return Ok(ClientEvent::Disconnected { reason });
                }
                Err(mpsc::error::TryRecvError::Empty) => return Err(RecvError::Empty),
                Err(_) => return Err(RecvError::Closed),
            }
        }
    }

    fn send(&mut self, msg: impl Into<C2S>) {
        let msg = msg.into();
        let _ = self.send.try_send(Request::Send {
            stream: msg.stream(),
            msg,
        });
    }

    fn info(&self) -> Option<Self::Info> {
        self.info.as_ref().cloned()
    }
}
