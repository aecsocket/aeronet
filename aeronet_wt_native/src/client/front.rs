use aeronet::{ClientEvent, ClientTransport, Message, TryFromBytes, TryIntoBytes};
use aeronet_wt_core::{Channels, OnChannel};
use tokio::sync::mpsc;

use crate::EndpointInfo;

use super::{Event, Request};

/// Client-side transport layer implementation for [`aeronet`] using the
/// WebTransport protocol.
///
/// This is the client-side entry point to the crate, allowing you to connect
/// the [`crate::WebTransportClientBackend`] to a server, then send and receive
/// data to/from the backend.
/// This is the type you should store and pass around in your app whenever you
/// want to interface with the server. Use [`crate::create_client`] to create
/// one.
///
/// # Usage
///
/// After creation, use [`WebTransportClient::connect`] to request a connection
/// to a specified URL. This request may only work when the client is not yet
/// connected
///
/// When dropped, the backend client is shut down and the current connection is
/// dropped.
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct WebTransportClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: Channels,
{
    pub(crate) send: mpsc::Sender<Request<C2S>>,
    pub(crate) recv: mpsc::Receiver<Event<S2C>>,
    pub(crate) info: Option<EndpointInfo>,
    pub(crate) events: Vec<ClientEvent<S2C>>,
}

impl<C2S, S2C, C> WebTransportClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: Channels,
{
    /// Requests the client to connect to a given URL.
    ///
    /// If the client is [connected], this request has no effect.
    ///
    /// [connected]: aeronet::ClientTransport::connected
    pub fn connect(&self, url: impl Into<String>) {
        let _ = self.send.try_send(Request::Connect(url.into()));
    }

    /// Requests the client to disconnect from the current connection.
    ///
    /// If the client is not [connected], this request has no effect.
    ///
    /// [connected]: aeronet::ClientTransport::connected
    pub fn disconnect(&self) {
        let _ = self.send.try_send(Request::Disconnect);
    }
}

impl<C2S, S2C, C> ClientTransport<C2S, S2C> for WebTransportClient<C2S, S2C, C>
where
    C2S: Message + TryIntoBytes + OnChannel<Channel = C>,
    S2C: Message + TryFromBytes,
    C: Channels,
{
    type EventIter<'a> = std::vec::Drain<'a, ClientEvent<S2C>> where Self: 'a;

    type Info = EndpointInfo;

    fn recv(&mut self) {
        while let Ok(event) = self.recv.try_recv() {
            match event {
                Event::Connected(info) => {
                    debug_assert!(self.info.is_none());
                    self.info = Some(info);
                    self.events.push(ClientEvent::Connected);
                }
                Event::UpdateInfo(info) => {
                    debug_assert!(self.info.is_some());
                    self.info = Some(info);
                }
                Event::Recv(msg) => {
                    self.events.push(ClientEvent::Recv(msg));
                }
                Event::Disconnected(reason) => {
                    debug_assert!(self.info.is_some());
                    self.info = None;
                    self.events.push(ClientEvent::Disconnected(reason));
                }
            }
        }
    }

    fn take_events(&mut self) -> Self::EventIter<'_> {
        self.events.drain(..)
    }

    fn send(&mut self, msg: impl Into<C2S>) {
        let msg = msg.into();
        let _ = self.send.try_send(Request::Send(msg));
    }

    fn info(&self) -> Option<Self::Info> {
        self.info.as_ref().cloned()
    }

    fn connected(&self) -> bool {
        self.info.is_some()
    }
}
