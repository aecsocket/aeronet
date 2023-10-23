use aeronet::{ClientId, Message, ServerEvent, ServerTransport, SessionError};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use derivative::Derivative;
use rustc_hash::FxHashMap;

use crate::{shared::CHANNEL_BUF, ChannelTransportClient, DisconnectedError};

/// Server-side transport layer implementation for [`aeronet`] using in-memory channels.
///
/// This is the entry point to the entire crate, as you must first create a server before creating
/// a client. Use [`ChannelTransportServer::new`] to create a new server,then use
/// [`ChannelTransportServer::connect`] to create and connect a client.
///
/// If this server is dropped, all clients will automatically be considered disconnected.
#[derive(Debug, Derivative)]
#[derivative(Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct ChannelTransportServer<C2S, S2C> {
    clients: FxHashMap<ClientId, ClientInfo<C2S, S2C>>,
    clients_to_remove: Vec<ClientId>,
    next_client: usize,
    events: Vec<ServerEvent<C2S>>,
}

#[derive(Debug)]
struct ClientInfo<C2S, S2C> {
    send: Sender<S2C>,
    recv: Receiver<C2S>,
}

impl<C2S, S2C> ChannelTransportServer<C2S, S2C>
where
    C2S: Message,
    S2C: Message,
{
    /// Creates a new server with zero connected clients.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates and connects a client to this server.
    ///
    /// The returned transport client also contains a [`ClientId`] which you can use to disconnect
    /// it later using [`ServerTransport::disconnect`].
    pub fn connect(&mut self) -> ChannelTransportClient<C2S, S2C> {
        let (send_c2s, recv_c2s) = crossbeam_channel::bounded::<C2S>(CHANNEL_BUF);
        let (send_s2c, recv_s2c) = crossbeam_channel::bounded::<S2C>(CHANNEL_BUF);

        let client_id = ClientId::from_raw(self.next_client);
        self.next_client += 1;

        let their_client = ChannelTransportClient {
            id: client_id,
            send: send_c2s,
            recv: recv_s2c,
            connected: true,
            events: Vec::new(),
        };
        let our_client = ClientInfo {
            send: send_s2c,
            recv: recv_c2s,
        };
        self.clients.insert(client_id, our_client);
        their_client
    }
}

impl<C2S, S2C> ServerTransport<C2S, S2C> for ChannelTransportServer<C2S, S2C>
where
    C2S: Message,
    S2C: Message,
{
    type EventIter<'a> = std::vec::Drain<'a, ServerEvent<C2S>>;

    type ClientInfo = ();

    fn recv(&mut self) {
        for client in self.clients_to_remove.drain(..) {
            debug_assert!(self.clients.contains_key(&client));
            self.clients.remove(&client);
        }

        for (client, ClientInfo { recv, .. }) in self.clients.iter() {
            self.events
                .extend(recv.try_iter().map(|msg| ServerEvent::Recv(*client, msg)));

            if let Err(TryRecvError::Disconnected) = recv.try_recv() {
                self.events.push(ServerEvent::Disconnected(
                    *client,
                    SessionError::Transport(DisconnectedError.into()),
                ));
                self.clients_to_remove.push(*client);
            }
        }
    }

    fn take_events(&mut self) -> Self::EventIter<'_> {
        self.events.drain(..)
    }

    fn send(&mut self, client: ClientId, msg: impl Into<S2C>) {
        let msg = msg.into();
        if let Some(ClientInfo { send, .. }) = self.clients.get(&client) {
            // if this channel is disconnected, we'll catch it on the next `recv`
            // so don't do anything here
            let _ = send.send(msg);
        }
    }

    fn disconnect(&mut self, client: ClientId) {
        self.clients.remove(&client);
    }

    fn client_info(&self, client: ClientId) -> Option<Self::ClientInfo> {
        if self.clients.contains_key(&client) {
            Some(())
        } else {
            None
        }
    }

    fn connected(&self, client: ClientId) -> bool {
        self.clients.contains_key(&client)
    }
}
