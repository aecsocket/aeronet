use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use aeronet::{
    ClientTransport, ClientTransportEvent, DisconnectReason, TransportSettings, TransportStats,
};
use anyhow::Result;
use crossbeam_channel::{Receiver, Sender, TryRecvError};

/// A client transport which uses [`crossbeam-channel`](https://docs.rs/crossbeam-channel) MPSC
/// senders and receivers to transmit data.
///
/// **Note:** you cannot construct this struct directly. Instead, you must use
/// [`ChannelServerTransport::connect`] to construct a client transport.
///
/// See the [crate docs](./index.html) for details.
///
/// [`ChannelServerTransport::connect`]: struct.ChannelServerTransport.html#method.connect
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct ChannelClientTransport<S: TransportSettings> {
    send: Sender<S::C2S>,
    recv: Receiver<S::S2C>,
    connected: Arc<AtomicBool>,
    last_connected: Option<bool>,
}

impl<S: TransportSettings> ChannelClientTransport<S> {
    pub(crate) fn new(send: Sender<S::C2S>, recv: Receiver<S::S2C>) -> (Self, Arc<AtomicBool>) {
        let connected = Arc::new(AtomicBool::new(true));
        let this = Self {
            send,
            recv,
            connected: connected.clone(),
            last_connected: None,
        };
        (this, connected)
    }
}

impl<S: TransportSettings> ClientTransport<S> for ChannelClientTransport<S> {
    fn pop_event(&mut self) -> Option<ClientTransportEvent> {
        let connected = self.connected.load(Ordering::SeqCst);
        match self.last_connected {
            None => {
                self.last_connected = Some(true);
                Some(ClientTransportEvent::Connect)
            }
            Some(last_connected) if last_connected != connected => {
                self.last_connected = Some(connected);
                Some(ClientTransportEvent::Disconnect { reason: DisconnectReason::ByServer })
            }
            Some(_) => None
        }
    }

    fn recv(&mut self) -> Result<Option<S::S2C>> {
        match self.recv.try_recv() {
            Ok(msg) => Ok(Some(msg)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    fn send(&mut self, msg: impl Into<S::C2S>) -> Result<()> {
        self.send.try_send(msg.into()).map_err(|err| err.into())
    }

    fn stats(&self) -> TransportStats {
        TransportStats::default()
    }
}
