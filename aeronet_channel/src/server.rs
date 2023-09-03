use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use aeronet::{
    Arena, ClientId, DisconnectReason, ServerClientsError, ServerTransport, ServerTransportEvent,
    TransportSettings, TransportStats,
};
use anyhow::Result;
use crossbeam_channel::{unbounded, Receiver, Sender, TryRecvError};

use crate::ChannelClientTransport;

/// A server transport which uses [`crossbeam-channel`](https://docs.rs/crossbeam-channel) MPSC
/// senders and receivers to transmit data.
///
/// See the [crate docs](./index.html) for details.
#[derive(Debug, derivative::Derivative)]
#[derivative(Default)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct ChannelServerTransport<S: TransportSettings> {
    clients: Arena<ClientData<S::S2C, S::C2S>>,
    events: VecDeque<ServerTransportEvent>,
}

#[derive(Debug)]
struct ClientData<S2C, C2S> {
    send: Sender<S2C>,
    recv: Receiver<C2S>,
    connected: Arc<AtomicBool>,
}

impl<S: TransportSettings> ChannelServerTransport<S> {
    /// Creates a new server transport with no connected clients.
    ///
    /// This is functionally equivalent to [`Default::default`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates and connects a client to this server transport, by creating new MPSC channels.
    ///
    /// This is the only way to construct a [`ChannelClientTransport`], as well as to get that
    /// client transport's corresponding [`ClientId`].
    pub fn connect(&mut self) -> (ChannelClientTransport<S>, ClientId) {
        let (send_c2s, recv_c2s) = unbounded::<S::C2S>();
        let (send_s2c, recv_s2c) = unbounded::<S::S2C>();

        let (transport, connected) = ChannelClientTransport::new(send_c2s, recv_s2c);
        let client = ClientId::from_raw(self.clients.insert(ClientData {
            send: send_s2c,
            recv: recv_c2s,
            connected,
        }));

        self.events
            .push_back(ServerTransportEvent::Connect { client });
        (transport, client)
    }

    fn client(&self, client: ClientId) -> Result<&ClientData<S::S2C, S::C2S>> {
        match self.clients.get(client.into_raw()) {
            Some(data) if data.connected.load(Ordering::SeqCst) => Ok(data),
            Some(_) => Err(ServerClientsError::Disconnected.into()),
            None => Err(ServerClientsError::Invalid.into()),
        }
    }
}

impl<S: TransportSettings> ServerTransport<S> for ChannelServerTransport<S> {
    fn pop_event(&mut self) -> Option<ServerTransportEvent> {
        self.events.pop_front()
    }

    fn recv(&mut self, from: ClientId) -> Result<Option<S::C2S>> {
        let ClientData { recv, .. } = self.client(from)?;
        match recv.try_recv() {
            Ok(msg) => Ok(Some(msg)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    fn send(&mut self, to: ClientId, msg: impl Into<S::S2C>) -> Result<()> {
        let ClientData { send, .. } = self.client(to)?;
        send.try_send(msg.into()).map_err(|err| err.into())
    }

    fn disconnect(&mut self, client: ClientId) -> Result<()> {
        match self.clients.remove(client.into_raw()) {
            Some(data) => {
                if data
                    .connected
                    .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    self.events.push_back(ServerTransportEvent::Disconnect {
                        client,
                        reason: DisconnectReason::ByServer,
                    });
                    Ok(())
                } else {
                    Err(ServerClientsError::Disconnected.into())
                }
            }
            None => Err(ServerClientsError::Invalid.into()),
        }
    }

    fn stats(&self, client: ClientId) -> Result<TransportStats> {
        let _ = self.client(client)?;
        Ok(TransportStats::default())
    }
}
