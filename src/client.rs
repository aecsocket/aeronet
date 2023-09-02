use anyhow::Result;

use crate::{DisconnectReason, TransportSettings};

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Event))]
pub enum ClientTransportEvent {
    Connect,
    Disconnect { reason: DisconnectReason },
}

pub trait ClientTransport<S: TransportSettings> {
    fn pop_event(&mut self) -> Option<ClientTransportEvent>;

    fn recv(&mut self) -> Result<Option<S::S2C>>;

    fn send(&mut self, msg: impl Into<S::C2S>) -> Result<()>;
}
