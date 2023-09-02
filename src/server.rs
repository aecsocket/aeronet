use crate::{ClientId, TransportSettings};

#[derive(Debug, Clone)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Event))]
pub enum ServerTransportEvent {
    Connect { id: ClientId },
    Disconnect { id: ClientId },
}

pub trait ServerTransport<S: TransportSettings> {
    fn recv_events(&mut self) -> Result<Option<ServerTransportEvent>, anyhow::Error>;

    fn recv(&mut self, from: ClientId) -> Result<Option<S::C2S>, anyhow::Error>;

    fn send(&mut self, to: ClientId, msg: impl Into<S::S2C>) -> Result<(), anyhow::Error>;
}
