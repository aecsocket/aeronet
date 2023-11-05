use aeronet::{ClientEvent, ClientId, ClientTransport, Message, SessionError};
use crossbeam_channel::{Receiver, Sender, TryRecvError};

use crate::DisconnectedError;

/// Client-side transport layer implementation for [`aeronet`] using in-memory
/// channels.
///
/// A client can only be created by connecting to an existing
/// [`ChannelTransportServer`] using [`ChannelTransportServer::connect`].
///
/// If this client is dropped, it is considered disconnected on the server side.
/// If the server is dropped, this client will not be considered connected by
/// [`ClientTransport::connected`].
///
/// [`ChannelTransportServer`]: crate::ChannelTransportServer
/// [`ChannelTransportServer::connect`]: crate::ChannelTransportServer::connect
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct ChannelTransportClient<C2S, S2C> {
    pub(crate) id: ClientId,
    pub(crate) send: Sender<C2S>,
    pub(crate) recv: Receiver<S2C>,
    pub(crate) connected: bool,
    pub(crate) events: Vec<ClientEvent<S2C>>,
}

impl<C2S, S2C> ChannelTransportClient<C2S, S2C> {
    /// Gets the server-side client ID of this client.
    ///
    /// This can be used to disconnect the client from the server using
    /// [`ChannelTransportServer::disconnect`].
    ///
    /// [`ChannelTransportServer::disconnect`]: aeronet::ServerTransport::disconnect
    pub fn id(&self) -> ClientId {
        self.id
    }
}

impl<C2S, S2C> ClientTransport<C2S, S2C> for ChannelTransportClient<C2S, S2C>
where
    C2S: Message,
    S2C: Message,
{
    type EventIter<'a> = std::vec::Drain<'a, ClientEvent<S2C>>;

    type Info = ();

    fn recv(&mut self) {
        self.events
            .extend(self.recv.try_iter().map(|msg| ClientEvent::Recv(msg)));

        if self.connected {
            if let Err(TryRecvError::Disconnected) = self.recv.try_recv() {
                self.connected = false;
                self.events
                    .push(ClientEvent::Disconnected(SessionError::Transport(
                        DisconnectedError.into(),
                    )));
            }
        }
    }

    fn take_events(&mut self) -> Self::EventIter<'_> {
        self.events.drain(..)
    }

    fn send(&mut self, msg: impl Into<C2S>) {
        let msg = msg.into();
        // if this channel is disconnected, we'll catch it on the next `recv`
        // so don't do anything here
        let _ = self.send.send(msg);
    }

    fn info(&self) -> Option<Self::Info> {
        if self.connected {
            Some(())
        } else {
            None
        }
    }

    fn connected(&self) -> bool {
        self.connected
    }
}
